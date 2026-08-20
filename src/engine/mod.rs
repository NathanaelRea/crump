//! Deterministic audio compilation and runtime shared by playback and rendering.

mod drums;
mod graph;
mod render;
pub(crate) mod schedule;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
#[cfg(any(feature = "pipewire", test))]
use sha2::{Digest, Sha256};

#[cfg(any(feature = "pipewire", test))]
use crate::model::Instrument;
use crate::{
    diagnostic::Diagnostic,
    model::{DrumVoice, InstrumentKind, Project},
    validate::validate_project,
};

use self::{
    drums::DrumSynth,
    graph::{CompiledGraph, GraphVoice},
    schedule::{RuntimeEvent, TrackSchedule, compile_schedules},
};

pub(crate) use render::inspect_render_destination;
pub use render::{RenderManifest, RenderOptions, RenderResult, render_project};

/// Immutable, fully allocated-away-from-callback audio program.
#[derive(Debug, Clone)]
pub struct CompiledProject {
    project_id: String,
    sample_rate: u32,
    arrangement_frames: u64,
    total_frames: u64,
    tracks: Vec<CompiledTrack>,
}

impl CompiledProject {
    /// Validates and compiles one Project for playback or rendering.
    pub fn compile(project: &Project) -> Result<Self, EngineError> {
        let diagnostics = validate_project(project);
        if diagnostics.has_errors() {
            return Err(EngineError::Validation(diagnostics.into_vec()));
        }
        let schedules = compile_schedules(project)?;
        let instruments: BTreeMap<_, _> = project
            .song
            .instruments
            .iter()
            .map(|instrument| (instrument.id.as_str(), instrument))
            .collect();
        let mut tracks = Vec::with_capacity(schedules.len());
        for schedule in schedules {
            let instrument = instruments
                .get(schedule.instrument_id.as_str())
                .ok_or_else(|| {
                    EngineError::Compile(format!("missing instrument `{}`", schedule.instrument_id))
                })?;
            let program = match instrument.kind {
                InstrumentKind::Graph => InstrumentProgram::Graph {
                    graph: CompiledGraph::compile(instrument, project.sample_rate)?,
                    polyphony: usize::from(instrument.polyphony),
                },
                InstrumentKind::StockDrums => InstrumentProgram::Drums {
                    instrument: Box::new((*instrument).clone()),
                },
            };
            #[cfg(any(feature = "pipewire", test))]
            let semantic_hash = track_semantic_hash(&schedule, instrument, project.sample_rate)?;
            tracks.push(CompiledTrack {
                schedule,
                program,
                #[cfg(any(feature = "pipewire", test))]
                semantic_hash,
            });
        }
        let arrangement_frames = project
            .song
            .end
            .to_frames(project.song.tempo_bpm, project.sample_rate)?;
        let tail_frames = seconds_to_frames(project.render_tail_seconds, project.sample_rate)?;
        let total_frames = arrangement_frames
            .checked_add(tail_frames)
            .ok_or_else(|| EngineError::Compile("render range exceeds frame limits".to_owned()))?;
        Ok(Self {
            project_id: project.id.clone(),
            sample_rate: project.sample_rate,
            arrangement_frames,
            total_frames,
            tracks,
        })
    }

    /// Creates a fresh, preallocated runtime at Song time zero.
    #[must_use]
    pub fn runtime(&self) -> SongRuntime {
        SongRuntime {
            project_id: self.project_id.clone(),
            sample_rate: self.sample_rate,
            arrangement_frames: self.arrangement_frames,
            total_frames: self.total_frames,
            frame: 0,
            audition_loop: None,
            tracks: self
                .tracks
                .iter()
                .enumerate()
                .map(|(index, track)| TrackRuntime::new(track, self.sample_rate, index as u64))
                .collect(),
            telemetry: RuntimeTelemetry::default(),
        }
    }

    /// Sample rate fixed by the Project.
    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Exact arrangement frame count before the explicit Render Tail.
    #[must_use]
    pub const fn arrangement_frames(&self) -> u64 {
        self.arrangement_frames
    }

    /// Exact complete Render Range frame count.
    #[must_use]
    pub const fn total_frames(&self) -> u64 {
        self.total_frames
    }
}

#[derive(Debug, Clone)]
struct CompiledTrack {
    schedule: TrackSchedule,
    program: InstrumentProgram,
    #[cfg(any(feature = "pipewire", test))]
    semantic_hash: [u8; 32],
}

