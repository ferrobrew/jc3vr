//! Teleport detection for the roomscale loop.
//!
//! The game has no reliable per-character teleport flag, but every discontinuity it makes to the
//! player resets the camera through `GameCameraManager::ResetCamera` right after the move: the
//! instant teleport, the deferred teleport's placement, the respawn after death, the load's start
//! position, and a playlist commit. A post-call detour on it counts a teleport epoch that the
//! roomscale consumers compare against the epoch they last saw. Writers that bypass the game world
//! (a vehicle teleport carrying its seated driver, scripted warps) are caught by the chase loop's
//! per-tick distance test instead ([`crate::controllers::roomscale::chase::is_warp`]), which counts
//! into the same epoch.

use std::sync::atomic::{AtomicU64, Ordering};

use detours_macro::detour;
use jc3gi::{camera::game_camera_manager::GameCameraManager, types::math::Matrix4};
use re_utilities::hook_library::HookLibrary;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&RESET_CAMERA_BINDER)
}

/// The number of teleports seen since the payload loaded.
pub fn epoch() -> u64 {
    EPOCH.load(Ordering::Relaxed)
}

/// How many of the teleports came from the distance test rather than the camera reset.
pub fn warps() -> u64 {
    WARPS.load(Ordering::Relaxed)
}

/// Count a teleport the distance test found.
pub(super) fn record_warp() {
    WARPS.fetch_add(1, Ordering::Relaxed);
    EPOCH.fetch_add(1, Ordering::Relaxed);
    tracing::debug!(target: "controllers", "roomscale: the body warped without a camera reset");
}

static EPOCH: AtomicU64 = AtomicU64::new(0);
static WARPS: AtomicU64 = AtomicU64::new(0);

#[detour(address = jc3gi::camera::game_camera_manager::GameCameraManager::ResetCamera_ADDRESS)]
fn reset_camera(manager: *mut GameCameraManager, transform: *const Matrix4) {
    RESET_CAMERA.get().unwrap().call(manager, transform);
    EPOCH.fetch_add(1, Ordering::Relaxed);
    tracing::debug!(target: "controllers", "roomscale: the game reset the camera for a teleport");
}
