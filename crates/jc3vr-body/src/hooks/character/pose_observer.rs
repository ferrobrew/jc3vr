//! The observer of the local player's pose-finalization pass: called with the character at the
//! pass's entry, after every target queued there and before the solves, and at the pass's end,
//! after the solves and every joint written after them. The observer is a field of the body's
//! contributions ([`crate::contributions::BodyContributions::pose_observer`]).
//!
//! Both calls run on the thread that owns the character's pass. The entry call comes only for a
//! local character whose live pose is readable; the end call comes for every character, and may
//! pass null.

use jc3gi::character::character::Character;

use crate::contributions;

/// The two points of the pose-finalization pass an observer sees.
pub struct PoseObserver {
    /// Called at the pass's entry, after every target queued there and before the solves.
    pub before_solve: unsafe fn(*mut Character),
    /// Called at the end of the pass, after the solves and every joint written after them.
    pub after_pass: unsafe fn(*mut Character),
}

/// Call the observer's [`PoseObserver::before_solve`], when one is registered.
///
/// # Safety
///
/// `character` must be a live character at the entry of its pose-finalization pass, after every
/// target queued there, on the thread that owns the pass.
pub(super) unsafe fn before_solve(character: *mut Character) {
    if let Some(observer) = observer() {
        unsafe { (observer.before_solve)(character) };
    }
}

/// Call the observer's [`PoseObserver::after_pass`], when one is registered.
///
/// # Safety
///
/// `character` must be null or a live character at the end of its pose-finalization pass, on the
/// thread that owns the pass.
pub(super) unsafe fn after_pass(character: *mut Character) {
    if let Some(observer) = observer() {
        unsafe { (observer.after_pass)(character) };
    }
}

fn observer() -> Option<&'static PoseObserver> {
    contributions::get().and_then(|contributions| contributions.pose_observer.as_ref())
}
