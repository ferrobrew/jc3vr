//! The native actor behind a safe grenade item.
//!
//! An item's grenade is a real pooled [`Grenade`], allocated by the grenade weapon's own factory
//! ([`ThrownGrenadeWeapon::PrimeGrenade`]). The factory's only lasting effect on the weapon is
//! [`ThrownGrenadeWeapon::m_PrimedGrenade`], so the actor is detached from that slot as soon as it
//! exists: the ammunition manager's update registration, the owner and weapon provenance the
//! constructor was given, and the pool entry itself all outlive the slot. Nothing cancels the
//! grenade and nothing refunds ammunition, so any number of items exist at once without competing
//! for the weapon's single primed slot.
//!
//! A grenade's fuse runs from the moment it spawns
//! ([`Grenade::m_ShouldTickDetonationTimer`]). The beacon weapon clears that flag on the beacons it
//! primes, whose timers then never run; a drawn item's is cleared for the same reason, so a held,
//! stowed, or dropped item stays inert until the player pulls its pin. Pulling the pin
//! ([`pull_pin`]) raises the flag once, on an item in a hand whose physics body exists, and nothing
//! here ever resets the flag or the timer it gates. A release never raises it: an item let go of
//! with its pin in stays inert, however it leaves the hand.
//!
//! A release gives the grenade's body the motion a gun's let-go gives a gun's
//! ([`crate::controllers::weapons::grenade::throw_release`]), linear and angular. The difference is
//! the engine's: a held grenade's body is not in the simulation, and only the engine's launch
//! ([`Explosive::m_DeferredPropelTowards`]) puts it there, at the end of the tick, from
//! [`Explosive::m_DeferredInitialVelocity`] and [`Explosive::m_InitialAngularVelocity`], so the
//! release writes those rather than the body's velocities.
//!
//! Every release has to be a launch the engine actually applied. Both engine paths that put a
//! grenade into the simulation start with a physics body
//! ([`Explosive::m_PfxInstance`](jc3gi::equipment::explosive::Explosive::m_PfxInstance)): the
//! throw path's `SetPhysicsTransform` and `PropelToward` do nothing without one, and the deferred
//! launch that the post-simulation update applies dereferences it unchecked. A release therefore
//! waits until the body exists, proves that it landed ([`Explosive::m_DeferredPropelTowards`]), and
//! only then retires anything.
//!
//! A release goes one of two ways, by the pin ([`settle`]). An item with its pin in is put into the
//! simulation inertly and stays the loadout's, a world item a hand can take up again. An item whose
//! pin is out is a live grenade: it leaves through the weapon's own launch, which owns the effects,
//! or inertly when the launch cannot go ahead, and either way it retires from the loadout, because
//! taking it up again would give it a fresh, inert actor and lose the running fuse. A live grenade
//! in the world is caught as any other is.
//!
//! The draw and the launch are both ammunition-free by construction. The factory deducts a round
//! through the weapon's `ConsumeAmmo` virtual; the grenade weapon's own launch never reaches that
//! call, because its `Fire_Projectile` override does not run the base implementation that makes it.
//!
//! # Lifetime and phase
//!
//! [`Actor`] holds only a weak reference. A weak count check does not lock the object against
//! forced pool erasure. Native accesses run on the game thread: in the input tick's game stage, at
//! the top of the world's pre-simulation queue, where releases go through ([`release_now`]), and
//! at the render boundary after the ammunition manager's post-simulation pass, where a release
//! still waiting for its body is serviced ([`update`]). The parallel weapon hook never services
//! this queue.
//!
//! An allocation has to come from the game stage at the top of that queue. The factory registers
//! the grenade for the world's updates, and the world makes registrations live at its update-queue
//! commits: the one that opens its pre-simulation update and the one that closes its render
//! update. Registered after the first, the grenade is first updated in the next tick, after the
//! ammunition manager's pass at the end of this one has made its physics body. Registered before
//! it, the grenade's post-simulation update runs in the same tick without a body and faults
//! ([`Grenade::UpdatePostSim`]). [`create`] therefore refuses unless the game stage allows spawning
//! ([`stage::spawn`]), which it does only at the top of the queue, after the commit.
//!
//! Factory allocations inside [`AmmoNeutral`] disable reclamation under the allocator's own
//! critical section. No reference outlives a removal, and only owned inert actors are removed.

