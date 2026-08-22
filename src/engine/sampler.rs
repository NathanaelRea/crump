//! Precompiled portable sample mappings and allocation-free sampler voices.

use std::{collections::BTreeMap, sync::Arc};

use crate::model::{Instrument, ParameterTarget, SamplePlayback};

use super::{
    EngineError,
    control::{CompiledControlProgram, ControlIndex},
    sample::{CompiledSampleLibrary, CompiledSampleSource},
    schedule::ClipEnvelope,
};

const STEAL_FADE_FRAMES: u32 = 64;
const CHOKE_FADE_FRAMES: u32 = 64;

/// Exact preallocated mutable voice storage for one sampler Track runtime.
pub(crate) fn sampler_voice_state_bytes(polyphony: u16) -> u64 {
    u64::from(polyphony).saturating_mul(std::mem::size_of::<SamplerVoice>() as u64)
}

/// Immutable sampler program shared by every runtime publication slot.
#[derive(Debug, Clone)]
pub(crate) struct CompiledSamplerProgram {
    mappings: Vec<CompiledMapping>,
    polyphony: usize,
    sample_rate: f64,
    gain: Option<ControlIndex>,
    tune: Option<ControlIndex>,
    pan: Option<ControlIndex>,
    attack: Option<ControlIndex>,
    release: Option<ControlIndex>,
}

#[derive(Debug, Clone)]
struct CompiledMapping {
    id: String,
    source: CompiledSampleSource,
    low_key: u8,
    high_key: u8,
    root_key: u8,
    start: f64,
    end: f64,
    source_frame_scale: f64,
    playback: SamplePlayback,
    gain_db: f32,
    pan: f32,
    attack_seconds: f32,
    release_seconds: f32,
    fade_in: f64,
    fade_out: f64,
    choke_group: Option<u16>,
}

impl CompiledSamplerProgram {
    pub(crate) fn compile(
        instrument: &Instrument,
        library: &CompiledSampleLibrary,
        controls: &CompiledControlProgram,
        sample_rate: u32,
    ) -> Result<Self, EngineError> {
        let sampler = instrument.sampler.as_ref().ok_or_else(|| {
            EngineError::Compile(format!(
                "sampler Instrument `{}` has no mappings",
                instrument.id
            ))
        })?;
        let mut mappings = Vec::with_capacity(sampler.mappings.len());
        let mut choke_groups = BTreeMap::<&str, u16>::new();
        for authored in &sampler.mappings {
            let source = library.get(&authored.asset).ok_or_else(|| {
                EngineError::Compile(format!(
                    "sample mapping `{}` references unavailable asset `{}`",
                    authored.id, authored.asset
                ))
            })?;
            let source_frame_scale =
                f64::from(source.sample_rate()) / f64::from(source.source_sample_rate());
            let start = authored.start_frame as f64 * source_frame_scale;
            let end = authored.end_frame as f64 * source_frame_scale;
            if start >= end || end > source.len() as f64 + 1.0e-9 {
                return Err(EngineError::Compile(format!(
                    "sample mapping `{}` region is outside compiled asset `{}`",
                    authored.id, authored.asset
                )));
            }
            let choke_group = if let Some(group) = authored.choke_group.as_deref() {
                if let Some(index) = choke_groups.get(group) {
                    Some(*index)
                } else {
                    let index = u16::try_from(choke_groups.len()).map_err(|_| {
                        EngineError::Compile("sampler choke-group index exceeds u16".to_owned())
                    })?;
                    choke_groups.insert(group, index);
                    Some(index)
                }
            } else {
                None
            };
            mappings.push(CompiledMapping {
                id: authored.id.clone(),
                source: source.clone(),
                low_key: authored.low_key,
                high_key: authored.high_key,
                root_key: authored.root_key,
                start,
                end,
                source_frame_scale,
                playback: authored.playback,
                gain_db: authored.gain_db as f32,
                pan: authored.pan as f32,
                attack_seconds: authored.attack_seconds as f32,
                release_seconds: authored.release_seconds as f32,
                fade_in: f64::from(authored.fade_in_frames) * source_frame_scale,
                fade_out: f64::from(authored.fade_out_frames) * source_frame_scale,
                choke_group,
            });
        }
        let target = |parameter: &str| {
            controls.index(&ParameterTarget::Instrument {
                instrument: instrument.id.clone(),
                parameter: parameter.to_owned(),
            })
        };
        Ok(Self {
            mappings,
            polyphony: usize::from(instrument.polyphony),
            sample_rate: f64::from(sample_rate),
            gain: target("gain"),
            tune: target("tune"),
            pan: target("pan"),
            attack: target("attack"),
            release: target("release"),
        })
    }

