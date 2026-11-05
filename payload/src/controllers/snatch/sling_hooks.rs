//! Scoped native-wire exclusions for the owned wire, while the sling swings its target or the
//! snatch's tether pulls it, and a tracked forearm transform for the sling.

use std::{
    cell::Cell,
    sync::atomic::{AtomicUsize, Ordering},
};

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

use crate::grapple::hook;

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

/// Publish on the game thread before native simulation; workers read only the copied value.
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

/// Exclude the native rope's constraint and tension from the owned wire `wire` while the snatch's
/// tether pulls its target, or end the exclusion with `None`. Unlike [`publish`], the wire's
/// visuals stay native.
pub(super) fn exclude(wire: Option<usize>) {
    EXCLUDED.store(wire.unwrap_or(0), Ordering::Release);
}

static SNAPSHOT: Mutex<Option<Snapshot>> = Mutex::new(None);
static EXCLUDED: AtomicUsize = AtomicUsize::new(0);

/// Whether the native rope of `wire` is excluded, by the sling or the tether.
fn excluded(wire: *mut GrapplingHookWire) -> bool {
    let wire = wire as usize;
    EXCLUDED.load(Ordering::Acquire) == wire || SNAPSHOT.lock().is_some_and(|s| s.wire == wire)
}
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
            && hook::device_end(unsafe { &*wire }).m_PointProxy as usize == s.proxy
    });
    let previous = CURRENT.replace(snapshot);
    UPDATE.get().unwrap().call(wire, owner, tuning, dt);
    CURRENT.set(previous);
}

#[detour(address = GrapplingHookWire::CreateConstraint_ADDRESS)]
fn create_constraint(wire: *mut GrapplingHookWire, shorten: f32, reattaching: bool) {
    if excluded(wire) {
        return;
    }
    CREATE_CONSTRAINT
        .get()
        .unwrap()
        .call(wire, shorten, reattaching);
}

#[detour(address = GrapplingHookWire::UpdateWireTension_ADDRESS)]
fn update_wire_tension(wire: *mut GrapplingHookWire, tuning: *const GrappleTuning, dt: f32) {
    if excluded(wire) {
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
