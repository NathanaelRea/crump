//! Disposable, absolute-frame pre-fader playback captured during live preparation.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{self, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    media::AssetInventory,
    model::{ChannelRef, Project, TapPoint},
    routing::{AuthoredTopology, ChannelIdentity},
};

use super::{
    AdmissionProfile, CompiledProject, EngineError, ProcessingMode, bounce, seconds_to_frames,
};

const ALGORITHM: &[u8] = b"crump-track-freeze-stereo-f32-le-v1";
type Key = [u8; 32];
type Audio = Arc<[[f32; 2]]>;
type FrozenTracks = BTreeMap<String, (Key, Audio)>;

pub(super) fn frame_count(project: &Project) -> Result<u64, EngineError> {
    let end = project
        .song
        .end
        .to_frames(project.song.tempo_bpm, project.sample_rate)?;
    let tail = seconds_to_frames(project.render_tail_seconds, project.sample_rate)?;
    end.checked_add(tail)
        .ok_or_else(|| EngineError::Compile("freeze range exceeds frame limits".into()))
}

pub(super) fn prepare(
    project: &Project,
    inventory: &AssetInventory,
    topology: &AuthoredTopology,
    directory: Option<&Path>,
) -> Result<FrozenTracks, EngineError> {
    prepare_cache(project, inventory, topology, directory)
        .map_err(|error| EngineError::Compile(format!("Track Freeze preparation failed: {error}")))
}

struct Capture {
    id: String,
    key: Key,
    path: Option<PathBuf>,
    source: bounce::BounceSource,
    channel: usize,
    audio: Vec<[f32; 2]>,
}

fn prepare_cache(
    project: &Project,
    inventory: &AssetInventory,
    topology: &AuthoredTopology,
    directory: Option<&Path>,
) -> Result<FrozenTracks, EngineError> {
    let mut ready = BTreeMap::new();
    if !project.song.tracks.iter().any(|track| track.frozen) {
        return Ok(ready);
    }
    let mut missing = Vec::new();
    let frames = usize::try_from(frame_count(project)?)
        .map_err(|_| EngineError::Compile("freeze range exceeds host frame limits".into()))?;
    let schedules = super::compile_schedules(project)?;
    let mut tracks = project.song.tracks.iter().collect::<Vec<_>>();
    tracks.sort_by(|left, right| left.id.cmp(&right.id));
    // Stable Track order gives each Track its own slot within the Track limit,
    // including after presentation reordering.
    for (slot, track) in tracks
        .into_iter()
        .enumerate()
        .filter(|(_, track)| track.frozen)
    {
        let key = cache_key(project, topology, &schedules, &track.id)?;
        let path = directory.map(|directory| directory.join(format!("{slot:02x}.pcm")));
        if let Some(audio) = path
            .as_deref()
            .and_then(|path| read_cache(path, key, frames).ok())
        {
            ready.insert(track.id.clone(), (key, audio));
        } else {
            missing.push((track.id.clone(), key, path));
        }
    }
    if missing.is_empty() {
        return Ok(ready);
    }

    let compiled = CompiledProject::compile_for(project, inventory, AdmissionProfile::Offline)?;
    let mut captures = Vec::new();
    for (id, key, path) in missing {
        let owner = ChannelRef::Track(id.clone());
        let channel = compiled
            .mix_graph_inspection()
            .channels
            .iter()
            .find(|channel| channel.identity == owner.to_string())
            .ok_or_else(|| EngineError::Compile(format!("Track `{id}` has no pre-fader tap")))?;
        captures.push(Capture {
            id,
            key,
            path,
            channel: channel.index,
            source: bounce::BounceSource::Channel {
                owner,
                tap: TapPoint::PreFader,
            },
            audio: Vec::with_capacity(frames),
        });
    }
    let mut runtime = compiled.runtime();
    for frame in 0..frames as u64 {
        let master = runtime.process_frame(ProcessingMode::Offline)?;
        for capture in &mut captures {
            let sample =
                bounce::capture_source(&runtime, master, &capture.source, Some(capture.channel))?;
            if !sample.iter().all(|sample| sample.is_finite()) {
                return Err(EngineError::Compile(format!(
                    "Track `{}` produced non-finite audio at frame {frame}",
                    capture.id
                )));
            }
            capture.audio.push(sample);
        }
    }
    for capture in captures {
        if let Some(path) = capture.path {
            write_cache(&path, capture.key, &capture.audio)?;
        }
        ready.insert(capture.id, (capture.key, capture.audio.into()));
    }
    Ok(ready)
}

