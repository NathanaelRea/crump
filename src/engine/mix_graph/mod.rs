//! Compiled, inspectable processing graph shared by playback and rendering.
//!
//! Compilation lowers authored routing into a deterministic acyclic program;
//! runtime state and admission accounting remain shared by playback and rendering.

mod admission;
pub(super) use admission::channel_and_master_processors;
mod compile;
mod inspection;
mod lowering;
mod runtime;

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use serde::{Deserialize, Serialize};

use crate::{
    model::{
        InstrumentKind, Panner, ParameterTarget, ProcessorKind, Project, TapPoint, TrackParameter,
        Unit,
    },
    processor_semantics::{ResolvedSettingValue, resolve_settings},
    routing::{
        AuthoredTopology, ChannelDeclaration, ProcessorOwner, RouteDeclaration, RoutingFindingKind,
        TopologyDestination, TopologyRouteRole,
    },
    validate::{MAX_CONTROL_TARGETS, MAX_GRAPH_VOICES},
};

use super::{
    EngineError, GRAPH_VOICE_STORAGE_MULTIPLIER, SongRuntime, TrackRuntime,
    control::{CompiledControlProgram, ControlIndex},
    drums::DrumSynth,
    graph::{
        CompiledGraph, authored_graph_voice_state_bytes, authored_graph_work_units,
        authored_shared_graph_state_bytes, authored_shared_graph_work_units,
    },
    graph_voice_slot_overhead_bytes,
    processors::{
        ProcessorAssets, ProcessorProgram, ProcessorRuntime, authored_processor_estimate,
    },
    sampler::sampler_voice_state_bytes,
    schedule::{MAX_RUNTIME_TRACKS, compile_schedules},
};

use inspection::*;
use lowering::*;
use runtime::*;

pub use crate::routing::ChannelRole;
pub use admission::{
    AdmissionFailure, AdmissionLimit, AdmissionProfile, MAX_MIX_CHANNELS, MAX_PROCESSOR_INSTANCES,
    MAX_PUBLICATION_RUNTIME_BYTES, MAX_RESIDENT_PUBLICATION_RUNTIMES, MAX_WORK_UNITS_PER_FRAME,
    RuntimeResourceEstimate, SUPPORTED_QUANTA, describe_admission_failures,
};
pub use compile::CompiledMixGraph;
pub use inspection::{
    CompiledChannelInspection, CompiledProcessorInspection, CompiledProcessorSettingInspection,
    CompiledRouteInspection, MixGraphInspection, PannerMode, ProcessorStatePolicy, RouteRole,
    SignalPathInspection,
};
pub(crate) use runtime::MixGraphRuntime;
pub use runtime::{ChannelMeter, ProcessorMeter};

#[cfg(test)]
mod tests;