#[derive(Debug, Clone)]
enum InstrumentProgram {
    Graph {
        graph: CompiledGraph,
        polyphony: usize,
    },
    Drums {
        instrument: Box<crate::model::Instrument>,
    },
}

#[cfg(any(feature = "pipewire", test))]
fn track_semantic_hash(
    schedule: &TrackSchedule,
    instrument: &Instrument,
    sample_rate: u32,
) -> Result<[u8; 32], EngineError> {
    let mut hasher = Sha256::new();
    hash_bytes(&mut hasher, schedule.track_id.as_bytes());
    hash_bytes(&mut hasher, schedule.instrument_id.as_bytes());
    hasher.update(sample_rate.to_le_bytes());
    hasher.update(schedule.level_db.to_bits().to_le_bytes());
    hasher.update(schedule.pan.to_bits().to_le_bytes());
    hasher.update((schedule.events.len() as u64).to_le_bytes());
    for event in &schedule.events {
        hasher.update(event.frame.to_le_bytes());
        match event.kind {
            RuntimeEvent::NoteOn {
                note_id,
                pitch,
                velocity,
                seed,
            } => {
                hasher.update([0]);
                hasher.update(note_id.to_le_bytes());
                hasher.update([pitch]);
                hasher.update(velocity.to_bits().to_le_bytes());
                hasher.update(seed.to_le_bytes());
            }
            RuntimeEvent::NoteOff { note_id } => {
                hasher.update([1]);
                hasher.update(note_id.to_le_bytes());
            }
            RuntimeEvent::Hit {
                voice,
                velocity,
                seed,
            } => {
                hasher.update([2, voice as u8]);
                hasher.update(velocity.to_bits().to_le_bytes());
                hasher.update(seed.to_le_bytes());
            }
        }
    }
    let parameters = instrument
        .parameters
        .iter()
        .map(|parameter| (&parameter.key, parameter.value))
        .collect::<Vec<_>>();
    let mut audio_program = serde_json::json!({
        "kind": instrument.kind,
        "polyphony": instrument.polyphony,
        "parameters": parameters,
        "graph": instrument.graph,
    });
    strip_source_metadata(&mut audio_program);
    hash_bytes(&mut hasher, &serde_json::to_vec(&audio_program)?);
    Ok(hasher.finalize().into())
}

#[cfg(any(feature = "pipewire", test))]
fn hash_bytes(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

#[cfg(any(feature = "pipewire", test))]
fn strip_source_metadata(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            object.retain(|key, _| key != "location" && key != "site" && !key.ends_with("_site"));
            for child in object.values_mut() {
                strip_source_metadata(child);
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                strip_source_metadata(child);
            }
        }
        _ => {}
    }
}

#[cfg(any(feature = "pipewire", test))]
#[derive(Default)]
struct TransitionFrame {
    common: [f32; 2],
    affected: [f32; 2],
    common_voices: usize,
    affected_voices: usize,
    frame: u64,
}

/// Mutable audio runtime. All vectors and DSP buffers are allocated at creation.
pub struct SongRuntime {
    project_id: String,
    sample_rate: u32,
    arrangement_frames: u64,
    total_frames: u64,
    frame: u64,
    audition_loop: Option<(u64, u64)>,
    tracks: Vec<TrackRuntime>,
    telemetry: RuntimeTelemetry,
}

