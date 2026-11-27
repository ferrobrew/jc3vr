//! Whether the grapple snatch has a line out, as the snatch publishes it for the weapon handling.
//!
//! The snatch writes it as its tick leaves the line's phase and as it stands down, while it holds
//! its own state, so a reader sees the line the snatch's last step left, not a copy a tick old.
//! The fact is published while the line is out and withdrawn when it is not.

use jc3vr_core::published::{Published, Stage};

/// Whether the snatch's own line is out: the hook flying to its target, freeing it, or the line
/// pulling or swinging it. A leaf slot, so the weapon handling reads it without waiting on the
/// snatch's state.
#[inline]
pub fn out() -> bool {
    OUT.latest().is_some()
}

/// Publish whether the snatch's line is out.
#[inline]
pub fn publish(out: bool) {
    if out {
        OUT.publish(());
    } else {
        OUT.clear();
    }
}

static OUT: Published<()> = Published::new(Stage::InputGame);