    pub(crate) fn runtime(self: &Arc<Self>) -> SamplerRuntime {
        SamplerRuntime {
            program: self.clone(),
            voices: vec![SamplerVoice::default(); self.polyphony],
            activation_counter: 0,
        }
    }

    pub(crate) fn allocation_bytes(&self) -> u64 {
        let mapping_bytes = (self.mappings.capacity() as u64)
            .saturating_mul(std::mem::size_of::<CompiledMapping>() as u64)
            .saturating_add(
                self.mappings
                    .iter()
                    .map(|mapping| mapping.id.capacity() as u64)
                    .fold(0_u64, u64::saturating_add),
            );
        (std::mem::size_of::<Self>() as u64).saturating_add(mapping_bytes)
    }
}

/// One preallocated sampler Track runtime.
#[derive(Debug, Clone)]
pub(crate) struct SamplerRuntime {
    program: Arc<CompiledSamplerProgram>,
    voices: Vec<SamplerVoice>,
    activation_counter: u64,
}

impl SamplerRuntime {
    #[cfg(test)]
    pub(crate) fn allocated_voice_state_bytes(&self) -> u64 {
        (self.voices.capacity() as u64).saturating_mul(std::mem::size_of::<SamplerVoice>() as u64)
    }

    pub(crate) fn mapping_index(&self, mapping_id: &str) -> Option<u16> {
        self.program
            .mappings
            .iter()
            .position(|mapping| mapping.id == mapping_id)
            .and_then(|index| u16::try_from(index).ok())
    }

    pub(crate) fn mapping_on(&mut self, note_id: u64, mapping_index: u16, velocity: f32) -> bool {
        let mapping_index = usize::from(mapping_index);
        let Some(mapping) = self.program.mappings.get(mapping_index) else {
            return false;
        };
        self.start_voice(
            note_id,
            mapping_index,
            mapping.start,
            mapping.end,
            0.0,
            mapping.gain_db,
            mapping.pan,
            mapping.fade_in,
            mapping.fade_out,
            velocity,
            false,
            ClipEnvelope::TRANSPARENT,
        );
        true
    }