impl SongRuntime {
    /// Processes one stereo frame.
    pub fn process_frame(&mut self, mode: ProcessingMode) -> Result<[f32; 2], EngineError> {
        if let Some((start, end)) = self.audition_loop
            && self.frame >= end
        {
            self.frame = start;
            for track in &mut self.tracks {
                track.rewind_loop();
            }
        }
        if self.frame >= self.total_frames {
            self.telemetry.current_peak = 0.0;
            self.telemetry.active_voices = 0;
            self.telemetry.frame = self.frame;
            return Ok([0.0, 0.0]);
        }
        let mut master = [0.0_f32, 0.0_f32];
        let mut active_voices = 0;
        for track in &mut self.tracks {
            track.dispatch(self.frame);
            let (sample, voices) = track.process();
            master[0] += sample[0];
            master[1] += sample[1];
            active_voices += voices;
        }
        if !master.into_iter().all(f32::is_finite) {
            let frame = self.frame;
            if mode == ProcessingMode::Monitoring {
                // The realtime caller reports the error and emits silence. Move
                // past the poisoned frame so one bad state cannot stall transport.
                self.telemetry.current_peak = 0.0;
                self.telemetry.active_voices = 0;
                self.telemetry.frame = frame;
                self.frame = self.frame.saturating_add(1);
            }
            return Err(EngineError::NonFinite { frame });
        }
        if mode == ProcessingMode::Monitoring {
            for channel in &mut master {
                *channel = monitor_sample(*channel);
            }
        }
        let peak = master[0].abs().max(master[1].abs());
        self.telemetry.peak = self.telemetry.peak.max(peak);
        self.telemetry.current_peak = peak;
        self.telemetry.active_voices = active_voices;
        self.telemetry.frame = self.frame;
        self.frame += 1;
        Ok(master)
    }

    /// Advances deterministically without retaining output.
    ///
    /// Publication preparation uses this off the audio thread so a replacement
    /// runtime can enter at the current transport frame without callback work.
    pub fn advance_to(&mut self, target_frame: u64) -> Result<(), EngineError> {
        let target = target_frame.min(self.total_frames);
        while self.frame < target {
            let _ = self.process_frame(ProcessingMode::Offline)?;
        }
        Ok(())
    }

    /// Positions a fresh runtime without rendering its prior timeline.
    ///
    /// Realtime publication uses this bounded cursor seek so changed instruments
    /// restart at the target transport frame without reconstructing effect state
    /// on the audio callback or blocking the workspace with a render-from-zero.
    pub(crate) fn seek_transport(&mut self, target_frame: u64) {
        self.frame = target_frame.min(self.total_frames);
        self.telemetry = RuntimeTelemetry {
            frame: self.frame,
            ..RuntimeTelemetry::default()
        };
        for track in &mut self.tracks {
            track.seek_transport(self.frame);
        }
    }

    /// Marks Tracks whose schedule, balance, and Instrument semantics are unchanged.
    ///
    /// Called on the PipeWire thread at a block boundary. It performs only bounded
    /// comparisons over preallocated Track arrays and does not allocate.
    #[cfg(any(feature = "pipewire", test))]
    pub(crate) fn prepare_transition(&mut self, next: &mut Self) {
        for track in &mut self.tracks {
            track.transition_unchanged = false;
        }
        for track in &mut next.tracks {
            track.transition_unchanged = false;
        }

        // Enabling or changing a loop is an explicit transport jump selected by
        // the musician. Every other publication continues at the callback's exact
        // current frame, regardless of how old the latest UI telemetry is.
        let requested_loop_jump =
            next.audition_loop.is_some() && self.audition_loop != next.audition_loop;
        let target_frame = if requested_loop_jump {
            next.frame
        } else {
            self.frame
        };
        next.seek_transport(target_frame);

        if self.sample_rate != next.sample_rate || target_frame != self.frame {
            return;
        }
        for current in &mut self.tracks {
            if let Some(replacement) = next
                .tracks
                .iter_mut()
                .find(|replacement| current.same_semantics(replacement))
            {
                current.transition_unchanged = true;
                replacement.transition_unchanged = true;
            }
        }
    }

    /// Processes one affected-Track transition frame while unchanged Tracks run once at unity.
    #[cfg(any(feature = "pipewire", test))]
    pub(crate) fn process_transition_frame(
        &mut self,
        next: &mut Self,
        old_gain: f32,
        new_gain: f32,
    ) -> Result<[f32; 2], EngineError> {
        let old = self.process_transition_side(true);
        let new = next.process_transition_side(false);
        let mut master = [
            old.common[0] + old.affected[0] * old_gain + new.affected[0] * new_gain,
            old.common[1] + old.affected[1] * old_gain + new.affected[1] * new_gain,
        ];
        let frame = old.frame;
        if !master.into_iter().all(f32::is_finite) {
            self.record_transition_output(frame, [0.0, 0.0], 0);
            next.record_transition_output(new.frame, [0.0, 0.0], 0);
            return Err(EngineError::NonFinite { frame });
        }
        for channel in &mut master {
            *channel = monitor_sample(*channel);
        }
        let active_voices = old
            .common_voices
            .saturating_add(old.affected_voices)
            .saturating_add(new.affected_voices);
        self.record_transition_output(frame, master, active_voices);
        next.record_transition_output(new.frame, master, active_voices);
        Ok(master)
    }

