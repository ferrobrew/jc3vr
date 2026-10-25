//! Executed work and reference-schedule requests for one update, including failed updates.

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub(crate) struct Work {
    /// Executed search trials, fixed-pose recovery checks, realization, and heading probes.
    pub evaluations: usize,
    /// Trials and checks requested by the unchanged topology schedule, including cache hits.
    pub logical_evaluations: usize,
    pub search_requests: usize,
    pub searches: usize,
    pub cache_hits: usize,
    pub preliminary_reuses: usize,
    pub interval_constructions: usize,
    /// Calls to the analytic leg solver, including endpoint feasibility checks.
    pub analytic_solves: usize,
}

impl Work {
    pub(crate) fn check(&mut self) {
        self.evaluations += 1;
        self.logical_evaluations += 1;
    }
}
