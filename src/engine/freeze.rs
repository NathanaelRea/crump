//! Disposable, absolute-frame pre-fader playback for Track Freeze.
//!
//! Live preparation never renders. [`lookup`] finds each frozen Track's audio in memory or in the
//! on-disk cache and returns any misses as a [`FreezeJob`]. The caller runs the job, either
//! straight away or in the background while the missing Tracks play live.
//!
//! A frozen Track's capture depends only on its *scope*: the Track plus every channel that can
//! reach its pre-fader tap through routing or an Envelope Follower, and the Instruments,
//! Patterns, Grooves, and controls those channels use. The cache key hashes that scope, so an
//! edit elsewhere in the Song leaves the cache valid. Each capture renders its scope by itself,
//! so missing Tracks render in parallel and skip unrelated channels, Returns, and the Master
//! chain.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
};

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    media::AssetInventory,
    model::{ChannelRef, ModulationScope, OutputTarget, ParameterTarget, Project, TapPoint},
    routing::{AuthoredTopology, ChannelIdentity, ProcessorOwner},
};

use super::{AdmissionProfile, CompiledProject, EngineError, OfflineThreads, seconds_to_frames};

const ALGORITHM: &[u8] = b"crump-track-freeze-stereo-f32-le-v2";
/// Fingerprint of the sources and toolchain that determine rendered audio. See `build.rs`.
const ENGINE_FINGERPRINT: &str = env!("CRUMP_ENGINE_FINGERPRINT");
const CANCELLED: &str = "Track Freeze capture was superseded";
/// Frames between cancellation checks during a capture.
const CANCEL_CHECK_FRAMES: u64 = 4_096;

type Key = [u8; 32];
type Audio = Arc<[[f32; 2]]>;
pub(super) type FrozenTracks = BTreeMap<String, (Key, Audio)>;

/// Audio verified in this process, by cache slot path.
///
/// Every preparation would otherwise re-read and re-verify hundreds of megabytes. An entry is
/// used only while the slot file on disk still carries the same key, so deleting or replacing the
/// cache invalidates it.
static RESIDENT: Mutex<BTreeMap<PathBuf, (Key, Audio)>> = Mutex::new(BTreeMap::new());

pub(super) fn frame_count(project: &Project) -> Result<u64, EngineError> {
    let end = project
        .song
        .end
        .to_frames(project.song.tempo_bpm, project.sample_rate)?;
    let tail = seconds_to_frames(project.render_tail_seconds, project.sample_rate)?;
    end.checked_add(tail)
        .ok_or_else(|| EngineError::Compile("freeze range exceeds frame limits".into()))
}

/// Frozen Track audio that is ready now, and the work that would produce the rest.
pub(super) struct Lookup {
    pub(super) ready: FrozenTracks,
    pub(super) job: Option<FreezeJob>,
}

/// Finds every frozen Track's audio without rendering any.
pub(super) fn lookup(
    project: &Project,
    inventory: &AssetInventory,
    topology: &AuthoredTopology,
    directory: Option<&Path>,
) -> Result<Lookup, EngineError> {
    lookup_cache(project, inventory, topology, directory)
        .map_err(|error| EngineError::Compile(format!("Track Freeze preparation failed: {error}")))
}

/// One frozen Track whose audio is not cached.
#[derive(Debug, Clone)]
struct Pending {
    id: String,
    key: Key,
    path: Option<PathBuf>,
    /// The Track's scope as a standalone Project.
    scope: Project,
}

/// What plays in place of a frozen Track while its capture is pending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreezeStandIn {
    /// The Track's Instrument and inserts run live.
    Live,
    /// The Track's previous capture plays, or silence when it has none. The Tracks did not fit
    /// the Live budget running live.
    PreviousCapture,
}

/// Captures for frozen Tracks whose cache is missing or stale.
#[derive(Debug, Clone)]
pub struct FreezeJob {
    project: Arc<Project>,
    inventory: AssetInventory,
    frames: usize,
    captures: Vec<Pending>,
    stand_in: FreezeStandIn,
}