fn cache_key(
    project: &Project,
    topology: &AuthoredTopology,
    schedules: &[super::TrackSchedule],
    id: &str,
) -> Result<Key, EngineError> {
    let schedule = schedules
        .iter()
        .find(|schedule| schedule.track_id == id)
        .ok_or_else(|| EngineError::Compile(format!("missing freeze schedule for Track `{id}`")))?;
    let instrument = project
        .song
        .instruments
        .iter()
        .find(|instrument| instrument.id == schedule.instrument_id)
        .ok_or_else(|| {
            EngineError::Compile(format!("missing freeze Instrument for Track `{id}`"))
        })?;

    // Followers bridge the signal and control graphs. Conservatively include
    // every detector's input, as well as the Track's transitive audio inputs.
    let mut upstream = BTreeSet::from([ChannelIdentity::Track(id.to_owned())]);
    let mut post_fader = BTreeSet::new();
    for follower in &project.song.envelope_followers {
        let source = ChannelIdentity::from_ref(&follower.source);
        upstream.insert(source.clone());
        if follower.tap == TapPoint::PostFader {
            post_fader.insert(source);
        }
    }
    loop {
        let count = upstream.len();
        for route in topology.routes() {
            if upstream.contains(route.owner()) {
                upstream.insert(route.source().clone());
                if route.tap() == TapPoint::PostFader {
                    post_fader.insert(route.source().clone());
                }
            }
        }
        if count == upstream.len() {
            break;
        }
    }

    let mut semantic = project.clone();
    for track in &mut semantic.song.tracks {
        track.frozen = false;
        track.freeze_site = None;
        if !post_fader.contains(&ChannelIdentity::Track(track.id.clone())) {
            track.channel.level_db = 0.0;
            track.channel.mute = false;
        }
    }
    semantic.song.tracks.sort_by(|a, b| a.id.cmp(&b.id));
    let mut model = serde_json::to_value(&semantic)?;
    strip_source_sites(&mut model);
    if let Some(tracks) = model
        .pointer_mut("/song/tracks")
        .and_then(Value::as_array_mut)
    {
        for (track, authored) in tracks.iter_mut().zip(&semantic.song.tracks) {
            if !post_fader.contains(&ChannelIdentity::Track(authored.id.clone()))
                && let Some(channel) = track.get_mut("channel").and_then(Value::as_object_mut)
            {
                channel.remove("panner");
                channel.remove("mute_site");
            }
        }
    }
    let mut hash = Sha256::new();
    hash.update(ALGORITHM);
    hash.update(build_hash()?);
    hash.update(super::track_semantic_hash(schedule, instrument, project)?);
    hash.update(serde_json::to_vec(&model)?);
    Ok(hash.finalize().into())
}

fn strip_source_sites(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            fields.retain(|_, value| {
                !value.as_object().is_some_and(|object| {
                    object.contains_key("file")
                        && object.contains_key("start")
                        && object.contains_key("end")
                        && (object.contains_key("expected") || object.contains_key("column"))
                })
            });
            fields.values_mut().for_each(strip_source_sites);
        }
        Value::Array(values) => values.iter_mut().for_each(strip_source_sites),
        _ => {}
    }
}

fn build_hash() -> Result<Key, EngineError> {
    static BUILD: OnceLock<Result<Key, String>> = OnceLock::new();
    BUILD
        .get_or_init(|| {
            let hash = || -> io::Result<Key> {
                let mut file = File::open(std::env::current_exe()?)?;
                let mut digest = Sha256::new();
                let mut buffer = [0; 64 * 1024];
                loop {
                    let count = file.read(&mut buffer)?;
                    if count == 0 {
                        break;
                    }
                    digest.update(&buffer[..count]);
                }
                Ok(digest.finalize().into())
            };
            hash().map_err(|error| error.to_string())
        })
        .clone()
        .map_err(|error| EngineError::Compile(format!("cannot hash Crump executable: {error}")))
}

fn read_cache(path: &Path, key: Key, frames: usize) -> io::Result<Audio> {
    let file = File::open(path)?;
    if file.metadata()?.len() != 64 + frames as u64 * 8 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "freeze cache size changed",
        ));
    }
    let mut reader = BufReader::new(file);
    let mut stored_key = [0; 32];
    reader.read_exact(&mut stored_key)?;
    if stored_key != key {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "freeze cache key changed",
        ));
    }
    let mut audio = Vec::with_capacity(frames);
    let mut digest = Sha256::new();
    for _ in 0..frames {
        let mut bytes = [0; 8];
        reader.read_exact(&mut bytes)?;
        digest.update(bytes);
        let (left, right) = bytes.split_at(4);
        audio.push([
            f32::from_le_bytes(left.try_into().expect("four bytes")),
            f32::from_le_bytes(right.try_into().expect("four bytes")),
        ]);
    }
    let mut checksum = [0; 32];
    reader.read_exact(&mut checksum)?;
    if checksum != <Key>::from(digest.finalize()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "freeze cache checksum changed",
        ));
    }
    Ok(audio.into())
}

