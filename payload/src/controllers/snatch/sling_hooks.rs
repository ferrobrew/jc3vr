//! Scoped native-wire exclusions and a tracked forearm transform for the owned sling wire.

use std::cell::Cell;

use detours_macro::detour;
use glam::Mat4;
use jc3gi::{
    character::character::Character,
    equipment::{
        grapple_point_proxy::CharacterBoneAttachedPointProxy,
        grappling_hook::{GrappleTuning, GrapplingHookWire},
    },
    types::{
        math::{Matrix4, Vector3},
        shared_ptr::SharedPtr,
    },
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

/// Only addresses identifying the owned wire and its native near-end proxy accompany the pose.
#[derive(Copy, Clone)]
struct Snapshot {
    wire: usize,
    proxy: usize,
    transform: Mat4,
}

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&UPDATE_BINDER)
        .with_static_binder(&CREATE_CONSTRAINT_BINDER)
        .with_static_binder(&UPDATE_WIRE_TENSION_BINDER)
        .with_static_binder(&GET_VISUAL_POSITION_AND_NORMAL_WS_BINDER)
        .with_static_binder(&GET_VISUAL_TRANSFORM_WS_BINDER)
}

/// Publish on the main thread before native simulation; workers read only the copied value.
pub(super) fn publish(wire: usize, proxy: usize, transform: Mat4) {
    *SNAPSHOT.lock() = Some(Snapshot {
        wire,
        proxy,
        transform,
    });
}

pub(super) fn clear() {
    *SNAPSHOT.lock() = None;
}

static SNAPSHOT: Mutex<Option<Snapshot>> = Mutex::new(None);
thread_local! { static CURRENT: Cell<Option<Snapshot>> = const { Cell::new(None) }; }

#[detour(address = GrapplingHookWire::Update_ADDRESS)]
fn update(
    wire: *mut GrapplingHookWire,
    owner: *const SharedPtr<Character>,
    tuning: *const GrappleTuning,
    dt: f32,
) {
    let snapshot = SNAPSHOT.lock().filter(|s| {
        s.wire == wire as usize
            // SAFETY: the native update owns this live wire throughout the call.
            && unsafe { (*wire).m_WireEnds[1].m_PointProxy as usize == s.proxy }
    });
    let previous = CURRENT.replace(snapshot);
    UPDATE.get().unwrap().call(wire, owner, tuning, dt);
    CURRENT.set(previous);
}

#[detour(address = GrapplingHookWire::CreateConstraint_ADDRESS)]
fn create_constraint(wire: *mut GrapplingHookWire, shorten: f32, reattaching: bool) {
    if SNAPSHOT.lock().is_some_and(|s| s.wire == wire as usize) {
        return;
    }
    CREATE_CONSTRAINT
        .get()
        .unwrap()
        .call(wire, shorten, reattaching);
}

#[detour(address = GrapplingHookWire::UpdateWireTension_ADDRESS)]
fn update_wire_tension(wire: *mut GrapplingHookWire, tuning: *const GrappleTuning, dt: f32) {
    if SNAPSHOT.lock().is_some_and(|s| s.wire == wire as usize) {
        return;
    }
    UPDATE_WIRE_TENSION.get().unwrap().call(wire, tuning, dt);
}

#[detour(address = CharacterBoneAttachedPointProxy::GetVisualPositionAndNormalWS_ADDRESS)]
fn get_visual_position_and_normal_ws(
    proxy: *const CharacterBoneAttachedPointProxy,
    position: *mut Vector3,
    normal: *mut Vector3,
) -> bool {
    if let Some(s) = CURRENT.get().filter(|s| s.proxy == proxy as usize) {
        // SAFETY: the native getter's output arguments are valid for this call.
        unsafe {
            position.write(Vector3 {
                data: s.transform.w_axis.truncate().to_array(),
            });
            normal.write(Vector3 {
                data: s.transform.z_axis.truncate().to_array(),
            });
        }
        return true;
    }
    GET_VISUAL_POSITION_AND_NORMAL_WS
        .get()
        .unwrap()
        .call(proxy, position, normal)
}

#[detour(address = CharacterBoneAttachedPointProxy::GetVisualTransformWS_ADDRESS)]
fn get_visual_transform_ws(
    proxy: *const CharacterBoneAttachedPointProxy,
    transform: *mut Matrix4,
) -> bool {
    if let Some(s) = CURRENT.get().filter(|s| s.proxy == proxy as usize) {
        // SAFETY: the native getter's output argument is valid for this call.
        unsafe {
            transform.write(Matrix4::from(s.transform));
        }
        return true;
    }
    GET_VISUAL_TRANSFORM_WS
        .get()
        .unwrap()
        .call(proxy, transform)
}
