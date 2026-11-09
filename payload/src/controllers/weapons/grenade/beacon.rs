use std::cell::Cell;

use detours_macro::detour;
use glam::Mat4;
use jc3gi::{
    character::inventory::WeaponSlot,
    equipment::{
        explosive::Explosive,
        weapon::{BeaconWeapon, ThrownGrenadeWeapon, WeaponBase, WeaponData},
    },
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        bindings,
        frame::{Hand, Pose},
        hands::held::{self, Held, Slot, held_matrix},
        motion, scheme,
        weapons::{
            engine,
            grenade::{
                LAUNCH, PlannedLaunch, Release, TickInputs, catch, hold_local, native::LaunchScope,
                primed_explosive, report_native_throw, safe, throw_release, tuning::GrenadeTuning,
            },
            hold::refs::Weak,
            inventory::engine::weapon_of,
        },
    },
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&FIRE_BEACON_BINDER)
}

pub(super) fn held() -> Option<(Hand, usize)> {
    STATE
        .lock()
        .pending
        .as_ref()
        .map(|pending| (pending.hand, pending.explosive))
}

pub(super) fn input_tick(inputs: &TickInputs, grips: [Option<Pose>; 2]) {
    let held = {
        let state = STATE.lock();
        state
            .pending
            .as_ref()
            .map(|pending| (pending.hand, pending.since_s))
    };
    let Some((hand, since_s)) = held else {
        STATE.lock().hand = inputs.dominant;
        return;
    };
    let Some(grip) = grips[hand.index()] else {
        stand_down();
        return;
    };
    let tuning = GrenadeTuning::of(&Config::lock_query(|config| {
        config.controllers.weapons.grenade
    }));
    let world = held_matrix(grip, hold_local(hand, &tuning));
    if !world.is_finite() {
        stand_down();
        return;
    }
    let release = inputs.opened[hand.index()]
        .then(|| throw_release(hand, world.w_axis.truncate(), since_s, "beacon", None));
    let mut state = STATE.lock();
    if let Some(pending) = state.pending.as_mut()
        && pending.release.is_none()
    {
        pending.world = world;
        pending.release = release;
    }
}

pub(super) fn input_unavailable(unavailable: [bool; 2]) {
    let mut state = STATE.lock();
    if let Some(pending) = state.pending.as_mut()
        && unavailable[pending.hand.index()]
    {
        pending.release = Some(Release::REST);
    }
}

pub(super) fn stand_down() {
    if let Some(pending) = STATE.lock().pending.as_mut() {
        pending.release = Some(Release::REST);
    }
}

pub(super) unsafe fn observe(weapon: *mut ThrownGrenadeWeapon) {
    let Some(base) = (unsafe { weapon.cast::<WeaponBase>().as_ref() }) else {
        return;
    };
    if !base.m_IsUserLocalPlayer
        || unsafe { engine::slot_of(base) } != Some(WeaponSlot::E_SLOT_BEACON)
        || !scheme::is_controllers()
        || !Config::lock_query(|config| config.controllers.weapons.grenade_throw)
        || STATE.lock().pending.is_some()
    {
        return;
    }
    let hand = STATE.lock().hand;
    if bindings::grip_holds_now(hand) != Some(true)
        || safe::in_hand(hand).is_some()
        || catch::caught().is_some_and(|caught| caught.hand == hand)
    {
        return;
    }
    let Some(grip) = crate::controllers::weapons::grenade::STATE.lock().grips[hand.index()] else {
        return;
    };
    let Some(explosive) = (unsafe { primed_explosive(&*weapon) }) else {
        return;
    };
    let tuning = GrenadeTuning::of(&Config::lock_query(|config| {
        config.controllers.weapons.grenade
    }));
    let world = held_matrix(grip, hold_local(hand, &tuning));
    if !world.is_finite() {
        return;
    }
    let pending = Pending {
        weapon: unsafe { Weak::of(&base.m_WeakThis) },
        actor: unsafe { Weak::of(&(*weapon).m_PrimedGrenade) },
        explosive: explosive as usize,
        hand,
        since_s: motion::latest_time().unwrap_or(f64::NEG_INFINITY),
        world,
        release: None,
        ready: false,
        data: None,
        waiting_frames: 0,
    };
    let mut state = STATE.lock();
    if state.pending.is_none() {
        state.pending = Some(pending);
    }
}

