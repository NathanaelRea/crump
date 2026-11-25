//! Offline timeline driver: Track sources run ahead on worker threads and feed the mix graph.
//!
//! A Track's source depends only on its own state and on the frame's control values. With no
//! Envelope Follower, control values are a function of the frame alone, so every worker can
//! evaluate its own copy and render its Tracks independently. The calling thread keeps the mix
//! graph, and consumes each Track's samples in frame order, so output is identical to
//! [`SongRuntime::process_frame`] whatever the thread count or block size.

use std::{
    ops::Range,
    sync::mpsc::{Receiver, Sender, channel},
    thread,
};

use super::{EngineError, ProcessingMode, SongRuntime, TrackRuntime, control::ControlRuntime};

/// Frames a worker renders before handing them to the mix graph.
const SOURCE_BLOCK_FRAMES: usize = 1_024;
/// Finished blocks a worker may hold ahead of the mix graph.
const SOURCE_BLOCKS_AHEAD: usize = 4;

/// How one offline pass over the remaining timeline is scheduled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfflineThreads(usize);

impl OfflineThreads {
    /// Everything on the calling thread.
    pub const SERIAL: Self = Self(1);

    /// Up to `threads` threads in total, counting the calling thread that runs the mix graph.
    #[must_use]
    pub const fn at_most(threads: usize) -> Self {
        Self(if threads == 0 { 1 } else { threads })
    }

    /// One thread per available CPU.
    #[must_use]
    pub fn available() -> Self {
        Self::at_most(thread::available_parallelism().map_or(1, usize::from))
    }

    const fn workers(self, tracks: usize) -> usize {
        let spare = self.0 - 1;
        if spare < tracks { spare } else { tracks }
    }
}

impl Default for OfflineThreads {
    fn default() -> Self {
        Self::available()
    }
}

/// One worker's Track sources for a run of consecutive frames.
struct SourceBlock {
    frames: usize,
    /// Track-major samples: Track `slot` owns `[slot * capacity..][..frames]`.
    samples: Vec<[f32; 2]>,
    /// Sounding voices per frame, summed over the worker's Tracks.
    voices: Vec<usize>,
    /// Feedback-guard mutes per frame, summed over the worker's Tracks.
    feedback_mutes: Vec<u64>,
}

impl SourceBlock {
    fn new(tracks: usize, capacity: usize) -> Self {
        Self {
            frames: 0,
            samples: vec![[0.0; 2]; tracks * capacity],
            voices: vec![0; capacity],
            feedback_mutes: vec![0; capacity],
        }
    }
}

struct Worker {
    /// Song Track index of each of this worker's Tracks, in slot order.
    track_indexes: Vec<usize>,
    ready: Receiver<SourceBlock>,
    recycle: Sender<SourceBlock>,
}

impl SongRuntime {
    /// Processes the rest of the timeline offline, handing every finished frame to `sink`.
    ///
    /// `sink` runs on the calling thread after the frame is mixed, so it can read channel taps
    /// and telemetry for exactly that frame. Output does not depend on `threads`.
    ///
    /// # Errors
    ///
    /// Returns the first non-finite frame or `sink` failure. Worker threads may already have
    /// rendered later frames by then, so the runtime must not be processed further.
    pub fn process_offline(
        &mut self,
        threads: OfflineThreads,
        mut sink: impl FnMut(&Self, [f32; 2]) -> Result<(), EngineError>,
    ) -> Result<(), EngineError> {
        let workers = threads.workers(self.tracks.len());
        if workers == 0 || !self.sources_can_run_ahead() {
            while !self.finished() {
                let sample = self.process_frame(ProcessingMode::Offline)?;
                sink(self, sample)?;
            }
            return Ok(());
        }
        // Workers borrow the Tracks for the whole pass; the calling thread keeps the rest.
        let mut tracks = std::mem::take(&mut self.tracks);
        let result = self.process_offline_with_workers(&mut tracks, workers, &mut sink);
        self.tracks = tracks;
        result
    }

