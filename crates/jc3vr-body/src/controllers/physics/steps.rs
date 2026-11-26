//! The physics-step clock's one writer, and how long a publication stamped on it stays current.
//!
//! The clock is core's [`Step`]. The hand bodies' pre-simulation callback is registered whenever
//! the physics system exists and fires once at the start of every step, so it alone begins the
//! steps, through [`count_step`]; a second caller would make the clock run fast.

use jc3vr_core::published::{Step, StepWindow};

use crate::tuning::FRESHNESS;

/// The physics steps run so far, for stamping a publication.
#[inline]
pub fn steps() -> u64 {
    Step::now().count()
}

/// Begin a physics step, returning its reading. Called once at the start of each step, by the hand
/// bodies' pre-simulation callback only.
pub fn count_step() -> Step {
    Step::begin()
}

/// How many steps a publication for the next step stays current: the freshness tuning's
/// `physics.stale_steps`.
pub fn window() -> StepWindow {
    StepWindow::new(FRESHNESS.get().physics.stale_steps)
}

/// Whether a publication stamped at step `published` is still current at step `now` ([`window`]).
pub fn current_at(published: Option<u64>, now: u64) -> bool {
    window().holds(published.map(Step::from_count), Step::from_count(now))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_publication_ages_by_steps_so_a_hitch_drops_nothing() {
        // Published after step 40: the next steps find it current however long they took to come.
        let published = Some(40);
        assert!(current_at(published, 41));
        assert!(current_at(
            published,
            40 + FRESHNESS.get().physics.stale_steps
        ));
        // Only steps that run without a new publication age it.
        assert!(!current_at(
            published,
            41 + FRESHNESS.get().physics.stale_steps
        ));
        assert!(!current_at(None, 41));
        // A step number behind the stamp (a publication between a step's count and its read)
        // is current.
        assert!(current_at(Some(42), 41));
    }
}
