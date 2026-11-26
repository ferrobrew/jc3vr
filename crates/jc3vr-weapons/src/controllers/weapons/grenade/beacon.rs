use std::{
    cell::Cell,
    time::{Duration, Instant},
};

use detours_macro::detour;
use glam::Mat4;
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::{
        explosive::Explosive,
        weapon::{BeaconWeapon, ThrownGrenadeWeapon, WeaponBase, WeaponData},
    },
    types::math::Vector3,
};
use jc3vr_body::controllers::{
    hands::{
        held,
        held::{Held, Slot, held_matrix},
    },
    held::{native, native::object_of},
    intent,
    physics::refs::{Weak, null_weak},
    scheme,
};
use jc3vr_input::{
    frame::{Hand, Pose},
    motion,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::weapons::{
    engine,
    grenade::{
        LAUNCH, PlannedLaunch, Release, TickInputs, catch, hold_local, native::LaunchScope,
        primed_explosive, report_native_throw, safe, throw_release, tuning::GRENADE,
    },
    inventory::engine::weapon_of,
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&FIRE_BEACON_BINDER)
}

pub(super) fn held() -> Option<(Hand, usize)> {
    STATE
        .lock()
        .pending
        .as_ref()
        .map(|pending| (pending.gesture.hand, pending.explosive))
}

pub(super) fn input_tick(inputs: &TickInputs, grips: [Option<Pose>; 2]) {
    let tuning = GRENADE.get();
    let mut state = STATE.lock();
    state.hand = inputs.dominant;
    let State {
        pending, selection, ..
    } = &mut *state;
    let gesture = pending
        .as_mut()
        .map(|pending| &mut pending.gesture)
        .or_else(|| selection.as_mut().map(|selection| &mut selection.gesture));
    let Some(gesture) = gesture else {
        return;
    };
    if gesture.release.is_some() {
        return;
    }
    let hand = gesture.hand;
    let Some(world) = grips[hand.index()]
        .map(|grip| held_matrix(grip, hold_local(hand, tuning)))
        .filter(|world| world.is_finite())
    else {
        gesture.cancel();
        return;
    };
    gesture.world = world;
    match intent::grip_holds_now(hand) {
        Some(closed) => {
            if gesture.grip.observe(closed, inputs.opened[hand.index()]) {
                gesture.release = Some(throw_release(
                    hand,
                    world.w_axis.truncate(),
                    gesture.since_s,
                    "beacon",
                    None,
                ));
            }
        }
        None => gesture.cancel(),
    }
}

pub(super) fn input_unavailable(unavailable: [bool; 2]) {
    let mut state = STATE.lock();
    if let Some(pending) = state.pending.as_mut()
        && unavailable[pending.gesture.hand.index()]
    {
        pending.gesture.cancel();
    }
    if let Some(selection) = state.selection.as_mut()
        && unavailable[selection.gesture.hand.index()]
    {
        selection.gesture.cancel();
    }
}

pub(super) fn stand_down() {
    let mut state = STATE.lock();
    if let Some(pending) = state.pending.as_mut() {
        pending.gesture.cancel();
    }
    if let Some(selection) = state.selection.as_mut() {
        selection.gesture.cancel();
    }
}

