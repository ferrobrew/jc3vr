//! Safe grenade items, each backed by its own inert native grenade.
//!
//! An item is identified by a monotonic lease, never by a native object address. The loadout table
//! owns the item's place. This module owns its inert actor and the held/stowed transform pair;
//! the simulation owns world-item transforms. Drawing uses the grenade weapon's factory with pool
//! reclamation disabled, so exhaustion refuses admission without removing another object.
//!
//! The actor is inert from the moment it is created: only pulling its pin in a hand ([`pull_pin`])
//! starts its fuse. An item with its pin in stays inert however it leaves the hand, so a throw, a
//! slow open, a lost tracker, or a lost input can end its time in a hand but never its safety; a
//! dropped one is a world item a hand can take up again. An item whose pin is out cannot be stowed,
//! and once let go it is a live grenade the loadout no longer owns ([`native`]).

use std::{
    num::{NonZeroU64, NonZeroUsize},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

use glam::{Mat4, Vec3};
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::ThrownGrenadeWeapon,
};
use parking_lot::Mutex;

use crate::controllers::{
    body::rig::frames::Frames,
    frame::{Hand, Pose},
    hands::held::held_matrix,
    loadout::{
        Departure, ItemId, Place, Socket, live,
        safe_items::{ItemLeaseId, SafeItemView},
    },
    motion,
    weapons::{
        engine,
        grenade::{
            Release,
            catch::body_of,
            hold_local,
            native::{self, Actor, PinPull},
            throw_release,
            tuning::GrenadeTuning,
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
    /// Items the world made in place of native pickups.
    pub spawned: u64,
}

pub(crate) fn counts() -> Counts {
    *COUNTS.lock()
}

/// Draw another safe grenade without reading or mutating ammunition.
///
/// Refused outside the top of the world's pre-simulation queue, where the actor could not be
/// spawned safely ([`native::create`]).
///
/// # Safety
///
/// The input tick's game stage, on the game thread.
pub(crate) unsafe fn draw(hand: Hand, tuning: &GrenadeTuning) -> bool {
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

/// Make a safe grenade lying in the world at `world`, moving with `release`: a grenade the world
/// made, in place of a native pickup. Its actor is created as a draw's is, under the same pool and
/// stage rules, admitted to the world, and released inert into the simulation once its body exists
/// ([`native::submit`]). `false`, with nothing made, when the pool or the stage refuses.
///
/// # Safety
///
/// The input tick's game stage, on the game thread.
pub(crate) unsafe fn spawn_in_world(world: Mat4, release: Release) -> bool {
    let Some(owner) = (unsafe { engine::local_player() }) else {
        return false;
    };
    let Some(weapon) = grenade_weapon(owner) else {
        return false;
    };
    let Some(model) = new_model_id() else {
        return false;
    };
    let Some(actor) = (unsafe { native::create(weapon, world) }) else {
        return false;
    };
    let Ok(item) = live::admit_safe_item_to_world(model) else {
        unsafe { native::remove(&actor) };
        return false;
    };
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
    native::submit(native::Launch {
        item,
        hand: None,
        world,
        release,
    });
    COUNTS.lock().spawned += 1;
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

/// Stow a safe grenade at an already resolved holster transform. An item whose pin is out is
/// refused: its fuse would run out on the body.
pub(crate) fn stow(hand: Hand, socket: Socket, world: Mat4) -> bool {
    if !world.is_finite() {
        return false;
    }
    let Some(item) = in_hand(hand) else {
        return false;
    };
    if actor(item).is_none_or(|actor| native::pin_out(&actor)) {
        return false;
    }
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

/// Pull the pin of the item in `hand`, starting its fuse. `None` when the hand holds no item.
///
/// # Safety
///
/// The input tick's game stage, on the game thread.
pub(super) unsafe fn pull_pin(hand: Hand) -> Option<PinPull> {
    let item = in_hand(hand)?;
    let actor = actor(item)?;
    // SAFETY: forwarded from the caller.
    Some(unsafe { native::pull_pin(&actor) })
}

/// Whether `hand` holds an item whose pin is still in.
pub(crate) fn pin_in(hand: Hand) -> bool {
    in_hand(hand)
        .and_then(actor)
        .is_some_and(|actor| !native::pin_out(&actor))
}

/// The running fuse of the item in `hand`, once its pin is out: the time left and the time it
/// started with, seconds.
pub(crate) fn fuse_in_hand(hand: Hand) -> Option<(f32, f32)> {
    native::fuse(&actor(in_hand(hand)?)?)
}

/// Release a safe grenade with the motion a gun's let-go gives a gun ([`throw_release`]). The pin
/// decides what the release is ([`native`]): with it in, the item lands inert and stays the
/// loadout's; with it out, it is a live grenade. The release goes through now when the actor has
/// its physics body, and waits for the body otherwise ([`native::release_now`]).
///
/// # Safety
///
/// The input tick's game stage, on the game thread.
pub(crate) unsafe fn opened(hand: Hand) {
    let Some(item) = in_hand(hand) else {
        return;
    };
    let Some(world) = position(item.binding.lease) else {
        return;
    };
    let (tracked, since_s) = {
        let state = STATE.lock();
        let since_s = state.held_since[hand.index()]
            .filter(|(held, _)| *held == item.item)
            .map_or(f64::INFINITY, |(_, since_s)| since_s);
        (state.grips[hand.index()].is_some(), since_s)
    };
    let release = if tracked {
        throw_release(
            hand,
            world.w_axis.truncate(),
            since_s,
            "grenade",
            Some(!pin_in(hand)),
        )
    } else {
        Release::REST
    };
    let launch = native::Launch {
        item,
        hand: Some(hand),
        world,
        release,
    };
    let weapon = unsafe { engine::local_player() }.and_then(grenade_weapon);
    match weapon {
        // SAFETY: forwarded from the caller; the weapon is the local player's.
        Some(weapon) => unsafe { native::release_now(weapon, launch) },
        None => {
            native::submit(launch);
        }
    }
}

/// Take up the nearest inert item within reach, giving it a fresh actor in the hand.
///
/// A released actor is a simulating body, which the engine no longer follows from its transform, so
/// the item is admitted to the hand with a new inert actor rather than a teleported one.
pub(crate) fn take_loose(
    hand: Hand,
    cockpit_grip: Vec3,
    frames: &Frames,
    tuning: &GrenadeTuning,
) -> bool {
    let Some(item) = nearest_loose(cockpit_grip, frames, tuning.catch.radius_m) else {
        return false;
    };
    take_item(hand, item, tuning)
}

/// Take up the inert item `item`, lying in the world, into `hand` with a fresh actor, wherever it
/// is: the grapple's snatch brought it to the hand.
///
/// Refused outside the top of the world's pre-simulation queue, as a draw is ([`native::create`]).
pub(crate) fn take_item(hand: Hand, item: SafeItemView, tuning: &GrenadeTuning) -> bool {
    if !matches!(item.place, Place::World(_)) {
        return false;
    }
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

/// The inert item lying in the world whose actor's physics body is `body`, owned by the physics
/// game object `owner`: what the grapple's aim found.
pub(crate) fn inert_in_world(owner: NonZeroUsize, body: u32) -> Option<SafeItemView> {
    let items = live::safe_items();
    let state = STATE.lock();
    items
        .into_iter()
        .filter(|item| matches!(item.place, Place::World(_)))
        .find(|item| {
            state
                .items
                .iter()
                .find(|entry| entry.model == item.binding.lease)
                .and_then(|entry| entry.actor.explosive())
                // SAFETY: the weak handle proved a strong reference to the object still exists.
                .and_then(|explosive| unsafe { body_of(&*explosive) })
                .is_some_and(|found| found == (body, owner))
        })
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
pub(crate) fn observe(grips: [Option<Pose>; 2], tuning: &GrenadeTuning) {
    let items = live::safe_items();
    let now_s = motion::latest_time();
    let mut state = STATE.lock();
    state.grips = grips;
    for hand in Hand::BOTH {
        let held = items
            .iter()
            .find(|item| item.place == Place::Hand(hand))
            .map(|item| item.item);
        let since = &mut state.held_since[hand.index()];
        *since = match (held, *since) {
            (Some(item), Some((was, since_s))) if was == item => Some((was, since_s)),
            (Some(item), _) => Some((item, now_s.unwrap_or(f64::INFINITY))),
            (None, _) => None,
        };
    }
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
/// The game thread, before the world update.
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
    stand_down_hands([true; 2]);
}

/// A masked hand releases its own safe item at rest without changing the other hand's release.
pub(crate) fn stand_down_hands(unavailable: [bool; 2]) {
    for hand in Hand::BOTH {
        if !unavailable[hand.index()] {
            continue;
        }
        let Some(item) = in_hand(hand) else {
            continue;
        };
        let Some(world) = position(item.binding.lease) else {
            continue;
        };
        native::submit(native::Launch {
            item,
            hand: Some(hand),
            world,
            release: Release::REST,
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

fn nearest_loose(grip: Vec3, frames: &Frames, radius_m: f32) -> Option<SafeItemView> {
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
            loose_distance(grip, centre, frames, radius_m).map(|distance| (*item, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(item, _)| item)
}

fn loose_distance(grip: Vec3, world: Vec3, frames: &Frames, radius_m: f32) -> Option<f32> {
    if !frames.is_usable()
        || !grip.is_finite()
        || !world.is_finite()
        || !radius_m.is_finite()
        || radius_m <= 0.0
    {
        return None;
    }
    // Completion uses the same cockpit geometry as the inert item's offer.
    let distance = frames.world_to_cockpit(world).distance(grip);
    (distance.is_finite() && distance <= radius_m).then_some(distance)
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

fn matrix(grip: Pose, hand: Hand, tuning: &GrenadeTuning) -> Mat4 {
    held_matrix(grip, hold_local(hand, tuning))
}

/// The transform a held item takes in a tracked hand.
fn grip_world(hand: Hand, tuning: &GrenadeTuning) -> Option<Mat4> {
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
            // SAFETY: the shutdown cleanups run on the game thread before the world update.
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
    /// The item each hand holds and when it came into the hand, display time: a throw reads the
    /// hand's motion from then on, as a gun's let-go does.
    held_since: [Option<(ItemId, f64)>; 2],
    items: Vec<Item>,
    retiring: Vec<Actor>,
}

static STATE: Mutex<State> = Mutex::new(State {
    owner: None,
    grips: [None; 2],
    held_since: [None; 2],
    items: Vec::new(),
    retiring: Vec::new(),
});
static COUNTS: Mutex<Counts> = Mutex::new(Counts {
    drawn: 0,
    stowed: 0,
    redrawn: 0,
    dropped: 0,
    recovered: 0,
    spawned: 0,
});
static NEXT_MODEL: AtomicU64 = AtomicU64::new(1);
static CLEANUP: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;
    use crate::controllers::loadout::{BodyLayout, Loadout};

    #[test]
    fn loose_completion_matches_offer_boundary_at_every_world_scale() {
        for scale in [0.5, 1.0, 2.0] {
            let frames = Frames {
                head_world: Vec3::ZERO,
                head_cockpit: Vec3::ZERO,
                view: Quat::IDENTITY,
                world_scale: scale,
                character: Mat4::IDENTITY,
            };
            let radius = 0.25;
            for distance in [radius - 0.001, radius, radius + 0.001] {
                let world = frames.cockpit_to_world(Vec3::X * distance);
                let offered = frames.world_to_cockpit(world).length() <= radius;
                assert_eq!(
                    loose_distance(Vec3::ZERO, world, &frames, radius).is_some(),
                    offered
                );
                assert_eq!(offered, distance <= radius);
            }
            for invalid in [0.0, -1.0, f32::NAN, f32::INFINITY] {
                let invalid_frames = Frames {
                    world_scale: invalid,
                    ..frames
                };
                assert!(loose_distance(Vec3::ZERO, Vec3::ZERO, &invalid_frames, radius).is_none());
                assert!(loose_distance(Vec3::ZERO, Vec3::ZERO, &frames, invalid).is_none());
            }
            assert!(loose_distance(Vec3::NAN, Vec3::ZERO, &frames, radius).is_none());
            assert!(loose_distance(Vec3::ZERO, Vec3::INFINITY, &frames, radius).is_none());
        }
    }

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
