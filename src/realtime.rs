//! Realtime playback with a bounded lock-free command seam and two audio drivers.
//!
//! Compilation, source parsing, graph construction, allocation, persistence,
//! and revision checks happen before this module receives a runtime. The
//! process callback only consumes fixed-capacity SPSC rings, executes already
//! allocated DSP state, and writes output buffers.
//!
//! A [`Player`] owns the controller ends of those rings. Its driver decides who
//! runs the callback: the native PipeWire stream thread, or an in-memory driver
//! whose owner steps the identical callback explicitly. Publication, pause,
//! audition commands, reclamation, and telemetry behave the same under both.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering},
};

use rtrb::{Consumer, Producer, PushError, RingBuffer};
use serde::{Deserialize, Serialize};

use crate::engine::{
    PUBLICATION_TRANSITION_MILLISECONDS, ProcessingMode, RuntimeTelemetry, SongRuntime,
};

const MAX_AUDITION_CHANNEL_IDENTITY_BYTES: usize = 256;
const COMMAND_CAPACITY: usize = 32;
const RECLAIM_CAPACITY: usize = 8;
const TELEMETRY_CAPACITY: usize = 64;
const MIDI_CAPACITY: usize = 128;
const FRAME_BYTES: usize = std::mem::size_of::<f32>() * 2;
/// Longest the callback may run back-to-back, without ever finishing a buffer early enough to
/// sleep, before playback latches into silence.
///
/// PipeWire raises its data thread to realtime priority with `RLIMIT_RTTIME` (200 ms by
/// default), and the kernel SIGKILLs the whole process when a realtime thread runs that long
/// without blocking. Half that limit leaves margin for PipeWire's own work in the same cycle.
const OVERLOAD_RUN_LIMIT: std::time::Duration = std::time::Duration::from_millis(100);
/// Frames processed between mid-buffer overload checks.
const OVERLOAD_CHECK_FRAMES: usize = 32;

/// Fixed-capacity channel identity carried across the callback command seam.
#[derive(Debug, Clone, Copy)]
struct AuditionChannelIdentity {
    bytes: [u8; MAX_AUDITION_CHANNEL_IDENTITY_BYTES],
    length: u16,
}

impl AuditionChannelIdentity {
    fn new(identity: &str) -> Result<Self, RealtimeError> {
        let bytes = identity.as_bytes();
        if bytes.len() > MAX_AUDITION_CHANNEL_IDENTITY_BYTES {
            return Err(RealtimeError::ChannelIdentityTooLong {
                length: bytes.len(),
                maximum: MAX_AUDITION_CHANNEL_IDENTITY_BYTES,
            });
        }
        let mut fixed = [0_u8; MAX_AUDITION_CHANNEL_IDENTITY_BYTES];
        fixed[..bytes.len()].copy_from_slice(bytes);
        Ok(Self {
            bytes: fixed,
            length: bytes.len() as u16,
        })
    }

    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..usize::from(self.length)]).unwrap_or("")
    }
}

/// One callback-safe transient audition operation.
#[derive(Debug, Clone, Copy)]
enum AuditionCommand {
    NoteOn {
        track_index: usize,
        note_id: u64,
        pitch: u8,
        velocity: f32,
    },
    NoteOff {
        track_index: usize,
        note_id: u64,
    },
    AllNotesOff,
    SetChannelSolo {
        identity: AuditionChannelIdentity,
        enabled: bool,
    },
    SetChannelCue {
        identity: Option<AuditionChannelIdentity>,
        tap: crate::model::TapPoint,
    },
    SetProcessorBypass {
        identity: AuditionChannelIdentity,
        bypass: Option<bool>,
    },
}

/// Realtime playback health sampled without locking the callback.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct PlayerTelemetry {
    /// Shared musical runtime status.
    pub engine: RuntimeTelemetry,
    /// Generation of the runtime that currently owns callback telemetry.
    pub publication_generation: u64,
    /// Number of PipeWire process callbacks completed.
    pub callbacks: u64,
    /// Callbacks whose wall time exceeded their buffer duration.
    pub late_callbacks: u64,
    /// Most recently measured process time divided by buffer duration.
    pub callback_utilization: f32,
    /// Maximum measured callback deadline utilization.
    pub peak_callback_utilization: f32,
    /// Approximate p99.9 utilization from a fixed 1%-wide bounded histogram.
    pub p999_callback_utilization: f32,
    /// Callback-side engine errors converted to silence.
    pub processing_errors: u64,
    /// True once the callback could not keep up in real time and latched into silence.
    ///
    /// Playback cannot recover from this; the owner must stop the player.
    pub overloaded: bool,
    /// True while old/new revisions are crossfading.
    pub transition_active: bool,
    /// True while the callback emits silence without advancing transport.
    pub paused: bool,
    /// Whether a PipeWire MIDI source is currently streaming.
    pub midi_connected: bool,
    /// MIDI messages consumed by the audio callback.
    pub midi_events: u64,
    /// MIDI messages dropped because the fixed queue was full.
    pub midi_dropped: u64,
    /// Native stream state.
    pub stream_state: StreamStatus,
}

/// Stable native playback state.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StreamStatus {
    /// Backend thread is initializing.
    #[default]
    Starting,
    /// Stream is negotiating with the PipeWire graph.
    Connecting,
    /// Stream exists but is not currently scheduled.
    Paused,
    /// Stream is actively producing audio buffers.
    Streaming,
    /// PipeWire reported a stream failure.
    Error,
    /// Stream has disconnected or shut down.
    Stopped,
}

/// Native playback startup and control failures.
#[derive(Debug, thiserror::Error)]
pub enum RealtimeError {
    /// Binary was built without the native backend.
    #[error("native PipeWire support is not enabled in this build")]
    Unavailable,
    /// PipeWire initialization or stream setup failed.
    #[error("PipeWire playback failed: {0}")]
    PipeWire(String),
    /// Fixed-capacity callback command queue is full.
    #[error("real-time command queue is full; retry after polling playback")]
    QueueFull,
    /// Native backend did not complete setup within its bounded wait.
    #[error("PipeWire playback setup timed out after 10 seconds")]
    StartupTimeout,
    /// Playback thread terminated unexpectedly.
    #[error("PipeWire playback thread terminated unexpectedly")]
    ThreadStopped,
    /// A callback command identity exceeded its fixed-capacity representation.
    #[error("channel identity is {length} bytes; callback command maximum is {maximum}")]
    ChannelIdentityTooLong {
        /// Attempted UTF-8 byte length.
        length: usize,
        /// Fixed command capacity.
        maximum: usize,
    },
}