use std::{cell::Cell, collections::VecDeque, ptr::NonNull};

use detours_macro::detour;
use glam::{Mat4, Vec3};
use jc3gi::{
    character::character::Character,
    equipment::{
        explosive::{Explosive, Grenade},
        weapon::{ThrownGrenadeWeapon, WeaponBase, WeaponData},
    },
    types::{math::Vector3, shared_ptr::WeakPtrGameObject},
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    aim::targets::weak_target,
    bindings::stage::{self, Spawn},
    frame::Hand,
    loadout::{live, safe_items::SafeItemView},
    weapons::{
        engine::gun_ptr,
        grenade::{LAUNCH, PlannedLaunch, Release, primed_explosive, report_native_throw, safe},
        hold::refs::{Weak, null_weak},
    },
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&CONSUME_AMMO_BINDER)
        .with_static_binder(&ALLOCATE_GRENADE_BINDER)
}

/// One asked-for release of an item's actor.
#[derive(Copy, Clone)]
pub(super) struct Launch {
    pub item: SafeItemView,
    /// The hand that let go of the item, or `None` for an item the world made.
    pub hand: Option<Hand>,
    pub world: Mat4,
    pub release: Release,
}

/// A weak handle to an item's actor, which never keeps the pooled object alive.
#[derive(Clone)]
pub(super) struct Actor {
    weak: Weak,
    upgrades: bool,
}

impl Actor {
    /// Take a weak handle to the object `weak` refers to, while it lives.
    ///
    /// # Safety
    ///
    /// `weak` must be a live weak reference.
    unsafe fn of(weak: &WeakPtrGameObject) -> Option<Self> {
        let mut actor = Self {
            weak: unsafe { Weak::of(weak) },
            upgrades: false,
        };
        let grenade = actor.grenade()?;
        actor.upgrades = unsafe { (*grenade).m_CanUseUpgrades };
        Some(actor)
    }

    /// The address of the actor's explosive, while the pool still holds it.
    pub(super) fn explosive(&self) -> Option<*mut Explosive> {
        // SAFETY: the weak reference keeps the object's control block alive, and `weak_target`
        // reports the object only while a strong reference to it remains.
        let object = unsafe { weak_target(self.weak.raw()) }?;
        Some(
            object
                .cast::<u8>()
                .wrapping_sub(Explosive::GAME_OBJECT_BASE_OFFSET as usize)
                .cast::<Explosive>(),
        )
    }

    /// Whether the actor has the physics body every release path requires.
    pub(super) fn physics_ready(&self) -> bool {
        let Some(explosive) = self.explosive() else {
            return false;
        };
        // SAFETY: the weak handle proved a strong reference to the object still exists.
        let explosive = unsafe { &*explosive };
        !explosive.m_PfxInstance.is_null()
    }

    /// The actor's world position, as its last simulated transform gives it.
    pub(super) fn position(&self) -> Option<Vec3> {
        let explosive = self.explosive()?;
        // SAFETY: the weak handle proved a strong reference to the object still exists.
        let explosive = unsafe { &*explosive };
        let world = Mat4::from(explosive.m_WorldMatrixT1);
        world.is_finite().then(|| world.w_axis.truncate())
    }

    /// The engine weak reference the weapon's slot is written from.
    fn handle(&self) -> &WeakPtrGameObject {
        self.weak.raw()
    }

    fn grenade(&self) -> Option<*mut Grenade> {
        self.explosive()
            .map(|explosive| explosive.cast::<Grenade>())
    }
}

/// What pulling an item's pin did.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum PinPull {
    /// The fuse runs from now.
    Pulled,
    /// The pin was already out.
    AlreadyOut,
    /// The actor has no physics body yet, or is gone: the pin stays in until it has one, so a fuse
    /// never runs on a grenade that no engine path could put into the simulation.
    NotReady,
}

