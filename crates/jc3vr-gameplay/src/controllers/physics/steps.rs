//! The physics steps counted as the hand bodies' pre-simulation callback runs them, and how long a
//! publication stamped with one stays current.

use std::sync::atomic::{AtomicU64, Ordering};

/// How many physics steps a publication stays current for the step and for what decides whether a
/// hand acts: six steps of the 30 Hz world, the same 200 ms a hand bodies' published pose stays
/// current for a reader between the steps. Counted in steps rather than wall-clock time, so a
/// hitch that holds up the whole simulation tick (a frame of a few hundred milliseconds) ages
/// nothing, and a live hold is not torn down; only steps that run without a new publication do.
pub const STALE_STEPS: u64 = 6;

/// The physics steps run so far, for stamping a publication.
pub fn steps() -> u64 {
    STEPS.load(Ordering::Acquire)
}

/// Count a physics step, returning its number. Called once at the start of each step.
pub(crate) fn count_step() -> u64 {
    STEPS.fetch_add(1, Ordering::AcqRel) + 1
}

/// Whether a publication stamped at step `published` is still current at step `now`: no more
/// than [`STALE_STEPS`] steps have run since.
pub fn current_at(published: Option<u64>, now: u64) -> bool {
    published.is_some_and(|at| now.saturating_sub(at) <= STALE_STEPS)
}

/// The physics steps run since the payload loaded.
static STEPS: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_publication_ages_by_steps_so_a_hitch_drops_nothing() {
        // Published after step 40: the next steps find it current however long they took to come.
        let published = Some(40);
        assert!(current_at(published, 41));
        assert!(current_at(published, 40 + STALE_STEPS));
        // Only steps that run without a new publication age it.
        assert!(!current_at(published, 41 + STALE_STEPS));
        assert!(!current_at(None, 41));
        // A step number behind the stamp (a publication between a step's count and its read)
        // is current.
        assert!(current_at(Some(42), 41));
    }
}
