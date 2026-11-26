//! Whether the grapple snatch has a line out, as the snatch publishes it for the weapon handling.
//!
//! The snatch writes it as its tick leaves the line's phase and as it stands down, while it holds
//! its own state, so a reader sees the line the snatch's last step left, not a copy a tick old.

use std::sync::atomic::{AtomicBool, Ordering};

/// Whether the snatch's own line is out: the hook flying to its target, freeing it, or the line
/// pulling or swinging it. An atomic, so the weapon handling reads it without waiting on the
/// snatch's state.
#[inline]
pub fn out() -> bool {
    LINE_OUT.load(Ordering::Acquire)
}

/// Publish whether the snatch's line is out.
pub fn publish(out: bool) {
    LINE_OUT.store(out, Ordering::Release);
}

static LINE_OUT: AtomicBool = AtomicBool::new(false);
