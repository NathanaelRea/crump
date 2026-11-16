//! Exact Project reconstruction and derived-view preparation away from the UI thread.

use std::{path::PathBuf, sync::Arc, time::Duration};

use crate::{
    engine::{control::ControlInspector, mix_graph::MixGraphInspection},
    history::{HistoryEntry, HistoryStore},
    project::{ProjectBundle, RevisionId, load_project},
};

use super::{
    MixerParameterView, WorkspaceWatch, candidate_instrument_sections, candidate_mixer_parameters,
    candidate_runtime_inspection, instrument::InstrumentSectionView, timeline::TimelineCache,
};

pub(super) struct CandidateDerivedState {
    pub(super) processing_graph: Option<MixGraphInspection>,
    pub(super) control_inspector: Option<ControlInspector>,
    pub(super) instrument_sections: Vec<Vec<InstrumentSectionView>>,
    pub(super) mixer_parameters: Vec<MixerParameterView>,
    pub(super) timeline: TimelineCache,
}

pub(super) struct CandidateUpdate {
    pub(super) bundle: ProjectBundle,
    pub(super) watch: WorkspaceWatch,
    pub(super) stable: bool,
    pub(super) derived: Option<CandidateDerivedState>,
    pub(super) load_elapsed: Duration,
    pub(super) derive_elapsed: Option<Duration>,
}

pub(super) enum CandidateReload {
    Loaded(Box<CandidateUpdate>),
    Verified(Box<CandidateUpdate>),
    Failed {
        error: String,
        load_elapsed: Duration,
    },
}

pub(super) fn load_candidate(
    root: PathBuf,
    previous_revision: RevisionId,
    before: WorkspaceWatch,
) -> CandidateReload {
    let started = std::time::Instant::now();
    let bundle = match load_project(&root) {
        Ok(bundle) => bundle,
        Err(error) => {
            return CandidateReload::Failed {
                error: error.to_string(),
                load_elapsed: started.elapsed(),
            };
        }
    };
    let load_elapsed = started.elapsed();
    let watch = WorkspaceWatch::capture_candidate(&root, &bundle);
    let stable = before.same_candidate(&watch);
    let (derived, derive_elapsed) = if !stable || bundle.revision == previous_revision {
        (None, None)
    } else {
        let started = std::time::Instant::now();
        let (processing_graph, control_inspector): (
            Option<MixGraphInspection>,
            Option<ControlInspector>,
        ) = candidate_runtime_inspection(&bundle);
        let instrument_sections = candidate_instrument_sections(&bundle);
        let mixer_parameters = candidate_mixer_parameters(&bundle);
        let mut timeline = TimelineCache::default();
        timeline.rebuild(bundle.publishable(), &bundle.assets);
        (
            Some(CandidateDerivedState {
                processing_graph,
                control_inspector,
                instrument_sections,
                mixer_parameters,
                timeline,
            }),
            Some(started.elapsed()),
        )
    };
    CandidateReload::Loaded(Box::new(CandidateUpdate {
        bundle,
        watch,
        stable,
        derived,
        load_elapsed,
        derive_elapsed,
    }))
}

pub(super) fn verify_candidate(mut update: Box<CandidateUpdate>) -> CandidateReload {
    let verified_watch = update.watch.recapture_candidate();
    update.stable &= update.watch.same_candidate(&verified_watch);
    update.watch = verified_watch;
    CandidateReload::Verified(update)
}

pub(super) struct HistoryState {
    pub(super) active_entry: Option<String>,
    pub(super) active_revision: Option<RevisionId>,
    pub(super) entries: Vec<HistoryEntry>,
    /// `None` retains the cached bundle; `Some(None)` clears it.
    pub(super) active_bundle_update: Option<Option<Arc<ProjectBundle>>>,
}

pub(super) struct HistoryUpdate {
    pub(super) state: HistoryState,
    pub(super) watch: WorkspaceWatch,
    pub(super) stable: bool,
    pub(super) load_elapsed: Duration,
}

pub(super) enum HistoryReload {
    Loaded(Box<HistoryUpdate>),
    Verified(Box<HistoryUpdate>),
    Failed {
        error: String,
        load_elapsed: Duration,
    },
}

/// Results from one serialized Project-scale workspace reconstruction pass.
pub(super) struct ReloadBatch {
    pub(super) candidate: Option<CandidateReload>,
    pub(super) history: Option<HistoryReload>,
}

pub(super) fn load_history(
    root: PathBuf,
    previous_revision: Option<RevisionId>,
    has_active_bundle: bool,
    before: WorkspaceWatch,
) -> HistoryReload {
    let started = std::time::Instant::now();
    let result = (|| {
        let store = HistoryStore::open(&root)?;
        let active = store.active_state()?;
        let entries = store.list()?;
        let active_revision = active.as_ref().map(|state| state.active_revision.clone());
        let needs_active_bundle = active_revision != previous_revision
            || (active_revision.is_some() && !has_active_bundle);
        let active_bundle_update = if needs_active_bundle {
            Some(store.active_bundle()?.map(Arc::new))
        } else {
            None
        };
        Ok::<_, crate::history::HistoryError>(HistoryState {
            active_entry: active.map(|state| state.active_entry),
            active_revision,
            entries,
            active_bundle_update,
        })
    })();
    let load_elapsed = started.elapsed();
    let state = match result {
        Ok(state) => state,
        Err(error) => {
            return HistoryReload::Failed {
                error: error.to_string(),
                load_elapsed,
            };
        }
    };
    let watch = WorkspaceWatch::capture_history(&root);
    let stable = before.same_history(&watch);
    HistoryReload::Loaded(Box::new(HistoryUpdate {
        state,
        watch,
        stable,
        load_elapsed,
    }))
}

pub(super) fn verify_history(mut update: Box<HistoryUpdate>) -> HistoryReload {
    let verified_watch = update.watch.recapture_history();
    update.stable &= update.watch.same_history(&verified_watch);
    update.watch = verified_watch;
    HistoryReload::Verified(update)
}