impl FreezeJob {
    /// Identity of the exact captures this job produces.
    #[must_use]
    pub fn identity(&self) -> [u8; 32] {
        let mut hash = Sha256::new();
        for capture in &self.captures {
            hash.update(capture.key);
            if let Some(path) = &capture.path {
                hash.update(path.as_os_str().as_encoded_bytes());
            }
            hash.update([0]);
        }
        hash.finalize().into()
    }

    /// Track IDs this job captures.
    pub fn tracks(&self) -> impl Iterator<Item = &str> {
        self.captures.iter().map(|capture| capture.id.as_str())
    }

    /// What the preparation that deferred this job plays in place of its Tracks.
    #[must_use]
    pub const fn stand_in(&self) -> FreezeStandIn {
        self.stand_in
    }

    pub(super) const fn set_stand_in(&mut self, stand_in: FreezeStandIn) {
        self.stand_in = stand_in;
    }

    /// Each capture's previous audio from the same slot, or silence.
    ///
    /// The previous audio carries its own key, so publishing the fresh capture later restarts
    /// the Track instead of treating it as unchanged.
    pub(super) fn previous_captures(&self) -> FrozenTracks {
        let previous = in_parallel(&self.captures, |capture| {
            capture.path.as_deref().and_then(previous_capture)
        });
        self.captures
            .iter()
            .zip(previous)
            .map(|(capture, previous)| {
                let (key, audio) = previous.unwrap_or_else(|| ([0; 32], Arc::from([])));
                (capture.id.clone(), (key, audio))
            })
            .collect()
    }

    /// Renders and caches every capture with up to `threads` threads in total.
    ///
    /// Returns `None` once `cancel` is observed; captures already finished stay cached.
    pub fn run(
        &self,
        threads: usize,
        cancel: &AtomicBool,
    ) -> Result<Option<FrozenTracks>, EngineError> {
        self.run_captures(threads.max(1), cancel)
            .map_err(|error| EngineError::Compile(format!("Track Freeze capture failed: {error}")))
    }

    fn run_captures(
        &self,
        threads: usize,
        cancel: &AtomicBool,
    ) -> Result<Option<FrozenTracks>, EngineError> {
        let count = self.captures.len();
        // Each capture pipelines its sources ahead of its mix on a second thread.
        let concurrency = count.min((threads / 2).max(1));
        let per_capture = (threads / concurrency).max(1);
        let next = AtomicUsize::new(0);
        let outcomes = thread::scope(|scope| {
            let workers = (0..concurrency)
                .map(|_| {
                    scope.spawn(|| {
                        let mut outcomes = Vec::new();
                        loop {
                            let index = next.fetch_add(1, Ordering::Relaxed);
                            if index >= count || cancel.load(Ordering::Relaxed) {
                                break;
                            }
                            let capture = &self.captures[index];
                            // A superseded job may already have captured this Track.
                            if let Some(audio) = capture
                                .path
                                .as_deref()
                                .and_then(|path| load(path, capture.key, self.frames))
                            {
                                outcomes.push((index, Ok(Some(Some(audio)))));
                                continue;
                            }
                            let outcome = match CompiledProject::compile_for(
                                &capture.scope,
                                &self.inventory,
                                AdmissionProfile::Offline,
                            ) {
                                // A scope that does not compile on its own is captured from
                                // the whole Project instead.
                                Err(_) => Ok(None),
                                Ok(compiled) => render_taps(
                                    &compiled,
                                    &[&capture.id],
                                    self.frames,
                                    per_capture,
                                    cancel,
                                )
                                .and_then(|audio| {
                                    audio
                                        .map(|mut audio| capture.store(audio.remove(0)))
                                        .transpose()
                                })
                                .map(Some),
                            };
                            outcomes.push((index, outcome));
                        }
                        outcomes
                    })
                })
                .collect::<Vec<_>>();
            workers
                .into_iter()
                .flat_map(|worker| worker.join().expect("freeze capture worker panicked"))
                .collect::<Vec<_>>()
        });
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }

