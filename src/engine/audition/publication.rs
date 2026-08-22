//! Active Revision refresh and bounded runtime publication policy.

use std::path::Path;

use crate::{
    engine::{RuntimeTelemetry, SongRuntime, mix_graph::MixGraphInspection},
    history::HistoryStore,
    project::RevisionId,
};

use super::{AuditionPlan, AuditionPrepareOptions, AuditionStatus, PreparedAudition};

/// Failure reported by a runtime publication adapter.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuntimePublicationError {
    /// The bounded replacement slot is occupied and the newest Active Revision should retry.
    #[error("runtime publication is busy")]
    Busy,
    /// The backend cannot accept the runtime.
    #[error("{0}")]
    Failed(String),
}

/// Engine-owned port for bounded replacement publication.
///
/// Implementations reserve before compilation allocates a replacement. A reservation must be
/// released by either `publish_reserved` or `cancel_publication`.
pub trait RuntimePublicationPort {
    /// Reserves the sole replacement slot.
    fn reserve_publication(&mut self) -> Result<(), RuntimePublicationError>;

    /// Publishes a runtime built while the replacement slot was reserved.
    fn publish_reserved(&mut self, runtime: SongRuntime) -> Result<(), RuntimePublicationError>;

    /// Releases an unused replacement reservation.
    fn cancel_publication(&mut self);
}

/// Result of refreshing playback from the Active Revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveRefresh {
    /// No Active Revision exists.
    NoActiveRevision,
    /// Publication is bounded; the requested revision remains pending for retry.
    Delayed,
    /// The Active Revision changed while it was being reloaded; the returned revision is pending.
    ActiveChanged(RevisionId),
    /// The Active Revision snapshot could not be loaded.
    ReloadFailed(String),
    /// The Active Revision snapshot was unexpectedly invalid.
    InvalidActiveRevision,
    /// The armed plan could not be resolved against the new Active Revision.
    PreparationFailed(String),
    /// The replacement backend rejected the prepared runtime.
    PublicationFailed(String),
    /// The replacement was queued successfully.
    Published {
        /// Whether the exact resolved range changed from the prior audible runtime.
        range_moved: bool,
    },
}

/// Active audition state retained independently of any terminal client.
#[derive(Default)]
pub struct AuditionRuntimePolicy {
    status: Option<AuditionStatus>,
    processing_graph: Option<MixGraphInspection>,
    pending_revision: Option<RevisionId>,
}

impl AuditionRuntimePolicy {
    /// Latest successfully armed or published audition status.
    #[must_use]
    pub const fn status(&self) -> Option<&AuditionStatus> {
        self.status.as_ref()
    }

    /// Processing inspection aligned with the retained audition runtime.
    #[must_use]
    pub const fn processing_graph(&self) -> Option<&MixGraphInspection> {
        self.processing_graph.as_ref()
    }

    /// Newest Active Revision waiting for the bounded publication slot.
    #[must_use]
    pub const fn pending_revision(&self) -> Option<&RevisionId> {
        self.pending_revision.as_ref()
    }

    /// Records preparation only after the caller has accepted the runtime (for example at start).
    pub fn accept_prepared(&mut self, prepared: &PreparedAudition) {
        self.accept_snapshot(
            prepared.status().clone(),
            prepared.mix_graph_inspection().clone(),
        );
    }

    /// Records a prepared snapshot after its runtime has been accepted by a startup adapter.
    pub fn accept_snapshot(&mut self, status: AuditionStatus, graph: MixGraphInspection) {
        self.status = Some(status);
        self.processing_graph = Some(graph);
    }

    /// Clears retry state when playback stops while retaining the armed plan inspection.
    pub fn clear_pending(&mut self) {
        self.pending_revision = None;
    }

