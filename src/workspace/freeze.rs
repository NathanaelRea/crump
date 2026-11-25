//! Background Track Freeze capture, off the workspace builder so edits and playback never wait.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::engine::{FreezeJob, FreezeStandIn};

/// Threads left for the audio callback and the window while a capture runs.
const RESERVED_THREADS: usize = 2;
/// Nice value for capture threads, so they yield to the window and other interactive work.
const CAPTURE_NICE: i32 = 10;

type Identity = [u8; 32];

/// What became of one submitted capture.
pub(super) struct FreezeOutcome {
    pub(super) tracks: Vec<String>,
    pub(super) stand_in: FreezeStandIn,
    /// `Ok(false)` when a newer capture superseded this one.
    pub(super) result: Result<bool, String>,
    pub(super) elapsed: Duration,
}

struct Request {
    identity: Identity,
    job: FreezeJob,
    cancel: Arc<AtomicBool>,
}

struct Completion {
    identity: Identity,
    outcome: FreezeOutcome,
}

/// One persistent capture thread that runs only the newest submitted job.
pub(super) struct FreezeBuilder {
    requests: Option<Sender<Request>>,
    results: Receiver<Completion>,
    current: Option<(Identity, Arc<AtomicBool>)>,
    handle: Option<JoinHandle<()>>,
}

impl FreezeBuilder {
    pub(super) fn spawn(name: &str) -> Result<Self, std::io::Error> {
        let (request_tx, request_rx) = mpsc::channel::<Request>();
        let (result_tx, result_rx) = mpsc::channel();
        let threads = thread::available_parallelism()
            .map_or(1, usize::from)
            .saturating_sub(RESERVED_THREADS)
            .max(1);
        let handle = thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                // Threads the capture spawns inherit this priority.
                let _ = rustix::process::setpriority_process(
                    Some(rustix::thread::gettid()),
                    CAPTURE_NICE,
                );
                while let Ok(request) = request_rx.recv() {
                    if request.cancel.load(Ordering::Relaxed) {
                        continue;
                    }
                    let started = Instant::now();
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        request.job.run(threads, &request.cancel)
                    }));
                    let result = match result {
                        Ok(Ok(captured)) => Ok(captured.is_some()),
                        Ok(Err(error)) => Err(error.to_string()),
                        Err(_) => Err("Track Freeze capture panicked".to_owned()),
                    };
                    let completion = Completion {
                        identity: request.identity,
                        outcome: FreezeOutcome {
                            tracks: request.job.tracks().map(str::to_owned).collect(),
                            stand_in: request.job.stand_in(),
                            result,
                            elapsed: started.elapsed(),
                        },
                    };
                    if result_tx.send(completion).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            requests: Some(request_tx),
            results: result_rx,
            current: None,
            handle: Some(handle),
        })
    }

    /// Starts `job`, superseding any other capture. Returns `false` when `job` is already running.
    pub(super) fn submit(&mut self, job: FreezeJob) -> bool {
        let identity = job.identity();
        if self
            .current
            .as_ref()
            .is_some_and(|(current, _)| *current == identity)
        {
            return false;
        }
        self.cancel();
        let Some(requests) = self.requests.as_ref() else {
            return false;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let request = Request {
            identity,
            job,
            cancel: Arc::clone(&cancel),
        };
        if requests.send(request).is_err() {
            return false;
        }
        self.current = Some((identity, cancel));
        true
    }

    /// Stops the running capture, if any.
    pub(super) fn cancel(&mut self) {
        if let Some((_, cancel)) = self.current.take() {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    pub(super) const fn busy(&self) -> bool {
        self.current.is_some()
    }

    /// The current capture's outcome once it finishes. Superseded captures are discarded.
    pub(super) fn poll(&mut self) -> Option<FreezeOutcome> {
        loop {
            match self.results.try_recv() {
                Ok(completion) => {
                    if self
                        .current
                        .as_ref()
                        .is_some_and(|(current, _)| *current == completion.identity)
                    {
                        self.current = None;
                        return Some(completion.outcome);
                    }
                }
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => {
                    self.current = None;
                    return None;
                }
            }
        }
    }
}

impl Drop for FreezeBuilder {
    fn drop(&mut self) {
        self.cancel();
        self.requests.take();
        // The capture observes cancellation within a few thousand frames; do not block exit.
        self.handle.take();
    }
}