/// Pull the pin of the item behind `actor`: its fuse starts counting down.
///
/// # Safety
///
/// The caller holds the game thread.
pub(super) unsafe fn pull_pin(actor: &Actor) -> PinPull {
    let Some(grenade) = actor.grenade() else {
        return PinPull::NotReady;
    };
    // SAFETY: the weak handle proved a strong reference to the object still exists.
    let ticking = unsafe { (*grenade).m_ShouldTickDetonationTimer };
    let pull = pin_pull(actor.physics_ready(), ticking);
    if pull == PinPull::Pulled {
        // SAFETY: as above; this is the only place an item's fuse is raised.
        unsafe { (*grenade).m_ShouldTickDetonationTimer = true };
    }
    pull
}

/// Whether the item behind `actor` has had its pin pulled.
pub(super) fn pin_out(actor: &Actor) -> bool {
    // SAFETY: the weak handle proved a strong reference to the object still exists.
    actor
        .grenade()
        .is_some_and(|grenade| unsafe { (*grenade).m_ShouldTickDetonationTimer })
}

/// The fuse of an item whose pin is out: the time left and the time it started with, seconds.
pub(super) fn fuse(actor: &Actor) -> Option<(f32, f32)> {
    if !pin_out(actor) {
        return None;
    }
    // SAFETY: the weak handle proved a strong reference to the object still exists.
    let explosive = unsafe { &*actor.explosive()? };
    Some((
        explosive.m_DetonationTimer,
        explosive.m_StartDetonationTimer,
    ))
}

/// Allocate an item's actor from the weapon's own factory and leave it inert.
///
/// Returns `None` when the game stage does not allow spawning here ([`stage::spawn`]), when the
/// pool refuses the allocation, or when the grenade's fuse did not arrive in a state this module
/// can hold inert: no item exists then, because a refused draw has nothing to show and nothing to
/// fall back to.
///
/// # Safety
///
/// `weapon` is the live local grenade weapon, and the caller runs on the game thread.
pub(super) unsafe fn create(weapon: &mut ThrownGrenadeWeapon, world: Mat4) -> Option<Actor> {
    if !world.is_finite() {
        return None;
    }
    if stage::spawn() != Spawn::Allowed {
        tracing::debug!(
            target: "controllers",
            "grenade: refused a draw outside the top of the world's pre-simulation queue",
        );
        return None;
    }
    let base = (weapon as *mut ThrownGrenadeWeapon).cast::<WeaponBase>();
    if unsafe { primed_explosive(weapon) }.is_some() {
        return None;
    }
    let data = unsafe { weapon_data(&*base) }?;
    let neutral = AmmoNeutral::enter(base as usize);
    let created = unsafe { weapon.PrimeGrenade(data) };
    drop(neutral);
    if created.is_null() {
        return None;
    }
    let Some(actor) = (unsafe { Actor::of(&weapon.m_PrimedGrenade) }) else {
        // SAFETY: the factory returned the grenade it allocated, which nothing else references yet.
        unsafe { (*created.cast::<Explosive>()).RemoveSelf() };
        return None;
    };
    // The slot is the weapon's claim on the grenade; clearing it before anything else runs keeps the
    // weapon from positioning, launching, or cancelling an item it no longer owns.
    let empty = null_weak();
    unsafe { weapon.m_PrimedGrenade.Assign(&empty) };
    let Some(grenade) = actor.grenade() else {
        unsafe { remove(&actor) };
        return None;
    };
    // SAFETY: the actor is live in the serialized allocation phase.
    let grenade = unsafe { &mut *grenade };
    grenade.m_ShouldTickDetonationTimer = false;
    set_upgrade_policy(grenade, actor.upgrades, false);
    let explosive = unsafe { &*actor.explosive()? };
    // A fuse that is already spent would still detonate despite the cleared flag, because the
    // post-simulation update dispatches on the timer alone. Refusing the draw is the only safe
    // answer to tuning this module cannot hold inert.
    if !inert_fuse(
        explosive.m_StartDetonationTimer,
        explosive.m_DetonationTimer,
    ) {
        unsafe { remove(&actor) };
        return None;
    }
    unsafe { place(&actor, world, world) };
    Some(actor)
}

