//! Cooperative interruption of long offline work by termination signals.
//!
//! A terminated process cannot explain itself or remove what it was writing. A command that may
//! run for minutes instead asks for an [`Interrupt`] and polls it between blocks of work, so a
//! `SIGINT`, `SIGTERM` or `SIGHUP` ends in an ordinary reported failure with staging removed.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};

const SIGNALS: [i32; 3] = [SIGINT, SIGTERM, SIGHUP];

/// A request to stop, raised by a termination signal and polled by long-running work.
#[derive(Debug, Clone, Default)]
pub struct Interrupt {
    signal: Arc<AtomicUsize>,
}

impl Interrupt {
    /// An interrupt that never fires, for callers that own no process signals.
    #[must_use]
    pub fn never() -> Self {
        Self::default()
    }

    /// Catches `SIGINT`, `SIGTERM` and `SIGHUP` for the rest of the process.
    ///
    /// The first signal only raises the request. A second one terminates the process the usual
    /// way, so work that stopped polling can still be killed. Handlers are installed once; every
    /// call returns the same process-wide request.
    ///
    /// # Errors
    ///
    /// Returns the operating-system error when a handler cannot be installed.
    pub fn on_termination_signals() -> std::io::Result<Self> {
        static INSTALLED: Mutex<Option<Interrupt>> = Mutex::new(None);
        let mut installed = INSTALLED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(interrupt) = installed.as_ref() {
            return Ok(interrupt.clone());
        }
        let interrupt = Self::default();
        let raised = Arc::new(AtomicBool::new(false));
        for signal in SIGNALS {
            // Handlers run in registration order: terminate if a request is already pending,
            // otherwise record it.
            signal_hook::flag::register_conditional_default(signal, Arc::clone(&raised))?;
            signal_hook::flag::register(signal, Arc::clone(&raised))?;
            signal_hook::flag::register_usize(
                signal,
                Arc::clone(&interrupt.signal),
                usize::try_from(signal).unwrap_or(usize::MAX),
            )?;
        }
        *installed = Some(interrupt.clone());
        Ok(interrupt)
    }

    /// The signal that asked this process to stop, if one arrived.
    #[must_use]
    pub fn requested(&self) -> Option<Signal> {
        match self.signal.load(Ordering::Relaxed) {
            0 => None,
            number => Some(Signal(i32::try_from(number).unwrap_or(i32::MAX))),
        }
    }

    /// Raises the request as `signal` would, without delivering a signal.
    #[cfg(test)]
    pub(crate) fn raise(&self, signal: i32) {
        self.signal.store(
            usize::try_from(signal).unwrap_or(usize::MAX),
            Ordering::Relaxed,
        );
    }
}

/// A termination signal number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Signal(i32);

impl Signal {
    /// The raw signal number.
    #[must_use]
    pub const fn number(self) -> i32 {
        self.0
    }

    /// The conventional shell exit status for a process ended by this signal.
    #[must_use]
    pub fn exit_code(self) -> u8 {
        u8::try_from(128_i32.saturating_add(self.0)).unwrap_or(u8::MAX)
    }
}

impl std::fmt::Display for Signal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            SIGINT => formatter.write_str("SIGINT"),
            SIGTERM => formatter.write_str("SIGTERM"),
            SIGHUP => formatter.write_str("SIGHUP"),
            number => write!(formatter, "signal {number}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_raised_interrupt_reports_its_signal_and_shell_exit_status() {
        let interrupt = Interrupt::never();
        assert_eq!(interrupt.requested(), None);
        let seen_by_worker = interrupt.clone();
        interrupt.raise(SIGTERM);
        let signal = seen_by_worker.requested().expect("raised");
        assert_eq!(signal.to_string(), "SIGTERM");
        assert_eq!(signal.exit_code(), 143);
    }
}