    /// True when Track sources depend on nothing the mix graph produces.
    ///
    /// An Envelope Follower feeds mixed audio back into the next frame's control values, and an
    /// audition loop rewinds the transport; either keeps every frame on one thread.
    fn sources_can_run_ahead(&self) -> bool {
        self.envelope_followers.route_count() == 0
            && self.audition_loop.is_none()
            && self.raw_audition.is_none()
    }

    fn process_offline_with_workers(
        &mut self,
        tracks: &mut [TrackRuntime],
        workers: usize,
        sink: &mut impl FnMut(&Self, [f32; 2]) -> Result<(), EngineError>,
    ) -> Result<(), EngineError> {
        let track_count = tracks.len();
        let range = self.frame..self.playback_end();
        let mut assigned = (0..workers)
            .map(|_| (Vec::new(), Vec::new()))
            .collect::<Vec<(Vec<usize>, Vec<&mut TrackRuntime>)>>();
        for (index, track) in tracks.iter_mut().enumerate() {
            let (indexes, owned) = &mut assigned[index % workers];
            indexes.push(index);
            owned.push(track);
        }
        thread::scope(|scope| {
            let mut handles = Vec::with_capacity(workers);
            for (track_indexes, owned) in assigned {
                let (ready_sender, ready) = channel();
                let (recycle, recycle_receiver) = channel();
                for _ in 0..SOURCE_BLOCKS_AHEAD {
                    // The receiver is alive; a block that cannot be queued is simply dropped.
                    let _ = recycle.send(SourceBlock::new(owned.len(), SOURCE_BLOCK_FRAMES));
                }
                let controls = self.controls.clone();
                let range = range.clone();
                scope.spawn(move || {
                    render_sources(owned, controls, range, &recycle_receiver, &ready_sender);
                });
                handles.push(Worker {
                    track_indexes,
                    ready,
                    recycle,
                });
            }
            self.mix_worker_sources(&handles, track_count, range, sink)
            // Dropping `handles` closes every channel, which stops workers that are still ahead.
        })
    }

    /// Mixes every frame of `range` from the workers' blocks, in frame order.
    fn mix_worker_sources(
        &mut self,
        workers: &[Worker],
        track_count: usize,
        range: Range<u64>,
        sink: &mut impl FnMut(&Self, [f32; 2]) -> Result<(), EngineError>,
    ) -> Result<(), EngineError> {
        let mut blocks = Vec::with_capacity(workers.len());
        while self.frame < range.end {
            blocks.clear();
            for worker in workers {
                blocks.push(worker.ready.recv().map_err(|_| worker_stopped())?);
            }
            let frames = blocks[0].frames;
            if frames == 0 || blocks.iter().any(|block| block.frames != frames) {
                return Err(worker_stopped());
            }
            for offset in 0..frames {
                if !self.begin_frame() {
                    return Err(worker_stopped());
                }
                self.controls.evaluate_frame(self.frame);
                let mut voices = 0_usize;
                for (worker, block) in workers.iter().zip(&blocks) {
                    for (slot, &index) in worker.track_indexes.iter().enumerate() {
                        self.track_sources[index] =
                            block.samples[slot * SOURCE_BLOCK_FRAMES + offset];
                    }
                    voices = voices.saturating_add(block.voices[offset]);
                    self.telemetry.feedback_mutes = self
                        .telemetry
                        .feedback_mutes
                        .saturating_add(block.feedback_mutes[offset]);
                }
                let sample = self.finish_frame(ProcessingMode::Offline, track_count, voices)?;
                sink(self, sample)?;
            }
            for (worker, block) in workers.iter().zip(blocks.drain(..)) {
                // A worker that already rendered its last frame no longer wants the block.
                let _ = worker.recycle.send(block);
            }
        }
        Ok(())
    }
}

