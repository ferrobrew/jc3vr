//! Actor-free grenade items, each with its own engine-owned model instance.
//!
//! A model lease is a monotonic identity, never a native object address. The loadout table owns
//! the item's place; this module owns its render instance. Only an explicit throwing release
//! submits a native activation request.

use std::{
    num::NonZeroU64,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

use glam::{Mat4, Vec3};
use jc3gi::{
    character::inventory::WeaponSlot,
    equipment::weapon::ThrownGrenadeWeapon,
    graphics_engine::model_instance::{ModelInstance, ModelInstanceFlags, ModelInstanceManager},
    types::math::Matrix4,
};
use parking_lot::Mutex;

use crate::controllers::{
    frame::{Hand, Pose},
    hands::late::lerp_transform,
    loadout::{
        Departure, Place, Socket, live,
        safe_items::{ModelLeaseId, SafeItemView},
    },
    weapons::{
        config::GrenadeConfig,
        engine,
        grenade::{hold_offset, native, release, stage::Release},
    },
};

/// Draw another safe grenade without reading or mutating ammunition.
///
/// # Safety
///
/// The game thread, outside the world update.
pub(crate) unsafe fn draw(hand: Hand, tuning: &GrenadeConfig) -> bool {
    let Some(owner) = (unsafe { engine::local_player() }) else {
        return false;
    };
    let Some(weapon) = owner
        .m_Inventory
        .m_Weapons
        .get(WeaponSlot::E_SLOT_HAND_GRENADE as usize)
        .and_then(|weapon| unsafe { weapon.px.cast::<ThrownGrenadeWeapon>().as_ref() })
    else {
        return false;
    };
    if weapon.m_PhysicalProjectileModel.px.is_null() {
        return false;
    }
    let Some(grip) = STATE.lock().grips[hand.index()] else {
        return false;
    };
    let world = matrix(grip, hand, tuning);
    let Some(manager) = (unsafe { ModelInstanceManager::get() }) else {
        return false;
    };
    let Some(model) = new_model_id() else {
        return false;
    };
    let Ok(view) = live::admit_safe_item(model, hand) else {
        return false;
    };
    let instance = unsafe {
        manager.CreateInstanceFromModel(
            &weapon.m_PhysicalProjectileModel,
            &Matrix4::from(world),
            true,
            false,
            true,
        )
    };
    let Some(instance) = (unsafe { instance.as_mut() }) else {
        let _ = live::retire_safe_item(view);
        return false;
    };
    instance
        .m_Flags
        .remove(ModelInstanceFlags::FLAG_CLIPONDISTANCE);
    unsafe { manager.ShowModelInstance(instance) };
    let owner_id = owner as *const _ as usize;
    let mut state = STATE.lock();
    state.owner = Some(owner_id);
    state.models.push(Model {
        model,
        instance: instance as *mut ModelInstance as usize,
        world: [world; 2],
        activating: false,
    });
    ensure_cleanup();
    true
}

/// The safe item in a hand; unlike a native grenade it has no explosive address.
pub(crate) fn in_hand(hand: Hand) -> Option<SafeItemView> {
    live::safe_items()
        .into_iter()
        .find(|item| item.place == Place::Hand(hand))
}

/// Move a safe item between hands, preserving its table and model identities.
pub(crate) fn transfer(from: Hand, to: Hand) -> bool {
    in_hand(from).is_some_and(|item| live::move_safe_item(item, Place::Hand(to)).is_ok())
}

/// Stow a safe grenade at an already resolved holster transform.
pub(crate) fn stow(hand: Hand, socket: Socket, world: Mat4) -> bool {
    if !world.is_finite() {
        return false;
    }
    let Some(item) = in_hand(hand) else {
        return false;
    };
    if live::move_safe_item(item, Place::Socket(socket)).is_err() {
        return false;
    }
    place(item.binding.model, world);
    true
}

/// Draw the exact safe item stored in a holster, without allocating another model.
pub(crate) fn draw_stowed(socket: Socket, hand: Hand) -> bool {
    live::safe_items()
        .into_iter()
        .find(|item| item.place == Place::Socket(socket))
        .is_some_and(|item| live::move_safe_item(item, Place::Hand(hand)).is_ok())
}

/// Update the resolved world transform of a safe item in a holster.
pub(crate) fn place_stowed(socket: Socket, world: Mat4) {
    if let Some(item) = live::safe_items()
        .into_iter()
        .find(|item| item.place == Place::Socket(socket))
    {
        place(item.binding.model, world);
    }
}

/// Release a safe grenade. An untracked or slow release remains an inert world model.
pub(crate) fn opened(hand: Hand, world_scale: f32, tuning: &GrenadeConfig) {
    let Some(item) = in_hand(hand) else {
        return;
    };
    let tracked = STATE.lock().grips[hand.index()].is_some();
    let estimate = tracked
        .then(|| release(hand, world_scale, tuning))
        .flatten();
    let Some(world) = STATE
        .lock()
        .models
        .iter()
        .find(|model| model.model == item.binding.model)
        .map(|model| model.world[1])
    else {
        return;
    };
    let Ok(loose) = live::move_safe_item(item, Place::World(Departure::Dropped)) else {
        return;
    };
    if deliberate(estimate) {
        let release = estimate.expect("a deliberate release has tracked velocity");
        if native::submit(loose, hand, world, release)
            && let Some(model) = STATE
                .lock()
                .models
                .iter_mut()
                .find(|model| model.model == item.binding.model)
        {
            model.activating = true;
        }
    }
}

/// Pick up the nearest inert loose model within reach, preserving its item identity.
pub(crate) fn take_loose(hand: Hand, world: Vec3, radius_m: f32) -> bool {
    let items = live::safe_items();
    let state = STATE.lock();
    let nearest = items
        .iter()
        .filter(|item| matches!(item.place, Place::World(_)))
        .filter_map(|item| {
            let model = state
                .models
                .iter()
                .find(|model| model.model == item.binding.model && !model.activating)?;
            let distance = model.world[1].w_axis.truncate().distance(world);
            (distance <= radius_m).then_some((*item, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(item, _)| item);
    drop(state);
    nearest.is_some_and(|item| live::move_safe_item(item, Place::Hand(hand)).is_ok())
}

/// World-space grab points for the inert loose models.
pub(crate) fn loose() -> Vec<Vec3> {
    let items = live::safe_items();
    STATE
        .lock()
        .models
        .iter()
        .filter(|model| {
            !model.activating
                && items.iter().any(|item| {
                    item.binding.model == model.model && matches!(item.place, Place::World(_))
                })
        })
        .map(|model| model.world[1].w_axis.truncate())
        .collect()
}

/// Note tracked world grips for the safe model placement.
pub(crate) fn observe(grips: [Option<Pose>; 2], tuning: &GrenadeConfig) {
    let items = live::safe_items();
    let mut state = STATE.lock();
    state.grips = grips;
    for item in items {
        if let Place::Hand(hand) = item.place
            && let Some(grip) = grips[hand.index()]
            && let Some(model) = state
                .models
                .iter_mut()
                .find(|model| model.model == item.binding.model)
        {
            model.world = [model.world[1], matrix(grip, hand, tuning)];
        }
    }
}

/// Retire the safe identity only after its replacement has launched successfully.
pub(crate) fn launched(item: SafeItemView) {
    if let Err(error) = live::retire_safe_item(item) {
        tracing::warn!(target: "controllers", %error, "grenade: launched item's safe representation could not retire");
    }
}

/// Keep the model inert and available for pickup when native activation cannot complete.
pub(crate) fn activation_failed(item: SafeItemView) {
    if let Some(model) = STATE
        .lock()
        .models
        .iter_mut()
        .find(|model| model.model == item.binding.model)
    {
        model.activating = false;
    }
}

/// Render the independent safe models; also reap models whose table identities have retired.
///
/// # Safety
///
/// The game thread, outside the world update.
pub(crate) unsafe fn render_frame(dtf: f32) {
    let owner = if crate::hooks::in_gameplay() {
        (unsafe { engine::local_player() }).map(|owner| owner as *const _ as usize)
    } else {
        None
    };
    let mut state = STATE.lock();
    if state.owner.is_some() && state.owner != owner {
        drop(state);
        unsafe { clear() };
        return;
    }
    let items = live::safe_items();
    let Some(manager) = (unsafe { ModelInstanceManager::get() }) else {
        return;
    };
    state.models.retain(|model| {
        let exists = items.iter().any(|item| item.binding.model == model.model);
        if !exists {
            unsafe { manager.AddToDeleteQueue(model.instance as *mut ModelInstance, false) };
        }
        exists
    });
    let t = if dtf.is_finite() {
        dtf.clamp(0.0, 1.0)
    } else {
        1.0
    };
    for model in &state.models {
        let world = lerp_transform(model.world[0], model.world[1], t);
        if world.is_finite() {
            unsafe {
                (*(model.instance as *mut ModelInstance)).UpdateMatrix(&Matrix4::from(world))
            };
        }
    }
}

/// Leave held items as inert models when input stops, rather than arming them.
pub(crate) fn stand_down() {
    native::cancel();
    for hand in Hand::BOTH {
        if let Some(item) = in_hand(hand) {
            let _ = live::move_safe_item(item, Place::World(Departure::Dropped));
        }
    }
}

fn deliberate(release: Option<Release>) -> bool {
    release.is_some_and(|release| release.velocity.is_finite() && release.velocity.length() >= 1.0)
}

fn new_model_id() -> Option<ModelLeaseId> {
    NEXT_MODEL
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
            next.checked_add(1)
        })
        .ok()
        .and_then(NonZeroU64::new)
        .map(ModelLeaseId::new)
}

fn matrix(grip: Pose, hand: Hand, tuning: &GrenadeConfig) -> Mat4 {
    Mat4::from_rotation_translation(
        grip.orientation,
        grip.position + grip.orientation * hold_offset(hand, tuning),
    )
}

fn place(id: ModelLeaseId, world: Mat4) {
    if !world.is_finite() {
        return;
    }
    if let Some(model) = STATE
        .lock()
        .models
        .iter_mut()
        .find(|model| model.model == id)
    {
        model.world = [model.world[1], world];
    }
}

unsafe fn clear() {
    native::cancel();
    let mut state = STATE.lock();
    for model in state.models.drain(..) {
        if let Some(item) = live::safe_items()
            .into_iter()
            .find(|item| item.binding.model == model.model)
        {
            let _ = live::retire_safe_item(item);
        }
        if let Some(manager) = unsafe { ModelInstanceManager::get() } {
            unsafe { manager.AddToDeleteQueue(model.instance as *mut ModelInstance, false) };
        }
    }
    state.owner = None;
    state.grips = [None; 2];
}

fn ensure_cleanup() {
    if !CLEANUP.swap(true, Ordering::AcqRel) {
        crate::lifecycle::on_cleanup(|_| {
            // SAFETY: the shutdown cleanups run on the game thread outside the world update.
            unsafe { clear() };
        });
    }
}

struct Model {
    model: ModelLeaseId,
    instance: usize,
    world: [Mat4; 2],
    activating: bool,
}

struct State {
    owner: Option<usize>,
    grips: [Option<Pose>; 2],
    models: Vec<Model>,
}

static STATE: Mutex<State> = Mutex::new(State {
    owner: None,
    grips: [None; 2],
    models: Vec::new(),
});
static NEXT_MODEL: AtomicU64 = AtomicU64::new(1);
static CLEANUP: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::loadout::{BodyLayout, Loadout};

    #[test]
    fn independent_runtime_bindings_keep_identity_across_hands_holsters_and_inert_drop() {
        let mut table = Loadout::new(BodyLayout::full(Hand::Right));
        let first = table
            .admit_safe_item(new_model_id().unwrap(), Hand::Left)
            .unwrap();
        let second = table
            .admit_safe_item(new_model_id().unwrap(), Hand::Right)
            .unwrap();
        assert_ne!(first.item, second.item);
        assert_ne!(first.binding.model, second.binding.model);
        let stowed = table
            .move_safe_item(first, Place::Socket(Socket::Hip(Hand::Left)))
            .unwrap();
        let dropped = table
            .move_safe_item(second, Place::World(Departure::Dropped))
            .unwrap();
        let drawn = table
            .move_safe_item(stowed, Place::Hand(Hand::Right))
            .unwrap();
        let transferred = table
            .move_safe_item(drawn, Place::Hand(Hand::Left))
            .unwrap();
        let recovered = table
            .move_safe_item(dropped, Place::Hand(Hand::Right))
            .unwrap();
        assert_eq!(transferred.item, first.item);
        assert_eq!(transferred.binding.model, first.binding.model);
        assert_eq!(recovered.item, second.item);
        assert_eq!(recovered.binding.model, second.binding.model);
        assert_eq!(table.retire_safe_item(transferred), Ok(first.binding.model));
        assert_eq!(table.safe_item(first.item), None);
        assert_eq!(table.safe_item(second.item), Some(recovered));
    }

    #[test]
    fn only_a_tracked_deliberate_release_activates_a_grenade() {
        assert!(!deliberate(None));
        for velocity in [Vec3::ZERO, Vec3::X * 0.99, Vec3::NAN] {
            assert!(!deliberate(Some(Release { velocity })));
        }
        assert!(deliberate(Some(Release { velocity: Vec3::X })));
        assert!(deliberate(Some(Release {
            velocity: -Vec3::Y * 8.0
        })));
    }

    #[test]
    fn model_identities_are_distinct_non_pointer_tokens() {
        let first = new_model_id().unwrap();
        let second = new_model_id().unwrap();
        assert_ne!(first, second);
    }
}
