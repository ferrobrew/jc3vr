//! Teleport detection for the roomscale loop.
//!
//! The game has no reliable per-character teleport flag, but every discontinuity it makes to the
//! player resets the camera through `GameCameraManager::ResetCamera` right after the move: the
//! instant teleport, the deferred teleport's placement, the respawn after death, the load's start
//! position, and a playlist commit. A post-call detour on it counts the teleport epoch
//! ([`jc3vr_view::headpose::teleport`]) that the pose consumers compare against the epoch they last
//! saw. Writers that bypass the game world (a vehicle teleport carrying its seated driver, scripted
//! warps) are caught by the root hook's per-tick distance test instead
//! ([`crate::controllers::body::rig::root::drive::is_warp`]), which counts into the same epoch.

use detours_macro::detour;
use jc3gi::{camera::game_camera_manager::GameCameraManager, types::math::Matrix4};
use jc3vr_view::{controllers::origin_provenance, headpose::teleport};
use re_utilities::hook_library::HookLibrary;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&RESET_CAMERA_BINDER)
}

/// Count a teleport the distance test found.
pub(crate) fn record_warp() {
    let _origin_mutation = origin_provenance::changing();
    teleport::count_warp();
    tracing::debug!(target: "controllers", "roomscale: the body warped without a camera reset");
}

#[detour(address = jc3gi::camera::game_camera_manager::GameCameraManager::ResetCamera_ADDRESS)]
fn reset_camera(manager: *mut GameCameraManager, transform: *const Matrix4) {
    let _origin_mutation = origin_provenance::changing();
    RESET_CAMERA.get().unwrap().call(manager, transform);
    teleport::count_teleport();
    tracing::debug!(target: "controllers", "roomscale: the game reset the camera for a teleport");
}