/// Renders `tracks` over `range` in the same per-frame order as the serial path.
fn render_sources(
    mut tracks: Vec<&mut TrackRuntime>,
    mut controls: ControlRuntime,
    range: Range<u64>,
    recycle: &Receiver<SourceBlock>,
    ready: &Sender<SourceBlock>,
) {
    let mut frame = range.start;
    while frame < range.end {
        // The mix graph stopped early: nothing more is wanted.
        let Ok(mut block) = recycle.recv() else {
            return;
        };
        let frames = usize::try_from(range.end - frame)
            .map_or(SOURCE_BLOCK_FRAMES, |left| left.min(SOURCE_BLOCK_FRAMES));
        for offset in 0..frames {
            controls.evaluate_frame(frame);
            let mut voices = 0_usize;
            let mut feedback_mutes = 0_u64;
            for (slot, track) in tracks.iter_mut().enumerate() {
                track.dispatch(frame);
                let (sample, count) = track.process(&controls, frame);
                block.samples[slot * SOURCE_BLOCK_FRAMES + offset] = sample;
                voices = voices.saturating_add(count);
                feedback_mutes = feedback_mutes.saturating_add(track.take_feedback_mutes());
            }
            block.voices[offset] = voices;
            block.feedback_mutes[offset] = feedback_mutes;
            frame += 1;
        }
        block.frames = frames;
        if ready.send(block).is_err() {
            return;
        }
    }
}