        let mut captured = vec![None; count];
        let mut whole_project = Vec::new();
        for (index, outcome) in outcomes {
            match outcome? {
                Some(Some(audio)) => captured[index] = Some(audio),
                Some(None) => return Ok(None),
                None => whole_project.push(index),
            }
        }
        if !whole_project.is_empty() {
            let compiled = CompiledProject::compile_for(
                &self.project,
                &self.inventory,
                AdmissionProfile::Offline,
            )?;
            let ids = whole_project
                .iter()
                .map(|&index| self.captures[index].id.as_str())
                .collect::<Vec<_>>();
            let Some(audio) = render_taps(&compiled, &ids, self.frames, threads, cancel)? else {
                return Ok(None);
            };
            for (index, audio) in whole_project.into_iter().zip(audio) {
                captured[index] = Some(self.captures[index].store(audio)?);
            }
        }

        Ok(Some(
            self.captures
                .iter()
                .zip(captured)
                .map(|(capture, audio)| {
                    let audio = audio.expect("every capture rendered");
                    (capture.id.clone(), (capture.key, audio))
                })
                .collect(),
        ))
    }
}

impl Pending {
    /// Caches one finished capture.
    fn store(&self, audio: Vec<[f32; 2]>) -> Result<Audio, EngineError> {
        let audio: Audio = audio.into();
        if let Some(path) = &self.path {
            write_cache(path, self.key, &audio)?;
            remember(path, self.key, &audio);
        }
        Ok(audio)
    }
}

