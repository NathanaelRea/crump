//! Native shared stereo processors used by instrument, channel, and Master chains.
//!
//! Programs are compiled away from the callback. Mutable runtimes own every
//! delay line and detector buffer up front; one-sample processing performs no
//! allocation, locking, graph construction, or source interpretation.

mod crusher;
mod delay;
mod dynamics;
mod eq;
mod modulation_delay;
mod nonlinear;
mod program;
mod reverb;
mod runtime;
mod stutter;
mod utility;

use crusher::*;
use delay::*;
use dynamics::*;
use eq::*;
use modulation_delay::*;
use nonlinear::*;
use reverb::*;
use stutter::*;
use utility::*;

pub(crate) use program::*;
pub(crate) use runtime::ProcessorRuntime;

#[cfg(test)]
mod tests;