    /// Moves live state for unchanged Tracks into the accepted replacement runtime.
    #[cfg(any(feature = "pipewire", test))]
    pub(crate) fn complete_transition(&mut self, next: &mut Self) {
        for current in &mut self.tracks {
            if !current.transition_unchanged {
                continue;
            }
            if let Some(replacement) = next
                .tracks
                .iter_mut()
                .find(|replacement| current.same_semantics(replacement))
            {
                std::mem::swap(current, replacement);
            }
        }
        for track in &mut self.tracks {
            track.transition_unchanged = false;
        }
        for track in &mut next.tracks {
            track.transition_unchanged = false;
        }
        if let Some((start, _)) = next.audition_loop {
            for track in &mut next.tracks {
                track.prepare_loop(start);
            }
        }
        next.telemetry.active_voices = next
            .tracks
            .iter()
            .map(TrackRuntime::active_voice_count)
            .fold(0_usize, usize::saturating_add);
    }

    #[cfg(any(feature = "pipewire", test))]
    fn process_transition_side(&mut self, process_unchanged: bool) -> TransitionFrame {
        if let Some((start, end)) = self.audition_loop
            && self.frame >= end
        {
            self.frame = start;
            for track in &mut self.tracks {
                track.rewind_loop();
            }
        }
        let frame = self.frame;
        if frame >= self.total_frames {
            return TransitionFrame {
                frame,
                ..TransitionFrame::default()
            };
        }
        let mut output = TransitionFrame {
            frame,
            ..TransitionFrame::default()
        };
        for track in &mut self.tracks {
            if track.transition_unchanged && !process_unchanged {
                continue;
            }
            track.dispatch(frame);
            let (sample, voices) = track.process();
            if track.transition_unchanged {
                output.common[0] += sample[0];
                output.common[1] += sample[1];
                output.common_voices = output.common_voices.saturating_add(voices);
            } else {
                output.affected[0] += sample[0];
                output.affected[1] += sample[1];
                output.affected_voices = output.affected_voices.saturating_add(voices);
            }
        }
        self.frame = self.frame.saturating_add(1);
        output
    }

    #[cfg(any(feature = "pipewire", test))]
    fn record_transition_output(&mut self, frame: u64, output: [f32; 2], active_voices: usize) {
        let peak = output[0].abs().max(output[1].abs());
        self.telemetry.peak = self.telemetry.peak.max(peak);
        self.telemetry.current_peak = peak;
        self.telemetry.active_voices = active_voices;
        self.telemetry.frame = frame;
    }

    /// Enables a non-authoritative transport loop over an arrangement frame range.
    ///
    /// Call this only after advancing a fresh runtime into the requested range.
    /// The loop rewinds schedule cursors and releases active notes without
    /// allocating or clearing effect buffers in the audio callback.
    pub fn enable_audition_loop(&mut self, start: u64, end: u64) -> Result<(), EngineError> {
        if start >= end || end > self.arrangement_frames || self.frame < start || self.frame >= end
        {
            return Err(EngineError::Compile(format!(
                "audition loop [{start}, {end}) does not contain runtime frame {} within arrangement {}",
                self.frame, self.arrangement_frames
            )));
        }
        for track in &mut self.tracks {
            track.prepare_loop(start);
        }
        self.audition_loop = Some((start, end));
        Ok(())
    }

    /// Disables a previously configured non-authoritative audition loop.
    pub fn disable_audition_loop(&mut self) {
        self.audition_loop = None;
    }

    /// Starts a transient audition note on the first Track using an Instrument.
    /// This does not mutate authoritative source or Project History.
    pub fn audition_note_on(
        &mut self,
        instrument_id: &str,
        note_id: u64,
        pitch: u8,
        velocity: f32,
    ) -> bool {
        let Some(track) = self
            .tracks
            .iter_mut()
            .find(|track| track.schedule.instrument_id == instrument_id)
        else {
            return false;
        };
        let seed = note_id ^ (u64::from(pitch) << 32) ^ 0x6a09_e667_f3bc_c909;
        track.instrument.dispatch(RuntimeEvent::NoteOn {
            note_id,
            pitch,
            velocity,
            seed,
        });
        true
    }

