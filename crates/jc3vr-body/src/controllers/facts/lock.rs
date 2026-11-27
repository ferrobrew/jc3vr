//! Whether the physical grapple holds a lock, as the grapple publishes it for the body and the
//! weapon handling.
//!
//! The lock writes it as it takes the line and as it lets go (the grapple crate's
//! `grapple::physical::lock`), so a reader sees the lock's own state, not a copy a tick old. The
//! fact is published while the lock holds and withdrawn when it lets go.

use jc3vr_core::published::{Published, Stage};

/// Whether the lock holds the line. A leaf slot, so the grapple's detours read it without waiting
/// on the lock's own state.
#[inline]
pub fn holds() -> bool {
    HELD.latest().is_some()
}

/// Publish whether the lock holds the line.
#[inline]
pub fn publish(holds: bool) {
    if holds {
        HELD.publish(());
    } else {
        HELD.clear();
    }
}

static HELD: Published<()> = Published::new(Stage::EngineCall);
