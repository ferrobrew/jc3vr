use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

/// A diagnostic counter bumped on the draw path, where a read-modify-write is dearer than it looks:
/// a locked add waits for the stores the Direct3D runtime has just made to drain, once per draw.
/// The draw path runs its calls one at a time under the engine's context mutex, so a plain load and
/// store counts every call; were two threads ever to bump one counter at once, a count could be lost,
/// which these counts tolerate.
pub trait DrawPathCounter {
    fn bump(&self);
}

impl DrawPathCounter for AtomicU64 {
    fn bump(&self) {
        self.store(
            self.load(Ordering::Relaxed).wrapping_add(1),
            Ordering::Relaxed,
        );
    }
}

impl DrawPathCounter for AtomicUsize {
    fn bump(&self) {
        self.store(
            self.load(Ordering::Relaxed).wrapping_add(1),
            Ordering::Relaxed,
        );
    }
}

impl DrawPathCounter for AtomicU32 {
    fn bump(&self) {
        self.store(
            self.load(Ordering::Relaxed).wrapping_add(1),
            Ordering::Relaxed,
        );
    }
}

/// Bump a [`DrawPathCounter`].
pub fn bump(counter: &impl DrawPathCounter) {
    counter.bump();
}