    /// Starts a transient audition note on one zero-based Timeline Track.
    pub fn audition_note_on_track(
        &mut self,
        track_index: usize,
        note_id: u64,
        pitch: u8,
        velocity: f32,
    ) -> bool {
        let Some(track) = self.tracks.get_mut(track_index) else {
            return false;
        };
        let seed = note_id ^ (u64::from(pitch) << 32) ^ 0xbb67_ae85_84ca_a73b;
        track.instrument.dispatch(RuntimeEvent::NoteOn {
            note_id,
            pitch,
            velocity,
            seed,
        });
        true
    }

    /// Releases one transient audition note on one zero-based Timeline Track.
    pub fn audition_note_off_track(&mut self, track_index: usize, note_id: u64) -> bool {
        let Some(track) = self.tracks.get_mut(track_index) else {
            return false;
        };
        track.instrument.dispatch(RuntimeEvent::NoteOff { note_id });
        true
    }

    /// Releases one transient audition note on the selected Instrument.
    pub fn audition_note_off(&mut self, instrument_id: &str, note_id: u64) -> bool {
        let Some(track) = self
            .tracks
            .iter_mut()
            .find(|track| track.schedule.instrument_id == instrument_id)
        else {
            return false;
        };
        track.instrument.dispatch(RuntimeEvent::NoteOff { note_id });
        true
    }

    /// Releases every graph voice, used on MIDI disconnect and shutdown.
    pub fn all_notes_off(&mut self) {
        for track in &mut self.tracks {
            track.instrument.all_notes_off();
        }
    }

    /// Fills a preallocated interleaved-neutral stereo block.
    pub fn process_block(
        &mut self,
        output: &mut [[f32; 2]],
        mode: ProcessingMode,
    ) -> Result<usize, EngineError> {
        let available = self.total_frames.saturating_sub(self.frame);
        let count = if self.audition_loop.is_some() {
            output.len()
        } else {
            usize::try_from(available.min(output.len() as u64)).unwrap_or(output.len())
        };
        for frame in &mut output[..count] {
            *frame = self.process_frame(mode)?;
        }
        output[count..].fill([0.0, 0.0]);
        Ok(count)
    }

    /// True after arrangement plus authored tail has been processed.
    #[must_use]
    pub const fn finished(&self) -> bool {
        self.audition_loop.is_none() && self.frame >= self.total_frames
    }

    /// Current absolute frame.
    #[must_use]
    pub const fn frame(&self) -> u64 {
        self.frame
    }

    /// Current engine telemetry.
    #[must_use]
    pub const fn telemetry(&self) -> RuntimeTelemetry {
        self.telemetry
    }

    /// Project identity for runtime reporting.
    #[must_use]
    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    /// Sample rate.
    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Arrangement frame count.
    #[must_use]
    pub const fn arrangement_frames(&self) -> u64 {
        self.arrangement_frames
    }
}

struct TrackRuntime {
    schedule: TrackSchedule,
    #[cfg(any(feature = "pipewire", test))]
    semantic_hash: [u8; 32],
    cursor: usize,
    loop_cursor: usize,
    #[cfg(any(feature = "pipewire", test))]
    transition_unchanged: bool,
    instrument: InstrumentRuntime,
}

impl TrackRuntime {
    fn new(track: &CompiledTrack, sample_rate: u32, seed: u64) -> Self {
        let instrument = match &track.program {
            InstrumentProgram::Graph { graph, polyphony } => InstrumentRuntime::Graph {
                graph: graph.clone(),
                voices: (0..*polyphony)
                    .map(|slot| graph.create_voice(seed ^ slot as u64))
                    .collect(),
            },
            InstrumentProgram::Drums { instrument } => {
                InstrumentRuntime::Drums(DrumSynth::new(instrument, sample_rate))
            }
        };
        Self {
            schedule: track.schedule.clone(),
            #[cfg(any(feature = "pipewire", test))]
            semantic_hash: track.semantic_hash,
            cursor: 0,
            loop_cursor: 0,
            #[cfg(any(feature = "pipewire", test))]
            transition_unchanged: false,
            instrument,
        }
    }

    #[cfg(any(feature = "pipewire", test))]
    fn same_semantics(&self, other: &Self) -> bool {
        self.schedule.track_id == other.schedule.track_id
            && self.semantic_hash == other.semantic_hash
    }

