//! The driven vehicle's rendered steering wheel turned with the steering, behind
//! [`PhysicalControlsConfig::animate_wheel`](crate::controllers::vehicles::config::PhysicalControlsConfig::animate_wheel).
//!
//! The game never turns a car's or a boat's steering wheel: the wheel is a part fixed to the body
//! (`docs/engine/gameplay/vehicle-parts.md`). Every render update,
//! [`Vehicle::UpdateRender_ModelMatrices`] sets each part's model matrix from the part's root-space
//! transform ([`PartInstance::m_RootSpaceTransform`]). The detour turns the wheel part's root-space
//! transform about the wheel's hub by the wheel angle for the original call, and puts it back
//! afterwards, so only the drawn model turns: the part's collision, deformation, and breaking never
//! see the turn.
//!
//! The render update runs on worker threads, so the detour takes nothing from the rest of the mod
//! but this module's own slot, which the input tick fills ([`publish`]): the vehicle, the part, the
//! hub and axis in the part's own space, and the angle. A vehicle that is not the published one
//! passes through after one atomic load. The part is re-checked against the published one by its
//! properties, and skipped once it, or a part it hangs from, has broken off.
//!
//! [`Vehicle::UpdateRender_ModelMatrices`]: jc3gi::vehicle::vehicle::Vehicle::UpdateRender_ModelMatrices
//! [`PartInstance::m_RootSpaceTransform`]: jc3gi::vehicle::parts::PartInstance::m_RootSpaceTransform

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use detours_macro::detour;
use glam::{Mat4, Vec3};
use jc3gi::{
    types::math::Matrix4,
    vehicle::{parts::PartInstance, vehicle::Vehicle},
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::vehicles::wheel;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&UPDATE_RENDER_MODEL_MATRICES_BINDER)
}

/// The wheel part of the vehicle the player drives, as the input tick found it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RenderWheel {
    /// The vehicle's address.
    pub vehicle: usize,
    /// The wheel part's index.
    pub part_index: u32,
    /// The wheel part's properties' address, which identifies the part.
    pub properties: usize,
    /// The hub, in the part's own space.
    pub hub: Vec3,
    /// The face normal toward the driver, in the part's own space.
    pub axis: Vec3,
}

/// Turn `target`'s wheel to `angle` radians (clockwise as the driver sees it) from the next render
/// update on, or stop turning any wheel with `None`.
pub fn publish(target: Option<(RenderWheel, f32)>) {
    let mut slot = SLOT.lock();
    *slot = target.filter(|(_, angle)| angle.is_finite());
    VEHICLE.store(slot.map_or(0, |(w, _)| w.vehicle), Ordering::Release);
}

/// How many render updates drew the wheel turned, for the debug UI.
pub fn turned() -> u64 {
    TURNED.load(Ordering::Relaxed)
}

/// The published wheel's vehicle, or zero: the detour's lock-free first test.
static VEHICLE: AtomicUsize = AtomicUsize::new(0);

/// The published wheel and its angle. Only [`publish`] and the detour take it, each for a copy.
static SLOT: Mutex<Option<(RenderWheel, f32)>> = Mutex::new(None);

static TURNED: AtomicU64 = AtomicU64::new(0);

#[detour(address = jc3gi::vehicle::vehicle::Vehicle::UpdateRender_ModelMatrices_ADDRESS)]
fn update_render_model_matrices(this: *mut Vehicle, transform: *const Matrix4, dtf: f32) {
    let original = || {
        UPDATE_RENDER_MODEL_MATRICES
            .get()
            .unwrap()
            .call(this, transform, dtf)
    };
    let address = this as usize;
    if address == 0 || VEHICLE.load(Ordering::Acquire) != address {
        return original();
    }
    let Some((target, angle)) = *SLOT.lock() else {
        return original();
    };
    if target.vehicle != address {
        return original();
    }
    // SAFETY: the render update owns the vehicle for the call, and the part array lives as long as
    // the vehicle's physics body.
    let restore = unsafe { turn(this, &target, angle) };
    original();
    if let Some((part, saved)) = restore {
        // SAFETY: as above; the part is the one just turned.
        unsafe { (*part).m_RootSpaceTransform = saved };
        TURNED.fetch_add(1, Ordering::Relaxed);
    }
}

/// Turn the target's wheel part, returning the part and its saved root-space transform, or `None`
/// when the part is not the published one, has no model, or has broken off.
///
/// # Safety
///
/// `vehicle` must be a live vehicle inside its own render update.
unsafe fn turn(
    vehicle: *mut Vehicle,
    target: &RenderWheel,
    angle: f32,
) -> Option<(*mut PartInstance, Matrix4)> {
    let pfx = unsafe { (*vehicle).m_PfxVehicle.as_mut()? };
    let part = unsafe { pfx.GetPartByIndex(target.part_index) };
    let instance = unsafe { part.as_mut()? };
    let same = instance.m_PartProperties as usize == target.properties;
    if !same
        || instance.m_PartModelInstance.is_null()
        || unsafe { instance.BrokeOffOrAncestorBrokeOff() }
    {
        return None;
    }
    let saved = instance.m_RootSpaceTransform;
    let turned = Mat4::from(saved) * wheel::turn_about_hub(target.hub, target.axis, angle);
    if !turned.is_finite() {
        return None;
    }
    instance.m_RootSpaceTransform = Matrix4::from(turned);
    Some((part, saved))
}
