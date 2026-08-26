//! Panic-contained persistent background work for the workspace control plane.

use std::{
    collections::VecDeque,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// Non-blocking completion state for one owned background operation.
pub(super) enum BackgroundPoll<T> {
    Pending,
    Complete { result: T, ready_for: Duration },
    Failed(String),
}

enum BackgroundMessage<T> {
    Complete(T, Instant),
    Panicked,
}

type Job<T> = Box<dyn FnOnce() -> T + Send + 'static>;

/// One persistent, capacity-one builder thread.
///
/// The terminal thread retains latest-intent flags and submits only while this
/// worker is idle, so filesystem bursts cannot create an unbounded queue.
pub(super) struct BackgroundWorker<T> {
    requests: Option<SyncSender<Job<T>>>,
    results: Receiver<BackgroundMessage<T>>,
    handle: Option<JoinHandle<()>>,
    busy: bool,
}

impl<T: Send + 'static> BackgroundWorker<T> {
    pub(super) fn spawn(name: &str) -> Result<Self, std::io::Error> {
        let (request_tx, request_rx) = mpsc::sync_channel::<Job<T>>(1);
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        let handle = thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                while let Ok(job) = request_rx.recv() {
                    let message = match catch_unwind(AssertUnwindSafe(job)) {
                        Ok(result) => BackgroundMessage::Complete(result, Instant::now()),
                        Err(_) => BackgroundMessage::Panicked,
                    };
                    if result_tx.send(message).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            requests: Some(request_tx),
            results: result_rx,
            handle: Some(handle),
            busy: false,
        })
    }

    pub(super) fn submit(
        &mut self,
        job: impl FnOnce() -> T + Send + 'static,
    ) -> Result<(), &'static str> {
        if self.busy {
            return Err("background worker is busy");
        }
        let Some(requests) = self.requests.as_ref() else {
            return Err("background worker is disconnected");
        };
        match requests.try_send(Box::new(job)) {
            Ok(()) => {
                self.busy = true;
                Ok(())
            }
            Err(TrySendError::Full(_)) => Err("background worker queue is full"),
            Err(TrySendError::Disconnected(_)) => Err("background worker is disconnected"),
        }
    }

    pub(super) fn poll(&mut self) -> BackgroundPoll<T> {
        if !self.busy {
            return BackgroundPoll::Pending;
        }
        match self.results.try_recv() {
            Ok(BackgroundMessage::Complete(result, completed_at)) => {
                self.busy = false;
                BackgroundPoll::Complete {
                    result,
                    ready_for: completed_at.elapsed(),
                }
            }
            Ok(BackgroundMessage::Panicked) => {
                self.busy = false;
                BackgroundPoll::Failed("background operation panicked".to_owned())
            }
            Err(TryRecvError::Empty) => BackgroundPoll::Pending,
            Err(TryRecvError::Disconnected) => {
                self.busy = false;
                BackgroundPoll::Failed("background worker disconnected".to_owned())
            }
        }
    }

    pub(super) const fn busy(&self) -> bool {
        self.busy
    }
}

impl<T> Drop for BackgroundWorker<T> {
    fn drop(&mut self) {
        self.requests.take();
        // A Project-scale operation may still be reading media. Detach instead
        // of blocking terminal restoration; the worker exits after that owned
        // operation observes the disconnected result channel.
        self.handle.take();
    }
}

const RETIRE_CHANNEL_CAPACITY: usize = 8;
const RETIRE_BACKLOG_LIMIT: usize = 8;
type RetiredValue = Box<dyn Send>;

/// Bounded, nonblocking destruction handoff for large snapshots and runtimes.
pub(super) struct RetireQueue {
    sender: Option<SyncSender<RetiredValue>>,
    backlog: VecDeque<RetiredValue>,
    handle: Option<JoinHandle<()>>,
}

