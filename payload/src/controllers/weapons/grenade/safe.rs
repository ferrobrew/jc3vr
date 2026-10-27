//! Safe grenade items, each backed by its own inert native grenade.
//!
//! An item is identified by a monotonic lease, never by a native object address. The loadout table
//! owns the item's place. This module owns its inert actor and the held/stowed transform pair;
//! the simulation owns world-item transforms. Drawing uses the grenade weapon's factory with pool
//! reclamation disabled, so exhaustion refuses admission without removing another object.
//!
//! The actor is inert from the moment it is created: only a deliberate, tracked release arms it, and
//! that happens after it has left the hand. A stowed or dropped item stays inert forever, so a lost
//! grip, a lost tracker, or a lost input can end an item's time in a hand but never its safety.

use std::{
    num::NonZeroU64,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

use glam::{Mat4, Vec3};
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::ThrownGrenadeWeapon,
};
use parking_lot::Mutex;

use crate::controllers::{
    frame::{Hand, Pose},
    loadout::{
        Departure, Place, Socket, live,
        safe_items::{ItemLeaseId, SafeItemView},
    },
    weapons::{
        config::GrenadeConfig,
        engine,
        grenade::{
            Release, hold_offset,
            native::{self, Actor},
            release,
        },
    },
};

/// Successful safe-item transitions since the payload loaded.
#[derive(Default, Debug, Copy, Clone)]
pub(crate) struct Counts {
    pub drawn: u64,
    pub stowed: u64,
    pub redrawn: u64,
    pub dropped: u64,
    pub recovered: u64,
}

pub(crate) fn counts() -> Counts {
    *COUNTS.lock()
}

/// Draw another safe grenade without reading or mutating ammunition.
///
/// # Safety
///
/// The serialized input fragment before ammunition pre-simulation, or the render boundary.
pub(crate) unsafe fn draw(hand: Hand, tuning: &GrenadeConfig) -> bool {
    let Some(owner) = (unsafe { engine::local_player() }) else {
        return false;
    };
    let Some(weapon) = grenade_weapon(owner) else {
        return false;
    };
    let Some(grip) = STATE.lock().grips[hand.index()] else {
        return false;
    };
    let world = matrix(grip, hand, tuning);
    let Some(model) = new_model_id() else {
        return false;
    };
    // The actor comes first: a refusal from the pool must leave no item behind, and an item must
    // never exist without the actor that shows it.
    let Some(actor) = (unsafe { native::create(weapon, world) }) else {
        return false;
    };
    if live::admit_safe_item(model, hand).is_err() {
        unsafe { native::remove(&actor) };
        return false;
    }
    let owner_id = owner as *const Character as usize;
    let mut state = STATE.lock();
    state.owner = Some(owner_id);
    state.items.push(Item {
        model,
        actor,
        world: [world; 2],
    });
    drop(state);
    ensure_cleanup();
    COUNTS.lock().drawn += 1;
    true
}

/// The safe item in a hand; unlike a caught grenade it has no explosive address to hand out.
pub(crate) fn in_hand(hand: Hand) -> Option<SafeItemView> {
    live::safe_items()
        .into_iter()
        .find(|item| item.place == Place::Hand(hand))
}

/// Move a safe item between hands, carrying its actor and its table identity.
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
    place(item.binding.lease, world);
    COUNTS.lock().stowed += 1;
    true
}

/// Draw the exact safe item stored in a holster, without allocating another actor.
pub(crate) fn draw_stowed(socket: Socket, hand: Hand) -> bool {
    let moved = live::safe_items()
        .into_iter()
        .find(|item| item.place == Place::Socket(socket))
        .is_some_and(|item| live::move_safe_item(item, Place::Hand(hand)).is_ok());
    if moved {
        COUNTS.lock().redrawn += 1;
    }
    moved
}

/// Update the resolved world transform of a safe item in a holster.
pub(crate) fn place_stowed(socket: Socket, world: Mat4) {
    if let Some(item) = live::safe_items()
        .into_iter()
        .find(|item| item.place == Place::Socket(socket))
    {
        place(item.binding.lease, world);
    }
}