/// Write an item's actor to a transform: `previous` is the tick before and `world` the current one,
/// which the render update interpolates between.
///
/// A body that is not simulating follows the transform, which is the engine's own mechanism for a
/// grenade held by a weapon, and is what carries a held or stowed item.
///
/// # Safety
///
/// The caller holds the game thread.
pub(super) unsafe fn place(actor: &Actor, previous: Mat4, world: Mat4) {
    if !world.is_finite() {
        return;
    }
    let Some(explosive) = actor.explosive() else {
        return;
    };
    // SAFETY: the weak handle proved a strong reference to the object still exists.
    let explosive = unsafe { &mut *explosive };
    explosive.m_WorldMatrixT1 = world.into();
    if previous.is_finite() {
        explosive.m_WorldMatrixT0 = previous.into();
    }
}

/// Remove an item's actor from the ammunition pool, without the refund a cancel would make.
///
/// # Safety
///
/// The caller holds the game thread, owns the item, and the actor is not armed.
pub(super) unsafe fn remove(actor: &Actor) {
    let Some(explosive) = actor.explosive() else {
        return;
    };
    // SAFETY: the weak handle proved a strong reference to the object still exists, and no reference
    // from this module outlives the call.
    let explosive = unsafe { &mut *explosive };
    // SAFETY: this is the pool removal `CancelPrimedGrenade` runs, minus its ammunition refund.
    unsafe { explosive.RemoveSelf() };
}

/// Ask for an item's release. A request for an item already waiting is refused.
pub(super) fn submit(launch: Launch) -> bool {
    let mut state = STATE.lock();
    if state
        .queue
        .iter()
        .any(|queued| queued.launch.item.item == launch.item.item)
    {
        return false;
    }
    state.queue.push_back(Waiting {
        launch,
        attempts: 0,
    });
    true
}

/// Release an item now when its actor has its physics body, and ask for the release otherwise.
///
/// A release in the input tick's game stage reaches the engine before the ammunition manager's
/// pass at the end of the same tick, which applies the launch, so the item leaves from where the
/// hand let go of it one tick after the hand did. Only a release in the tick of the draw, before
/// the body exists, waits ([`update`]).
///
/// # Safety
///
/// `weapon` is the live local grenade weapon, and the caller runs on the game thread outside the
/// world update.
pub(super) unsafe fn release_now(weapon: &mut ThrownGrenadeWeapon, launch: Launch) {
    let ready = safe::actor(launch.item).filter(Actor::physics_ready);
    match ready {
        Some(actor) if live::safe_item(launch.item.item) == Some(launch.item) => {
            let base = (weapon as *mut ThrownGrenadeWeapon).cast::<WeaponBase>();
            // SAFETY: forwarded from the caller; the actor is live and has its body.
            unsafe { release(weapon, base, &launch, &actor) };
        }
        _ => {
            submit(launch);
        }
    }
}

/// Drop every waiting release, so nothing this module queued outlives the items it names.
pub(super) fn clear() {
    STATE.lock().queue.clear();
}

pub(super) fn reconcile(items: &[SafeItemView]) {
    STATE
        .lock()
        .queue
        .retain(|waiting| items.contains(&waiting.launch.item));
}

pub(super) fn waiting(item: SafeItemView) -> bool {
    STATE
        .lock()
        .queue
        .iter()
        .any(|waiting| waiting.launch.item == item)
}

/// Service one waiting release after the simulation has completed.
///
/// # Safety
///
/// `weapon` is the live local grenade weapon at the serialized render boundary.
pub(super) unsafe fn update(weapon: &mut ThrownGrenadeWeapon) {
    let base = (weapon as *mut ThrownGrenadeWeapon).cast::<WeaponBase>();
    let Some(mut waiting) = STATE.lock().queue.pop_front() else {
        return;
    };
    // The item must still be the one the caller asked about: it may have been transferred, stowed,
    // or retired while the request waited.
    if live::safe_item(waiting.launch.item.item) != Some(waiting.launch.item) {
        return;
    }
    let Some(actor) = safe::actor(waiting.launch.item) else {
        return;
    };
    match decide(actor.physics_ready(), waiting.attempts) {
        Decision::Wait => {
            waiting.attempts += 1;
            STATE.lock().queue.push_back(waiting);
        }
        Decision::Abandon => {
            tracing::debug!(
                target: "controllers",
                attempts = waiting.attempts,
                "grenade: item never gained a physics body; leaving it inert where it is",
            );
        }
        Decision::Release => {
            unsafe { release(weapon, base, &waiting.launch, &actor) };
        }
    }
}