    pub(crate) fn note_on(
        &mut self,
        note_id: u64,
        pitch: u8,
        velocity: f32,
        clip_envelope: ClipEnvelope,
    ) {
        let Some(mapping_index) = self
            .program
            .mappings
            .iter()
            .position(|mapping| (mapping.low_key..=mapping.high_key).contains(&pitch))
        else {
            return;
        };
        let mapping = &self.program.mappings[mapping_index];
        let semitones = f64::from(pitch) - f64::from(mapping.root_key);
        self.start_voice(
            note_id,
            mapping_index,
            mapping.start,
            mapping.end,
            semitones,
            mapping.gain_db,
            mapping.pan,
            mapping.fade_in,
            mapping.fade_out,
            velocity,
            false,
            clip_envelope,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn sample_on(
        &mut self,
        note_id: u64,
        mapping_index: u16,
        start_frame: u64,
        end_frame: u64,
        gain_db: f32,
        pan: f32,
        fade_in_frames: u32,
        fade_out_frames: u32,
        velocity: f32,
        clip_envelope: ClipEnvelope,
    ) {
        let mapping_index = usize::from(mapping_index);
        let Some(mapping) = self.program.mappings.get(mapping_index) else {
            return;
        };
        let scale = mapping.source_frame_scale;
        let start = start_frame as f64 * scale;
        let end = end_frame as f64 * scale;
        if start >= end || end > mapping.source.len() as f64 + 1.0e-9 {
            return;
        }
        self.start_voice(
            note_id,
            mapping_index,
            start,
            end,
            0.0,
            mapping.gain_db + gain_db,
            (mapping.pan + pan).clamp(-1.0, 1.0),
            f64::from(fade_in_frames) * scale,
            f64::from(fade_out_frames) * scale,
            velocity,
            true,
            clip_envelope,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn start_voice(
        &mut self,
        note_id: u64,
        mapping_index: usize,
        start: f64,
        end: f64,
        semitones: f64,
        gain_db: f32,
        pan: f32,
        fade_in: f64,
        fade_out: f64,
        velocity: f32,
        occurrence: bool,
        clip_envelope: ClipEnvelope,
    ) {
        let choke = self.program.mappings[mapping_index].choke_group;
        if let Some(choke) = choke {
            for voice in &mut self.voices {
                if voice.active
                    && self.program.mappings[voice.mapping_index].choke_group == Some(choke)
                {
                    voice.begin_release(CHOKE_FADE_FRAMES);
                }
            }
        }
        let slot = select_voice(&self.voices);
        self.activation_counter = self.activation_counter.saturating_add(1);
        self.voices[slot].activate(
            note_id,
            mapping_index,
            start,
            end,
            semitones,
            gain_db,
            pan,
            fade_in,
            fade_out,
            velocity,
            occurrence,
            self.activation_counter,
            clip_envelope,
        );
    }

    pub(crate) fn note_off(&mut self, note_id: u64) {
        for voice in &mut self.voices {
            if voice.active
                && voice.note_id == note_id
                && self.program.mappings[voice.mapping_index].playback == SamplePlayback::Gated
            {
                voice.release_requested = true;
            }
        }
    }

    pub(crate) fn sample_off(&mut self, note_id: u64) {
        for voice in &mut self.voices {
            if voice.active && voice.note_id == note_id && voice.occurrence {
                voice.release_requested = true;
            }
        }
    }

    pub(crate) fn all_notes_off(&mut self) {
        for voice in &mut self.voices {
            if voice.active {
                voice.begin_release(CHOKE_FADE_FRAMES);
            }
        }
    }

    pub(crate) fn active_voice_count(&self) -> usize {
        self.voices.iter().filter(|voice| voice.active).count()
    }

    pub(crate) fn process(&mut self, controls: &[f64]) -> [f32; 2] {
        let gain_db = self.program.gain.map_or(0.0, |index| controls[index.0]);
        let tune = self.program.tune.map_or(0.0, |index| controls[index.0]);
        let pan_offset = self.program.pan.map_or(0.0, |index| controls[index.0]);
        let attack_override = self.program.attack.map(|index| controls[index.0]);
        let release_override = self.program.release.map(|index| controls[index.0]);
        let mut output = [0.0_f32; 2];
        for voice in &mut self.voices {
            if !voice.active {
                continue;
            }
            let mapping = &self.program.mappings[voice.mapping_index];
            let attack_seconds = attack_override.unwrap_or(f64::from(mapping.attack_seconds));
            let release_seconds = release_override.unwrap_or(f64::from(mapping.release_seconds));
            if voice.release_requested && !voice.releasing {
                voice.begin_release(seconds_to_frames(
                    release_seconds as f32,
                    self.program.sample_rate,
                ));
            }
            let sample = voice.process(
                mapping,
                gain_db as f32,
                tune,
                pan_offset as f32,
                seconds_to_frames(attack_seconds as f32, self.program.sample_rate),
            );
            output[0] += sample[0];
            output[1] += sample[1];
        }
        output
    }
}

#[derive(Debug, Clone)]
struct SamplerVoice {
    active: bool,
    occurrence: bool,
    note_id: u64,
    mapping_index: usize,
    position: f64,
    start: f64,
    end: f64,
    semitones: f64,
    gain_db: f32,
    pan: f32,
    fade_in: f64,
    fade_out: f64,
    velocity: f32,
    age_frames: u64,
    activation_order: u64,
    releasing: bool,
    release_requested: bool,
    release_level: f32,
    release_frames: u32,
    release_cursor: u32,
    residual: [f32; 2],
    residual_frames: u32,
    last_output: [f32; 2],
    clip_envelope: ClipEnvelope,
}

impl Default for SamplerVoice {
    fn default() -> Self {
        Self {
            active: false,
            occurrence: false,
            note_id: 0,
            mapping_index: 0,
            position: 0.0,
            start: 0.0,
            end: 0.0,
            semitones: 0.0,
            gain_db: 0.0,
            pan: 0.0,
            fade_in: 0.0,
            fade_out: 0.0,
            velocity: 0.0,
            age_frames: 0,
            activation_order: 0,
            releasing: false,
            release_requested: false,
            release_level: 1.0,
            release_frames: 0,
            release_cursor: 0,
            residual: [0.0; 2],
            residual_frames: 0,
            last_output: [0.0; 2],
            clip_envelope: ClipEnvelope::TRANSPARENT,
        }
    }
}

impl SamplerVoice {
    #[allow(clippy::too_many_arguments)]
    fn activate(
        &mut self,
        note_id: u64,
        mapping_index: usize,
        start: f64,
        end: f64,
        semitones: f64,
        gain_db: f32,
        pan: f32,
        fade_in: f64,
        fade_out: f64,
        velocity: f32,
        occurrence: bool,
        activation_order: u64,
        clip_envelope: ClipEnvelope,
    ) {
        if self.active {
            self.residual = self.last_output;
            self.residual_frames = STEAL_FADE_FRAMES;
        } else {
            self.residual = [0.0; 2];
            self.residual_frames = 0;
        }
        self.active = true;
        self.occurrence = occurrence;
        self.note_id = note_id;
        self.mapping_index = mapping_index;
        self.position = start;
        self.start = start;
        self.end = end;
        self.semitones = semitones;
        self.gain_db = gain_db;
        self.pan = pan;
        self.fade_in = fade_in;
        self.fade_out = fade_out;
        self.velocity = velocity.clamp(0.0, 1.0);
        self.age_frames = 0;
        self.activation_order = activation_order;
        self.releasing = false;
        self.release_requested = false;
        self.release_level = 1.0;
        self.release_frames = 0;
        self.release_cursor = 0;
        self.last_output = [0.0; 2];
        self.clip_envelope = clip_envelope;
    }

    fn begin_release(&mut self, frames: u32) {
        if self.active && !self.releasing {
            self.releasing = true;
            self.release_requested = false;
            self.release_frames = frames.max(1);
            self.release_cursor = 0;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn process(
        &mut self,
        mapping: &CompiledMapping,
        instrument_gain_db: f32,
        tune: f64,
        instrument_pan: f32,
        attack_frames: u32,
    ) -> [f32; 2] {
        if self.position >= self.end {
            self.active = false;
            self.last_output = [0.0; 2];
            return [0.0; 2];
        }
        let sample = interpolated_frame(mapping.source.frames(), self.position);
        let attack = if attack_frames == 0 {
            1.0
        } else {
            (self.age_frames as f32 / attack_frames as f32).clamp(0.0, 1.0)
        };
        let source_fade_in = if self.fade_in <= f64::EPSILON {
            1.0
        } else {
            ((self.position - self.start) / self.fade_in).clamp(0.0, 1.0) as f32
        };
        let source_fade_out = if self.fade_out <= f64::EPSILON {
            1.0
        } else {
            ((self.end - self.position) / self.fade_out).clamp(0.0, 1.0) as f32
        };
        if self.releasing {
            self.release_cursor = self.release_cursor.saturating_add(1);
            self.release_level =
                (1.0 - self.release_cursor as f32 / self.release_frames.max(1) as f32).max(0.0);
            if self.release_level <= 0.0 {
                self.active = false;
            }
        }
        let gain = 10.0_f32.powf((self.gain_db + instrument_gain_db) / 20.0)
            * self.velocity
            * attack
            * source_fade_in
            * source_fade_out
            * self.release_level
            * self.clip_envelope.gain_at(self.age_frames);
        let pan = (self.pan + instrument_pan).clamp(-1.0, 1.0);
        let angle = (f64::from(pan) + 1.0) * std::f64::consts::FRAC_PI_4;
        let mut output = [
            sample[0] * gain * angle.cos() as f32,
            sample[1] * gain * angle.sin() as f32,
        ];
        if self.residual_frames > 0 {
            let residual_gain = self.residual_frames as f32 / STEAL_FADE_FRAMES as f32;
            output[0] += self.residual[0] * residual_gain;
            output[1] += self.residual[1] * residual_gain;
            self.residual_frames -= 1;
        }
        let increment = 2.0_f64.powf((self.semitones + tune) / 12.0);
        self.position += increment;
        self.age_frames = self.age_frames.saturating_add(1);
        self.last_output = output;
        output
    }
}

fn select_voice(voices: &[SamplerVoice]) -> usize {
    voices
        .iter()
        .position(|voice| !voice.active)
        .or_else(|| {
            voices
                .iter()
                .enumerate()
                .filter(|(_, voice)| voice.releasing)
                .min_by_key(|(_, voice)| voice.activation_order)
                .map(|(index, _)| index)
        })
        .or_else(|| {
            voices
                .iter()
                .enumerate()
                .min_by_key(|(_, voice)| voice.activation_order)
                .map(|(index, _)| index)
        })
        .unwrap_or(0)
}

fn interpolated_frame(frames: &[[f32; 2]], position: f64) -> [f32; 2] {
    let index = position.floor().max(0.0) as usize;
    let fraction = (position - index as f64) as f32;
    let first = frames.get(index).copied().unwrap_or([0.0; 2]);
    let second = frames
        .get(index.saturating_add(1))
        .copied()
        .unwrap_or(first);
    [
        first[0] + (second[0] - first[0]) * fraction,
        first[1] + (second[1] - first[1]) * fraction,
    ]
}

fn seconds_to_frames(seconds: f32, sample_rate: f64) -> u32 {
    (f64::from(seconds.max(0.0)) * sample_rate)
        .round()
        .clamp(0.0, f64::from(u32::MAX)) as u32
}

#[cfg(test)]
mod tests {
    use std::fs;

    use sha2::{Digest, Sha256};
    use tempfile::tempdir;

    use crate::{
        engine::{CompiledProject, ProcessingMode},
        project::{PROJECT_FILE, load_project},
    };

    fn write_sample_project(polyphony: u16) -> crate::project::ProjectBundle {
        let root = tempdir().expect("Project root").keep();
        fs::create_dir(root.join("media")).expect("media directory");
        let wav = root.join("media/source.wav");
        let mut writer = hound::WavWriter::create(
            &wav,
            hound::WavSpec {
                channels: 1,
                sample_rate: 48_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .expect("WAV writer");
        for index in 0..1_024 {
            let sample = if (index / 16) % 2 == 0 {
                16_000
            } else {
                -16_000
            };
            writer.write_sample(sample).expect("WAV sample");
        }
        writer.finalize().expect("finalize WAV");
        let digest = Sha256::digest(fs::read(&wav).expect("WAV bytes"));
        let sha256 = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let source = format!(
            r#"crump 1
project "Sampler Test" {{
  id sampler-test
  seed 17
  sample_rate 48000
  render_tail 10ms
  asset source "Source" wav "media/source.wav" sha256 "{sha256}"
  song {{
    tempo 120bpm
    meter 4/4
    end 1bar
    instrument samples "Samples" sampler {{
      polyphony {polyphony}
      parameter gain "Gain" db -96db 24db 0db 0db
      parameter tune "Tune" semitones -24st 24st 0st 0st
      parameter release "Release" seconds 0s 1s 5ms 5ms
      faceplate {{
        control gain "Gain" "Sampler level" maps gain
        control tune "Tune" "Sampler repitch" maps tune
        control release "Release" "Sampler release" maps release
      }}
      mapping zone asset source keys C4 D4 root C4 {{
        region 0 1024
        playback one_shot
        gain 0db
        pan 0
        envelope 0s 1ms
        fade 4 4
        choke cuts
      }}
    }}
    pattern phrase 1bar {{
      note 0beat C4 1/100beat 1
      note 1/100beat D4 1/100beat 0.8
      sample chop 1beat mapping zone duration 1/100beat start 64 end 768 gain -3db pan 0.25 fade 8 12 velocity 0.75
    }}
    track samples "Samples" instrument samples level 0db pan 0 {{
      clip phrase pattern phrase start 0bar duration 1bar mode once
    }}
  }}
}}
"#
        );
        fs::write(root.join(PROJECT_FILE), source).expect("Project source");
        load_project(&root).expect("load sampler Project")
    }

    fn render_prefix(bundle: &crate::project::ProjectBundle, frames: usize) -> Vec<[f32; 2]> {
        let project = bundle.publishable().expect("publishable sampler Project");
        let compiled = CompiledProject::compile_with_assets(project, &bundle.assets)
            .expect("compile sampler Project");
        let mut runtime = compiled.runtime();
        (0..frames)
            .map(|_| {
                runtime
                    .process_frame(ProcessingMode::Offline)
                    .expect("finite sampler")
            })
            .collect()
    }

    #[test]
    fn mapped_notes_and_durable_sample_occurrences_render_deterministically() {
        let bundle = write_sample_project(2);
        let project = bundle.publishable().expect("sampler Project");
        let compiled = CompiledProject::compile_with_assets(project, &bundle.assets)
            .expect("compile sampler Project");
        assert_eq!(
            compiled
                .mix_graph_inspection()
                .resources
                .sampler_voice_state_bytes,
            super::sampler_voice_state_bytes(2)
        );
        let first = render_prefix(&bundle, 25_000);
        let second = render_prefix(&bundle, 25_000);
        assert_eq!(first, second);
        assert!(
            first[..1_024]
                .iter()
                .any(|frame| frame[0].abs().max(frame[1].abs()) > 0.01)
        );
        assert!(
            first[24_000..24_500]
                .iter()
                .any(|frame| frame[0].abs().max(frame[1].abs()) > 0.01)
        );
        assert!(first.iter().flatten().all(|sample| sample.is_finite()));
    }

    #[test]
    fn sampler_track_identity_covers_exact_asset_content_identity() {
        let bundle = write_sample_project(2);
        let project = bundle.publishable().expect("sampler Project");
        let schedule = crate::engine::schedule::compile_schedules(project)
            .expect("schedule")
            .remove(0);
        let instrument = &project.song.instruments[0];
        let original = crate::engine::track_semantic_hash(&schedule, instrument, project)
            .expect("semantic hash");
        let mut changed = project.clone();
        changed.assets[0].sha256 = "0".repeat(64);
        let changed_hash =
            crate::engine::track_semantic_hash(&schedule, &changed.song.instruments[0], &changed)
                .expect("changed semantic hash");
        assert_ne!(original, changed_hash);
    }

    #[test]
    fn one_shots_ignore_note_off_and_choke_or_steal_with_a_bounded_crossfade() {
        let bundle = write_sample_project(1);
        let output = render_prefix(&bundle, 1_100);
        // The authored Note Off is at frame 240, but one-shot playback continues.
        assert!(
            output[300..500]
                .iter()
                .any(|frame| frame[0].abs().max(frame[1].abs()) > 0.01)
        );
        // A second mapped note deterministically steals/chokes the sole voice.
        assert!(output.iter().flatten().all(|sample| sample.is_finite()));
        assert_eq!(output, render_prefix(&bundle, 1_100));
    }
}