pub(super) unsafe fn observe(weapon: *mut ThrownGrenadeWeapon) {
    let Some(base) = (unsafe { weapon.cast::<WeaponBase>().as_ref() }) else {
        return;
    };
    if !base.m_IsUserLocalPlayer
        || unsafe { engine::slot_of(base) } != Some(WeaponSlot::E_SLOT_BEACON)
        || !scheme::is_controllers()
        || STATE.lock().pending.is_some()
    {
        return;
    }
    let hand = STATE.lock().hand;
    if safe::in_hand(hand).is_some() || catch::caught().is_some_and(|caught| caught.hand == hand) {
        return;
    }
    let Some(grip) = crate::controllers::weapons::grenade::STATE.lock().grips[hand.index()] else {
        return;
    };
    let Some(explosive) = (unsafe { primed_explosive(&*weapon) }) else {
        return;
    };
    let tuning = GRENADE.get();
    let world = held_matrix(grip, hold_local(hand, tuning));
    if !world.is_finite() {
        return;
    }
    let index = usize::from(base.m_NextWeaponToFire);
    let data = unsafe { native::gun_ptr(base, index) }
        .map(|_| index)
        .or_else(|| unsafe { native::gun_ptr(base, 0) }.map(|_| 0));
    let Some(data) = data else {
        return;
    };
    let mut state = STATE.lock();
    if state.pending.is_some() {
        return;
    }
    let gesture = state
        .selection
        .take()
        .filter(|selection| selection.weapon.raw().pn == base.m_WeakThis.pn)
        .map(|selection| selection.gesture)
        .unwrap_or_else(|| Gesture::new(hand, world));
    state.pending = Some(Pending {
        weapon: unsafe { Weak::of(&base.m_WeakThis) },
        actor: unsafe { Weak::of(&(*weapon).m_PrimedGrenade) },
        explosive: explosive as usize,
        gesture,
        data,
    });
}

pub(super) unsafe fn render_frame() {
    unsafe { observe_selection() };
    let Some(mut pending) = STATE.lock().pending.take() else {
        return;
    };
    let weapon_alive = pending.weapon.lock();
    let actor_alive = pending.actor.lock();
    let base = weapon_alive.as_ref().map(weapon_of);
    let explosive = pending.explosive as *mut Explosive;
    let same_actor = base.zip(actor_alive.as_ref()).is_some_and(|(base, actor)| {
        let weapon = base.cast::<ThrownGrenadeWeapon>();
        (unsafe { primed_explosive(&*weapon) }) == Some(explosive)
            && unsafe { (*weapon).m_PrimedGrenade.pn } == actor.control_block()
    });
    if actor_alive.is_none() || (base.is_some() && !same_actor) {
        held::hold(Slot::Grenade, None);
        return;
    }
    // The render update has no game tick: the game thread, outside the world update.
    let owner_valid = base.is_some_and(|base| unsafe {
        Character::GetLocalPlayerCharacter()
            .as_ref()
            .is_some_and(|owner| {
                owner.m_Inventory.m_Weapons[WeaponSlot::E_SLOT_BEACON as usize].px == base
                    && (*base).m_IsUserLocalPlayer
            })
    });
    let selected = unsafe { Character::GetLocalPlayerCharacter().as_ref() }.is_some_and(|owner| {
        (unsafe { (&raw const owner.m_SelectedWeaponSlot).cast::<i32>().read() })
            == WeaponSlot::E_SLOT_BEACON as i32
    });
    if !selected
        || !same_actor
        || !owner_valid
        || !jc3vr_core::hooks::in_gameplay()
        || !scheme::is_controllers()
        || intent::grip_holds_now(pending.gesture.hand).is_none()
    {
        pending.gesture.cancel();
    }
    let physics_ready = unsafe { !(*explosive).m_PfxInstance.is_null() };
    if should_drop(
        pending.gesture.grip,
        physics_ready,
        pending.gesture.started.elapsed(),
    ) {
        pending.gesture.cancel();
    }
    if let Some(release) = pending.gesture.release
        && (physics_ready || pending.gesture.started.elapsed() >= BODY_WAIT)
    {
        held::hold(Slot::Grenade, None);
        let world = pending.gesture.world;
        if same_actor
            && owner_valid
            && let Some(base) = base
        {
            let data = unsafe { native::gun_ptr(&*base, pending.data) }
                .or_else(|| unsafe { native::gun_ptr(&*base, 0) });
            if let Some(data) = data {
                let scope = LaunchScope::enter(PlannedLaunch {
                    explosive: pending.explosive,
                    transform: Some(world.into()),
                    release,
                    catchable: false,
                });
                unsafe { (*base.cast::<BeaconWeapon>()).Fire_Projectile(data.as_ptr()) };
                drop(scope);
            } else {
                tracing::warn!(target: "controllers", "beacon: weapon data disappeared; releasing the actor without a weapon event");
            }
        }
        if same_actor && let Some(base) = base {
            let weapon = unsafe { &mut *base.cast::<ThrownGrenadeWeapon>() };
            if unsafe { primed_explosive(weapon) } == Some(explosive) {
                unsafe { weapon.m_PrimedGrenade.Assign(&null_weak()) };
            }
        }
        unsafe { queue_release(&mut *explosive, world, release) };
        report_native_throw(pending.gesture.hand, release);
        return;
    }
    let world = pending.gesture.world.into();
    unsafe {
        (*explosive).m_WorldMatrixT0 = (*explosive).m_WorldMatrixT1;
        (*explosive).m_WorldMatrixT1 = world;
        (*explosive).SetPhysicsTransform(&world);
    }
    let tuning = GRENADE.get();
    held::hold(
        Slot::Grenade,
        Some(Held {
            explosive: pending.explosive,
            hand: pending.gesture.hand,
            local: hold_local(pending.gesture.hand, tuning),
        }),
    );
    STATE.lock().pending = Some(pending);
}

