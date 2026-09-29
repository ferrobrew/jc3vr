//! The game's camera recoil, suppressed while the guns aim by hand.
//!
//! The weapon camera modifier ([`WeaponCameraModifier::ProcessCameraContext`]) turns the camera
//! boom by the held weapon's per-tick recoil and runs the snap-aim camera assist. The boom
//! persists, so each shot also turns the gameplay camera's lasting yaw and pitch, which the body
//! and the movement frame follow. Under the controller scheme the view belongs to the headset and
//! the recoil belongs to the hand ([`crate::controllers::weapons::recoil`]), so while the guns aim
//! by hand the detour skips the modifier's boom changes and only writes the camera transform it
//! would have written last.

use detours_macro::detour;
use jc3gi::{
    camera::{
        camera_context::{CameraContext, CameraPipelineContext},
        weapon_camera_modifier::WeaponCameraModifier,
    },
    character::character::Character,
};
use re_utilities::hook_library::HookLibrary;

use crate::{config::Config, controllers::scheme};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&PROCESS_CAMERA_CONTEXT_BINDER)
}

#[detour(address = jc3gi::camera::weapon_camera_modifier::WeaponCameraModifier::ProcessCameraContext_ADDRESS)]
fn process_camera_context(
    modifier: *mut WeaponCameraModifier,
    pipeline: *mut CameraPipelineContext,
    previous: *const CameraContext,
    out: *mut CameraContext,
) {
    let suppress = Config::lock_query(|c| {
        let c = &c.controllers;
        c.enabled && c.aim.gun && c.weapons.recoil.suppress_camera_kick
    }) && scheme::is_controllers();
    // Without a local player the modifier does nothing at all, so the transform is written only
    // when it would have been.
    let local = unsafe { !Character::GetLocalPlayerCharacter().is_null() };
    match (suppress && local, unsafe { pipeline.as_ref() }, unsafe {
        out.as_mut()
    }) {
        (true, Some(pipeline), Some(out)) => unsafe {
            pipeline.CalculateCameraTransform(&mut out.m_CameraTransform);
        },
        _ => PROCESS_CAMERA_CONTEXT
            .get()
            .unwrap()
            .call(modifier, pipeline, previous, out),
    }
}