/// Release the item at the place, hand, transform, and velocity it was asked for.
///
/// # Safety
///
/// `weapon` and `base` are the live local grenade weapon, the actor is live and physics-ready, and
/// the caller holds the game thread.
unsafe fn release(
    weapon: &mut ThrownGrenadeWeapon,
    base: *mut WeaponBase,
    launch: &Launch,
    actor: &Actor,
) {
    let live = pin_out(actor);
    // A live grenade that the launch cannot take falls back to an inert release: the item was let
    // go, so it leaves the hand with its fuse still running.
    let launched = live && unsafe { launch_live(weapon, base, launch, actor) };
    let inert = !launched && unsafe { release_inert(launch, actor) };
    match settle(live, launched, inert) {
        Settle::Thrown => {
            if let Some(hand) = launch.hand {
                report_native_throw(hand, launch.release);
            }
            safe::launched(launch.item);
        }
        Settle::Retired => safe::launched(launch.item),
        Settle::Dropped => safe::dropped(launch.item),
        Settle::Kept => {}
    }
}

/// What a release leaves of an item.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Settle {
    /// A live grenade left through the weapon's launch: counted as a throw and retired.
    Thrown,
    /// A live grenade left inertly: retired, its fuse still running.
    Retired,
    /// An item with its pin in is in the world, the loadout's still.
    Dropped,
    /// Nothing reached the simulation; the item stays where it is.
    Kept,
}

/// Settle a release of an item whose pin is out (`live`) or in, from whether the weapon's launch
/// took it and whether the inert release did.
fn settle(live: bool, launched: bool, inert: bool) -> Settle {
    match (live, launched, inert) {
        (true, true, _) => Settle::Thrown,
        (true, false, true) => Settle::Retired,
        (false, _, true) => Settle::Dropped,
        (_, _, false) => Settle::Kept,
    }
}

/// Whether pulling the pin goes ahead, for an actor with or without its physics body and whose fuse
/// does or does not already run.
fn pin_pull(physics_ready: bool, ticking: bool) -> PinPull {
    match (physics_ready, ticking) {
        (_, true) => PinPull::AlreadyOut,
        (false, false) => PinPull::NotReady,
        (true, false) => PinPull::Pulled,
    }
}

/// Release a live grenade through the weapon's own launch. Its fuse already runs; the launch only
/// restores the grenade's own collision upgrades, which stay off while it is in the hand.
///
/// # Safety
///
/// `weapon` and `base` are the live local grenade weapon, and the actor is live and physics-ready.
unsafe fn launch_live(
    weapon: &mut ThrownGrenadeWeapon,
    base: *mut WeaponBase,
    launch: &Launch,
    actor: &Actor,
) -> bool {
    let permits = unsafe { (*base).m_User[0].cast::<Character>().as_ref() }
        .is_some_and(|character| character.m_Inventory.m_DoesAnimationAllowGrenades);
    if !permits || unsafe { primed_explosive(weapon) }.is_some() {
        return false;
    }
    let Some(data) = (unsafe { weapon_data(&*base) }) else {
        return false;
    };
    let (Some(explosive), Some(grenade)) = (actor.explosive(), actor.grenade()) else {
        return false;
    };
    unsafe { set_upgrade_policy(&mut *grenade, actor.upgrades, true) };
    unsafe { weapon.m_PrimedGrenade.Assign(actor.handle()) };
    LAUNCH.set(Some(PlannedLaunch {
        explosive: explosive as usize,
        transform: Some(launch.world.into()),
        release: launch.release,
    }));
    // The launch reads the slot it was just given, applies the planned transform and velocity, and
    // clears the slot as it releases the grenade.
    unsafe { (*base).Fire_Projectile(data) };
    let hooked = LAUNCH.get().is_none();
    LAUNCH.set(None);
    // The launch clears its slot whatever happened, so leave nothing behind if it did not go through.
    if unsafe { primed_explosive(weapon) } == Some(explosive) {
        let empty = null_weak();
        unsafe { weapon.m_PrimedGrenade.Assign(&empty) };
    }
    // Neither launch signal is proof by itself: the hook runs around the grenade's launch entry
    // point whether or not the engine applied it, and the engine's own paths skip a grenade without
    // a body. Only the queued deferred launch shows that the body took the release.
    let applied = hooked
        && unsafe { explosive.as_ref() }.is_some_and(|explosive| explosive.m_DeferredPropelTowards);
    if !applied {
        unsafe { set_upgrade_policy(&mut *grenade, actor.upgrades, false) };
        return false;
    }
    true
}

