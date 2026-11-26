//! The teleport epoch: a count of the game's discontinuities to the player, which every consumer
//! of a continuous pose compares against the epoch it last saw.
//!
//! The game has no reliable per-character teleport flag. The roomscale loop counts each camera
//! reset the game makes after moving the player, and each warp the body rig's per-tick distance
//! test finds ([`crate::controllers::roomscale::teleport`]).

use std::sync::atomic::{AtomicU64, Ordering};

/// The number of teleports seen since the payload loaded.
pub fn epoch() -> u64 {
    EPOCH.load(Ordering::Relaxed)
}

/// How many of the teleports came from the distance test rather than the camera reset.
pub fn warps() -> u64 {
    WARPS.load(Ordering::Relaxed)
}

/// Count a teleport the game's camera reset marked.
pub(crate) fn count_teleport() {
    EPOCH.fetch_add(1, Ordering::Relaxed);
}

/// Count a teleport the distance test found.
pub(crate) fn count_warp() {
    WARPS.fetch_add(1, Ordering::Relaxed);
    EPOCH.fetch_add(1, Ordering::Relaxed);
}

static EPOCH: AtomicU64 = AtomicU64::new(0);
static WARPS: AtomicU64 = AtomicU64::new(0);