fn reserve_publication(slot: &AtomicBool) -> bool {
    slot.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

fn release_publication(slot: &AtomicBool) {
    slot.store(false, Ordering::Release);
}

/// Exclusive claim on the sole replacement slot, released unless committed.
pub(crate) struct PublicationReservation {
    slot: Arc<AtomicBool>,
    committed: bool,
}

impl PublicationReservation {
    fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for PublicationReservation {
    fn drop(&mut self) {
        if !self.committed {
            release_publication(&self.slot);
        }
    }
}

fn histogram_quantile(histogram: &[u64; 256], total: u64, numerator: u64, denominator: u64) -> f32 {
    if total == 0 || denominator == 0 {
        return 0.0;
    }
    let target =
        ((u128::from(total) * u128::from(numerator)).div_ceil(u128::from(denominator))) as u64;
    let mut cumulative = 0_u64;
    for (index, count) in histogram.iter().enumerate() {
        cumulative = cumulative.saturating_add(*count);
        if cumulative >= target {
            return index as f32 / 100.0;
        }
    }
    (histogram.len() - 1) as f32 / 100.0
}

// Audition identities stay inline so the audio callback never frees a boxed
// command; the bounded ring's larger slots are an intentional memory tradeoff.
#[allow(clippy::large_enum_variant)]
enum AudioCommand {
    Publish {
        runtime: Box<SongRuntime>,
        generation: u64,
    },
    Audition(AuditionCommand),
    SetPaused(bool),
    Stop,
}

struct Transition {
    next: Box<SongRuntime>,
    generation: u64,
    position: u64,
    frames: u64,
}

/// Watches how long the callback has run without a chance to sleep.
///
/// A callback that finishes within its buffer duration lets the driver thread block until the
/// next cycle. A late one leaves the next cycle already due, so consecutive late callbacks are
/// one uninterrupted run of realtime CPU time. Once that run reaches the limit, the guard trips
/// and stays tripped.
#[derive(Debug, Clone, Copy)]
struct OverloadGuard {
    limit: std::time::Duration,
    /// Time spent in consecutive late callbacks before the current one.
    run: std::time::Duration,
    tripped: bool,
}

impl OverloadGuard {
    const fn new(limit: std::time::Duration) -> Self {
        Self {
            limit,
            run: std::time::Duration::ZERO,
            tripped: false,
        }
    }

    /// Checks the current callback's elapsed time so far, tripping if the run would exceed the
    /// limit.
    fn check(&mut self, elapsed: std::time::Duration) -> bool {
        if self.run.saturating_add(elapsed) >= self.limit {
            self.tripped = true;
        }
        self.tripped
    }

    /// Records one completed callback against its buffer duration.
    fn finish(&mut self, elapsed: std::time::Duration, budget: std::time::Duration) {
        if elapsed > budget {
            self.run = self.run.saturating_add(elapsed);
            self.check(std::time::Duration::ZERO);
        } else {
            self.run = std::time::Duration::ZERO;
        }
    }
}

struct CallbackState {
    current: Box<SongRuntime>,
    current_generation: u64,
    transition: Option<Transition>,
    pending_reclaim: Option<Box<SongRuntime>>,
    publication_in_flight: Arc<AtomicBool>,
    commands: Consumer<AudioCommand>,
    midi: Consumer<AuditionCommand>,
    reclaimed: Producer<Box<SongRuntime>>,
    telemetry: Producer<PlayerTelemetry>,
    callbacks: u64,
    late_callbacks: u64,
    callback_utilization: f32,
    peak_callback_utilization: f32,
    utilization_histogram: [u64; 256],
    processing_errors: u64,
    midi_events: u64,
    midi_was_connected: bool,
    paused: bool,
    stopping: bool,
    stream_status: Arc<AtomicU8>,
    midi_connected: Arc<AtomicBool>,
    midi_dropped: Arc<AtomicU64>,
    midi_panic: Arc<AtomicBool>,
    /// Enabled only for drivers on a realtime clock; in-memory rendering has no deadline.
    overload_guard: Option<OverloadGuard>,
}

impl CallbackState {
    fn process_buffer(&mut self, output: &mut [u8]) {
        let started = std::time::Instant::now();
        self.return_retired_runtime();
        self.consume_midi();
        self.consume_commands();
        let frames = output.len() / FRAME_BYTES;
        for frame in 0..frames {
            let sample = if self.overloaded(frame, started) {
                [0.0, 0.0]
            } else {
                self.process_frame()
            };
            let offset = frame * FRAME_BYTES;
            output[offset..offset + 4].copy_from_slice(&sample[0].to_le_bytes());
            output[offset + 4..offset + 8].copy_from_slice(&sample[1].to_le_bytes());
        }
        self.callbacks = self.callbacks.saturating_add(1);
        let budget = std::time::Duration::from_secs_f64(
            frames as f64 / f64::from(self.current.sample_rate()),
        );
        let elapsed = started.elapsed();
        if let Some(guard) = self.overload_guard.as_mut() {
            guard.finish(elapsed, budget);
        }
        if elapsed > budget {
            self.late_callbacks = self.late_callbacks.saturating_add(1);
        }
        self.callback_utilization = if budget.is_zero() {
            0.0
        } else {
            (elapsed.as_secs_f64() / budget.as_secs_f64()) as f32
        };
        self.peak_callback_utilization = self
            .peak_callback_utilization
            .max(self.callback_utilization);
        let bucket = ((self.callback_utilization * 100.0).ceil() as usize)
            .min(self.utilization_histogram.len() - 1);
        self.utilization_histogram[bucket] = self.utilization_histogram[bucket].saturating_add(1);
        let report = PlayerTelemetry {
            engine: self.current.telemetry(),
            publication_generation: self.current_generation,
            callbacks: self.callbacks,
            late_callbacks: self.late_callbacks,
            callback_utilization: self.callback_utilization,
            peak_callback_utilization: self.peak_callback_utilization,
            p999_callback_utilization: histogram_quantile(
                &self.utilization_histogram,
                self.callbacks,
                999,
                1_000,
            ),
            processing_errors: self.processing_errors,
            overloaded: self.overload_guard.is_some_and(|guard| guard.tripped),
            transition_active: self.transition.is_some(),
            paused: self.paused,
            midi_connected: self.midi_was_connected,
            midi_events: self.midi_events,
            midi_dropped: self.midi_dropped.load(Ordering::Relaxed),
            stream_state: decode_status(self.stream_status.load(Ordering::Relaxed)),
        };
        let _ = self.telemetry.push(report);
    }

    /// Whether this frame must be silence because the callback cannot keep up.
    ///
    /// Elapsed time is sampled every [`OVERLOAD_CHECK_FRAMES`], so one very slow buffer is cut
    /// short rather than running past the kernel's realtime limit.
    fn overloaded(&mut self, frame: usize, started: std::time::Instant) -> bool {
        let Some(guard) = self.overload_guard.as_mut() else {
            return false;
        };
        if guard.tripped {
            return true;
        }
        frame.is_multiple_of(OVERLOAD_CHECK_FRAMES) && frame > 0 && guard.check(started.elapsed())
    }

    fn process_frame(&mut self) -> [f32; 2] {
        if self.paused {
            return [0.0, 0.0];
        }
        let Some(transition) = self.transition.as_mut() else {
            return match self.current.process_frame(ProcessingMode::Monitoring) {
                Ok(sample) => sample,
                Err(_) => {
                    self.processing_errors = self.processing_errors.saturating_add(1);
                    [0.0, 0.0]
                }
            };
        };
        transition.position = transition.position.saturating_add(1);
        let phase = (transition.position as f64 / transition.frames as f64).clamp(0.0, 1.0);
        let old_gain = (phase * std::f64::consts::FRAC_PI_2).cos() as f32;
        let new_gain = (phase * std::f64::consts::FRAC_PI_2).sin() as f32;
        let output =
            match self
                .current
                .process_transition_frame(&mut transition.next, old_gain, new_gain)
            {
                Ok(sample) => sample,
                Err(_) => {
                    self.processing_errors = self.processing_errors.saturating_add(1);
                    [0.0, 0.0]
                }
            };
        if transition.position >= transition.frames
            && let Some(mut transition) = self.transition.take()
        {
            self.current.complete_transition(&mut transition.next);
            self.current_generation = transition.generation;
            let retired = std::mem::replace(&mut self.current, transition.next);
            self.retire(retired);
            if self.pending_reclaim.is_none() {
                release_publication(&self.publication_in_flight);
            }
        }
        output
    }

    fn consume_midi(&mut self) {
        let connected = self.midi_connected.load(Ordering::Relaxed);
        if self.midi_panic.swap(false, Ordering::AcqRel) || (self.midi_was_connected && !connected)
        {
            self.audition(AuditionCommand::AllNotesOff);
        }
        self.midi_was_connected = connected;
        while let Ok(command) = self.midi.pop() {
            self.audition(command);
            self.midi_events = self.midi_events.saturating_add(1);
        }
    }

    fn consume_commands(&mut self) {
        if self.commands.is_abandoned() {
            self.stopping = true;
            return;
        }
        // The producer-side reservation prevents another Publish while a
        // transition or retired runtime is resident. Continue draining
        // callback-local controls so pause/resume and Stop cannot deadlock
        // behind a transition that is itself paused.
        while let Ok(command) = self.commands.pop() {
            match command {
                AudioCommand::Publish {
                    runtime,
                    generation,
                } => {
                    self.begin_transition(runtime, generation);
                    break;
                }
                AudioCommand::Audition(command) => self.audition(command),
                AudioCommand::SetPaused(paused) => self.paused = paused,
                AudioCommand::Stop => {
                    self.current.all_notes_off();
                    if let Some(transition) = self.transition.as_mut() {
                        transition.next.all_notes_off();
                    }
                    self.stopping = true;
                    break;
                }
            }
        }
    }

    fn audition(&mut self, command: AuditionCommand) {
        apply_audition(&mut self.current, command);
        if let Some(transition) = self.transition.as_mut() {
            apply_audition(&mut transition.next, command);
        }
    }

    fn begin_transition(&mut self, mut runtime: Box<SongRuntime>, generation: u64) {
        let frames = u64::from(runtime.sample_rate()) * PUBLICATION_TRANSITION_MILLISECONDS / 1_000;
        self.current.prepare_transition(&mut runtime);
        self.transition = Some(Transition {
            next: runtime,
            generation,
            position: 0,
            frames: frames.max(1),
        });
    }

    fn retire(&mut self, runtime: Box<SongRuntime>) {
        match self.reclaimed.push(runtime) {
            Ok(()) => {}
            Err(PushError::Full(runtime)) => self.pending_reclaim = Some(runtime),
        }
    }

    fn return_retired_runtime(&mut self) {
        let Some(runtime) = self.pending_reclaim.take() else {
            return;
        };
        match self.reclaimed.push(runtime) {
            Ok(()) => release_publication(&self.publication_in_flight),
            Err(PushError::Full(runtime)) => self.pending_reclaim = Some(runtime),
        }
    }
}

fn apply_audition(runtime: &mut SongRuntime, command: AuditionCommand) {
    match command {
        AuditionCommand::NoteOn {
            track_index,
            note_id,
            pitch,
            velocity,
        } => {
            let _ = runtime.audition_note_on_track(track_index, note_id, pitch, velocity);
        }
        AuditionCommand::NoteOff {
            track_index,
            note_id,
        } => {
            let _ = runtime.audition_note_off_track(track_index, note_id);
        }
        AuditionCommand::AllNotesOff => runtime.all_notes_off(),
        AuditionCommand::SetChannelSolo { identity, enabled } => {
            let _ = runtime.set_channel_solo(identity.as_str(), enabled);
        }
        AuditionCommand::SetChannelCue { identity, tap } => {
            let _ = runtime
                .set_channel_cue(identity.as_ref().map(AuditionChannelIdentity::as_str), tap);
        }
        AuditionCommand::SetProcessorBypass { identity, bypass } => {
            let _ = runtime.set_processor_bypass(identity.as_str(), bypass);
        }
    }
}

const fn encode_status(status: StreamStatus) -> u8 {
    match status {
        StreamStatus::Starting => 0,
        StreamStatus::Connecting => 1,
        StreamStatus::Paused => 2,
        StreamStatus::Streaming => 3,
        StreamStatus::Error => 4,
        StreamStatus::Stopped => 5,
    }
}

const fn decode_status(value: u8) -> StreamStatus {
    match value {
        1 => StreamStatus::Connecting,
        2 => StreamStatus::Paused,
        3 => StreamStatus::Streaming,
        4 => StreamStatus::Error,
        5 => StreamStatus::Stopped,
        _ => StreamStatus::Starting,
    }
}

/// Selects the audio driver a newly started [`Player`] uses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PlaybackBackend {
    /// Native PipeWire output and MIDI input streams.
    #[default]
    PipeWire,
    /// An in-memory callback that produces audio only when its owner renders it.
    InMemory,
}

impl PlaybackBackend {
    /// Starts a player with a controller-assigned publication generation.
    pub(crate) fn start(
        self,
        runtime: SongRuntime,
        generation: u64,
    ) -> Result<Player, RealtimeError> {
        match self {
            Self::PipeWire => Player::start_with_generation(runtime, generation),
            Self::InMemory => Ok(Player::in_memory(runtime, generation)),
        }
    }
}

/// Who runs the callback for one [`Player`].
enum Driver {
    /// The PipeWire stream thread owns the callback.
    #[cfg(feature = "pipewire")]
    PipeWire {
        stop_requested: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    },
    /// The player owns the callback and runs it on [`Player::render`].
    InMemory(Box<CallbackState>),
}

/// Callback-side ends handed to a driver when a [`Player`] connects.
///
/// Only the native driver consumes MIDI; the in-memory driver drops those ends.
struct CallbackEnds {
    callback: CallbackState,
    #[cfg_attr(not(feature = "pipewire"), allow(dead_code))]
    midi_events: Producer<AuditionCommand>,
    #[cfg_attr(not(feature = "pipewire"), allow(dead_code))]
    selected_track: Arc<AtomicUsize>,
}

/// Controller for one playback callback, whichever driver runs it.
pub struct Player {
    commands: Option<Producer<AudioCommand>>,
    reclaimed: Consumer<Box<SongRuntime>>,
    publication_in_flight: Arc<AtomicBool>,
    telemetry: Consumer<PlayerTelemetry>,
    latest: PlayerTelemetry,
    stream_status: Arc<AtomicU8>,
    selected_track: Arc<AtomicUsize>,
    driver: Driver,
}

impl Player {
    /// Starts a native stereo float output stream with a preallocated runtime.
    pub fn start(runtime: SongRuntime) -> Result<Self, RealtimeError> {
        Self::start_with_generation(runtime, 0)
    }

    /// Starts native playback with a controller-assigned inspection generation.
    #[cfg(feature = "pipewire")]
    pub(crate) fn start_with_generation(
        runtime: SongRuntime,
        generation: u64,
    ) -> Result<Self, RealtimeError> {
        Self::connect(runtime, generation, StreamStatus::Starting, |mut ends| {
            ends.callback.overload_guard = Some(OverloadGuard::new(OVERLOAD_RUN_LIMIT));
            let stop_requested = Arc::new(AtomicBool::new(false));
            let thread = native::spawn(native::NativeStream {
                callback: ends.callback,
                midi_events: ends.midi_events,
                selected_track: ends.selected_track,
                stop_requested: Arc::clone(&stop_requested),
            })?;
            Ok(Driver::PipeWire {
                stop_requested,
                thread: Some(thread),
            })
        })
    }

    /// Reports that this build lacks native playback.
    #[cfg(not(feature = "pipewire"))]
    pub(crate) fn start_with_generation(
        _runtime: SongRuntime,
        _generation: u64,
    ) -> Result<Self, RealtimeError> {
        Err(RealtimeError::Unavailable)
    }

    /// Creates a player whose callback runs only when [`Player::render`] is called.
    ///
    /// It has no device clock or MIDI input. Everything else, including
    /// publication crossfades, pause, and runtime reclamation, is the same
    /// callback the PipeWire driver runs.
    #[must_use]
    pub fn in_memory(runtime: SongRuntime, generation: u64) -> Self {
        match Self::connect(runtime, generation, StreamStatus::Streaming, |ends| {
            Ok(Driver::InMemory(Box::new(ends.callback)))
        }) {
            Ok(player) => player,
            Err(_) => unreachable!("the in-memory driver cannot fail to start"),
        }
    }

    /// Builds both ends of every ring for one runtime and hands the callback to a driver.
    fn connect(
        runtime: SongRuntime,
        generation: u64,
        status: StreamStatus,
        driver: impl FnOnce(CallbackEnds) -> Result<Driver, RealtimeError>,
    ) -> Result<Self, RealtimeError> {
        let (command_producer, command_consumer) = RingBuffer::new(COMMAND_CAPACITY);
        let (reclaim_producer, reclaim_consumer) = RingBuffer::new(RECLAIM_CAPACITY);
        let (telemetry_producer, telemetry_consumer) = RingBuffer::new(TELEMETRY_CAPACITY);
        let (midi_producer, midi_consumer) = RingBuffer::new(MIDI_CAPACITY);
        let stream_status = Arc::new(AtomicU8::new(encode_status(status)));
        let publication_in_flight = Arc::new(AtomicBool::new(false));
        let selected_track = Arc::new(AtomicUsize::new(0));
        let callback = CallbackState {
            current: Box::new(runtime),
            current_generation: generation,
            transition: None,
            pending_reclaim: None,
            publication_in_flight: Arc::clone(&publication_in_flight),
            commands: command_consumer,
            midi: midi_consumer,
            reclaimed: reclaim_producer,
            telemetry: telemetry_producer,
            callbacks: 0,
            late_callbacks: 0,
            callback_utilization: 0.0,
            peak_callback_utilization: 0.0,
            utilization_histogram: [0; 256],
            processing_errors: 0,
            midi_events: 0,
            midi_was_connected: false,
            paused: false,
            stopping: false,
            stream_status: Arc::clone(&stream_status),
            midi_connected: Arc::new(AtomicBool::new(false)),
            midi_dropped: Arc::new(AtomicU64::new(0)),
            midi_panic: Arc::new(AtomicBool::new(false)),
            overload_guard: None,
        };
        let driver = driver(CallbackEnds {
            callback,
            midi_events: midi_producer,
            selected_track: Arc::clone(&selected_track),
        })?;
        Ok(Self {
            commands: Some(command_producer),
            reclaimed: reclaim_consumer,
            publication_in_flight,
            telemetry: telemetry_consumer,
            latest: PlayerTelemetry::default(),
            stream_status,
            selected_track,
            driver,
        })
    }

    /// Runs an in-memory callback for `frames` stereo frames and returns them.
    ///
    /// A native stream's callback belongs to PipeWire, so it reports
    /// [`RealtimeError::Unavailable`] instead.
    pub fn render(&mut self, frames: usize) -> Result<Vec<[f32; 2]>, RealtimeError> {
        match &mut self.driver {
            Driver::InMemory(callback) => {
                let mut output = vec![0_u8; frames * FRAME_BYTES];
                callback.process_buffer(&mut output);
                Ok(output
                    .as_chunks::<FRAME_BYTES>()
                    .0
                    .iter()
                    .map(|frame| {
                        let [l0, l1, l2, l3, r0, r1, r2, r3] = *frame;
                        [
                            f32::from_le_bytes([l0, l1, l2, l3]),
                            f32::from_le_bytes([r0, r1, r2, r3]),
                        ]
                    })
                    .collect())
            }
            #[cfg(feature = "pipewire")]
            Driver::PipeWire { .. } => Err(RealtimeError::Unavailable),
        }
    }

    /// Reserves the sole replacement slot before a candidate runtime is allocated.
    pub(crate) fn reserve_publication(&mut self) -> Result<PublicationReservation, RealtimeError> {
        if !reserve_publication(&self.publication_in_flight) {
            return Err(RealtimeError::QueueFull);
        }
        Ok(PublicationReservation {
            slot: Arc::clone(&self.publication_in_flight),
            committed: false,
        })
    }

    /// Queues an already primed and boxed runtime without controller-side allocation.
    pub(crate) fn publish_prepared(
        &mut self,
        runtime: Box<SongRuntime>,
        reservation: PublicationReservation,
        generation: u64,
    ) -> Result<(), (RealtimeError, Box<SongRuntime>, PublicationReservation)> {
        debug_assert!(Arc::ptr_eq(&reservation.slot, &self.publication_in_flight));
        let Some(commands) = self.commands.as_mut() else {
            return Err((RealtimeError::ThreadStopped, runtime, reservation));
        };
        match commands.push(AudioCommand::Publish {
            runtime,
            generation,
        }) {
            Ok(()) => {
                reservation.commit();
                Ok(())
            }
            Err(PushError::Full(command)) => {
                let AudioCommand::Publish { runtime, .. } = command else {
                    unreachable!("only a Publish command was pushed")
                };
                Err((RealtimeError::QueueFull, runtime, reservation))
            }
        }
    }

    /// Enqueues one transient Note On by zero-based Track index.
    pub fn note_on(
        &mut self,
        track_index: usize,
        note_id: u64,
        pitch: u8,
        velocity: f32,
    ) -> Result<(), RealtimeError> {
        self.push_audition(AuditionCommand::NoteOn {
            track_index,
            note_id,
            pitch,
            velocity,
        })
    }

    /// Enqueues one transient Note Off.
    pub fn note_off(&mut self, track_index: usize, note_id: u64) -> Result<(), RealtimeError> {
        self.push_audition(AuditionCommand::NoteOff {
            track_index,
            note_id,
        })
    }

    /// Releases all transient and scheduled graph notes.
    pub fn all_notes_off(&mut self) -> Result<(), RealtimeError> {
        self.push_audition(AuditionCommand::AllNotesOff)
    }

    /// Enables or disables transient Solo for one compiled channel identity.
    pub fn set_channel_solo(&mut self, identity: &str, enabled: bool) -> Result<(), RealtimeError> {
        self.push_audition(AuditionCommand::SetChannelSolo {
            identity: AuditionChannelIdentity::new(identity)?,
            enabled,
        })
    }

    /// Selects a transient channel cue tap, or clears cue with `None`.
    pub fn set_channel_cue(
        &mut self,
        identity: Option<&str>,
        tap: crate::model::TapPoint,
    ) -> Result<(), RealtimeError> {
        self.push_audition(AuditionCommand::SetChannelCue {
            identity: identity.map(AuditionChannelIdentity::new).transpose()?,
            tap,
        })
    }

    /// Sets or clears one monitoring-only processor bypass override.
    pub fn set_processor_bypass(
        &mut self,
        processor_id: &str,
        bypass: Option<bool>,
    ) -> Result<(), RealtimeError> {
        self.push_audition(AuditionCommand::SetProcessorBypass {
            identity: AuditionChannelIdentity::new(processor_id)?,
            bypass,
        })
    }

    /// Pauses or resumes without destroying the runtime or advancing transport.
    pub fn set_paused(&mut self, paused: bool) -> Result<(), RealtimeError> {
        self.commands
            .as_mut()
            .ok_or(RealtimeError::ThreadStopped)?
            .push(AudioCommand::SetPaused(paused))
            .map_err(|PushError::Full(_)| RealtimeError::QueueFull)
    }

    /// Selects the zero-based Track that receives transient PipeWire MIDI.
    pub fn set_audition_track(&mut self, track_index: usize) {
        self.selected_track.store(track_index, Ordering::Release);
    }

    /// Drains callback telemetry and destroys retired runtimes on this caller.
    pub fn poll(&mut self) -> PlayerTelemetry {
        self.collect_garbage();
        self.poll_telemetry()
    }

    /// Drains callback telemetry while transferring retired runtimes elsewhere.
    pub(crate) fn poll_with_reclaimer(
        &mut self,
        mut retire: impl FnMut(Box<SongRuntime>),
    ) -> PlayerTelemetry {
        while let Ok(runtime) = self.reclaimed.pop() {
            retire(runtime);
        }
        self.poll_telemetry()
    }

    fn poll_telemetry(&mut self) -> PlayerTelemetry {
        while let Ok(report) = self.telemetry.pop() {
            self.latest = report;
        }
        self.latest.stream_state = decode_status(self.stream_status.load(Ordering::Relaxed));
        self.latest
    }

    /// Requests callback-boundary shutdown and joins a native stream thread.
    pub fn shutdown(mut self) -> Result<(), RealtimeError> {
        self.request_stop();
        #[cfg(feature = "pipewire")]
        if let Driver::PipeWire { thread, .. } = &mut self.driver
            && let Some(thread) = thread.take()
        {
            thread.join().map_err(|_| RealtimeError::ThreadStopped)?;
        }
        self.collect_garbage();
        Ok(())
    }

    /// Signals the driver to stop and closes the command ring.
    fn request_stop(&mut self) {
        #[cfg(feature = "pipewire")]
        if let Driver::PipeWire { stop_requested, .. } = &self.driver {
            stop_requested.store(true, Ordering::Release);
        }
        if let Some(commands) = self.commands.as_mut() {
            let _ = commands.push(AudioCommand::Stop);
        }
        self.commands.take();
    }

    fn push_audition(&mut self, command: AuditionCommand) -> Result<(), RealtimeError> {
        self.commands
            .as_mut()
            .ok_or(RealtimeError::ThreadStopped)?
            .push(AudioCommand::Audition(command))
            .map_err(|PushError::Full(_)| RealtimeError::QueueFull)
    }

    fn collect_garbage(&mut self) {
        while let Ok(runtime) = self.reclaimed.pop() {
            drop(runtime);
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.request_stop();
        // Dropping a native JoinHandle detaches. The callback also observes the
        // abandoned command producer and quits on its next graph cycle.
        #[cfg(feature = "pipewire")]
        if let Driver::PipeWire { thread, .. } = &mut self.driver {
            thread.take();
        }
        self.collect_garbage();
    }
}

/// Native PipeWire driver: one stream thread owns the callback and MIDI input.
#[cfg(feature = "pipewire")]
mod native {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
            mpsc,
        },
        thread,
        time::Duration,
    };

    use pipewire as pw;
    use pw::{properties::properties, spa};
    use rtrb::Producer;
    use spa::pod::Pod;

    use super::{
        AuditionCommand, CallbackState, FRAME_BYTES, RealtimeError, StreamStatus, decode_status,
        encode_status,
    };

    /// Thread-owned pieces handed to the native stream.
    pub(super) struct NativeStream {
        pub(super) callback: CallbackState,
        pub(super) midi_events: Producer<AuditionCommand>,
        pub(super) selected_track: Arc<AtomicUsize>,
        pub(super) stop_requested: Arc<AtomicBool>,
    }

    /// Starts the PipeWire thread that owns `stream` and waits for stream setup.
    pub(super) fn spawn(stream: NativeStream) -> Result<thread::JoinHandle<()>, RealtimeError> {
        let stop_requested = Arc::clone(&stream.stop_requested);
        let (startup_tx, startup_rx) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("crump-pipewire".to_owned())
            .spawn(move || {
                if let Err(error) = run_pipewire(stream, &startup_tx) {
                    let _ = startup_tx.try_send(Err(error.to_string()));
                }
            })
            .map_err(|error| RealtimeError::PipeWire(error.to_string()))?;
        match startup_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(thread),
            Ok(Err(message)) => {
                let _ = thread.join();
                Err(RealtimeError::PipeWire(message))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                stop_requested.store(true, Ordering::Release);
                Err(RealtimeError::StartupTimeout)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = thread.join();
                Err(RealtimeError::ThreadStopped)
            }
        }
    }

    fn requested_output_frames(capacity_bytes: usize, requested_frames: u64) -> usize {
        let capacity_frames = capacity_bytes / FRAME_BYTES;
        let requested_frames = usize::try_from(requested_frames).unwrap_or(usize::MAX);
        if requested_frames == 0 {
            capacity_frames
        } else {
            requested_frames.min(capacity_frames)
        }
    }

    struct MidiInputState {
        events: Producer<AuditionCommand>,
        selected_track: Arc<AtomicUsize>,
        dropped: Arc<AtomicU64>,
        panic: Arc<AtomicBool>,
        held_tracks: [usize; 16 * 128],
        held: [bool; 16 * 128],
    }

    impl MidiInputState {
        fn consume_sequence(&mut self, bytes: &[u8]) {
            visit_spa_controls(bytes, |control_type, payload| {
                if control_type == spa::sys::SPA_CONTROL_Midi {
                    self.consume_midi_one(payload);
                } else if control_type == spa::sys::SPA_CONTROL_UMP {
                    for word in payload.as_chunks::<4>().0 {
                        let word = u32::from_ne_bytes([word[0], word[1], word[2], word[3]]);
                        if word >> 28 == 0x2 {
                            self.consume_midi_one(&[
                                ((word >> 16) & 0xff) as u8,
                                ((word >> 8) & 0xff) as u8,
                                (word & 0xff) as u8,
                            ]);
                        }
                    }
                }
            });
        }

        fn consume_midi_one(&mut self, message: &[u8]) {
            let Some(&status) = message.first() else {
                return;
            };
            let selected_track = self.selected_track.load(Ordering::Relaxed);
            let channel = status & 0x0f;
            let Some(pitch) = message.get(1).copied().filter(|pitch| *pitch < 128) else {
                return;
            };
            let held_index = usize::from(channel) * 128 + usize::from(pitch);
            let note_id = (u64::from(channel) << 8) | u64::from(pitch);
            let command = match (status & 0xf0, message.get(2).copied()) {
                (0x90, Some(velocity)) if velocity > 0 => {
                    self.held_tracks[held_index] = selected_track;
                    self.held[held_index] = true;
                    Some(AuditionCommand::NoteOn {
                        track_index: selected_track,
                        note_id,
                        pitch,
                        velocity: f32::from(velocity) / 127.0,
                    })
                }
                (0x80 | 0x90, Some(_)) => {
                    let track_index = if self.held[held_index] {
                        self.held_tracks[held_index]
                    } else {
                        selected_track
                    };
                    self.held[held_index] = false;
                    Some(AuditionCommand::NoteOff {
                        track_index,
                        note_id,
                    })
                }
                (0xb0, Some(_)) if matches!(pitch, 120 | 123) => {
                    self.held.fill(false);
                    Some(AuditionCommand::AllNotesOff)
                }
                _ => None,
            };
            if let Some(command) = command
                && self.events.push(command).is_err()
            {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                self.panic.store(true, Ordering::Release);
            }
        }
    }

    fn visit_spa_controls(mut bytes: &[u8], mut visitor: impl FnMut(u32, &[u8])) {
        if bytes.len() < 16 {
            return;
        }
        let body_size = read_u32(bytes, 0) as usize;
        if read_u32(bytes, 4) != spa::sys::SPA_TYPE_Sequence {
            return;
        }
        let complete_size = body_size.saturating_add(8).min(bytes.len());
        bytes = &bytes[..complete_size];
        let mut cursor = 16_usize;
        while cursor.saturating_add(16) <= bytes.len() {
            let control_type = read_u32(bytes, cursor + 4);
            let value_size = read_u32(bytes, cursor + 8) as usize;
            let value_type = read_u32(bytes, cursor + 12);
            let body_start = cursor + 16;
            let Some(body_end) = body_start.checked_add(value_size) else {
                return;
            };
            if body_end > bytes.len() {
                return;
            }
            if value_type == spa::sys::SPA_TYPE_Bytes {
                visitor(control_type, &bytes[body_start..body_end]);
            }
            let Some(next) = align_eight(body_end) else {
                return;
            };
            if next <= cursor {
                return;
            }
            cursor = next;
        }
    }

    fn read_u32(bytes: &[u8], offset: usize) -> u32 {
        bytes
            .get(offset..offset + 4)
            .and_then(|value| value.try_into().ok())
            .map(u32::from_ne_bytes)
            .unwrap_or(0)
    }

    fn align_eight(value: usize) -> Option<usize> {
        value.checked_add(7).map(|value| value & !7)
    }

    fn run_pipewire(
        stream_parts: NativeStream,
        startup: &mpsc::SyncSender<Result<(), String>>,
    ) -> Result<(), pw::Error> {
        let NativeStream {
            callback: callback_state,
            midi_events: midi_producer,
            selected_track,
            stop_requested,
        } = stream_parts;
        let stream_status = Arc::clone(&callback_state.stream_status);
        let midi_connected = Arc::clone(&callback_state.midi_connected);
        let midi_dropped = Arc::clone(&callback_state.midi_dropped);
        let midi_panic = Arc::clone(&callback_state.midi_panic);
        pw::init();
        let mainloop = pw::main_loop::MainLoopRc::new(None)?;
        let context = pw::context::ContextRc::new(&mainloop, None)?;
        let core = context.connect_rc(None)?;
        let timer_quit = mainloop.clone();
        let stop_timer = mainloop.loop_().add_timer(move |_| {
            if stop_requested.load(Ordering::Acquire) {
                timer_quit.quit();
            }
        });
        let interval = Duration::from_millis(50);
        let _ = stop_timer.update_timer(Some(interval), Some(interval));
        let stream = pw::stream::StreamBox::new(
            &core,
            "crump-output",
            properties! {
                *pw::keys::MEDIA_TYPE => "Audio",
                *pw::keys::MEDIA_ROLE => "Music",
                *pw::keys::MEDIA_CATEGORY => "Playback",
                *pw::keys::AUDIO_CHANNELS => "2",
                *pw::keys::NODE_NAME => "crump",
                *pw::keys::NODE_DESCRIPTION => "Crump Musical Sketch",
            },
        )?;
        let state_status = Arc::clone(&stream_status);
        let _state_listener = stream
            .add_local_listener::<()>()
            .state_changed(move |_, _, _, state| {
                let status = match state {
                    pw::stream::StreamState::Unconnected => StreamStatus::Stopped,
                    pw::stream::StreamState::Connecting => StreamStatus::Connecting,
                    pw::stream::StreamState::Paused => StreamStatus::Paused,
                    pw::stream::StreamState::Streaming => StreamStatus::Streaming,
                    pw::stream::StreamState::Error(_) => StreamStatus::Error,
                };
                state_status.store(encode_status(status), Ordering::Relaxed);
            })
            .register()?;
        let quit_loop = mainloop.clone();
        let _process_listener = stream
            .add_local_listener_with_user_data(callback_state)
            .process(move |stream, state| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let requested_frames = buffer.requested();
                let datas = buffer.datas_mut();
                let Some(data) = datas.first_mut() else {
                    return;
                };
                let frames = if let Some(output) = data.data() {
                    let frames = requested_output_frames(output.len(), requested_frames);
                    state.process_buffer(&mut output[..frames * FRAME_BYTES]);
                    frames
                } else {
                    0
                };
                let chunk = data.chunk_mut();
                *chunk.offset_mut() = 0;
                *chunk.stride_mut() = FRAME_BYTES as i32;
                *chunk.size_mut() = (frames * FRAME_BYTES) as u32;
                if state.stopping {
                    quit_loop.quit();
                }
            })
            .register()?;

        let midi_stream = pw::stream::StreamBox::new(
            &core,
            "crump-midi-input",
            properties! {
                *pw::keys::MEDIA_TYPE => "Midi",
                *pw::keys::MEDIA_ROLE => "Music",
                *pw::keys::MEDIA_CATEGORY => "Capture",
                *pw::keys::NODE_NAME => "crump-midi-input",
                *pw::keys::NODE_DESCRIPTION => "Crump MIDI Note Audition",
            },
        )?;
        let midi_connection = Arc::clone(&midi_connected);
        let _midi_state_listener = midi_stream
            .add_local_listener::<()>()
            .state_changed(move |_, _, _, state| {
                let connected = matches!(state, pw::stream::StreamState::Streaming);
                midi_connection.store(connected, Ordering::Relaxed);
            })
            .register()?;
        let midi_state = MidiInputState {
            events: midi_producer,
            selected_track,
            dropped: Arc::clone(&midi_dropped),
            panic: midi_panic,
            held_tracks: [0; 16 * 128],
            held: [false; 16 * 128],
        };
        let _midi_process_listener = midi_stream
            .add_local_listener_with_user_data(midi_state)
            .process(|stream, state| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let datas = buffer.datas_mut();
                let Some(data) = datas.first_mut() else {
                    return;
                };
                let offset = data.chunk().offset() as usize;
                let size = data.chunk().size() as usize;
                let Some(input) = data.data() else {
                    return;
                };
                let Some(end) = offset.checked_add(size) else {
                    return;
                };
                if let Some(sequence) = input.get(offset..end) {
                    state.consume_sequence(sequence);
                }
            })
            .register()?;
        let mut midi_object = pw::spa::pod::object!(
            pw::spa::utils::SpaTypes::ObjectParamFormat,
            pw::spa::param::ParamType::EnumFormat,
            pw::spa::pod::property!(
                pw::spa::param::format::FormatProperties::MediaType,
                Id,
                pw::spa::param::format::MediaType::Application
            ),
            pw::spa::pod::property!(
                pw::spa::param::format::FormatProperties::MediaSubtype,
                Id,
                pw::spa::param::format::MediaSubtype::Control
            ),
        );
        let control_types =
            (1_u32 << spa::sys::SPA_CONTROL_Midi) | (1_u32 << spa::sys::SPA_CONTROL_UMP);
        midi_object.properties.push(pw::spa::pod::Property::new(
            spa::sys::SPA_FORMAT_CONTROL_types,
            pw::spa::pod::Value::Int(control_types as i32),
        ));
        let midi_values = pw::spa::pod::serialize::PodSerializer::serialize(
            std::io::Cursor::new(Vec::new()),
            &pw::spa::pod::Value::Object(midi_object),
        )
        .map_err(|_| pw::Error::CreationFailed)?
        .0
        .into_inner();
        let mut midi_params = [Pod::from_bytes(&midi_values).ok_or(pw::Error::CreationFailed)?];
        midi_stream.connect(
            spa::utils::Direction::Input,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut midi_params,
        )?;

        let sample_rate = 48_000;
        let mut audio_info = spa::param::audio::AudioInfoRaw::new();
        audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
        audio_info.set_rate(sample_rate);
        audio_info.set_channels(2);
        let mut position = [0; spa::param::audio::MAX_CHANNELS];
        position[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
        position[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
        audio_info.set_position(position);
        let object = pw::spa::pod::Object {
            type_: pw::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
            id: pw::spa::param::ParamType::EnumFormat.as_raw(),
            properties: audio_info.into(),
        };
        let values = pw::spa::pod::serialize::PodSerializer::serialize(
            std::io::Cursor::new(Vec::new()),
            &pw::spa::pod::Value::Object(object),
        )
        .map_err(|_| pw::Error::CreationFailed)?
        .0
        .into_inner();
        let mut params = [Pod::from_bytes(&values).ok_or(pw::Error::CreationFailed)?];
        stream.connect(
            spa::utils::Direction::Output,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut params,
        )?;
        let _ = startup.send(Ok(()));
        mainloop.run();
        if decode_status(stream_status.load(Ordering::Relaxed)) != StreamStatus::Error {
            stream_status.store(encode_status(StreamStatus::Stopped), Ordering::Relaxed);
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use rtrb::RingBuffer;

        use super::*;

        #[test]
        fn output_frames_respect_pipewire_request_and_capacity() {
            assert_eq!(requested_output_frames(12_288 * FRAME_BYTES, 1_024), 1_024);
            assert_eq!(requested_output_frames(12_288 * FRAME_BYTES, 0), 12_288);
            assert_eq!(requested_output_frames(256 * FRAME_BYTES, 1_024), 256);
        }

        #[test]
        fn parses_bounded_spa_midi_sequence() {
            let mut sequence = vec![0_u8; 40];
            put_u32(&mut sequence, 0, 32);
            put_u32(&mut sequence, 4, spa::sys::SPA_TYPE_Sequence);
            put_u32(&mut sequence, 20, spa::sys::SPA_CONTROL_Midi);
            put_u32(&mut sequence, 24, 3);
            put_u32(&mut sequence, 28, spa::sys::SPA_TYPE_Bytes);
            sequence[32..35].copy_from_slice(&[0x90, 60, 100]);
            let mut found = Vec::new();
            visit_spa_controls(&sequence, |kind, payload| {
                found.push((kind, payload.to_vec()))
            });
            assert_eq!(
                found,
                vec![(spa::sys::SPA_CONTROL_Midi, vec![0x90, 60, 100])]
            );
        }

        #[test]
        fn midi_note_off_uses_the_track_selected_at_note_on() {
            let (producer, mut consumer) = RingBuffer::new(4);
            let selected_track = Arc::new(AtomicUsize::new(2));
            let mut input = MidiInputState {
                events: producer,
                selected_track: Arc::clone(&selected_track),
                dropped: Arc::new(AtomicU64::new(0)),
                panic: Arc::new(AtomicBool::new(false)),
                held_tracks: [0; 16 * 128],
                held: [false; 16 * 128],
            };
            input.consume_midi_one(&[0x90, 60, 100]);
            selected_track.store(7, Ordering::Relaxed);
            input.consume_midi_one(&[0x80, 60, 0]);

            assert!(matches!(
                consumer.pop(),
                Ok(AuditionCommand::NoteOn {
                    track_index: 2,
                    pitch: 60,
                    ..
                })
            ));
            assert!(matches!(
                consumer.pop(),
                Ok(AuditionCommand::NoteOff { track_index: 2, .. })
            ));
        }

        #[test]
        fn dropped_midi_requests_callback_all_notes_off() {
            let (producer, _consumer) = RingBuffer::new(1);
            let panic = Arc::new(AtomicBool::new(false));
            let dropped = Arc::new(AtomicU64::new(0));
            let mut input = MidiInputState {
                events: producer,
                selected_track: Arc::new(AtomicUsize::new(0)),
                dropped: Arc::clone(&dropped),
                panic: Arc::clone(&panic),
                held_tracks: [0; 16 * 128],
                held: [false; 16 * 128],
            };
            input.consume_midi_one(&[0x90, 60, 100]);
            input.consume_midi_one(&[0x80, 60, 0]);
            assert_eq!(dropped.load(Ordering::Relaxed), 1);
            assert!(panic.load(Ordering::Relaxed));
        }

        #[test]
        fn rejects_truncated_spa_sequence() {
            let mut sequence = vec![0_u8; 32];
            put_u32(&mut sequence, 0, 100);
            put_u32(&mut sequence, 4, spa::sys::SPA_TYPE_Sequence);
            put_u32(&mut sequence, 20, spa::sys::SPA_CONTROL_Midi);
            put_u32(&mut sequence, 24, 40);
            put_u32(&mut sequence, 28, spa::sys::SPA_TYPE_Bytes);
            let mut calls = 0;
            visit_spa_controls(&sequence, |_, _| calls += 1);
            assert_eq!(calls, 0);
        }

        fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
            bytes[offset..offset + 4].copy_from_slice(&value.to_ne_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compiled_fixture(relative: &str) -> crate::engine::CompiledProject {
        let bundle = crate::project::load_project(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative),
        )
        .expect("load callback qualification Project");
        let project = bundle
            .publishable()
            .expect("valid callback qualification Project");
        crate::engine::CompiledProject::compile_with_assets(project, &bundle.assets)
            .expect("compile callback qualification Project")
    }

    fn maximum_compiled_project() -> crate::engine::CompiledProject {
        compiled_fixture("fixtures/synth-alpha/maximum-topology.crump")
    }

    fn callback_state(
        current: SongRuntime,
        commands: Consumer<AudioCommand>,
        midi: Consumer<AuditionCommand>,
        reclaimed: Producer<Box<SongRuntime>>,
        telemetry: Producer<PlayerTelemetry>,
        publication_in_flight: Arc<AtomicBool>,
    ) -> CallbackState {
        CallbackState {
            current: Box::new(current),
            current_generation: 0,
            transition: None,
            pending_reclaim: None,
            publication_in_flight,
            commands,
            midi,
            reclaimed,
            telemetry,
            callbacks: 0,
            late_callbacks: 0,
            callback_utilization: 0.0,
            peak_callback_utilization: 0.0,
            utilization_histogram: [0; 256],
            processing_errors: 0,
            midi_events: 0,
            midi_was_connected: false,
            paused: false,
            stopping: false,
            stream_status: Arc::new(AtomicU8::new(encode_status(StreamStatus::Streaming))),
            midi_connected: Arc::new(AtomicBool::new(false)),
            midi_dropped: Arc::new(AtomicU64::new(0)),
            midi_panic: Arc::new(AtomicBool::new(false)),
            overload_guard: None,
        }
    }

    #[test]
    fn publication_protocol_admits_only_active_plus_one_replacement() {
        let slot = Arc::new(AtomicBool::new(false));
        assert!(reserve_publication(&slot));
        let reservation = PublicationReservation {
            slot: Arc::clone(&slot),
            committed: false,
        };
        assert!(!reserve_publication(&slot));
        drop(reservation);
        assert!(!slot.load(Ordering::Acquire));

        assert!(reserve_publication(&slot));
        let reservation = PublicationReservation {
            slot: Arc::clone(&slot),
            committed: false,
        };
        reservation.commit();
        assert!(slot.load(Ordering::Acquire));
        release_publication(&slot);
        assert_eq!(
            crate::engine::mix_graph::MAX_RESIDENT_PUBLICATION_RUNTIMES,
            2
        );
    }

    #[test]
    fn maximum_topology_steady_state_and_controls_are_allocation_free() {
        let compiled = maximum_compiled_project();
        let (mut commands, command_consumer) = RingBuffer::new(4);
        let (mut midi, midi_consumer) = RingBuffer::new(4);
        let (reclaimed, _reclaim_consumer) = RingBuffer::new(1);
        let (telemetry, _telemetry_consumer) = RingBuffer::new(16);
        let mut callback = callback_state(
            compiled.runtime(),
            command_consumer,
            midi_consumer,
            reclaimed,
            telemetry,
            Arc::new(AtomicBool::new(false)),
        );
        let mut output = [0_u8; 256 * FRAME_BYTES];

        // Initialize thread-local accounting and lazy DSP paths before the
        // measured maximum-topology steady-state callback sequence.
        callback.process_buffer(&mut output);
        let _ = allocation_counter::measure(|| {});
        commands
            .push(AudioCommand::SetPaused(false))
            .expect("queue pause-state command");
        commands
            .push(AudioCommand::Audition(AuditionCommand::AllNotesOff))
            .expect("queue audition command");
        midi.push(AuditionCommand::NoteOn {
            track_index: 0,
            note_id: 99,
            pitch: 60,
            velocity: 0.75,
        })
        .expect("queue MIDI-equivalent command");

        let allocations = allocation_counter::measure(|| {
            callback.process_buffer(&mut output);
            callback.process_buffer(&mut output);
        });

        assert_eq!(allocations.count_total, 0, "{allocations:?}");
        assert_eq!(allocations.count_current, 0, "{allocations:?}");
        assert_eq!(allocations.bytes_total, 0, "{allocations:?}");
        assert_eq!(allocations.bytes_current, 0, "{allocations:?}");
        assert!(callback.transition.is_none());
        assert!(callback.pending_reclaim.is_none());
        assert_eq!(callback.midi_events, 1);
    }

    #[test]
    fn granular_spectral_independent_maxima_callback_is_allocation_free() {
        let compiled = compiled_fixture("fixtures/synth-alpha/maximum-granular-spectral.crump");
        let (_commands, command_consumer) = RingBuffer::new(2);
        let (_midi, midi_consumer) = RingBuffer::new(2);
        let (reclaimed, _reclaim_consumer) = RingBuffer::new(1);
        let (telemetry, _telemetry_consumer) = RingBuffer::new(8);
        let mut callback = callback_state(
            compiled.runtime(),
            command_consumer,
            midi_consumer,
            reclaimed,
            telemetry,
            Arc::new(AtomicBool::new(false)),
        );
        let mut output = [0_u8; 256 * FRAME_BYTES];

        callback.process_buffer(&mut output);
        let _ = allocation_counter::measure(|| {});
        let allocations = allocation_counter::measure(|| {
            callback.process_buffer(&mut output);
            callback.process_buffer(&mut output);
        });

        assert_eq!(allocations.count_total, 0, "{allocations:?}");
        assert_eq!(allocations.count_current, 0, "{allocations:?}");
        assert_eq!(allocations.bytes_total, 0, "{allocations:?}");
        assert_eq!(allocations.bytes_current, 0, "{allocations:?}");
        assert!(callback.transition.is_none());
        assert!(callback.pending_reclaim.is_none());
    }

    #[test]
    fn maximum_topology_publication_transition_and_reclamation_are_allocation_free() {
        let compiled = maximum_compiled_project();
        let (mut commands, command_consumer) = RingBuffer::new(4);
        let (_midi, midi_consumer) = RingBuffer::new(4);
        let (mut reclaimed, mut reclaim_consumer) = RingBuffer::new(1);
        let (telemetry, mut telemetry_consumer) = RingBuffer::new(16);
        reclaimed
            .push(Box::new(compiled.runtime()))
            .expect("occupy reclaim ring");
        let publication_in_flight = Arc::new(AtomicBool::new(true));
        let mut callback = callback_state(
            compiled.runtime(),
            command_consumer,
            midi_consumer,
            reclaimed,
            telemetry,
            Arc::clone(&publication_in_flight),
        );
        let mut output = [0_u8; 256 * FRAME_BYTES];

        callback.process_buffer(&mut output);
        let _ = allocation_counter::measure(|| {});
        commands
            .push(AudioCommand::Publish {
                runtime: Box::new(compiled.runtime()),
                generation: 1,
            })
            .expect("queue replacement runtime");

        let transition_allocations = allocation_counter::measure(|| {
            callback.process_buffer(&mut output);
            callback.process_buffer(&mut output);
        });
        assert_eq!(
            transition_allocations.count_total, 0,
            "{transition_allocations:?}"
        );
        assert_eq!(
            transition_allocations.count_current, 0,
            "{transition_allocations:?}"
        );
        assert_eq!(
            transition_allocations.bytes_total, 0,
            "{transition_allocations:?}"
        );
        assert_eq!(
            transition_allocations.bytes_current, 0,
            "{transition_allocations:?}"
        );
        assert!(callback.transition.is_none());
        assert_eq!(callback.current_generation, 1);
        let generations = std::iter::from_fn(|| telemetry_consumer.pop().ok())
            .map(|report| report.publication_generation)
            .collect::<Vec<_>>();
        assert_eq!(generations.first(), Some(&0));
        assert_eq!(generations.last(), Some(&1));
        assert!(callback.pending_reclaim.is_some());
        assert!(publication_in_flight.load(Ordering::Acquire));

        let occupied_reclaim_slot = reclaim_consumer
            .pop()
            .expect("release occupied reclaim slot");
        let reclamation_allocations = allocation_counter::measure(|| {
            callback.process_buffer(&mut output);
        });
        assert_eq!(
            reclamation_allocations.count_total, 0,
            "{reclamation_allocations:?}"
        );
        assert_eq!(
            reclamation_allocations.count_current, 0,
            "{reclamation_allocations:?}"
        );
        assert_eq!(
            reclamation_allocations.bytes_total, 0,
            "{reclamation_allocations:?}"
        );
        assert_eq!(
            reclamation_allocations.bytes_current, 0,
            "{reclamation_allocations:?}"
        );
        assert!(callback.pending_reclaim.is_none());
        assert!(!publication_in_flight.load(Ordering::Acquire));
        drop(occupied_reclaim_slot);
    }

    #[test]
    fn pause_resume_commands_are_consumed_during_a_publication_transition() {
        let document = crate::dsl::parse_project(
            std::path::Path::new("project.crump"),
            include_str!("../tests/fixtures/minimal.crump"),
        );
        let project = document.project.expect("parsed fixture");
        let compiled = crate::engine::CompiledProject::compile(&project).expect("compile");
        let (mut commands, command_consumer) = RingBuffer::new(4);
        let (_midi, midi_consumer) = RingBuffer::new(4);
        let (reclaimed, _reclaim_consumer) = RingBuffer::new(2);
        let (telemetry, _telemetry_consumer) = RingBuffer::new(2);
        let mut callback = CallbackState {
            current: Box::new(compiled.runtime()),
            current_generation: 0,
            transition: None,
            pending_reclaim: None,
            publication_in_flight: Arc::new(AtomicBool::new(true)),
            commands: command_consumer,
            midi: midi_consumer,
            reclaimed,
            telemetry,
            callbacks: 0,
            late_callbacks: 0,
            callback_utilization: 0.0,
            peak_callback_utilization: 0.0,
            utilization_histogram: [0; 256],
            processing_errors: 0,
            midi_events: 0,
            midi_was_connected: false,
            paused: true,
            stopping: false,
            stream_status: Arc::new(AtomicU8::new(encode_status(StreamStatus::Streaming))),
            midi_connected: Arc::new(AtomicBool::new(false)),
            midi_dropped: Arc::new(AtomicU64::new(0)),
            midi_panic: Arc::new(AtomicBool::new(false)),
            overload_guard: None,
        };
        callback.begin_transition(Box::new(compiled.runtime()), 1);
        assert!(callback.transition.is_some());
        assert_eq!(callback.current_generation, 0);

        commands.push(AudioCommand::SetPaused(false)).unwrap();
        callback.consume_commands();

        assert!(!callback.paused);
        assert!(callback.transition.is_some());
    }

    #[test]
    fn overload_guard_trips_on_one_uninterrupted_late_run_and_latches() {
        let ms = std::time::Duration::from_millis;
        let mut guard = OverloadGuard::new(ms(100));
        // Late callbacks run back to back; on-time ones let the thread sleep and reset the run.
        for _ in 0..3 {
            guard.finish(ms(30), ms(3));
        }
        assert!(!guard.tripped);
        guard.finish(ms(2), ms(3));
        assert_eq!(guard.run, std::time::Duration::ZERO);
        for _ in 0..3 {
            guard.finish(ms(30), ms(3));
        }
        assert!(!guard.tripped);
        // The fourth late callback in the run is cut short mid-buffer at the limit.
        assert!(!guard.check(ms(9)));
        assert!(guard.check(ms(10)));
        guard.finish(ms(2), ms(3));
        assert!(
            guard.tripped,
            "an overload latches until the player is replaced"
        );
    }

    #[test]
    fn a_tripped_overload_guard_silences_the_callback_and_freezes_transport() {
        let compiled = maximum_compiled_project();
        let (_commands, command_consumer) = RingBuffer::new(4);
        let (_midi, midi_consumer) = RingBuffer::new(4);
        let (reclaimed, _reclaim_consumer) = RingBuffer::new(1);
        let (telemetry, mut telemetry_consumer) = RingBuffer::new(16);
        let mut callback = callback_state(
            compiled.runtime(),
            command_consumer,
            midi_consumer,
            reclaimed,
            telemetry,
            Arc::new(AtomicBool::new(false)),
        );
        callback.overload_guard = Some(OverloadGuard::new(std::time::Duration::ZERO));
        let mut output = [0xff_u8; 256 * FRAME_BYTES];

        callback.process_buffer(&mut output);
        let first = telemetry_consumer.pop().expect("first callback telemetry");
        assert!(first.overloaded);
        // Only the frames before the first mid-buffer check were processed.
        assert_eq!(first.engine.frame, OVERLOAD_CHECK_FRAMES as u64 - 1);
        assert!(
            output[OVERLOAD_CHECK_FRAMES * FRAME_BYTES..]
                .iter()
                .all(|byte| *byte == 0),
            "frames after the trip are silence"
        );

        output.fill(0xff);
        callback.process_buffer(&mut output);
        let second = telemetry_consumer.pop().expect("second callback telemetry");
        assert!(second.overloaded);
        assert_eq!(second.engine.frame, first.engine.frame);
        assert!(output.iter().all(|byte| *byte == 0));
    }

    #[test]
    fn callback_channel_identity_is_fixed_capacity_and_exact() {
        let identity = AuditionChannelIdentity::new("return/room").expect("identity");
        assert_eq!(identity.as_str(), "return/room");
        let oversized = "x".repeat(MAX_AUDITION_CHANNEL_IDENTITY_BYTES + 1);
        assert!(matches!(
            AuditionChannelIdentity::new(&oversized),
            Err(RealtimeError::ChannelIdentityTooLong { .. })
        ));
    }
}