/// Put an item into the simulation without the weapon's launch: the launch writes exactly these
/// fields, so the body takes the release velocity on its next update, and the fuse is left as it
/// is.
///
/// Returns whether the item was put into the simulation, which needs a body to receive it.
///
/// # Safety
///
/// The actor is live and the caller holds the game thread.
unsafe fn release_inert(launch: &Launch, actor: &Actor) -> bool {
    if !launch.world.is_finite()
        || !launch.release.velocity.is_finite()
        || !launch.release.angular.is_finite()
        || !actor.physics_ready()
    {
        return false;
    }
    let Some(explosive) = actor.explosive() else {
        return false;
    };
    // SAFETY: the weak handle proved a strong reference to the object still exists.
    let explosive = unsafe { &mut *explosive };
    queue_inert(explosive, launch);
    true
}

fn queue_inert(explosive: &mut Explosive, launch: &Launch) {
    let velocity = Vector3 {
        data: launch.release.velocity.to_array(),
    };
    explosive.m_InitialAngularVelocity = Vector3 {
        data: launch.release.angular.to_array(),
    };
    explosive.m_WorldMatrixT0 = launch.world.into();
    explosive.m_WorldMatrixT1 = launch.world.into();
    explosive.m_DeferredPhysicsTransform = launch.world.into();
    explosive.m_DeferredSetPhysicsTransform = true;
    explosive.m_RelativeVelocity = velocity;
    explosive.m_DeferredInitialVelocity = velocity;
    explosive.m_TimeSinceThrow = 0.0;
    explosive.m_DeferredPropelTowards = true;
}

fn set_upgrade_policy(grenade: &mut Grenade, original: bool, armed: bool) {
    // Contact upgrades can zero the timer without consulting the countdown switch.
    grenade.m_CanUseUpgrades = armed && original;
}

/// What one serialized service pass does with a waiting release.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Decision {
    /// The body is not there yet: look again on a later update.
    Wait,
    /// Stop waiting and leave the item inert where it is.
    Abandon,
    /// Release the item, through the weapon's launch when its pin is out.
    Release,
}

/// Decide what to do with a release that has waited `attempts` updates.
///
/// A grenade gets its physics body on the first post-simulation update after it spawns, so a release
/// normally goes through on its first look; the bound exists so an item whose body never appears
/// cannot hold the queue or be released by a path that would dereference nothing.
fn decide(physics_ready: bool, attempts: u32) -> Decision {
    if physics_ready {
        return Decision::Release;
    }
    if attempts >= MAX_WAIT_UPDATES {
        return Decision::Abandon;
    }
    Decision::Wait
}

/// Whether a freshly primed grenade's fuse is one this module can hold inert: both timers finite and
/// positive, so clearing the countdown flag leaves a fuse that cannot reach zero and detonate.
fn inert_fuse(start_s: f32, remaining_s: f32) -> bool {
    start_s.is_finite() && start_s > 0.0 && remaining_s.is_finite() && remaining_s > 0.0
}

#[detour(address = jc3gi::equipment::explosive::AllocateGrenade_ADDRESS)]
fn allocate_grenade(
    manager: *mut std::ffi::c_void,
    allocation_type: u32,
    position: *const Vector3,
    reclaim: bool,
) -> *mut std::ffi::c_void {
    ALLOCATE_GRENADE.get().unwrap().call(
        manager,
        allocation_type,
        position,
        allocation_reclaim(reclaim),
    )
}

fn allocation_reclaim(requested: bool) -> bool {
    requested && AMMO_NEUTRAL.get().is_none()
}