/// Release a safe grenade. Only a tracked, deliberate release arms it; anything else leaves the item
/// inert and lets the simulation take it.
pub(crate) fn opened(hand: Hand, world_scale: f32, tuning: &GrenadeConfig) {
    let Some(item) = in_hand(hand) else {
        return;
    };
    let tracked = STATE.lock().grips[hand.index()].is_some();
    let estimate = tracked
        .then(|| release(hand, world_scale, tuning))
        .flatten();
    let Some(world) = position(item.binding.lease) else {
        return;
    };
    let launch = native::Launch {
        item,
        hand,
        world,
        velocity: estimate.map_or(Vec3::ZERO, |release| release.velocity),
        arm: deliberate(estimate, tuning),
    };
    native::submit(launch);
}

/// Take up the nearest inert item within reach, giving it a fresh actor in the hand.
///
/// A released actor is a simulating body, which the engine no longer follows from its transform, so
/// the item is admitted to the hand with a new inert actor rather than a teleported one.
pub(crate) fn take_loose(hand: Hand, world: Vec3, tuning: &GrenadeConfig) -> bool {
    let Some(item) = nearest_loose(world, tuning.catch.radius_m) else {
        return false;
    };
    let Some(old) = actor(item).filter(|actor| actor.explosive().is_some()) else {
        return false;
    };
    if native::waiting(item) || live::safe_item(item.item) != Some(item) {
        return false;
    }
    let Some(held) = grip_world(hand, tuning) else {
        return false;
    };
    let Some(owner) = (unsafe { engine::local_player() }) else {
        return false;
    };
    let Some(weapon) = grenade_weapon(owner) else {
        return false;
    };
    let Some(replacement) = (unsafe { native::create(weapon, held) }) else {
        return false;
    };
    let landed = {
        let mut state = STATE.lock();
        state
            .items
            .iter_mut()
            .find(|entry| entry.model == item.binding.lease)
            .filter(|_| old.explosive().is_some())
            .filter(|_| live::move_safe_item(item, Place::Hand(hand)).is_ok())
            .map(|entry| {
                entry.world = [held; 2];
                std::mem::replace(&mut entry.actor, replacement.clone())
            })
    };
    let Some(landed) = landed else {
        unsafe { native::remove(&replacement) };
        return false;
    };
    unsafe { native::remove(&landed) };
    COUNTS.lock().recovered += 1;
    true
}

/// World-space grab points for the inert loose items.
pub(crate) fn loose() -> Vec<Vec3> {
    let items = live::safe_items();
    let state = STATE.lock();
    state
        .items
        .iter()
        .filter(|entry| {
            items.iter().any(|item| {
                item.binding.lease == entry.model && matches!(item.place, Place::World(_))
            })
        })
        .filter_map(|entry| entry.actor.position())
        .collect()
}

/// The actor behind an item, while the pool still holds it.
pub(super) fn actor(item: SafeItemView) -> Option<Actor> {
    STATE
        .lock()
        .items
        .iter()
        .find(|entry| entry.model == item.binding.lease)
        .map(|entry| entry.actor.clone())
}

/// Move a released item into the world: its actor is in the simulation, which owns it from here.
pub(super) fn dropped(item: SafeItemView) {
    let Some(current) = live::safe_item(item.item) else {
        return;
    };
    if matches!(current.place, Place::World(_)) {
        return;
    }
    if let Err(error) = live::move_safe_item(current, Place::World(Departure::Dropped)) {
        tracing::debug!(target: "controllers", %error, "grenade: released item could not be placed in the world");
    } else {
        COUNTS.lock().dropped += 1;
    }
}

/// Retire an item whose tree became a live grenade: the loadout no longer owns it.
pub(super) fn launched(item: SafeItemView) {
    retire(item, "launched item");
}

/// Retire an item whose actor the pool has already reclaimed.
pub(super) fn lost(item: SafeItemView) {
    forget(item.binding.lease);
    retire(item, "item whose actor was reclaimed");
}

/// Note tracked world grips for the item transforms.
pub(crate) fn observe(grips: [Option<Pose>; 2], tuning: &GrenadeConfig) {
    let items = live::safe_items();
    let mut state = STATE.lock();
    state.grips = grips;
    for item in items {
        if let Place::Hand(hand) = item.place
            && let Some(grip) = grips[hand.index()]
            && let Some(entry) = state
                .items
                .iter_mut()
                .find(|entry| entry.model == item.binding.lease)
        {
            entry.world = [entry.world[1], matrix(grip, hand, tuning)];
        }
    }
}

