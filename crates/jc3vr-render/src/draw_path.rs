//! The counter the draw path bumps, where a read-modify-write is dearer than it looks: a locked add
//! waits for the stores the Direct3D runtime has just made to drain, once per draw.

use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

/// A diagnostic counter bumped on the draw path with a plain load and store. The draw path runs its
/// calls one at a time under the engine's context mutex, so that counts every call; were two threads
/// ever to bump one counter at once, a count could be lost, which these counts tolerate.
pub(crate) trait DrawPathCounter {
    fn bump(&self);
}

macro_rules! draw_path_counter {
    ($($atomic:ty),*) => {$(
        impl DrawPathCounter for $atomic {
            #[inline]
            fn bump(&self) {
                self.store(
                    self.load(Ordering::Relaxed).wrapping_add(1),
                    Ordering::Relaxed,
                );
            }
        }
    )*};
}

draw_path_counter!(AtomicU32, AtomicU64, AtomicUsize);

/// Bump a [`DrawPathCounter`].
#[inline]
pub fn bump(counter: &impl DrawPathCounter) {
    counter.bump();
}
