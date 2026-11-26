//! The collider view's step counter: a pre-simulation callback
//! ([`PhysicsSystem::AddPreSimulationCallback`]) that counts the physics steps, so that the view
//! reads the world once after each step rather than once per rendered frame. It changes nothing.
//!
//! As the hand bodies' own callback, the object is a static with a static virtual table, registered
//! from the game thread's frame tick ahead of the game's update, so never while the callbacks fire,
//! and removed there when the view is switched off, or by the shutdown cleanup.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use jc3gi::{
    physics::{
        PfxPreSimulationCallback, PfxPreSimulationCallbackVftable, PhysicsSystem, hknpWorld,
    },
    types::vector_math::AlignedVector3,
};

/// Register the callback when `wanted` and the physics system exists, and remove it when not
/// `wanted`. Returns whether it is registered.
///
/// # Safety
///
/// The game thread, ahead of the game's update.
pub(super) unsafe fn reconcile(wanted: bool) -> bool {
    let registered = REGISTERED.load(Ordering::Acquire);
    if wanted == registered {
        return registered;
    }
    let Some(physics) = (unsafe { PhysicsSystem::get() }) else {
        REGISTERED.store(false, Ordering::Release);
        return false;
    };
    if wanted {
        unsafe { physics.AddPreSimulationCallback(object()) };
        tracing::info!("collider view: registered the step counter");
    } else {
        unsafe { physics.RemovePreSimulationCallback(object()) };
        tracing::info!("collider view: removed the step counter");
    }
    REGISTERED.store(wanted, Ordering::Release);
    wanted
}

/// Remove the callback, if it is registered.
///
/// # Safety
///
/// The game thread, outside the game's update.
pub(super) unsafe fn unregister() {
    unsafe { reconcile(false) };
}

/// The physics steps counted since the payload loaded.
pub(super) fn steps() -> u64 {
    STEPS.load(Ordering::Relaxed)
}

static REGISTERED: AtomicBool = AtomicBool::new(false);
static STEPS: AtomicU64 = AtomicU64::new(0);

/// The engine-facing object: a pointer to its virtual table, and nothing the engine writes.
#[repr(C)]
struct CallbackObject {
    vftable: &'static PfxPreSimulationCallbackVftable,
}

static VFTABLE: PfxPreSimulationCallbackVftable = PfxPreSimulationCallbackVftable {
    Destructor: destructor,
    preSimulationCallback: pre_simulation,
    onWorldShiftCallback: world_shift,
};

static OBJECT: CallbackObject = CallbackObject { vftable: &VFTABLE };

fn object() -> *mut PfxPreSimulationCallback {
    (&raw const OBJECT).cast_mut().cast()
}

/// The engine never deletes a pre-simulation callback; the object is a static either way.
unsafe extern "system" fn destructor(_this: *mut PfxPreSimulationCallback, _flags: u8) {}

unsafe extern "system" fn pre_simulation(
    _this: *mut PfxPreSimulationCallback,
    _world: *mut hknpWorld,
    _dt: f32,
) {
    STEPS.fetch_add(1, Ordering::Relaxed);
}

/// The view keeps game-space positions only, which a recentring does not move.
unsafe extern "system" fn world_shift(
    _this: *mut PfxPreSimulationCallback,
    _shift: *const AlignedVector3,
) {
}