fn write_cache(path: &Path, key: Key, audio: &[[f32; 2]]) -> io::Result<()> {
    let directory = path.parent().expect("cache has a directory");
    fs::create_dir_all(directory)?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    {
        let mut writer = BufWriter::new(temporary.as_file_mut());
        let mut digest = Sha256::new();
        writer.write_all(&key)?;
        for sample in audio.iter().flatten() {
            let bytes = sample.to_le_bytes();
            digest.update(bytes);
            writer.write_all(&bytes)?;
        }
        writer.write_all(&digest.finalize())?;
        writer.flush()?;
    }
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::load_project;

    const SOURCE: &str = include_str!("../../tests/fixtures/production_mix.crump");

    fn project(source: &str) -> Project {
        let parsed = crate::dsl::parse_project(Path::new("project.crump"), source);
        assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
        parsed.project.unwrap()
    }

    fn key(project: &Project, id: &str) -> Key {
        cache_key(
            project,
            &AuthoredTopology::compile(project),
            &super::super::compile_schedules(project).unwrap(),
            id,
        )
        .unwrap()
    }

    #[test]
    fn freeze_key_depends_on_pre_fader_inputs() {
        let source = SOURCE;
        let baseline = project(source);
        // The Bass fader feeds neither the drums tap nor the follower detector.
        // The drums fader feeds the Bass sidechains and the follower.
        let edited = project(&source.replace("level -6db pan 0", "level -10.5db pan 0"));
        assert_eq!(key(&baseline, "drums"), key(&edited, "drums"));
        let edited =
            project(&source.replace("instrument drums level 0db", "instrument drums level -4db"));
        assert_ne!(key(&baseline, "bass"), key(&edited, "bass"));
        let edited = project(&source.replace("setting width 1", "setting width 0.8"));
        assert_ne!(key(&baseline, "drums"), key(&edited, "drums"));
        let edited = project(&source.replace("render_tail 2s", "render_tail 1s"));
        assert_ne!(key(&baseline, "drums"), key(&edited, "drums"));
    }

    #[test]
    fn frozen_tap_preserves_sends_sidechains_followers_and_transport() {
        let original = project(SOURCE);
        let mut frozen = original.clone();
        frozen
            .song
            .tracks
            .iter_mut()
            .find(|track| track.id == "drums")
            .unwrap()
            .frozen = true;
        let assets = AssetInventory::default();
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join(".crump/freeze");
        let live = CompiledProject::compile_with_cache(
            &frozen,
            &assets,
            AdmissionProfile::Live,
            Some(&cache),
        )
        .unwrap();
        let reference =
            CompiledProject::compile_for(&original, &assets, AdmissionProfile::Offline).unwrap();
        let index = live
            .mix_graph_inspection()
            .channels
            .iter()
            .find(|channel| channel.identity == "track/drums")
            .unwrap()
            .index;
        let resources = &live.mix_graph_inspection().resources;
        assert_eq!(
            resources.frozen_audio_bytes,
            frame_count(&frozen).unwrap() * 8
        );
        assert!(
            resources.processor_state_bytes
                < reference
                    .mix_graph_inspection()
                    .resources
                    .processor_state_bytes
        );
        let mut live = live.runtime();
        let mut reference = reference.runtime();
        let mut samples = Vec::new();
        for frame in 0..frame_count(&frozen).unwrap() {
            let expected = reference.process_frame(ProcessingMode::Offline).unwrap();
            let actual = live.process_frame(ProcessingMode::Offline).unwrap();
            let expected_tap = reference
                .channel_tap_sample(index, TapPoint::PreFader)
                .unwrap();
            assert_eq!(
                live.channel_tap_sample(index, TapPoint::PreFader),
                Some(expected_tap)
            );
            assert_eq!(actual, expected);
            if frame < 100_000 {
                samples.push(expected_tap);
            }
        }
        let first_sound = samples
            .iter()
            .position(|sample| sample[0] != 0.0)
            .expect("fixture produces an audible pre-fader tap");
        live.seek_transport(first_sound as u64);
        live.enable_audition_loop(first_sound as u64, first_sound as u64 + 2)
            .unwrap();
        for frame in [first_sound, first_sound + 1, first_sound, first_sound + 1] {
            live.process_frame(ProcessingMode::Monitoring).unwrap();
            assert_eq!(
                live.channel_tap_sample(index, TapPoint::PreFader),
                Some(samples[frame])
            );
        }
        let path = fs::read_dir(root.path().join(".crump/freeze"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let bytes = fs::read(&path).unwrap();
        fs::write(&path, &bytes[..64]).unwrap();
        CompiledProject::compile_with_cache(&frozen, &assets, AdmissionProfile::Live, Some(&cache))
            .unwrap();
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn offline_render_audio_bytes_ignore_freeze() {
        use super::super::render::{RenderOptions, RenderStemRequest, render_project};

        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("project.crump");
        let source = SOURCE.replace("render_tail 2s", "render_tail 0s");
        for (name, source) in [
            ("original", source.clone()),
            (
                "frozen",
                source.replace(
                    "      processor drum_latency",
                    "      freeze true\n      processor drum_latency",
                ),
            ),
        ] {
            fs::write(&path, source).unwrap();
            let bundle = load_project(root.path()).unwrap();
            let mut options = RenderOptions::new(root.path().join(name));
            options.stems.push(RenderStemRequest {
                owner: ChannelRef::Track("drums".into()),
                tap: TapPoint::PreFader,
            });
            render_project(&bundle, &options).unwrap();
        }
        for file in ["master.wav", "stem-track-drums-pre_fader.wav"] {
            assert_eq!(
                fs::read(root.path().join("original").join(file)).unwrap(),
                fs::read(root.path().join("frozen").join(file)).unwrap(),
            );
        }
    }
}