    /// Reloads the exact Active Revision, prepares the armed plan, and publishes it through the
    /// bounded adapter. Existing status and the prior audible runtime survive every failure path.
    pub fn refresh_active(
        &mut self,
        root: &Path,
        expected_revision: &RevisionId,
        plan: &AuditionPlan,
        telemetry: RuntimeTelemetry,
        explicit_seek: bool,
        publication: &mut impl RuntimePublicationPort,
    ) -> ActiveRefresh {
        if let Err(error) = publication.reserve_publication() {
            return self.publication_error(expected_revision, error);
        }

        let active = match HistoryStore::open(root).and_then(|store| store.active_bundle()) {
            Ok(Some(active)) => active,
            Ok(None) => {
                publication.cancel_publication();
                return ActiveRefresh::NoActiveRevision;
            }
            Err(error) => {
                publication.cancel_publication();
                return ActiveRefresh::ReloadFailed(error.to_string());
            }
        };
        if active.revision != *expected_revision {
            publication.cancel_publication();
            self.pending_revision = Some(active.revision.clone());
            return ActiveRefresh::ActiveChanged(active.revision);
        }
        let Some(project) = active.publishable() else {
            publication.cancel_publication();
            return ActiveRefresh::InvalidActiveRevision;
        };

        let previous_range = self
            .status
            .as_ref()
            .map(|status| (status.start_frame, status.end_frame));
        let initial_range_offset = if explicit_seek {
            None
        } else {
            self.status.as_ref().map(|status| {
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
                publication.cancel_publication();
                return ActiveRefresh::PreparationFailed(error.to_string());
            }
        };
        let status = prepared.status().clone();
        let processing_graph = prepared.mix_graph_inspection().clone();
        let range_moved =
            previous_range.is_some_and(|range| range != (status.start_frame, status.end_frame));
        match publication.publish_reserved(prepared.into_runtime()) {
            Ok(()) => {
                self.pending_revision = None;
                self.status = Some(status);
                self.processing_graph = Some(processing_graph);
                ActiveRefresh::Published { range_moved }
            }
            Err(error) => self.publication_error(expected_revision, error),
        }
    }

    fn publication_error(
        &mut self,
        revision: &RevisionId,
        error: RuntimePublicationError,
    ) -> ActiveRefresh {
        match error {
            RuntimePublicationError::Busy => {
                self.pending_revision = Some(revision.clone());
                ActiveRefresh::Delayed
            }
            RuntimePublicationError::Failed(message) => {
                self.pending_revision = None;
                ActiveRefresh::PublicationFailed(message)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use crate::{
        dsl::parse_project,
        history::{ChangeOrigin, HistoryStore, Publication},
        project::{PROJECT_FILE, ProjectBundle, load_project},
    };

    use super::*;

    #[derive(Default)]
    struct LocalPublication {
        busy: bool,
        reserved: bool,
        fail_publish: bool,
        published: Vec<SongRuntime>,
    }

    impl RuntimePublicationPort for LocalPublication {
        fn reserve_publication(&mut self) -> Result<(), RuntimePublicationError> {
            if self.busy || self.reserved {
                return Err(RuntimePublicationError::Busy);
            }
            self.reserved = true;
            Ok(())
        }

        fn publish_reserved(
            &mut self,
            runtime: SongRuntime,
        ) -> Result<(), RuntimePublicationError> {
            assert!(self.reserved, "publication requires a reservation");
            self.reserved = false;
            if self.fail_publish {
                Err(RuntimePublicationError::Failed("local failure".to_owned()))
            } else {
                self.published.push(runtime);
                Ok(())
            }
        }

        fn cancel_publication(&mut self) {
            assert!(self.reserved, "only a held reservation can be cancelled");
            self.reserved = false;
        }
    }

    fn published_fixture() -> (tempfile::TempDir, ProjectBundle) {
        let root = tempfile::tempdir().expect("temporary Project");
        fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/minimal.crump"),
            root.path().join(PROJECT_FILE),
        )
        .expect("copy fixture");
        let bundle = load_project(root.path()).expect("load fixture");
        HistoryStore::open(root.path())
            .and_then(|store| {
                store.publish(
                    &bundle,
                    Publication::new(ChangeOrigin::human("test"), "Publish", "audition test"),
                )
            })
            .expect("publish fixture");
        (root, bundle)
    }

    #[test]
    fn local_port_refreshes_active_revision_and_retains_relative_transport() {
        let (root, bundle) = published_fixture();
        let plan = AuditionPlan::default();
        let prepared = plan
            .prepare(
                &bundle.revision,
                bundle.publishable().expect("fixture Project"),
                &bundle.assets,
            )
            .expect("prepare current audition");
        let mut policy = AuditionRuntimePolicy::default();
        policy.accept_prepared(&prepared);
        let mut port = LocalPublication::default();

        assert_eq!(
            policy.refresh_active(
                root.path(),
                &bundle.revision,
                &plan,
                RuntimeTelemetry {
                    frame: 123,
                    ..RuntimeTelemetry::default()
                },
                false,
                &mut port,
            ),
            ActiveRefresh::Published { range_moved: false }
        );
        assert_eq!(port.published.len(), 1);
        assert_eq!(port.published[0].frame(), 124);
        assert_eq!(
            policy.status().expect("published status").active_revision,
            bundle.revision
        );
        assert!(!port.reserved);
    }

    #[test]
    fn failed_refresh_preserves_prior_runtime_state_and_releases_reservation() {
        let (root, bundle) = published_fixture();
        let project = bundle.publishable().expect("fixture Project");
        let prepared = AuditionPlan::default()
            .prepare(&bundle.revision, project, &bundle.assets)
            .expect("prepare initial audition");
        let mut policy = AuditionRuntimePolicy::default();
        policy.accept_prepared(&prepared);
        let previous = policy.status().cloned();
        let invalid = AuditionPlan {
            target: super::super::AuditionTarget::Track {
                track: "missing".to_owned(),
            },
            ..AuditionPlan::default()
        };
        let mut port = LocalPublication::default();

        assert!(matches!(
            policy.refresh_active(
                root.path(),
                &bundle.revision,
                &invalid,
                RuntimeTelemetry::default(),
                false,
                &mut port,
            ),
            ActiveRefresh::PreparationFailed(_)
        ));
        assert_eq!(policy.status(), previous.as_ref());
        assert!(port.published.is_empty());
        assert!(!port.reserved);
    }

    #[test]
    fn adapter_publish_failure_preserves_prior_runtime_state() {
        let (root, bundle) = published_fixture();
        let project = bundle.publishable().expect("fixture Project");
        let prepared = AuditionPlan::default()
            .prepare(&bundle.revision, project, &bundle.assets)
            .expect("prepare initial audition");
        let mut policy = AuditionRuntimePolicy::default();
        policy.accept_prepared(&prepared);
        let previous = policy.status().cloned();
        let mut port = LocalPublication {
            fail_publish: true,
            ..LocalPublication::default()
        };

        assert_eq!(
            policy.refresh_active(
                root.path(),
                &bundle.revision,
                &AuditionPlan::default(),
                RuntimeTelemetry::default(),
                false,
                &mut port,
            ),
            ActiveRefresh::PublicationFailed("local failure".to_owned())
        );
        assert_eq!(policy.status(), previous.as_ref());
        assert!(port.published.is_empty());
        assert!(!port.reserved);
    }

    #[test]
    fn busy_refresh_retries_only_the_newest_active_revision() {
        let (root, bundle) = published_fixture();
        let mut policy = AuditionRuntimePolicy::default();
        let mut port = LocalPublication {
            busy: true,
            ..LocalPublication::default()
        };

        assert_eq!(
            policy.refresh_active(
                root.path(),
                &bundle.revision,
                &AuditionPlan::default(),
                RuntimeTelemetry::default(),
                false,
                &mut port,
            ),
            ActiveRefresh::Delayed
        );
        assert_eq!(policy.pending_revision(), Some(&bundle.revision));
        assert!(port.published.is_empty());
    }

    #[test]
    fn active_revision_change_during_reload_supersedes_stale_request() {
        let (root, bundle) = published_fixture();
        let source = fs::read_to_string(root.path().join(PROJECT_FILE))
            .expect("read fixture")
            .replace("project \"Minimal\"", "project \"Changed\"");
        fs::write(root.path().join(PROJECT_FILE), source).expect("edit fixture");
        let changed = load_project(root.path()).expect("load changed Candidate");
        HistoryStore::open(root.path())
            .and_then(|store| {
                store.publish(
                    &changed,
                    Publication::new(ChangeOrigin::human("test"), "Change", "refresh race"),
                )
            })
            .expect("publish change");
        let mut policy = AuditionRuntimePolicy::default();
        let mut port = LocalPublication::default();

        assert_eq!(
            policy.refresh_active(
                root.path(),
                &bundle.revision,
                &AuditionPlan::default(),
                RuntimeTelemetry::default(),
                false,
                &mut port,
            ),
            ActiveRefresh::ActiveChanged(changed.revision.clone())
        );
        assert_eq!(policy.pending_revision(), Some(&changed.revision));
        assert!(!port.reserved);
    }

    #[test]
    fn fixture_still_parses_without_pipewire() {
        let document = parse_project(
            Path::new("project.crump"),
            include_str!("../../../tests/fixtures/minimal.crump"),
        );
        assert!(!document.diagnostics.has_errors());
    }
}
