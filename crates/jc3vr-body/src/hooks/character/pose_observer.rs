//! The observer of the local player's pose-finalization pass: called with the character at the
//! pass's entry, after every target queued there and before the solves, and at the pass's end,
//! after the solves and every joint written after them.

use std::sync::OnceLock;

use jc3gi::character::character::Character;

/// The two points of the pose-finalization pass an observer sees.
pub struct PoseObserver {
    /// Called at the pass's entry, after every target queued there and before the solves.
    pub before_solve: unsafe fn(*mut Character),
    /// Called at the end of the pass, after the solves and every joint written after them.
    pub after_pass: unsafe fn(*mut Character),
}

/// Register the pose-finalization observer. One observer: a second registration is refused and
/// logged.
///
/// Both calls run on the thread that owns the character's pass. The entry call comes only for a
/// local character whose live pose is readable; the end call comes for every character, and may
/// pass null.
pub fn register_pose_observer(observer: PoseObserver) {
    if OBSERVER.set(observer).is_err() {
        tracing::error!("character: a pose observer is already registered; keeping the first");
    }
}

/// Whether the pose-finalization observer is registered.
pub fn pose_observer_registered() -> bool {
    OBSERVER.get().is_some()
}

/// Call the observer's [`PoseObserver::before_solve`], when one is registered.
///
/// # Safety
///
/// `character` must be a live character at the entry of its pose-finalization pass, after every
/// target queued there, on the thread that owns the pass.
pub(super) unsafe fn before_solve(character: *mut Character) {
    if let Some(observer) = OBSERVER.get() {
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
    if let Some(observer) = OBSERVER.get() {
        unsafe { (observer.after_pass)(character) };
    }
}

static OBSERVER: OnceLock<PoseObserver> = OnceLock::new();