fn worker_stopped() -> EngineError {
    EngineError::Compile("an offline Track source worker stopped before the timeline ended".into())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::{
        dsl::parse_project,
        engine::{CompiledProject, EngineError, ProcessingMode, SongRuntime},
        model::TapPoint,
    };

    use super::{OfflineThreads, SOURCE_BLOCK_FRAMES};

    const TWO_TRACKS: &str = include_str!("../../tests/fixtures/minimal.crump");
    const SIDECHAIN: &str = include_str!("../../tests/fixtures/compressor_sidechain.crump");
    const FOLLOWER: &str = include_str!("../../tests/fixtures/production_mix.crump");

    fn compile(source: &str) -> CompiledProject {
        let document = parse_project(Path::new("project.crump"), source);
        assert!(
            !document.diagnostics.has_errors(),
            "{:?}",
            document.diagnostics.as_slice()
        );
        CompiledProject::compile(&document.project.expect("parsed fixture")).expect("compile")
    }

    /// Everything a frame makes observable: Master, every channel tap, and telemetry.
    #[derive(Debug, PartialEq)]
    struct Frame {
        master: [u32; 2],
        taps: Vec<[u32; 2]>,
        telemetry: String,
    }

    fn observe(runtime: &SongRuntime, master: [f32; 2]) -> Frame {
        let bits = |sample: [f32; 2]| [sample[0].to_bits(), sample[1].to_bits()];
        let telemetry = runtime.telemetry();
        Frame {
            master: bits(master),
            taps: (0..telemetry.channel_count)
                .flat_map(|index| {
                    [TapPoint::PreFader, TapPoint::PostFader]
                        .map(|tap| bits(runtime.channel_tap_sample(index, tap).expect("tap")))
                })
                .collect(),
            telemetry: format!(
                "{} {} {} {} {} {:?}",
                telemetry.frame,
                telemetry.active_voices,
                telemetry.peak.to_bits(),
                telemetry.current_peak.to_bits(),
                telemetry.feedback_mutes,
                &telemetry.channel_activity[..telemetry.channel_count],
            ),
        }
    }

    fn frame_by_frame(compiled: &CompiledProject) -> Vec<Frame> {
        let mut runtime = compiled.runtime();
        let mut frames = Vec::new();
        while !runtime.finished() {
            let master = runtime
                .process_frame(ProcessingMode::Offline)
                .expect("frame");
            frames.push(observe(&runtime, master));
        }
        frames
    }

    fn offline(compiled: &CompiledProject, threads: usize) -> Vec<Frame> {
        let mut runtime = compiled.runtime();
        let mut frames = Vec::new();
        runtime
            .process_offline(OfflineThreads::at_most(threads), |runtime, master| {
                frames.push(observe(runtime, master));
                Ok(())
            })
            .expect("offline pass");
        assert!(runtime.finished());
        assert_eq!(runtime.tracks.len(), compiled.tracks.len());
        frames
    }

    #[test]
    fn worker_sources_match_the_frame_by_frame_path_for_every_thread_count() {
        for source in [TWO_TRACKS, SIDECHAIN] {
            let compiled = compile(source);
            let runtime = compiled.runtime();
            assert!(runtime.sources_can_run_ahead());
            assert_eq!(runtime.tracks.len(), 2);
            let expected = frame_by_frame(&compiled);
            // The timeline must cross block boundaries and end inside a block.
            assert!(expected.len() > 4 * SOURCE_BLOCK_FRAMES);
            assert_ne!(expected.len() % SOURCE_BLOCK_FRAMES, 0);
            assert!(expected.iter().any(|frame| frame.master != [0, 0]));
            // One thread stays serial, two share one worker, three give each Track its own,
            // and more threads than Tracks leave the extra ones unused.
            for threads in [0, 1, 2, 3, 16] {
                assert!(
                    offline(&compiled, threads) == expected,
                    "{threads} threads changed the output"
                );
            }
        }
    }

    #[test]
    fn an_envelope_follower_keeps_every_frame_on_one_thread_with_identical_output() {
        let compiled = compile(FOLLOWER);
        assert!(!compiled.runtime().sources_can_run_ahead());
        let expected = frame_by_frame(&compiled);
        assert!(offline(&compiled, 8) == expected);
    }

    #[test]
    fn a_pass_can_start_mid_timeline() {
        let compiled = compile(TWO_TRACKS);
        let start = 3 * SOURCE_BLOCK_FRAMES as u64 + 17;
        let mut serial = compiled.runtime();
        serial.advance_to(start).expect("advance");
        let mut expected = Vec::new();
        while !serial.finished() {
            let master = serial
                .process_frame(ProcessingMode::Offline)
                .expect("frame");
            expected.push(observe(&serial, master));
        }
        let mut threaded = compiled.runtime();
        threaded.advance_to(start).expect("advance");
        let mut actual = Vec::new();
        threaded
            .process_offline(OfflineThreads::at_most(3), |runtime, master| {
                actual.push(observe(runtime, master));
                Ok(())
            })
            .expect("offline pass");
        assert!(actual == expected);
    }

    #[test]
    fn a_non_finite_frame_fails_at_the_same_frame_and_stops_the_workers() {
        let compiled = compile(TWO_TRACKS);
        for threads in [1, 3] {
            let mut runtime = compiled.runtime();
            runtime.mix.poison_track_level_for_test(0);
            let mut seen = 0_u64;
            let error = runtime
                .process_offline(OfflineThreads::at_most(threads), |_, _| {
                    seen += 1;
                    Ok(())
                })
                .expect_err("non-finite output");
            assert!(matches!(error, EngineError::NonFinite { frame: 0 }));
            assert_eq!(seen, 0);
            assert_eq!(runtime.tracks.len(), 2);
        }
    }

    #[test]
    fn a_failing_sink_ends_the_pass_with_its_error() {
        let compiled = compile(TWO_TRACKS);
        for threads in [1, 3] {
            let mut runtime = compiled.runtime();
            let mut seen = 0_usize;
            let error = runtime
                .process_offline(OfflineThreads::at_most(threads), |_, _| {
                    seen += 1;
                    if seen == SOURCE_BLOCK_FRAMES + 5 {
                        return Err(EngineError::Compile("sink stopped".into()));
                    }
                    Ok(())
                })
                .expect_err("sink failure");
            assert_eq!(error.to_string(), "audio compilation failed: sink stopped");
            assert_eq!(seen, SOURCE_BLOCK_FRAMES + 5);
            assert_eq!(runtime.frame(), (SOURCE_BLOCK_FRAMES + 5) as u64);
        }
    }
}
