//! Low-overhead workspace latency counters and bounded timing samples.

use std::{fmt::Write as _, time::Duration};

const SAMPLE_CAPACITY: usize = 128;

#[derive(Debug, Clone)]
struct TimingStats {
    count: u64,
    total_micros: u128,
    maximum_micros: u64,
    last_micros: u64,
    samples: [u64; SAMPLE_CAPACITY],
    sample_count: usize,
    next_sample: usize,
}

impl Default for TimingStats {
    fn default() -> Self {
        Self {
            count: 0,
            total_micros: 0,
            maximum_micros: 0,
            last_micros: 0,
            samples: [0; SAMPLE_CAPACITY],
            sample_count: 0,
            next_sample: 0,
        }
    }
}

impl TimingStats {
    fn observe(&mut self, elapsed: Duration) {
        let micros = elapsed.as_micros().min(u128::from(u64::MAX)) as u64;
        self.count = self.count.saturating_add(1);
        self.total_micros = self.total_micros.saturating_add(u128::from(micros));
        self.maximum_micros = self.maximum_micros.max(micros);
        self.last_micros = micros;
        self.samples[self.next_sample] = micros;
        self.next_sample = (self.next_sample + 1) % SAMPLE_CAPACITY;
        self.sample_count = self.sample_count.saturating_add(1).min(SAMPLE_CAPACITY);
    }

    fn mean_micros(&self) -> u64 {
        if self.count == 0 {
            0
        } else {
            (self.total_micros / u128::from(self.count)).min(u128::from(u64::MAX)) as u64
        }
    }

    fn percentile_micros(&self, numerator: usize, denominator: usize) -> u64 {
        if self.sample_count == 0 || denominator == 0 {
            return 0;
        }
        let mut ordered = self.samples;
        ordered[..self.sample_count].sort_unstable();
        let rank = self
            .sample_count
            .saturating_mul(numerator)
            .saturating_add(denominator - 1)
            / denominator;
        ordered[rank.saturating_sub(1).min(self.sample_count - 1)]
    }

    fn summary(&self) -> String {
        format!(
            "count={} last={:.3}ms mean={:.3}ms p50={:.3}ms p95={:.3}ms p99={:.3}ms max={:.3}ms",
            self.count,
            micros_to_ms(self.last_micros),
            micros_to_ms(self.mean_micros()),
            micros_to_ms(self.percentile_micros(50, 100)),
            micros_to_ms(self.percentile_micros(95, 100)),
            micros_to_ms(self.percentile_micros(99, 100)),
            micros_to_ms(self.maximum_micros),
        )
    }
}

fn micros_to_ms(micros: u64) -> f64 {
    micros as f64 / 1_000.0
}

/// In-memory counters for the event-driven workspace control plane.
#[derive(Debug, Default)]
pub(super) struct WorkspacePerformance {
    visible: bool,
    report_on_exit: bool,
    pub(super) draws: u64,
    pub(super) refresh_checks: u64,
    pub(super) candidate_reloads: u64,
    pub(super) history_reloads: u64,
    pub(super) candidate_changes: u64,
    pub(super) active_changes: u64,
    pub(super) derived_rebuilds: u64,
    pub(super) active_preparations: u64,
    pub(super) mutations: u64,
    pub(super) unstable_retries: u64,
    pub(super) coalesced_requests: u64,
    refresh_probe: TimingStats,
    candidate_load: TimingStats,
    history_load: TimingStats,
    derived_rebuild: TimingStats,
    active_preparation: TimingStats,
    mutation: TimingStats,
    background_commit_wait: TimingStats,
    background_apply: TimingStats,
    draw: TimingStats,
}

