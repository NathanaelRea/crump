//! Crump's terminal-independent project, authoring, and audio engine.

pub mod acoustics;
pub mod cli;
pub mod diagnostic;
pub mod dsl;
pub mod dsp;
mod edit;
pub mod engine;
mod graph_schedule;
pub mod history;
pub(crate) mod instrument_module;
pub mod interrupt;
pub mod media;
pub mod model;
mod node_definition;
mod processor_semantics;
pub mod production_view;
pub mod project;
mod project_picker;
pub mod realtime;
pub mod routing;
pub mod time;
pub mod validate;
pub mod workspace;
