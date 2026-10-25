//! Human-hull availability diagnostics without collider migration or activation.
//!
//! The clearance evaluator remains test-only because no native adapter can construct its evidence.

#[cfg(test)]
mod clearance;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum AdapterAvailability {
    Unavailable,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum ClearanceUnavailableReason {
    MissingDispatcherContract,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct HumanHullStatus {
    pub(crate) adapter: AdapterAvailability,
    pub(crate) clearance: ClearanceUnavailableReason,
}

impl HumanHullStatus {
    pub(crate) const UNAVAILABLE: Self = Self {
        adapter: AdapterAvailability::Unavailable,
        clearance: ClearanceUnavailableReason::MissingDispatcherContract,
    };
}

impl Default for HumanHullStatus {
    fn default() -> Self {
        Self::UNAVAILABLE
    }
}

/// The production readout stays closed until verified engine evidence can be supplied.
pub(crate) fn status() -> HumanHullStatus {
    HumanHullStatus::UNAVAILABLE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_diagnostic_is_closed_without_an_adapter() {
        assert_eq!(
            status(),
            HumanHullStatus {
                adapter: AdapterAvailability::Unavailable,
                clearance: ClearanceUnavailableReason::MissingDispatcherContract,
            },
        );
        assert_eq!(HumanHullStatus::default(), HumanHullStatus::UNAVAILABLE);
        assert_eq!(status(), HumanHullStatus::UNAVAILABLE);
    }
}