#[detour(address = jc3gi::equipment::weapon::WeaponBase::ConsumeAmmoBase_ADDRESS)]
fn consume_ammo(weapon: *mut WeaponBase, data: *mut WeaponData, count: i32) {
    if AMMO_NEUTRAL.get() == Some(weapon as usize) {
        return;
    }
    CONSUME_AMMO.get().unwrap().call(weapon, data, count);
}

unsafe fn weapon_data(weapon: &WeaponBase) -> Option<*mut WeaponData> {
    unsafe { gun_ptr(weapon, usize::from(weapon.m_NextWeaponToFire)) }.map(NonNull::as_ptr)
}

/// An ammunition exemption for one weapon on one thread: drawing an item neither reads nor changes
/// an inventory's grenade stock.
struct AmmoNeutral(Option<usize>);

impl AmmoNeutral {
    fn enter(weapon: usize) -> Self {
        Self(AMMO_NEUTRAL.replace(Some(weapon)))
    }
}

impl Drop for AmmoNeutral {
    fn drop(&mut self) {
        AMMO_NEUTRAL.set(self.0);
    }
}

#[derive(Copy, Clone)]
struct Waiting {
    launch: Launch,
    attempts: u32,
}

/// How many weapon updates a release waits for the actor's physics body before giving up.
const MAX_WAIT_UPDATES: u32 = 60;

struct State {
    queue: VecDeque<Waiting>,
}

static STATE: Mutex<State> = Mutex::new(State {
    queue: VecDeque::new(),
});