    fn seek_transport(&mut self, frame: u64) {
        self.cursor = self
            .schedule
            .events
            .partition_point(|event| event.frame < frame);
        self.instrument.all_notes_off();
    }

    fn prepare_loop(&mut self, start: u64) {
        self.loop_cursor = self
            .schedule
            .events
            .partition_point(|event| event.frame < start);
    }

    fn rewind_loop(&mut self) {
        self.cursor = self.loop_cursor;
        self.instrument.all_notes_off();
    }

    fn dispatch(&mut self, frame: u64) {
        while self
            .schedule
            .events
            .get(self.cursor)
            .is_some_and(|event| event.frame == frame)
        {
            let event = self.schedule.events[self.cursor].kind.clone();
            self.cursor += 1;
            self.instrument.dispatch(event);
        }
    }

    #[cfg(any(feature = "pipewire", test))]
    fn active_voice_count(&self) -> usize {
        self.instrument.active_voice_count()
    }

    fn process(&mut self) -> ([f32; 2], usize) {
        let (mut sample, active) = self.instrument.process();
        let gain = 10.0_f64.powf(self.schedule.level_db / 20.0) as f32;
        sample[0] *= gain;
        sample[1] *= gain;
        let pan = self.schedule.pan.clamp(-1.0, 1.0);
        if pan > 0.0 {
            sample[0] *= (pan * std::f64::consts::FRAC_PI_2).cos() as f32;
        } else if pan < 0.0 {
            sample[1] *= (-pan * std::f64::consts::FRAC_PI_2).cos() as f32;
        }
        (sample, active)
    }
}

enum InstrumentRuntime {
    Graph {
        graph: CompiledGraph,
        voices: Vec<GraphVoice>,
    },
    Drums(DrumSynth),
}

impl InstrumentRuntime {
    fn dispatch(&mut self, event: RuntimeEvent) {
        match (self, event) {
            (
                Self::Graph { voices, .. },
                RuntimeEvent::NoteOn {
                    note_id,
                    pitch,
                    velocity,
                    seed,
                },
            ) => {
                let slot = select_voice(voices);
                voices[slot].activate(note_id, pitch, velocity, seed);
            }
            (Self::Graph { voices, .. }, RuntimeEvent::NoteOff { note_id }) => {
                for voice in voices {
                    voice.release(note_id);
                }
            }
            (
                Self::Drums(drums),
                RuntimeEvent::Hit {
                    voice,
                    velocity,
                    seed,
                },
            ) => {
                drums.trigger(voice, velocity, seed);
            }
            (
                Self::Drums(drums),
                RuntimeEvent::NoteOn {
                    pitch,
                    velocity,
                    seed,
                    ..
                },
            ) => {
                if let Some(voice) = midi_drum_voice(pitch) {
                    drums.trigger(voice, velocity, seed);
                }
            }
            _ => {}
        }
    }

    fn all_notes_off(&mut self) {
        if let Self::Graph { voices, .. } = self {
            for voice in voices {
                voice.release_all();
            }
        }
    }

    #[cfg(any(feature = "pipewire", test))]
    fn active_voice_count(&self) -> usize {
        match self {
            Self::Graph { voices, .. } => voices.iter().filter(|voice| voice.active()).count(),
            Self::Drums(drums) => drums.active_voice_count(),
        }
    }

    fn process(&mut self) -> ([f32; 2], usize) {
        match self {
            Self::Graph { graph, voices } => {
                let mut output = [0.0_f32, 0.0_f32];
                let mut active = 0;
                for voice in voices {
                    if voice.active() {
                        active += 1;
                        let sample = graph.process(voice);
                        output[0] += sample[0];
                        output[1] += sample[1];
                    }
                }
                (output, active)
            }
            Self::Drums(drums) => (drums.process(), drums.active_voice_count()),
        }
    }
}

fn midi_drum_voice(pitch: u8) -> Option<DrumVoice> {
    match pitch {
        35 | 36 => Some(DrumVoice::Kick),
        37 | 38 | 40 => Some(DrumVoice::Snare),
        39 => Some(DrumVoice::Clap),
        42 | 44 | 46 => Some(DrumVoice::Hat),
        _ => None,
    }
}

