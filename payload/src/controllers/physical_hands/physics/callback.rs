//! The hand bodies' pre-simulation callback: an object the physics system calls before each step
//! ([`PhysicsSystem::AddPreSimulationCallback`]), on the thread that steps the world and outside the
//! step, where bodies may be made, added, removed, and driven.
//!
//! The object is a static with a static virtual table, so the engine's pointer to it stays valid
//! for the payload's lifetime. It is registered from the game thread's frame tick, ahead of the
//! game's update and so never while the callbacks fire (a registration then would invalidate the
//! engine's walk over them), and removed by the shutdown cleanup before the payload unloads.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use jc3gi::{
    physics::{
        PfxPreSimulationCallback, PfxPreSimulationCallbackVftable, PhysicsSystem, hknpWorld,
    },
    types::vector_math::AlignedVector3,
};
use windows::Win32::System::Threading::GetCurrentThreadId;

/// Register the callback once, when the physics system exists. Returns whether it is registered.
///
/// # Safety
///
/// The game thread, ahead of the game's update (the physics system fires the callbacks inside it).
pub(super) unsafe fn ensure_registered() -> bool {
    if REGISTERED.load(Ordering::Acquire) {
        return true;
    }
    let Some(physics) = (unsafe { PhysicsSystem::get() }) else {
        return false;
    };
    unsafe { physics.AddPreSimulationCallback(object()) };
    REGISTERED.store(true, Ordering::Release);
    // SAFETY: reads the calling thread's own id.
    let thread = unsafe { GetCurrentThreadId() };
    REGISTERING_THREAD.store(thread, Ordering::Relaxed);
    tracing::info!(
        thread,
        "hand bodies: registered the pre-simulation callback"
    );
    true
}

/// Remove the callback, if it is registered.
///
/// # Safety
///
/// The game thread, outside the game's update.
pub(super) unsafe fn unregister() {
    if !REGISTERED.swap(false, Ordering::AcqRel) {
        return;
    }
    if let Some(physics) = unsafe { PhysicsSystem::get() } {
        unsafe { physics.RemovePreSimulationCallback(object()) };
        tracing::info!("hand bodies: removed the pre-simulation callback");
    }
}

/// The game thread's id, as the registration saw it; zero before it.
pub(super) fn registering_thread() -> u32 {
    REGISTERING_THREAD.load(Ordering::Relaxed)
}

/// Whether the callback is registered with the physics system.
pub(super) fn registered() -> bool {
    REGISTERED.load(Ordering::Acquire)
}

/// The id of the thread the callback last fired on; zero before it first fired.
pub(super) fn firing_thread() -> u32 {
    FIRING_THREAD.load(Ordering::Relaxed)
}

/// How many times the callback has fired since the payload loaded.
pub(super) fn fires() -> u64 {
    FIRES.load(Ordering::Relaxed)
}

static REGISTERED: AtomicBool = AtomicBool::new(false);
static REGISTERING_THREAD: AtomicU32 = AtomicU32::new(0);
static FIRST_FIRE: AtomicBool = AtomicBool::new(true);
static FIRING_THREAD: AtomicU32 = AtomicU32::new(0);
static FIRES: AtomicU64 = AtomicU64::new(0);

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
    FIRES.fetch_add(1, Ordering::Relaxed);
    // SAFETY: reads the calling thread's own id.
    FIRING_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
    if FIRST_FIRE.swap(false, Ordering::Relaxed) {
        // SAFETY: reads the calling thread's own id.
        let thread = unsafe { GetCurrentThreadId() };
        let registering = REGISTERING_THREAD.load(Ordering::Relaxed);
        if thread == registering {
            tracing::info!(
                thread,
                "hand bodies: the pre-simulation callback first fired"
            );
        } else {
            tracing::warn!(
                thread,
                registering,
                "hand bodies: the pre-simulation callback fired on another thread than it was \
                 registered from"
            );
        }
    }
    // The payload aborts on a panic, so the step must not panic: its paths handle a missing body,
    // joint, or grip as a lost one.
    let _timing = crate::frame_timing::scope(crate::frame_timing::Slot::HandBodies);
    // SAFETY: the engine calls this before a step, on the thread that steps the world.
    unsafe { crate::controllers::physical_hands::physics::step() }
}

/// The hand bodies keep no Havok-space positions: the step converts game space with the world's
/// current shift each time.
unsafe extern "system" fn world_shift(
    _this: *mut PfxPreSimulationCallback,
    _shift: *const AlignedVector3,
) {
}