pub(super) fn propelling(explosive: usize) -> bool {
    PROPELLING.get() == Some(explosive)
}

unsafe fn observe_selection() {
    if !scheme::is_controllers() || !jc3vr_core::hooks::in_gameplay() {
        STATE.lock().selection = None;
        return;
    }
    if STATE.lock().pending.is_some() {
        return;
    }
    // The render update has no game tick: the game thread, outside the world update.
    let Some(owner) = (unsafe { Character::GetLocalPlayerCharacter().as_ref() }) else {
        STATE.lock().selection = None;
        return;
    };
    let selected = unsafe { (&raw const owner.m_SelectedWeaponSlot).cast::<i32>().read() };
    if selected != WeaponSlot::E_SLOT_BEACON as i32 {
        STATE.lock().selection = None;
        return;
    }
    let base = owner.m_Inventory.m_Weapons[WeaponSlot::E_SLOT_BEACON as usize].px;
    let Some(weapon) = (unsafe { base.as_ref() }) else {
        return;
    };
    let hand = STATE.lock().hand;
    let fresh = STATE
        .lock()
        .selection
        .as_ref()
        .is_none_or(|selection| selection.weapon.raw().pn != weapon.m_WeakThis.pn);
    let grip = crate::controllers::weapons::grenade::STATE.lock().grips[hand.index()];
    if fresh && let Some(grip) = grip {
        let tuning = GRENADE.get();
        let world = held_matrix(grip, hold_local(hand, tuning));
        if world.is_finite() {
            STATE.lock().selection = Some(Selection {
                weapon: unsafe { Weak::of(&weapon.m_WeakThis) },
                gesture: Gesture::new(hand, world),
            });
        }
    }
    unsafe { observe(base.cast()) };
}

unsafe fn queue_release(explosive: &mut Explosive, world: Mat4, release: Release) {
    explosive.m_WorldMatrixT0 = world.into();
    explosive.m_WorldMatrixT1 = world.into();
    explosive.m_DeferredPhysicsTransform = world.into();
    explosive.m_DeferredSetPhysicsTransform = true;
    explosive.m_InitialAngularVelocity = Vector3 {
        data: release.angular.to_array(),
    };
    explosive.m_RelativeVelocity = Vector3 {
        data: release.velocity.to_array(),
    };
    explosive.m_DeferredInitialVelocity = Vector3 {
        data: release.velocity.to_array(),
    };
    explosive.m_TimeSinceThrow = 0.0;
    explosive.m_DeferredPropelTowards = true;
}

