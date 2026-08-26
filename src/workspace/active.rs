//! Latest-intent Active audition preparation away from the terminal thread.

use std::{collections::BTreeMap, sync::Arc};

use crate::{
    engine::{
        audition::{AuditionPlan, AuditionPrepareOptions, AuditionStatus},
        control::ControlInspector,
        mix_graph::MixGraphInspection,
        RuntimeTelemetry, SongRuntime,
    },
    project::{ProjectBundle, RevisionId},
    realtime::{PipeWirePlayer, PublicationReservation},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ActiveRequestKey {
    pub(super) active_revision: RevisionId,
    pub(super) audition_epoch: u64,
    pub(super) playback_epoch: u64,
    pub(super) generation: u64,
}

pub(super) enum ActiveMode {
    Arm,
    Start,
    Replace { reservation: PublicationReservation },
}

pub(super) struct ActiveRequest {
    pub(super) key: ActiveRequestKey,
    pub(super) active: Arc<ProjectBundle>,
    pub(super) plan: AuditionPlan,
    pub(super) telemetry: RuntimeTelemetry,
    pub(super) previous_status: Option<AuditionStatus>,
    pub(super) explicit_seek: bool,
    pub(super) mode: ActiveMode,
}

pub(super) struct PreparedActiveInspection {
    pub(super) active: Arc<ProjectBundle>,
    pub(super) status: AuditionStatus,
    pub(super) processing_graph: MixGraphInspection,
    pub(super) control_inspector: ControlInspector,
    pub(super) mix_channel_indexes: BTreeMap<String, usize>,
    pub(super) range_moved: bool,
}

pub(super) enum PreparedActiveMode {
    Armed,
    Started(Box<PipeWirePlayer>),
    Replacement {
        runtime: Box<SongRuntime>,
        reservation: PublicationReservation,
    },
}

pub(super) enum ActiveResult {
    Prepared {
        key: ActiveRequestKey,
        inspection: Box<PreparedActiveInspection>,
        mode: Box<PreparedActiveMode>,
    },
    Failed {
        key: ActiveRequestKey,
        error: String,
    },
}

pub(super) fn prepare_active(request: ActiveRequest) -> ActiveResult {
    let ActiveRequest {
        key,
        active,
        plan,
        telemetry,
        previous_status,
        explicit_seek,
        mode,
    } = request;
    if active.revision != key.active_revision {
        return ActiveResult::Failed {
            key,
            error: "cached Active Revision changed before preparation".to_owned(),
        };
    }
    let Some(project) = active.publishable() else {
        return ActiveResult::Failed {
            key,
            error: "Active Revision snapshot is invalid".to_owned(),
        };
    };

    let previous_range = previous_status
        .as_ref()
        .map(|status| (status.start_frame, status.end_frame));
    let initial_range_offset = if explicit_seek {
        None
    } else {
        previous_status.as_ref().map(|status| {
            let next_frame = telemetry.frame.saturating_add(1);
            if status.looping {
                let length = status.end_frame.saturating_sub(status.start_frame).max(1);
                next_frame.saturating_sub(status.start_frame) % length
            } else {
                next_frame
                    .clamp(status.start_frame, status.end_frame.saturating_sub(1))
                    .saturating_sub(status.start_frame)
            }
        })
    };
    let prepared = match plan.prepare_with_options(
        &active.revision,
        project,
        &active.assets,
        AuditionPrepareOptions {
            initial_frame: None,
            initial_range_offset,
            explicit_seek,
        },
    ) {
        Ok(prepared) => prepared,
        Err(error) => {
            return ActiveResult::Failed {
                key,
                error: error.to_string(),
            };
        }
    };
    let status = prepared.status().clone();
    let processing_graph = prepared.mix_graph_inspection().clone();
    let mix_channel_indexes = processing_graph
        .channels
        .iter()
        .enumerate()
        .map(|(index, channel)| (channel.identity.clone(), index))
        .collect();
    let inspection = PreparedActiveInspection {
        active: Arc::clone(&active),
        range_moved: previous_range
            .is_some_and(|range| range != (status.start_frame, status.end_frame)),
        status,
        processing_graph,
        control_inspector: prepared.control_program().clone().into_inspector(),
        mix_channel_indexes,
    };
    let runtime = prepared.into_runtime();
    let mode = match mode {
        ActiveMode::Arm => {
            drop(runtime);
            PreparedActiveMode::Armed
        }
        ActiveMode::Start => match PipeWirePlayer::start_with_generation(runtime, key.generation) {
            Ok(player) => PreparedActiveMode::Started(Box::new(player)),
            Err(error) => {
                return ActiveResult::Failed {
                    key,
                    error: format!("Playback unavailable: {error}"),
                };
            }
        },
        ActiveMode::Replace { reservation } => {
            #[cfg(feature = "pipewire")]
            let runtime = {
                let mut runtime = runtime;
                runtime.prime_for_publication();
                runtime
            };
            PreparedActiveMode::Replacement {
                runtime: Box::new(runtime),
                reservation,
            }
        }
    };
    ActiveResult::Prepared {
        key,
        inspection: Box::new(inspection),
        mode: Box::new(mode),
    }
}