fn select_voice(voices: &[GraphVoice]) -> usize {
    voices
        .iter()
        .position(|voice| !voice.active())
        .or_else(|| {
            voices
                .iter()
                .enumerate()
                .filter(|(_, voice)| voice.releasing())
                .max_by_key(|(_, voice)| voice.age_frames())
                .map(|(index, _)| index)
        })
        .or_else(|| {
            voices
                .iter()
                .enumerate()
                .max_by_key(|(_, voice)| voice.age_frames())
                .map(|(index, _)| index)
        })
        .unwrap_or(0)
}

fn monitor_sample(sample: f32) -> f32 {
    if !sample.is_finite() {
        return 0.0;
    }
    let absolute = sample.abs();
    if absolute <= 0.9 {
        sample
    } else {
        sample.signum() * (0.9 + (absolute - 0.9).tanh() * 0.099)
    }
}

fn seconds_to_frames(seconds: f64, sample_rate: u32) -> Result<u64, EngineError> {
    let frames = seconds * f64::from(sample_rate);
    if !frames.is_finite() || frames < 0.0 || frames > u64::MAX as f64 {
        return Err(EngineError::Compile(
            "render tail exceeds frame limits".to_owned(),
        ));
    }
    Ok((frames + 0.5).floor() as u64)
}

/// Whether output passes through the non-authoritative monitor safety stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessingMode {
    /// Bit-stable authored output; non-finite samples fail.
    Offline,
    /// Sanitized and emergency-limited local monitoring output.
    Monitoring,
}

/// Low-cost runtime status suitable for terminal visualization.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct RuntimeTelemetry {
    /// Last processed frame.
    pub frame: u64,
    /// Current absolute stereo peak.
    pub current_peak: f32,
    /// Maximum peak observed since runtime creation.
    pub peak: f32,
    /// Currently active graph and percussion voices.
    pub active_voices: usize,
}