/// Give every item's actor the transform its place resolves to, and retire the items whose actor the
/// pool has taken.
///
/// # Safety
///
/// The game thread, outside the world update.
pub(crate) unsafe fn render_frame() {
    let owner = if crate::hooks::in_gameplay() {
        (unsafe { engine::local_player() }).map(|owner| owner as *const Character as usize)
    } else {
        None
    };
    let current_owner = STATE.lock().owner;
    if current_owner.is_some() && current_owner != owner {
        unsafe { clear() };
        return;
    }
    let retiring = std::mem::take(&mut STATE.lock().retiring);
    for actor in retiring {
        unsafe { native::remove(&actor) };
    }
    let items = live::safe_items();
    native::reconcile(&items);
    let RegistryPlan {
        gone,
        removed,
        placements,
    } = reconcile_registry(&mut STATE.lock().items, &items, |actor| {
        actor.explosive().is_some()
    });
    for actor in removed {
        unsafe { native::remove(&actor) };
    }
    for (actor, [previous, world]) in placements {
        unsafe { native::place(&actor, previous, world) };
    }
    for item in &items {
        if gone.contains(&item.binding.lease) || actor(*item).is_none() {
            lost(*item);
        }
    }
    if let Some(owner) = unsafe { engine::local_player() }
        && let Some(weapon) = grenade_weapon(owner)
    {
        unsafe { native::update(weapon) };
    }
}

/// Leave held items to the world when input stops, without arming anything.
pub(crate) fn stand_down() {
    native::disarm();
    for hand in Hand::BOTH {
        let Some(item) = in_hand(hand) else {
            continue;
        };
        let Some(world) = position(item.binding.lease) else {
            continue;
        };
        native::submit(native::Launch {
            item,
            hand,
            world,
            velocity: Vec3::ZERO,
            arm: false,
        });
    }
}

