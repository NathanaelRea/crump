//! Compilation and deterministic execution of validated Composable DSP Graphs.
//!
//! The facade keeps graph orchestration private to the engine while the closed native Node
//! programs own their compilation, state, DSP, and resource facts in `graph::nodes`.

mod nodes;
mod runtime;

pub(crate) use runtime::{
    CompiledGraph, GraphProcessIdentity, GraphVoice, SharedGraphRuntime,
    authored_graph_voice_state_bytes, authored_graph_work_units, authored_shared_graph_state_bytes,
    authored_shared_graph_work_units,
};