struct Pending {
    weapon: Weak,
    actor: Weak,
    explosive: usize,
    gesture: Gesture,
    data: usize,
}

struct Selection {
    weapon: Weak,
    gesture: Gesture,
}

struct Gesture {
    hand: Hand,
    since_s: f64,
    started: Instant,
    world: Mat4,
    release: Option<Release>,
    grip: Grip,
}

impl Gesture {
    fn new(hand: Hand, world: Mat4) -> Self {
        Self {
            hand,
            since_s: motion::latest_time().unwrap_or(f64::NEG_INFINITY),
            started: Instant::now(),
            world,
            release: None,
            grip: if intent::grip_holds_now(hand) == Some(true) {
                Grip::Held
            } else {
                Grip::Waiting
            },
        }
    }

    fn cancel(&mut self) {
        self.release.get_or_insert(Release::REST);
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Grip {
    Waiting,
    Held,
}

impl Grip {
    fn observe(&mut self, closed: bool, opened: bool) -> bool {
        match self {
            Self::Waiting => {
                if closed {
                    *self = Self::Held;
                }
                false
            }
            Self::Held => opened || !closed,
        }
    }
}

fn should_drop(grip: Grip, physics_ready: bool, elapsed: Duration) -> bool {
    (grip == Grip::Waiting && elapsed >= GRIP_WAIT) || (!physics_ready && elapsed >= BODY_WAIT)
}

const GRIP_WAIT: Duration = Duration::from_secs(5);
const BODY_WAIT: Duration = Duration::from_secs(2);

struct State {
    pending: Option<Pending>,
    selection: Option<Selection>,
    hand: Hand,
}

static STATE: Mutex<State> = Mutex::new(State {
    pending: None,
    selection: None,
    hand: Hand::Right,
});

thread_local! {
    static PROPELLING: Cell<Option<usize>> = const { Cell::new(None) };
}

struct Propelling(Option<usize>);

impl Drop for Propelling {
    fn drop(&mut self) {
        PROPELLING.set(self.0);
    }
}

fn original(weapon: *mut BeaconWeapon, data: *mut WeaponData) {
    let actor = unsafe { primed_explosive(&*weapon.cast::<ThrownGrenadeWeapon>()) };
    let scope = Propelling(PROPELLING.replace(actor.map(|actor| actor as usize)));
    FIRE_BEACON.get().unwrap().call(weapon, data);
    drop(scope);
}

/// # Safety
///
/// The caller runs the grenade weapon's native placement pass and keeps `weapon` live for this
/// call.
pub(super) unsafe fn after_placement(weapon: *mut ThrownGrenadeWeapon) {
    let placement = {
        let state = STATE.lock();
        state
            .pending
            .as_ref()
            .filter(|pending| {
                pending.weapon.raw().px == object_of(weapon.cast()).cast()
                    && pending.actor.raw().pn == unsafe { (*weapon).m_PrimedGrenade.pn }
            })
            .map(|pending| {
                (
                    pending.actor.clone(),
                    pending.explosive,
                    pending.gesture.world,
                )
            })
    };
    if let Some((actor, explosive, world)) = placement
        && let Some(_alive) = actor.lock()
    {
        let explosive = unsafe { &mut *(explosive as *mut Explosive) };
        let world = world.into();
        explosive.m_WorldMatrixT1 = world;
        unsafe { explosive.SetPhysicsTransform(&world) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_initially_open_hand_can_grip_and_release_the_beacon() {
        let mut grip = Grip::Waiting;
        assert!(!grip.observe(false, true));
        assert!(!grip.observe(true, false));
        assert_eq!(grip, Grip::Held);
        assert!(grip.observe(false, false));
    }

    #[test]
    fn a_release_does_not_need_an_animation_ready_signal() {
        let mut grip = Grip::Held;
        assert!(grip.observe(false, true));
        assert!(!should_drop(grip, true, Duration::from_secs(120)));
    }

    #[test]
    fn readiness_and_unclaimed_selection_have_duration_deadlines() {
        assert!(!should_drop(
            Grip::Waiting,
            true,
            GRIP_WAIT - Duration::from_nanos(1)
        ));
        assert!(should_drop(Grip::Waiting, true, GRIP_WAIT));
        assert!(!should_drop(
            Grip::Held,
            false,
            BODY_WAIT - Duration::from_nanos(1)
        ));
        assert!(should_drop(Grip::Held, false, BODY_WAIT));
    }

    #[test]
    fn a_fallback_queues_a_complete_body_release_without_native_propulsion() {
        let mut explosive: Explosive = unsafe { std::mem::zeroed() };
        let world = Mat4::from_translation(glam::Vec3::new(1.0, 2.0, 3.0));
        let release = Release {
            velocity: glam::Vec3::X,
            angular: glam::Vec3::Y,
        };
        unsafe { queue_release(&mut explosive, world, release) };
        assert!(explosive.m_DeferredSetPhysicsTransform);
        assert!(explosive.m_DeferredPropelTowards);
        assert_eq!(Mat4::from(explosive.m_DeferredPhysicsTransform), world);
        assert_eq!(
            explosive.m_DeferredInitialVelocity.data,
            release.velocity.to_array()
        );
        assert_eq!(
            explosive.m_InitialAngularVelocity.data,
            release.angular.to_array()
        );
        assert_eq!(Mat4::from(explosive.m_WorldMatrixT0), world);
        assert_eq!(Mat4::from(explosive.m_WorldMatrixT1), world);
    }

    #[test]
    fn cancellation_preserves_a_recorded_release_and_only_defaults_to_rest() {
        let mut gesture = Gesture {
            hand: Hand::Right,
            since_s: 0.0,
            started: Instant::now(),
            world: Mat4::IDENTITY,
            release: Some(Release {
                velocity: glam::Vec3::X,
                angular: glam::Vec3::Y,
            }),
            grip: Grip::Held,
        };
        let recorded = gesture.release;
        assert!(should_drop(gesture.grip, false, BODY_WAIT));
        gesture.cancel();
        gesture.cancel();
        assert_eq!(gesture.release, recorded);

        gesture.release = None;
        gesture.cancel();
        gesture.cancel();
        assert_eq!(gesture.release, Some(Release::REST));
    }
}

#[detour(address = jc3gi::equipment::weapon::BeaconWeapon::Fire_Projectile_ADDRESS)]
fn fire_beacon(weapon: *mut BeaconWeapon, data: *mut WeaponData) {
    let releasing = LAUNCH.get().is_some_and(|launch| {
        !launch.catchable
            && unsafe { primed_explosive(&*weapon.cast::<ThrownGrenadeWeapon>()) }
                == Some(launch.explosive as *mut Explosive)
    });
    if releasing {
        original(weapon, data);
        return;
    }
    unsafe { observe(weapon.cast::<ThrownGrenadeWeapon>()) };
    let defer = {
        let mut state = STATE.lock();
        if let Some(pending) = state.pending.as_mut() {
            let base = weapon.cast::<WeaponBase>();
            pending.weapon.raw().px == object_of(base).cast()
                && pending.weapon.raw().pn == unsafe { (*base).m_WeakThis.pn }
                && pending.actor.raw().pn
                    == unsafe { (*weapon.cast::<ThrownGrenadeWeapon>()).m_PrimedGrenade.pn }
        } else {
            false
        }
    };
    if !defer {
        let base = unsafe { &*weapon.cast::<WeaponBase>() };
        let empty_local = base.m_IsUserLocalPlayer
            && scheme::is_controllers()
            && unsafe { primed_explosive(&*weapon.cast::<ThrownGrenadeWeapon>()) }.is_none();
        if !empty_local {
            original(weapon, data);
        }
    }
}
