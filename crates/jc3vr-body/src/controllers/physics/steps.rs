//! The physics-step clock's one writer, and how long a publication stamped on it stays current.
//!
//! The clock is core's [`Step`]. The hand bodies' pre-simulation callback is registered whenever
//! the physics system exists and fires once at the start of every step, so it alone begins the
//! steps, through [`count_step`]; a second caller would make the clock run fast.

use jc3vr_core::published::{Step, StepWindow};

use crate::tuning::FRESHNESS;

/// Begin a physics step, returning its reading. Called once at the start of each step, by the hand
/// bodies' pre-simulation callback only.
pub fn count_step() -> Step {
    Step::begin()
}

/// How many steps a publication for the next step stays current: the freshness tuning's
/// `physics.stale_steps`. Freshness is inclusive and a stamp ahead of the reader's step is current,
/// so a publication made after step 40 still holds at step 40 plus the window.
pub fn window() -> StepWindow {
    StepWindow::new(FRESHNESS.get().physics.stale_steps)
}