impl RetireQueue {
    pub(super) fn spawn(name: &str) -> Result<Self, std::io::Error> {
        let (sender, receiver) = mpsc::sync_channel::<RetiredValue>(RETIRE_CHANNEL_CAPACITY);
        let handle = thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                while let Ok(value) = receiver.recv() {
                    let _ = catch_unwind(AssertUnwindSafe(|| drop(value)));
                }
            })?;
        Ok(Self {
            sender: Some(sender),
            backlog: VecDeque::with_capacity(RETIRE_BACKLOG_LIMIT),
            handle: Some(handle),
        })
    }

    pub(super) fn retire<T: Send + 'static>(&mut self, value: T) {
        self.flush();
        let value: RetiredValue = Box::new(value);
        let Some(sender) = self.sender.as_ref() else {
            self.backlog.push_back(value);
            return;
        };
        match sender.try_send(value) {
            Ok(()) => {}
            Err(TrySendError::Full(value) | TrySendError::Disconnected(value)) => {
                self.backlog.push_back(value);
            }
        }
    }

    pub(super) fn flush(&mut self) {
        let Some(sender) = self.sender.as_ref() else {
            return;
        };
        while let Some(value) = self.backlog.pop_front() {
            match sender.try_send(value) {
                Ok(()) => {}
                Err(TrySendError::Full(value) | TrySendError::Disconnected(value)) => {
                    self.backlog.push_front(value);
                    break;
                }
            }
        }
    }

    pub(super) fn at_capacity(&self) -> bool {
        self.backlog.len() >= RETIRE_BACKLOG_LIMIT
    }

    pub(super) fn pending(&self) -> bool {
        !self.backlog.is_empty()
    }
}

impl Drop for RetireQueue {
    fn drop(&mut self) {
        self.sender.take();
        self.handle.take();
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{mpsc, Arc, Barrier},
        time::Duration,
    };

    use super::*;

    #[test]
    fn submission_and_polling_never_wait_for_the_job() {
        let gate = Arc::new(Barrier::new(2));
        let worker_gate = Arc::clone(&gate);
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let mut worker = BackgroundWorker::spawn("background-test")
            .unwrap_or_else(|error| panic!("spawn background test: {error}"));
        worker
            .submit(move || {
                started_tx.send(()).ok();
                worker_gate.wait();
                42
            })
            .expect("submit background test");
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap_or_else(|error| panic!("background test did not start: {error}"));

        assert!(matches!(worker.poll(), BackgroundPoll::Pending));
        gate.wait();
        loop {
            match worker.poll() {
                BackgroundPoll::Pending => std::thread::yield_now(),
                BackgroundPoll::Complete { result, .. } => {
                    assert_eq!(result, 42);
                    break;
                }
                BackgroundPoll::Failed(error) => panic!("background test failed: {error}"),
            }
        }
    }

    #[test]
    fn retire_handoff_is_nonblocking_and_exposes_bounded_backpressure() {
        struct SlowDrop {
            started: mpsc::SyncSender<()>,
            gate: Arc<Barrier>,
        }

        impl Drop for SlowDrop {
            fn drop(&mut self) {
                self.started.send(()).ok();
                self.gate.wait();
            }
        }

        let mut queue = RetireQueue::spawn("retire-backpressure-test")
            .unwrap_or_else(|error| panic!("spawn reclaimer: {error}"));
        let gate = Arc::new(Barrier::new(2));
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        queue.retire(SlowDrop {
            started: started_tx,
            gate: Arc::clone(&gate),
        });
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("reclaimer began the blocking destruction");
        for value in 0..(RETIRE_CHANNEL_CAPACITY + RETIRE_BACKLOG_LIMIT) {
            queue.retire(value);
        }
        assert!(queue.at_capacity());
        assert!(queue.pending());
        gate.wait();
    }

    #[test]
    fn panics_are_reported_and_the_worker_accepts_the_next_job() {
        let mut worker = BackgroundWorker::spawn("background-panic-test")
            .unwrap_or_else(|error| panic!("spawn background panic test: {error}"));
        worker
            .submit(|| -> u8 { panic!("injected background panic") })
            .expect("submit panicking job");

        loop {
            match worker.poll() {
                BackgroundPoll::Pending => std::thread::yield_now(),
                BackgroundPoll::Complete { .. } => panic!("panicking job completed"),
                BackgroundPoll::Failed(error) => {
                    assert!(error.contains("panicked"));
                    break;
                }
            }
        }
        worker.submit(|| 7).expect("submit recovery job");
        loop {
            match worker.poll() {
                BackgroundPoll::Pending => std::thread::yield_now(),
                BackgroundPoll::Complete { result, .. } => {
                    assert_eq!(result, 7);
                    break;
                }
                BackgroundPoll::Failed(error) => panic!("recovery job failed: {error}"),
            }
        }
    }
}
