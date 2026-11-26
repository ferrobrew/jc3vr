//! The grappling device drawn on the forearm as drawn. The device's mesh is a rigid object under
//! the local player's grappling hook, whose world transform the hook takes from Rico's forearm
//! twist bone after each tick's pose ([`GrapplingHook::OnPostPoseUpdate`]), and its render update
//! ([`RigidObject::UpdateRender`]) draws it from those simulation transforms. The presentation
//! layer re-poses the drawn arms every rendered frame from the latest tracking
//! (`docs/mod/rendering/presentation-layer.md`), so the forearm and hand moved on while the device
//! stayed where the tick left the forearm. A detour on the render update re-pushes the mesh's
//! matrix with the drawn left forearm's edit ([`late::drawn_forearm_delta`]), as the held guns are
//! re-pushed with their hand's.
//!
//! [`GrapplingHook::OnPostPoseUpdate`]: jc3gi::equipment::grappling_hook::GrapplingHook
//! [`RigidObject::UpdateRender`]: jc3gi::physics::RigidObject::UpdateRender

use std::ffi::c_void;

use detours_macro::detour;
use glam::Mat4;
use jc3gi::{
    game::{GameObject, GameObjectRenderContext},
    physics::RigidObject,
    types::math::Matrix4,
};
use jc3vr_body::controllers::{hands::late, physics::grapple_hook as hook};
use jc3vr_input::frame::Hand;
use re_utilities::hook_library::HookLibrary;

/// The device's render hook, registered by `crate::hooks`.
pub fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&RIGID_OBJECT_UPDATE_RENDER_BINDER)
}

#[detour(address = jc3gi::physics::RigidObject::UpdateRender_ADDRESS)]
fn rigid_object_update_render(game_object: *mut c_void, context: *const GameObjectRenderContext) {
    RIGID_OBJECT_UPDATE_RENDER
        .get()
        .unwrap()
        .call(game_object, context);
    // SAFETY: the render update's own arguments, on the game thread's render update.
    unsafe { place_device(game_object, context) };
}

/// Re-push the device mesh's matrix with the drawn forearm's edit, when `game_object` is the mesh
/// under the local player's hook.
///
/// # Safety
///
/// `game_object` must be null or a live rigid object's `CGameObject` subobject, and `context` null
/// or live, on the game thread.
unsafe fn place_device(game_object: *mut c_void, context: *const GameObjectRenderContext) {
    let (Some(object), Some(context)) = (
        unsafe { game_object.cast::<GameObject>().as_ref() },
        unsafe { context.as_ref() },
    ) else {
        return;
    };
    // Almost every rigid object in the world is a root; the hook is read only for a child.
    let parent = object.m_Parent.px;
    if parent.is_null() {
        return;
    }
    let Some(hook) = (unsafe { hook::local_hook() }) else {
        return;
    };
    if parent.cast_const() != std::ptr::from_ref(hook).cast::<c_void>() {
        return;
    }
    let Some(delta) = late::drawn_forearm_delta(DEVICE_FOREARM, context.m_Dtf) else {
        return;
    };
    // The rigid object starts a pointer before its `CGameObject` subobject.
    let rigid = unsafe {
        game_object
            .byte_sub(GAME_OBJECT_OFFSET)
            .cast::<RigidObject>()
            .as_mut()
    };
    let Some(instance) = rigid.and_then(|rigid| unsafe { rigid.m_ModelInstance.as_mut() }) else {
        return;
    };
    let mut drawn = Matrix4::default();
    unsafe { instance.GetMatrix(&mut drawn) };
    let placed = delta * Mat4::from(drawn);
    if placed.is_finite() {
        unsafe { instance.UpdateMatrix(&Matrix4::from(placed)) };
    }
}

/// The forearm the game's grapple device rides, whatever the player's handedness.
const DEVICE_FOREARM: Hand = Hand::Left;

/// Where a rigid object's `CGameObject` subobject sits in it, bytes: after the physics interface's
/// vtable pointer.
const GAME_OBJECT_OFFSET: usize = 8;