/// Audio compilation and processing failures.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// Project did not pass whole-project validation.
    #[error("Project cannot compile because validation failed")]
    Validation(Vec<Diagnostic>),
    /// Validated model could not compile to bounded runtime state.
    #[error("audio compilation failed: {0}")]
    Compile(String),
    /// Exact musical-time arithmetic failed.
    #[error(transparent)]
    Time(#[from] crate::time::TimeError),
    /// Offline authored output became non-finite.
    #[error("offline render produced a non-finite sample at frame {frame}")]
    NonFinite {
        /// First failing absolute frame.
        frame: u64,
    },
    /// WAV or publication I/O failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// WAV encoding failed.
    #[error(transparent)]
    Wav(#[from] hound::Error),
    /// Manifest serialization failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::dsl::parse_project;

    use super::{CompiledProject, ProcessingMode};

    fn fixture(source: &str) -> crate::model::Project {
        let document = parse_project(Path::new("project.crump"), source);
        assert!(
            !document.diagnostics.has_errors(),
            "{:?}",
            document.diagnostics.as_slice()
        );
        document.project.expect("parsed fixture")
    }

    #[test]
    fn monitoring_stage_is_transparent_below_threshold() {
        assert_eq!(super::monitor_sample(0.5), 0.5);
        assert!(super::monitor_sample(100.0) < 1.0);
        assert_eq!(super::monitor_sample(f32::NAN), 0.0);
    }

    #[test]
    fn monitoring_reports_non_finite_output_and_keeps_transport_moving() {
        let project = fixture(include_str!("../../tests/fixtures/minimal.crump"));
        let compiled = CompiledProject::compile(&project).expect("compile");
        let mut runtime = compiled.runtime();
        runtime.tracks[0].schedule.level_db = f64::NAN;

        let error = runtime
            .process_frame(ProcessingMode::Monitoring)
            .expect_err("non-finite monitoring output");
        assert!(matches!(error, super::EngineError::NonFinite { frame: 0 }));
        assert_eq!(runtime.frame(), 1);
        assert_eq!(runtime.telemetry().current_peak, 0.0);
    }

    #[test]
    fn finished_runtime_clears_current_activity_telemetry() {
        let project = fixture(include_str!("../../tests/fixtures/minimal.crump"));
        let compiled = CompiledProject::compile(&project).expect("compile");
        let mut runtime = compiled.runtime();
        runtime
            .advance_to(compiled.total_frames())
            .expect("advance to end");
        runtime
            .process_frame(ProcessingMode::Monitoring)
            .expect("finished frame");
        assert_eq!(runtime.telemetry().current_peak, 0.0);
        assert_eq!(runtime.telemetry().active_voices, 0);
        assert_eq!(runtime.telemetry().frame, compiled.total_frames());
    }

    #[test]
    fn unchanged_tracks_continue_exactly_across_transition() {
        let project = fixture(include_str!("../../tests/fixtures/minimal.crump"));
        let compiled = CompiledProject::compile(&project).expect("compile");
        let mut baseline = compiled.runtime();
        let mut current = compiled.runtime();
        let mut next = compiled.runtime();
        baseline.advance_to(1_000).expect("advance baseline");
        current.advance_to(1_000).expect("advance current");
        current.prepare_transition(&mut next);
        assert_eq!(next.frame(), 1_000);
        assert!(
            current
                .tracks
                .iter()
                .all(|track| track.transition_unchanged)
        );

        for position in 1..=480_u64 {
            let expected = baseline
                .process_frame(ProcessingMode::Monitoring)
                .expect("baseline frame");
            let phase = position as f64 / 480.0;
            let old_gain = (phase * std::f64::consts::FRAC_PI_2).cos() as f32;
            let new_gain = (phase * std::f64::consts::FRAC_PI_2).sin() as f32;
            let actual = current
                .process_transition_frame(&mut next, old_gain, new_gain)
                .expect("transition frame");
            assert_eq!(actual, expected);
        }
        current.complete_transition(&mut next);
        assert!(next.tracks.iter().all(|track| !track.transition_unchanged));

        for _ in 0..128 {
            assert_eq!(
                next.process_frame(ProcessingMode::Monitoring)
                    .expect("replacement frame"),
                baseline
                    .process_frame(ProcessingMode::Monitoring)
                    .expect("baseline frame")
            );
        }
    }

    #[test]
    fn explicit_loop_enable_seeks_without_preserving_old_track_state() {
        let project = fixture(include_str!("../../tests/fixtures/minimal.crump"));
        let compiled = CompiledProject::compile(&project).expect("compile");
        let mut current = compiled.runtime();
        let mut next = compiled.runtime();
        current.advance_to(1_000).expect("advance current");
        next.seek_transport(100);
        next.enable_audition_loop(100, 500).expect("enable loop");

        current.prepare_transition(&mut next);

        assert_eq!(next.frame(), 100);
        assert!(
            current
                .tracks
                .iter()
                .all(|track| !track.transition_unchanged)
        );
    }

    #[test]
    fn source_location_changes_do_not_restart_tracks() {
        let source = include_str!("../../tests/fixtures/minimal.crump");
        let original = CompiledProject::compile(&fixture(source)).expect("compile original");
        let shifted =
            CompiledProject::compile(&fixture(&format!("# shifted source locations\n{source}")))
                .expect("compile shifted");
        let mut current = original.runtime();
        let mut next = shifted.runtime();
        current.prepare_transition(&mut next);
        assert!(
            current
                .tracks
                .iter()
                .all(|track| track.transition_unchanged)
        );
    }

    #[test]
    fn transition_marks_only_tracks_with_audio_semantic_changes() {
        let project = fixture(include_str!("../../tests/fixtures/minimal.crump"));
        let original = CompiledProject::compile(&project).expect("compile original");

        let mut renamed = project.clone();
        renamed.song.instruments[0].name = "Renamed for display".to_owned();
        renamed.song.instruments[0].faceplate[0].explanation = "Different help text".to_owned();
        let display_only = CompiledProject::compile(&renamed).expect("compile display edit");
        let mut current = original.runtime();
        let mut next = display_only.runtime();
        current.prepare_transition(&mut next);
        assert!(
            current
                .tracks
                .iter()
                .all(|track| track.transition_unchanged)
        );

        let mut changed = project;
        changed.song.instruments[0].parameters[0].value = 4_000.0;
        let changed = CompiledProject::compile(&changed).expect("compile Parameter edit");
        let mut current = original.runtime();
        let mut next = changed.runtime();
        current.prepare_transition(&mut next);
        assert!(
            current
                .tracks
                .iter()
                .find(|track| track.schedule.track_id == "drums")
                .expect("drums Track")
                .transition_unchanged
        );
        assert!(
            !current
                .tracks
                .iter()
                .find(|track| track.schedule.track_id == "lead")
                .expect("lead Track")
                .transition_unchanged
        );
    }
}