thread_local! {
    static AMMO_NEUTRAL: Cell<Option<usize>> = const { Cell::new(None) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::loadout::{BodyLayout, Loadout, safe_items::ItemLeaseId};

    static QUEUE_TEST: Mutex<()> = Mutex::new(());

    #[test]
    fn queue_reconciliation_retires_changed_leases_and_reset_requests() {
        let _guard = QUEUE_TEST.lock();
        clear();
        let item = view();
        assert!(submit(launch(item)));
        reconcile(&[item]);
        assert!(waiting(item));
        let changed = SafeItemView {
            place: crate::controllers::loadout::Place::World(
                crate::controllers::loadout::Departure::Dropped,
            ),
            ..item
        };
        reconcile(&[changed]);
        assert!(!waiting(item));
        assert!(submit(launch(item)));
        clear();
        assert!(!waiting(item));
        assert!(submit(launch(changed)));
        reconcile(&[changed]);
        assert!(waiting(changed));
        clear();
    }

    fn view() -> SafeItemView {
        let mut table = Loadout::new(BodyLayout::full(Hand::Right));
        let model = ItemLeaseId::new(std::num::NonZeroU64::new(1).unwrap());
        table.admit_safe_item(model, Hand::Right).unwrap()
    }

    fn launch(item: SafeItemView) -> Launch {
        Launch {
            item,
            hand: Some(Hand::Right),
            world: Mat4::IDENTITY,
            release: Release {
                velocity: Vec3::X,
                angular: Vec3::Y,
            },
        }
    }

    #[test]
    fn a_pin_comes_out_once_and_only_from_a_grenade_with_its_body() {
        assert_eq!(pin_pull(true, false), PinPull::Pulled);
        assert_eq!(pin_pull(false, false), PinPull::NotReady);
        assert_eq!(pin_pull(true, true), PinPull::AlreadyOut);
        assert_eq!(pin_pull(false, true), PinPull::AlreadyOut);
    }

    #[test]
    fn only_a_pulled_pin_makes_a_release_live() {
        // With its pin in, a release is a drop the loadout keeps, whatever the launch would do.
        assert_eq!(settle(false, false, true), Settle::Dropped);
        assert_eq!(settle(false, false, false), Settle::Kept);
        // With its pin out, it leaves the loadout either way, so its running fuse is never reset.
        assert_eq!(settle(true, true, false), Settle::Thrown);
        assert_eq!(settle(true, false, true), Settle::Retired);
        assert_eq!(settle(true, false, false), Settle::Kept);
    }

    #[test]
    fn a_release_waits_for_the_physics_body_and_then_goes_through() {
        assert_eq!(decide(false, 0), Decision::Wait);
        assert_eq!(decide(false, MAX_WAIT_UPDATES - 1), Decision::Wait);
        assert_eq!(decide(true, 0), Decision::Release);
        assert_eq!(decide(true, MAX_WAIT_UPDATES - 1), Decision::Release);
    }

    #[test]
    fn a_body_that_never_appears_is_given_up_on_rather_than_released() {
        assert_eq!(decide(false, MAX_WAIT_UPDATES), Decision::Abandon);
        assert_eq!(decide(false, MAX_WAIT_UPDATES + 5), Decision::Abandon);
    }

    #[test]
    fn a_fuse_is_held_inert_only_when_it_could_not_have_run_out() {
        assert!(inert_fuse(4.0, 4.0));
        assert!(inert_fuse(f32::MIN_POSITIVE, f32::MIN_POSITIVE));
        for start in [0.0, -0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(!inert_fuse(start, 4.0), "start {start} must be refused");
        }
        for remaining in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(
                !inert_fuse(4.0, remaining),
                "remaining {remaining} must be refused"
            );
        }
    }

    #[test]
    fn a_release_waits_once_per_item() {
        let _guard = QUEUE_TEST.lock();
        clear();
        let item = view();
        let request = launch(item);
        assert!(submit(request));
        assert!(!submit(request));
        clear();
    }

    #[test]
    fn clearing_drops_every_waiting_release() {
        let _guard = QUEUE_TEST.lock();
        clear();
        let item = view();
        assert!(submit(launch(item)));
        clear();
        assert!(STATE.lock().queue.is_empty());
    }

    #[test]
    fn inert_collision_upgrades_are_disabled_and_only_original_armed_policy_returns() {
        // The generated grenade stores scalar fields only.
        let mut grenade: Grenade = unsafe { std::mem::zeroed() };
        for original in [false, true] {
            set_upgrade_policy(&mut grenade, original, false);
            assert!(!grenade.m_CanUseUpgrades);
            set_upgrade_policy(&mut grenade, original, true);
            assert_eq!(grenade.m_CanUseUpgrades, original);
            set_upgrade_policy(&mut grenade, original, false);
            assert!(!grenade.m_CanUseUpgrades);
        }
    }

    #[test]
    fn inert_release_queues_captured_pose_before_launch_without_changing_the_fuse() {
        // These generated fields contain only raw pointers and scalar storage.
        let mut explosive: Explosive = unsafe { std::mem::zeroed() };
        explosive.m_DetonationTimer = 3.5;
        explosive.m_StartDetonationTimer = 4.0;
        let mut request = launch(view());
        request.world = Mat4::from_translation(Vec3::new(3.0, 2.0, 1.0));
        queue_inert(&mut explosive, &request);
        assert!(explosive.m_DeferredSetPhysicsTransform);
        assert_eq!(
            Mat4::from(explosive.m_DeferredPhysicsTransform),
            request.world
        );
        assert!(explosive.m_DeferredPropelTowards);
        assert_eq!(explosive.m_DeferredInitialVelocity.data, Vec3::X.to_array());
        assert_eq!(explosive.m_InitialAngularVelocity.data, Vec3::Y.to_array());
        assert_eq!(explosive.m_DetonationTimer, 3.5);
        assert_eq!(explosive.m_StartDetonationTimer, 4.0);
    }

    #[test]
    fn an_ammunition_exemption_is_thread_local_and_nests() {
        assert_eq!(AMMO_NEUTRAL.get(), None);
        assert!(allocation_reclaim(true));
        assert!(!allocation_reclaim(false));
        let outer = AmmoNeutral::enter(11);
        assert!(!allocation_reclaim(true));
        assert_eq!(AMMO_NEUTRAL.get(), Some(11));
        {
            let _inner = AmmoNeutral::enter(22);
            assert_eq!(AMMO_NEUTRAL.get(), Some(22));
        }
        assert_eq!(AMMO_NEUTRAL.get(), Some(11));
        assert_eq!(
            std::thread::spawn(|| AMMO_NEUTRAL.get()).join().unwrap(),
            None
        );
        drop(outer);
        assert_eq!(AMMO_NEUTRAL.get(), None);
    }
}
