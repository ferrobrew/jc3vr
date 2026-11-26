//! Whether the physical grapple holds a lock, as the grapple publishes it for the body and the
//! weapon handling.
//!
//! The lock writes it as it takes the line and as it lets go
//! ([`crate::grapple::physical::lock`]), so a reader sees the lock's own state, not a copy a tick
//! old.

use std::sync::atomic::{AtomicBool, Ordering};

/// Whether the lock holds the line. An atomic, so the grapple's detours read it without waiting on
/// the lock's own state.
#[inline]
pub fn holds() -> bool {
    LOCKED.load(Ordering::Acquire)
}

/// Publish whether the lock holds the line.
pub fn publish(holds: bool) {
    LOCKED.store(holds, Ordering::Release);
}

static LOCKED: AtomicBool = AtomicBool::new(false);