pub(super) unsafe fn render_frame() {
    let Some(mut pending) = STATE.lock().pending.take() else {
        return;
    };
    let weapon_alive = pending.weapon.lock();
    let actor_alive = pending.actor.lock();
    let valid =
        weapon_alive
            .as_ref()
            .zip(actor_alive.as_ref())
            .and_then(|(weapon_alive, actor_alive)| {
                let base = weapon_of(weapon_alive);
                let owner = unsafe { engine::local_player() }?;
                let inventory = &owner.m_Inventory.m_Weapons[WeaponSlot::E_SLOT_BEACON as usize];
                if inventory.px != base || !unsafe { (*base).m_IsUserLocalPlayer } {
                    return None;
                }
                let weapon = base.cast::<ThrownGrenadeWeapon>();
                if unsafe { primed_explosive(&*weapon) }
                    != Some(pending.explosive as *mut Explosive)
                    || unsafe { (*weapon).m_PrimedGrenade.pn } != actor_alive.control_block()
                {
                    return None;
                }
                Some(base)
            });
    let Some(base) = valid else {
        held::hold(Slot::Grenade, None);
        return;
    };
    if !crate::hooks::in_gameplay()
        || !scheme::is_controllers()
        || !Config::lock_query(|config| config.controllers.weapons.grenade_throw)
        || bindings::grip_holds_now(pending.hand).is_none()
    {
        pending.release = Some(Release::REST);
    }
    let explosive = pending.explosive as *mut Explosive;
    let physics_ready = unsafe { !(*explosive).m_PfxInstance.is_null() };
    if !pending.ready || pending.release.is_some() {
        pending.waiting_frames += 1;
        if pending.waiting_frames >= MAX_WAIT_FRAMES {
            held::hold(Slot::Grenade, None);
            tracing::warn!(target: "controllers", "beacon: native release readiness timed out; the weapon retains its primed actor");
            return;
        }
    }
    if pending.ready
        && physics_ready
        && let Some(release) = pending.release
    {
        let data = pending
            .data
            .and_then(|index| unsafe { engine::gun_ptr(&*base, index) });
        if let Some(data) = data {
            held::hold(Slot::Grenade, None);
            let scope = LaunchScope::enter(PlannedLaunch {
                explosive: pending.explosive,
                transform: Some(pending.world.into()),
                release,
                catchable: false,
            });
            unsafe { (*base).Fire_Projectile(data.as_ptr()) };
            let consumed = LAUNCH.get().is_none();
            drop(scope);
            if consumed && unsafe { (*explosive).m_DeferredPropelTowards } {
                report_native_throw(pending.hand, release);
            } else {
                tracing::warn!(target: "controllers", "beacon: the native release did not queue propulsion");
            }
            return;
        }
    }
    unsafe {
        (*explosive).m_WorldMatrixT0 = (*explosive).m_WorldMatrixT1;
        (*explosive).m_WorldMatrixT1 = pending.world.into();
    }
    let tuning = GrenadeTuning::of(&Config::lock_query(|config| {
        config.controllers.weapons.grenade
    }));
    held::hold(
        Slot::Grenade,
        Some(Held {
            explosive: pending.explosive,
            hand: pending.hand,
            local: hold_local(pending.hand, &tuning),
        }),
    );
    STATE.lock().pending = Some(pending);
}

struct Pending {
    weapon: Weak,
    actor: Weak,
    explosive: usize,
    hand: Hand,
    since_s: f64,
    world: Mat4,
    release: Option<Release>,
    ready: bool,
    data: Option<usize>,
    waiting_frames: u32,
}

const MAX_WAIT_FRAMES: u32 = 120;

struct State {
    pending: Option<Pending>,
    hand: Hand,
}

static STATE: Mutex<State> = Mutex::new(State {
    pending: None,
    hand: Hand::Right,
});

pub(super) fn propelling(explosive: usize) -> bool {
    PROPELLING.get() == Some(explosive)
}

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
            let same = pending.weapon.raw().px == engine::object_of(base).cast()
                && pending.weapon.raw().pn == unsafe { (*base).m_WeakThis.pn }
                && pending.actor.raw().pn
                    == unsafe { (*weapon.cast::<ThrownGrenadeWeapon>()).m_PrimedGrenade.pn };
            if same {
                let index = usize::from(unsafe { (*base).m_NextWeaponToFire });
                let expected = unsafe { engine::gun_ptr(&*base, index) };
                if expected.is_some_and(|expected| expected.as_ptr() == data) {
                    pending.ready = true;
                    pending.data = Some(index);
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        }
    };
    if !defer {
        original(weapon, data);
    }
}