/// Retire every owned actor at the next serialized render boundary. This is safe from reset
/// callbacks, which may run on a simulation worker and must not dereference pooled actors.
pub(crate) fn reset() {
    native::clear();
    let mut state = STATE.lock();
    let items = std::mem::take(&mut state.items);
    state
        .retiring
        .extend(items.into_iter().map(|item| item.actor));
    state.owner = None;
    state.grips = [None; 2];
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Membership {
    Remove,
    Lost,
    Drive,
    Simulated,
}

fn membership(place: Option<Place>, alive: bool) -> Membership {
    if !alive {
        Membership::Lost
    } else {
        match place {
            None => Membership::Remove,
            Some(Place::Hand(_) | Place::Socket(_)) => Membership::Drive,
            Some(_) => Membership::Simulated,
        }
    }
}

fn deliberate(release: Option<Release>, tuning: &GrenadeConfig) -> bool {
    release.is_some_and(|release| {
        release.velocity.is_finite() && release.velocity.length() >= tuning.arm_speed_mps.max(0.0)
    })
}

fn nearest_loose(world: Vec3, radius_m: f32) -> Option<SafeItemView> {
    let items = live::safe_items();
    let state = STATE.lock();
    items
        .iter()
        .filter(|item| matches!(item.place, Place::World(_)))
        .filter_map(|item| {
            let centre = state
                .items
                .iter()
                .find(|entry| entry.model == item.binding.lease)?
                .actor
                .position()?;
            let distance = centre.distance(world);
            (distance.is_finite() && distance <= radius_m).then_some((*item, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(item, _)| item)
}

fn grenade_weapon(owner: &mut Character) -> Option<&mut ThrownGrenadeWeapon> {
    let weapon = owner
        .m_Inventory
        .m_Weapons
        .get(WeaponSlot::E_SLOT_HAND_GRENADE as usize)?;
    // SAFETY: the inventory owns its weapons for as long as the owner lives.
    unsafe { weapon.px.cast::<ThrownGrenadeWeapon>().as_mut() }
}

fn position(model: ItemLeaseId) -> Option<Mat4> {
    STATE
        .lock()
        .items
        .iter()
        .find(|entry| entry.model == model)
        .map(|entry| entry.world[1])
}

fn place(model: ItemLeaseId, world: Mat4) {
    if !world.is_finite() {
        return;
    }
    if let Some(entry) = STATE
        .lock()
        .items
        .iter_mut()
        .find(|entry| entry.model == model)
    {
        entry.world = [entry.world[1], world];
    }
}

fn forget(model: ItemLeaseId) {
    STATE.lock().items.retain(|entry| entry.model != model);
}

fn retire(item: SafeItemView, what: &str) {
    forget(item.binding.lease);
    if let Err(error) = live::retire_safe_item(item) {
        tracing::warn!(target: "controllers", %error, "grenade: {what}'s safe representation could not retire");
    }
}

fn new_model_id() -> Option<ItemLeaseId> {
    NEXT_MODEL
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
            next.checked_add(1)
        })
        .ok()
        .and_then(NonZeroU64::new)
        .map(ItemLeaseId::new)
}

fn matrix(grip: Pose, hand: Hand, tuning: &GrenadeConfig) -> Mat4 {
    Mat4::from_rotation_translation(
        grip.orientation,
        grip.position + grip.orientation * hold_offset(hand, tuning),
    )
}

/// The transform a held item takes in a tracked hand.
fn grip_world(hand: Hand, tuning: &GrenadeConfig) -> Option<Mat4> {
    STATE.lock().grips[hand.index()].map(|grip| matrix(grip, hand, tuning))
}

unsafe fn clear() {
    // The queue names items that are about to be retired, so it goes first.
    native::clear();
    let actors: Vec<Actor> = {
        let mut state = STATE.lock();
        let mut actors = std::mem::take(&mut state.retiring);
        actors.extend(state.items.drain(..).map(|item| item.actor));
        actors
    };
    if !actors.is_empty() {
        tracing::debug!(
            target: "controllers",
            actors = actors.len(),
            "grenade: clearing every safe item's actor",
        );
    }
    for actor in &actors {
        unsafe { native::remove(actor) };
    }
    let items = live::safe_items();
    for item in items {
        if let Err(error) = live::retire_safe_item(item) {
            tracing::warn!(target: "controllers", %error, "grenade: item could not retire on cleanup");
        }
    }
    let mut state = STATE.lock();
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

struct RegistryPlan<A> {
    gone: Vec<ItemLeaseId>,
    removed: Vec<A>,
    placements: Vec<(A, [Mat4; 2])>,
}

fn reconcile_registry<A: Clone>(
    entries: &mut Vec<Item<A>>,
    items: &[SafeItemView],
    alive: impl Fn(&A) -> bool,
) -> RegistryPlan<A> {
    let mut plan = RegistryPlan {
        gone: Vec::new(),
        removed: Vec::new(),
        placements: Vec::new(),
    };
    entries.retain(|entry| {
        let item = items.iter().find(|item| item.binding.lease == entry.model);
        match membership(item.map(|item| item.place), alive(&entry.actor)) {
            Membership::Remove => {
                plan.removed.push(entry.actor.clone());
                false
            }
            Membership::Lost => {
                plan.gone.push(entry.model);
                false
            }
            Membership::Drive => {
                plan.placements.push((entry.actor.clone(), entry.world));
                true
            }
            Membership::Simulated => true,
        }
    });
    plan
}

struct Item<A = Actor> {
    model: ItemLeaseId,
    actor: A,
    /// The item's transform for the previous tick and the current one, which the actor's render
    /// update interpolates between.
    world: [Mat4; 2],
}

struct State {
    owner: Option<usize>,
    grips: [Option<Pose>; 2],
    items: Vec<Item>,
    retiring: Vec<Actor>,
}

static STATE: Mutex<State> = Mutex::new(State {
    owner: None,
    grips: [None; 2],
    items: Vec::new(),
    retiring: Vec::new(),
});
static COUNTS: Mutex<Counts> = Mutex::new(Counts {
    drawn: 0,
    stowed: 0,
    redrawn: 0,
    dropped: 0,
    recovered: 0,
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
        assert_ne!(first.binding.lease, second.binding.lease);
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
        assert_eq!(transferred.binding.lease, first.binding.lease);
        assert_eq!(recovered.item, second.item);
        assert_eq!(recovered.binding.lease, second.binding.lease);
        assert_eq!(table.retire_safe_item(transferred), Ok(first.binding.lease));
        assert_eq!(table.safe_item(first.item), None);
        assert_eq!(table.safe_item(second.item), Some(recovered));
    }

    #[test]
    fn only_a_tracked_deliberate_release_arms_a_grenade() {
        let tuning = GrenadeConfig::new();
        assert!(!deliberate(None, &tuning));
        for velocity in [
            Vec3::ZERO,
            Vec3::NAN,
            Vec3::X * (tuning.arm_speed_mps * 0.5),
        ] {
            assert!(!deliberate(Some(Release { velocity }), &tuning));
        }
        for velocity in [Vec3::X * tuning.arm_speed_mps, -Vec3::Y * 8.0] {
            assert!(deliberate(Some(Release { velocity }), &tuning));
        }
    }

    #[test]
    fn the_arming_speed_comes_from_the_tuning() {
        let slow = GrenadeConfig {
            arm_speed_mps: 0.25,
            ..GrenadeConfig::new()
        };
        let fast = GrenadeConfig {
            arm_speed_mps: 4.0,
            ..GrenadeConfig::new()
        };
        let gentle = Release {
            velocity: Vec3::X * 0.5,
        };
        assert!(deliberate(Some(gentle), &slow));
        assert!(!deliberate(Some(gentle), &fast));
    }

    #[test]
    fn registry_plan_preserves_world_pose_and_retires_orphaned_or_expired_actors() {
        let mut table = Loadout::new(BodyLayout::full(Hand::Right));
        let held = table
            .admit_safe_item(new_model_id().unwrap(), Hand::Left)
            .unwrap();
        let loose = table
            .admit_safe_item(new_model_id().unwrap(), Hand::Right)
            .unwrap();
        let loose = table
            .move_safe_item(loose, Place::World(Departure::Dropped))
            .unwrap();
        let orphan = new_model_id().unwrap();
        let expired = new_model_id().unwrap();
        let cached = Mat4::from_translation(Vec3::X);
        let mut entries = vec![
            Item {
                model: held.binding.lease,
                actor: 1,
                world: [cached; 2],
            },
            Item {
                model: loose.binding.lease,
                actor: 2,
                world: [cached; 2],
            },
            Item {
                model: orphan,
                actor: 3,
                world: [cached; 2],
            },
            Item {
                model: expired,
                actor: 4,
                world: [cached; 2],
            },
        ];
        let items: Vec<_> = table.safe_items().collect();
        let plan = reconcile_registry(&mut entries, &items, |actor| *actor != 4);
        assert_eq!(plan.placements, vec![(1, [cached; 2])]);
        assert_eq!(plan.removed, vec![3]);
        assert_eq!(plan.gone, vec![expired]);
        assert_eq!(
            entries.iter().map(|entry| entry.actor).collect::<Vec<_>>(),
            [1, 2]
        );
        let plan = reconcile_registry(&mut entries, &[], |_| true);
        assert_eq!(plan.removed, [1, 2]);
        assert!(plan.placements.is_empty());
        assert!(entries.is_empty());
    }

    #[test]
    fn rendering_drives_only_held_and_stowed_live_items() {
        assert_eq!(
            membership(Some(Place::Hand(Hand::Left)), true),
            Membership::Drive
        );
        assert_eq!(
            membership(Some(Place::Socket(Socket::Hip(Hand::Right))), true),
            Membership::Drive
        );
        assert_eq!(
            membership(Some(Place::World(Departure::Dropped)), true),
            Membership::Simulated
        );
        assert_eq!(membership(None, true), Membership::Remove);
        assert_eq!(
            membership(Some(Place::Hand(Hand::Left)), false),
            Membership::Lost
        );
        assert_eq!(membership(None, false), Membership::Lost);
    }

    #[test]
    fn stop_and_restart_reconcile_old_leases_even_with_the_same_owner() {
        let mut table = Loadout::new(BodyLayout::full(Hand::Right));
        let held = table
            .admit_safe_item(new_model_id().unwrap(), Hand::Left)
            .unwrap();
        let stowed = table
            .admit_safe_item(new_model_id().unwrap(), Hand::Right)
            .unwrap();
        let stowed = table
            .move_safe_item(stowed, Place::Socket(Socket::Hip(Hand::Right)))
            .unwrap();
        let loose = table
            .admit_safe_item(new_model_id().unwrap(), Hand::Right)
            .unwrap();
        let loose = table
            .move_safe_item(loose, Place::World(Departure::Dropped))
            .unwrap();
        table = Loadout::new(BodyLayout::full(Hand::Right));
        let fresh = table
            .admit_safe_item(new_model_id().unwrap(), Hand::Left)
            .unwrap();
        for old in [held, stowed, loose] {
            let place = table
                .safe_items()
                .into_iter()
                .find(|item| item.binding.lease == old.binding.lease)
                .map(|item| item.place);
            assert_eq!(membership(place, true), Membership::Remove);
        }
        assert_eq!(membership(Some(fresh.place), true), Membership::Drive);
    }

    #[test]
    fn model_identities_are_distinct_non_pointer_tokens() {
        let first = new_model_id().unwrap();
        let second = new_model_id().unwrap();
        assert_ne!(first, second);
    }
}
