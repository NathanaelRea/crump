//! Native shared stereo processors used by instrument, channel, and Master chains.
//!
//! Programs are compiled away from the callback. Mutable runtimes own every
//! delay line and detector buffer up front; one-sample processing performs no
//! allocation, locking, graph construction, or source interpretation.

mod delay;
mod dynamics;
mod eq;
mod nonlinear;
mod program;
mod reverb;
mod runtime;
mod utility;

use delay::*;
use dynamics::*;
use eq::*;
use nonlinear::*;
use reverb::*;
use utility::*;

pub(crate) use program::*;
pub(crate) use runtime::ProcessorRuntime;

#[cfg(test)]
mod tests;