/// Renders the pre-fader taps of `tracks` from Song zero, or `None` once `cancel` is observed.
fn render_taps(
    compiled: &CompiledProject,
    tracks: &[&str],
    frames: usize,
    threads: usize,
    cancel: &AtomicBool,
) -> Result<Option<Vec<Vec<[f32; 2]>>>, EngineError> {
    let channels = tracks
        .iter()
        .map(|id| {
            let owner = ChannelRef::Track((*id).to_owned()).to_string();
            compiled
                .mix_graph_inspection()
                .channels
                .iter()
                .find(|channel| channel.identity == owner)
                .map(|channel| channel.index)
                .ok_or_else(|| EngineError::Compile(format!("Track `{id}` has no pre-fader tap")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut audio = tracks
        .iter()
        .map(|_| Vec::with_capacity(frames))
        .collect::<Vec<_>>();
    let mut runtime = compiled.runtime();
    let mut frame = 0_u64;
    let result = runtime.process_offline(OfflineThreads::at_most(threads), |runtime, _| {
        if frame.is_multiple_of(CANCEL_CHECK_FRAMES) && cancel.load(Ordering::Relaxed) {
            return Err(EngineError::Compile(CANCELLED.to_owned()));
        }
        for ((&channel, audio), id) in channels.iter().zip(&mut audio).zip(tracks) {
            let sample = runtime
                .channel_tap_sample(channel, TapPoint::PreFader)
                .ok_or_else(|| {
                    EngineError::Compile(format!("Track `{id}` has no pre-fader tap"))
                })?;
            if !sample.iter().all(|sample| sample.is_finite()) {
                return Err(EngineError::Compile(format!(
                    "Track `{id}` produced non-finite audio at frame {frame}"
                )));
            }
            audio.push(sample);
        }
        frame += 1;
        Ok(())
    });
    match result {
        Err(_) if cancel.load(Ordering::Relaxed) => return Ok(None),
        Err(error) => return Err(error),
        Ok(()) => {}
    }
    if audio.iter().any(|audio| audio.len() != frames) {
        return Err(EngineError::Compile(format!(
            "freeze capture produced {frame} of {frames} frames"
        )));
    }
    Ok(Some(audio))
}

fn lookup_cache(
    project: &Project,
    inventory: &AssetInventory,
    topology: &AuthoredTopology,
    directory: Option<&Path>,
) -> Result<Lookup, EngineError> {
    let mut ready = BTreeMap::new();
    if !project.song.tracks.iter().any(|track| track.frozen) {
        return Ok(Lookup { ready, job: None });
    }
    let frames = usize::try_from(frame_count(project)?)
        .map_err(|_| EngineError::Compile("freeze range exceeds host frame limits".into()))?;
    let schedules = super::compile_schedules(project)?;
    let mut tracks = project.song.tracks.iter().collect::<Vec<_>>();
    tracks.sort_by(|left, right| left.id.cmp(&right.id));
    // Stable Track order gives each Track its own slot within the Track limit,
    // including after presentation reordering.
    let mut wanted = Vec::new();
    for (slot, track) in tracks
        .into_iter()
        .enumerate()
        .filter(|(_, track)| track.frozen)
    {
        let scope = Scope::new(project, topology, &track.id);
        let key = cache_key(project, &scope, &schedules, &track.id)?;
        wanted.push(Pending {
            id: track.id.clone(),
            key,
            path: directory.map(|directory| directory.join(format!("{slot:02x}.pcm"))),
            scope: scope.project,
        });
    }
    let loaded = in_parallel(&wanted, |pending| {
        pending
            .path
            .as_deref()
            .and_then(|path| load(path, pending.key, frames))
    });
    let mut missing = Vec::new();
    for (pending, audio) in wanted.into_iter().zip(loaded) {
        match audio {
            Some(audio) => {
                ready.insert(pending.id, (pending.key, audio));
            }
            None => missing.push(pending),
        }
    }
    let job = (!missing.is_empty()).then(|| FreezeJob {
        project: Arc::new(project.clone()),
        inventory: inventory.clone(),
        frames,
        captures: missing,
        stand_in: FreezeStandIn::Live,
    });
    Ok(Lookup { ready, job })
}

/// Everything that can reach one Track's pre-fader tap, as a standalone Project.
struct Scope {
    project: Project,
    /// Channels whose fader, panner, or Mute can reach the tap.
    post_fader: BTreeSet<ChannelIdentity>,
}

impl Scope {
    fn new(project: &Project, topology: &AuthoredTopology, id: &str) -> Self {
        let song = &project.song;
        let mut upstream = BTreeSet::from([ChannelIdentity::Track(id.to_owned())]);
        let mut post_fader = BTreeSet::new();
        // Routes reach a channel's convergence point. An Envelope Follower reaches whatever
        // its target belongs to, one sample later.
        loop {
            let count = (upstream.len(), post_fader.len());
            for route in topology.routes() {
                if upstream.contains(route.owner()) {
                    upstream.insert(route.source().clone());
                    if route.tap() == TapPoint::PostFader {
                        post_fader.insert(route.source().clone());
                    }
                }
            }
            let followed = {
                let kept = Kept::new(project, topology, &upstream, &post_fader);
                song.envelope_followers
                    .iter()
                    .filter(|follower| kept.target(&follower.target))
                    .map(|follower| (ChannelIdentity::from_ref(&follower.source), follower.tap))
                    .collect::<Vec<_>>()
            };
            for (source, tap) in followed {
                if tap == TapPoint::PostFader {
                    post_fader.insert(source.clone());
                }
                upstream.insert(source);
            }
            if count == (upstream.len(), post_fader.len()) {
                break;
            }
        }
        let kept = Kept::new(project, topology, &upstream, &post_fader);

        let mut scope = project.clone();
        scope.name.clear();
        let song = &mut scope.song;
        song.tracks
            .retain(|track| upstream.contains(&ChannelIdentity::Track(track.id.clone())));
        song.groups
            .retain(|group| upstream.contains(&ChannelIdentity::Group(group.id.clone())));
        song.returns
            .retain(|r#return| upstream.contains(&ChannelIdentity::Return(r#return.id.clone())));
        for track in &mut song.tracks {
            track.frozen = false;
            track.freeze_site = None;
        }
        let strips = song
            .tracks
            .iter_mut()
            .map(|track| (ChannelIdentity::Track(track.id.clone()), &mut track.channel))
            .chain(
                song.groups
                    .iter_mut()
                    .map(|group| (ChannelIdentity::Group(group.id.clone()), &mut group.channel)),
            )
            .chain(song.returns.iter_mut().map(|r#return| {
                (
                    ChannelIdentity::Return(r#return.id.clone()),
                    &mut r#return.channel,
                )
            }));
        for (identity, channel) in strips {
            // Outputs and Sends into channels outside the scope cannot reach the tap.
            channel
                .sends
                .retain(|send| kept.sends.contains(send.id.as_str()));
            if !upstream.contains(&ChannelIdentity::from_output(&channel.output)) {
                channel.output = OutputTarget::Master;
            }
            if !post_fader.contains(&identity) {
                channel.level_db = 0.0;
                channel.mute = false;
            }
        }
        if let Some(master) = &mut song.master {
            master.inserts.clear();
            master.level_db = 0.0;
        }
        song.sidechains.retain(|route| {
            kept.processors.contains(route.processor.as_str())
                && upstream.contains(&ChannelIdentity::from_ref(&route.from))
        });
        song.automations.retain(|lane| kept.target(&lane.target));
        song.modulations.retain(|route| {
            kept.target(&route.target)
                && match &route.scope {
                    ModulationScope::Global => true,
                    ModulationScope::Instrument(id) | ModulationScope::PerVoice(id) => {
                        kept.instruments.contains(id.as_str())
                    }
                }
        });
        song.envelope_followers.retain(|follower| {
            kept.target(&follower.target)
                && upstream.contains(&ChannelIdentity::from_ref(&follower.source))
        });
        song.instruments
            .retain(|instrument| kept.instruments.contains(instrument.id.as_str()));
        song.patterns
            .retain(|pattern| kept.patterns.contains(pattern.id.as_str()));
        song.grooves
            .retain(|groove| kept.grooves.contains(groove.id.as_str()));
        song.locators.clear();
        Self {
            project: scope,
            post_fader,
        }
    }
}

/// Model identities inside one scope.
struct Kept<'a> {
    post_fader: &'a BTreeSet<ChannelIdentity>,
    instruments: BTreeSet<&'a str>,
    processors: BTreeSet<&'a str>,
    sends: BTreeSet<&'a str>,
    patterns: BTreeSet<&'a str>,
    grooves: BTreeSet<&'a str>,
}

impl<'a> Kept<'a> {
    fn new(
        project: &'a Project,
        topology: &'a AuthoredTopology,
        upstream: &'a BTreeSet<ChannelIdentity>,
        post_fader: &'a BTreeSet<ChannelIdentity>,
    ) -> Self {
        let song = &project.song;
        let tracks = song
            .tracks
            .iter()
            .filter(|track| upstream.contains(&ChannelIdentity::Track(track.id.clone())))
            .collect::<Vec<_>>();
        let instruments = tracks
            .iter()
            .map(|track| track.instrument.as_str())
            .collect::<BTreeSet<_>>();
        let processors = topology
            .processors()
            .iter()
            .filter(|processor| match processor.owner() {
                ProcessorOwner::Instrument { instrument_id, .. } => {
                    instruments.contains(instrument_id.as_str())
                }
                ProcessorOwner::Channel { channel, .. } => upstream.contains(channel),
            })
            .map(|processor| processor.id())
            .collect();
        let strips = tracks
            .iter()
            .map(|track| &track.channel)
            .chain(
                song.groups
                    .iter()
                    .filter(|group| upstream.contains(&ChannelIdentity::Group(group.id.clone())))
                    .map(|group| &group.channel),
            )
            .chain(
                song.returns
                    .iter()
                    .filter(|r#return| {
                        upstream.contains(&ChannelIdentity::Return(r#return.id.clone()))
                    })
                    .map(|r#return| &r#return.channel),
            );
        let sends = strips
            .flat_map(|channel| &channel.sends)
            .filter(|send| upstream.contains(&ChannelIdentity::Return(send.target.clone())))
            .map(|send| send.id.as_str())
            .collect();
        let clips = tracks.iter().flat_map(|track| &track.clips);
        let grooves = clips
            .clone()
            .filter_map(|clip| clip.groove.as_deref())
            .collect();
        // A Pattern Variation derives its events from its source Pattern.
        let mut patterns = clips
            .map(|clip| clip.pattern.as_str())
            .collect::<BTreeSet<_>>();
        loop {
            let count = patterns.len();
            for pattern in &song.patterns {
                if patterns.contains(pattern.id.as_str())
                    && let Some(variation) = &pattern.variation
                {
                    patterns.insert(variation.source.as_str());
                }
            }
            if count == patterns.len() {
                break;
            }
        }
        Self {
            post_fader,
            instruments,
            processors,
            sends,
            patterns,
            grooves,
        }
    }

    /// Whether a control on `target` can change the scope's tap.
    fn target(&self, target: &ParameterTarget) -> bool {
        // Channel controls act at the fader, after every channel's pre-fader tap.
        let fader = |identity: ChannelIdentity| self.post_fader.contains(&identity);
        match target {
            ParameterTarget::Instrument { instrument, .. } => {
                self.instruments.contains(instrument.as_str())
            }
            ParameterTarget::Track { track, .. } => fader(ChannelIdentity::Track(track.clone())),
            ParameterTarget::Group { group, .. } => fader(ChannelIdentity::Group(group.clone())),
            ParameterTarget::Return { r#return, .. } => {
                fader(ChannelIdentity::Return(r#return.clone()))
            }
            ParameterTarget::Send { send } => self.sends.contains(send.as_str()),
            ParameterTarget::Processor { processor, .. } => {
                self.processors.contains(processor.as_str())
            }
            ParameterTarget::Master => false,
        }
    }
}

fn cache_key(
    project: &Project,
    scope: &Scope,
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

    let mut semantic = scope.project.clone();
    semantic.song.tracks.sort_by(|a, b| a.id.cmp(&b.id));
    let mut model = serde_json::to_value(&semantic)?;
    strip_source_sites(&mut model);
    for (pointer, identity) in [
        (
            "/song/tracks",
            ChannelIdentity::Track as fn(String) -> ChannelIdentity,
        ),
        ("/song/groups", ChannelIdentity::Group),
        ("/song/returns", ChannelIdentity::Return),
    ] {
        for channel in model
            .pointer_mut(pointer)
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
            .filter_map(Value::as_object_mut)
        {
            // Names and presentation never reach audio.
            channel.remove("name");
            let id = channel
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !scope.post_fader.contains(&identity(id.to_owned()))
                && let Some(strip) = channel.get_mut("channel").and_then(Value::as_object_mut)
            {
                strip.remove("panner");
                strip.remove("mute_site");
            }
        }
    }
    for instrument in model
        .pointer_mut("/song/instruments")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object_mut)
    {
        instrument.remove("name");
        instrument.remove("presentation");
    }
    let mut hash = Sha256::new();
    hash.update(ALGORITHM);
    hash.update(ENGINE_FINGERPRINT.as_bytes());
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

fn file_len(frames: usize) -> u64 {
    64 + frames as u64 * 8
}

/// Audio for one slot, from memory when the slot file still carries `key`, else from disk.
fn load(path: &Path, key: Key, frames: usize) -> Option<Audio> {
    let resident = RESIDENT
        .lock()
        .ok()
        .and_then(|resident| resident.get(path).cloned())
        .filter(|(resident, audio)| *resident == key && audio.len() == frames);
    if let Some((_, audio)) = resident
        && header_matches(path, key, frames).unwrap_or(false)
    {
        return Some(audio);
    }
    let audio = read_cache(path, key, frames).ok()?;
    remember(path, key, &audio);
    Some(audio)
}

/// Slots are independent files, so they load side by side.
fn in_parallel<T: Sync, R: Send>(
    items: &[T],
    load: impl Fn(&T) -> Option<R> + Sync,
) -> Vec<Option<R>> {
    thread::scope(|scope| {
        let loads = items
            .iter()
            .map(|item| scope.spawn(|| load(item)))
            .collect::<Vec<_>>();
        loads
            .into_iter()
            .map(|load| load.join().unwrap_or(None))
            .collect()
    })
}

/// Whatever verified capture a slot holds, whatever its key and length.
fn previous_capture(path: &Path) -> Option<(Key, Audio)> {
    if let Some(resident) = RESIDENT
        .lock()
        .ok()
        .and_then(|resident| resident.get(path).cloned())
    {
        return Some(resident);
    }
    let len = fs::metadata(path).ok()?.len();
    let frames = usize::try_from(len.checked_sub(64)? / 8).ok()?;
    let mut key = [0; 32];
    File::open(path).ok()?.read_exact(&mut key).ok()?;
    let audio = read_cache(path, key, frames).ok()?;
    remember(path, key, &audio);
    Some((key, audio))
}

fn remember(path: &Path, key: Key, audio: &Audio) {
    if let Ok(mut resident) = RESIDENT.lock() {
        resident.insert(path.to_owned(), (key, Arc::clone(audio)));
    }
}

fn header_matches(path: &Path, key: Key, frames: usize) -> io::Result<bool> {
    let mut file = File::open(path)?;
    if file.metadata()?.len() != file_len(frames) {
        return Ok(false);
    }
    let mut stored = [0; 32];
    file.read_exact(&mut stored)?;
    Ok(stored == key)
}

fn read_cache(path: &Path, key: Key, frames: usize) -> io::Result<Audio> {
    let invalid = |message| io::Error::new(io::ErrorKind::InvalidData, message);
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    if len != file_len(frames) {
        return Err(invalid("freeze cache size changed"));
    }
    let mut stored = [0; 32];
    file.read_exact(&mut stored)?;
    if stored != key {
        return Err(invalid("freeze cache key changed"));
    }
    let mut bytes = Vec::with_capacity(len as usize - 32);
    file.read_to_end(&mut bytes)?;
    let (payload, checksum) = bytes.split_at(frames * 8);
    if checksum != Sha256::digest(payload).as_slice() {
        return Err(invalid("freeze cache checksum changed"));
    }
    Ok(payload
        .as_chunks::<4>()
        .0
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[left, right]| [f32::from_le_bytes(*left), f32::from_le_bytes(*right)])
        .collect())
}

fn write_cache(path: &Path, key: Key, audio: &[[f32; 2]]) -> io::Result<()> {
    let directory = path.parent().expect("cache has a directory");
    fs::create_dir_all(directory)?;
    let mut bytes = Vec::with_capacity(file_len(audio.len()) as usize);
    bytes.extend_from_slice(&key);
    for sample in audio.iter().flatten() {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    let checksum = Sha256::digest(&bytes[32..]);
    bytes.extend_from_slice(&checksum);
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.as_file_mut().write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::super::{FreezePolicy, FreezeStandIn, ProcessingMode};
    use super::*;
    use crate::project::load_project;

    const SOURCE: &str = include_str!("../../tests/fixtures/production_mix.crump");

    fn project(source: &str) -> Project {
        let parsed = crate::dsl::parse_project(Path::new("project.crump"), source);
        assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
        parsed.project.unwrap()
    }

    fn key(project: &Project, id: &str) -> Key {
        let topology = AuthoredTopology::compile(project);
        cache_key(
            project,
            &Scope::new(project, &topology, id),
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

    /// Pre-fader taps of `tracks` over the first `frames` frames.
    fn taps(
        project: &Project,
        assets: &AssetInventory,
        tracks: &[&str],
        frames: u64,
    ) -> Vec<Vec<[f32; 2]>> {
        let compiled = CompiledProject::compile_for(project, assets, AdmissionProfile::Offline)
            .unwrap_or_else(|error| panic!("{tracks:?} scope compiles: {error}"));
        let channels = tracks
            .iter()
            .map(|id| {
                compiled
                    .mix_graph_inspection()
                    .channels
                    .iter()
                    .find(|channel| channel.identity == format!("track/{id}"))
                    .unwrap()
                    .index
            })
            .collect::<Vec<_>>();
        let mut runtime = compiled.runtime();
        runtime
            .set_audition_range(0, frames.min(compiled.arrangement_frames))
            .unwrap();
        let mut audio = vec![Vec::new(); tracks.len()];
        runtime
            .process_offline(OfflineThreads::at_most(3), |runtime, _| {
                for (&channel, audio) in channels.iter().zip(&mut audio) {
                    audio.push(
                        runtime
                            .channel_tap_sample(channel, TapPoint::PreFader)
                            .unwrap(),
                    );
                }
                Ok(())
            })
            .unwrap();
        audio
    }

    #[test]
    fn every_track_scope_captures_the_whole_project_tap() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/");
        let mut projects = [
            "tests/fixtures/compressor_sidechain.crump",
            "tests/fixtures/instrument_modulation.crump",
            "tests/fixtures/shared_graph_foundation.crump",
        ]
        .into_iter()
        .map(|path| {
            let source = fs::read_to_string(format!("{root}{path}")).unwrap();
            (path, project(&source), AssetInventory::default())
        })
        .collect::<Vec<_>>();
        projects.push(("production_mix", project(SOURCE), AssetInventory::default()));
        for path in [
            "fixtures/deep-house-reference",
            "fixtures/production",
            "fixtures/reference",
        ] {
            let bundle = load_project(format!("{root}{path}")).unwrap();
            projects.push((path, bundle.project.unwrap(), bundle.assets));
        }
        for (path, project, assets) in projects {
            let topology = AuthoredTopology::compile(&project);
            let frames = u64::from(project.sample_rate) / 4;
            let ids = project
                .song
                .tracks
                .iter()
                .map(|track| track.id.as_str())
                .collect::<Vec<_>>();
            let whole = taps(&project, &assets, &ids, frames);
            for (id, expected) in ids.iter().zip(whole) {
                let scope = Scope::new(&project, &topology, id);
                let actual = taps(&scope.project, &assets, &[id], frames).remove(0);
                assert!(
                    actual == expected,
                    "{path}: Track `{id}` scope changed its tap"
                );
            }
        }
    }

    #[test]
    fn deferred_preparation_returns_the_capture_and_offers_the_previous_one() {
        let frozen = project(&SOURCE.replace(
            "      processor drum_latency",
            "      freeze true\n      processor drum_latency",
        ));
        let assets = AssetInventory::default();
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join(".crump/freeze");
        let compile = |project: &Project, policy| {
            CompiledProject::compile_with_cache(
                project,
                &assets,
                AdmissionProfile::Live,
                Some(&cache),
                policy,
            )
            .unwrap()
        };

        // Nothing is cached: the Track runs live and the capture comes back as pending work.
        let mut deferred = compile(&frozen, FreezePolicy::Defer);
        let job = deferred.take_pending_freeze().expect("pending capture");
        assert_eq!(job.stand_in(), FreezeStandIn::Live);
        assert_eq!(job.tracks().collect::<Vec<_>>(), ["drums"]);
        assert!(fs::read_dir(&cache).is_err(), "deferral renders nothing");
        assert_eq!(
            deferred.mix_graph_inspection().resources.frozen_audio_bytes,
            0
        );
        let (_, captured) = job
            .run(4, &AtomicBool::new(false))
            .unwrap()
            .unwrap()
            .remove("drums")
            .unwrap();
        assert!(
            compile(&frozen, FreezePolicy::Defer)
                .take_pending_freeze()
                .is_none()
        );

        // An edit to the drums makes that capture the previous one.
        let edited = project(&frozen_source_with_drum_width());
        let mut deferred = compile(&edited, FreezePolicy::Defer);
        let job = deferred
            .take_pending_freeze()
            .expect("edit invalidates the capture");
        let previous = job.previous_captures().remove("drums").unwrap();
        assert_eq!(previous.1, captured);

        // A cancelled capture writes nothing and reports that it stopped.
        assert!(job.run(4, &AtomicBool::new(true)).unwrap().is_none());
        assert_eq!(job.previous_captures().remove("drums").unwrap().1, captured);
    }

    fn frozen_source_with_drum_width() -> String {
        SOURCE
            .replace(
                "      processor drum_latency",
                "      freeze true\n      processor drum_latency",
            )
            .replace("setting width 1", "setting width 0.8")
    }

    #[test]
    fn freeze_key_ignores_channels_outside_the_scope() {
        let baseline = project(SOURCE);
        // Bass and its Room Send cannot reach the drums tap; the drums fader reaches Bass.
        let edited = project(&SOURCE.replace(
            "send bass_room target return/room tap post level -18db",
            "send bass_room target return/room tap post level -12db",
        ));
        assert_eq!(key(&baseline, "drums"), key(&edited, "drums"));
        let edited = project(&SOURCE.replace(
            "track bass \"Bass Sidechain Probe\"",
            "track bass \"Renamed\"",
        ));
        assert_eq!(key(&baseline, "drums"), key(&edited, "drums"));
        assert_eq!(key(&baseline, "bass"), key(&edited, "bass"));
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
            FreezePolicy::Wait,
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
        CompiledProject::compile_with_cache(
            &frozen,
            &assets,
            AdmissionProfile::Live,
            Some(&cache),
            FreezePolicy::Wait,
        )
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