impl WorkspacePerformance {
    pub(super) fn from_environment() -> Self {
        let enabled = std::env::var("CRUMP_WORKSPACE_DIAGNOSTICS").is_ok_and(|value| {
            !matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "" | "0" | "false" | "no" | "off"
            )
        });
        Self {
            visible: enabled,
            report_on_exit: enabled,
            ..Self::default()
        }
    }

    pub(super) const fn visible(&self) -> bool {
        self.visible
    }

    pub(super) fn toggle(&mut self) -> bool {
        self.visible = !self.visible;
        self.report_on_exit = true;
        self.visible
    }

    pub(super) fn observe_refresh_probe(&mut self, elapsed: Duration) {
        self.refresh_checks = self.refresh_checks.saturating_add(1);
        self.refresh_probe.observe(elapsed);
    }

    pub(super) fn observe_candidate_load(&mut self, elapsed: Duration) {
        self.candidate_reloads = self.candidate_reloads.saturating_add(1);
        self.candidate_load.observe(elapsed);
    }

    pub(super) fn observe_history_load(&mut self, elapsed: Duration) {
        self.history_reloads = self.history_reloads.saturating_add(1);
        self.history_load.observe(elapsed);
    }

    pub(super) fn observe_derived_rebuild(&mut self, elapsed: Duration) {
        self.derived_rebuilds = self.derived_rebuilds.saturating_add(1);
        self.derived_rebuild.observe(elapsed);
    }

    pub(super) fn observe_active_preparation(&mut self, elapsed: Duration) {
        self.active_preparations = self.active_preparations.saturating_add(1);
        self.active_preparation.observe(elapsed);
    }

    pub(super) fn observe_mutation(&mut self, elapsed: Duration) {
        self.mutations = self.mutations.saturating_add(1);
        self.mutation.observe(elapsed);
    }

    pub(super) fn observe_background_commit_wait(&mut self, elapsed: Duration) {
        self.background_commit_wait.observe(elapsed);
    }

    pub(super) fn observe_background_apply(&mut self, elapsed: Duration) {
        self.background_apply.observe(elapsed);
    }

    pub(super) fn observe_draw(&mut self, elapsed: Duration) {
        self.draws = self.draws.saturating_add(1);
        self.draw.observe(elapsed);
    }

    pub(super) fn footer(&self) -> Option<String> {
        self.visible.then(|| {
            format!(
                "perf draw {:.2}/{:.2}ms · check/reload {}/{}/{}",
                micros_to_ms(self.draw.last_micros),
                micros_to_ms(self.draw.maximum_micros),
                self.refresh_checks,
                self.candidate_reloads,
                self.history_reloads,
            )
        })
    }

    pub(super) fn report(&self) -> Option<String> {
        if !self.report_on_exit {
            return None;
        }
        let mut report = format!(
            "workspace-performance draws={} refresh_checks={} candidate_reloads={} history_reloads={} candidate_changes={} active_changes={} derived_rebuilds={} active_preparations={} mutations={} unstable_retries={} coalesced_requests={}",
            self.draws,
            self.refresh_checks,
            self.candidate_reloads,
            self.history_reloads,
            self.candidate_changes,
            self.active_changes,
            self.derived_rebuilds,
            self.active_preparations,
            self.mutations,
            self.unstable_retries,
            self.coalesced_requests,
        );
        for (name, timing) in [
            ("refresh_probe", &self.refresh_probe),
            ("candidate_load", &self.candidate_load),
            ("history_load", &self.history_load),
            ("derived_rebuild", &self.derived_rebuild),
            ("active_preparation", &self.active_preparation),
            ("mutation", &self.mutation),
            (
                "background_completion_to_commit",
                &self.background_commit_wait,
            ),
            ("background_apply", &self.background_apply),
            ("draw", &self.draw),
        ] {
            let _ = write!(
                report,
                "\nworkspace-performance {name} {}",
                timing.summary()
            );
        }
        Some(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_stats_keep_bounded_recent_percentiles_and_lifetime_totals() {
        let mut stats = TimingStats::default();
        for micros in 1..=130 {
            stats.observe(Duration::from_micros(micros));
        }

        assert_eq!(stats.count, 130);
        assert_eq!(stats.sample_count, SAMPLE_CAPACITY);
        assert_eq!(stats.last_micros, 130);
        assert_eq!(stats.maximum_micros, 130);
        assert_eq!(stats.mean_micros(), 65);
        assert_eq!(stats.percentile_micros(50, 100), 66);
        assert_eq!(stats.percentile_micros(99, 100), 129);
    }

    #[test]
    fn report_is_opt_in_and_contains_each_measured_phase() {
        let mut performance = WorkspacePerformance::default();
        assert!(performance.report().is_none());
        assert!(performance.toggle());
        performance.observe_refresh_probe(Duration::from_micros(12));
        performance.observe_candidate_load(Duration::from_millis(3));
        performance.observe_history_load(Duration::from_millis(2));
        performance.observe_derived_rebuild(Duration::from_millis(1));
        performance.observe_active_preparation(Duration::from_millis(4));
        performance.observe_mutation(Duration::from_millis(5));
        performance.observe_background_commit_wait(Duration::from_micros(30));
        performance.observe_background_apply(Duration::from_micros(50));
        performance.observe_draw(Duration::from_micros(400));

        let report = performance.report().unwrap_or_default();
        for phase in [
            "refresh_probe",
            "candidate_load",
            "history_load",
            "derived_rebuild",
            "active_preparation",
            "mutation",
            "background_completion_to_commit",
            "background_apply",
            "draw",
        ] {
            assert!(report.contains(phase), "missing phase {phase}");
        }
        assert!(report.contains("p95="));
        assert!(performance.footer().is_some());
    }
}
