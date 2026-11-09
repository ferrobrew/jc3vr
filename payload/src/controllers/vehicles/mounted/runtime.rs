//! The runtime behind grip-driven mounted weapons: observe mounted weapons, retain the ones the
//! physical hands grip, give them a temporary shooter, and tear them off on a sustained pull.

use std::{ffi::c_void, time::Instant};

use jc3gi::{
    character::character::Character,
    equipment::weapon::{WeaponBase, WeaponState},
};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        frame::Hand,
        loadout::{Delivery, TakeSource},
        physical_hands, scheme,
        vehicles::mounted::{
            control::{Identity, Input, Tear},
            native::{self, Motor, Prompt},
        },
        weapons::{
            FinishInputs,
            engine::{character_object, object_of},
            hold::{armed, pickup, refs::Weak},
        },
    },
};

/// Record a mounted weapon the late weapon update saw, so the next input tick can find it.
///
/// # Safety
/// The weapon is live on the game thread.
pub(crate) unsafe fn observe(weapon: &WeaponBase) {
    if weapon.m_State != WeaponState::E_WS_MOUNTED || weapon.m_WeakThis.pn.is_null() {
        return;
    }
    let object = object_of(weapon) as usize;
    let control = weapon.m_WeakThis.pn as usize;
    let mut seen = SEEN.lock();
    if seen
        .iter()
        .any(|record| record.object == object && record.control == control)
    {
        return;
    }
    if seen.len() >= MAX_SEEN {
        seen.remove(0);
    }
    seen.push(Seen {
        object,
        control,
        weak: unsafe { Weak::of(&weapon.m_WeakThis) },
    });
}

/// Retain the mounted weapons the physical hands grip, and release the rest.
///
/// # Safety
/// The game thread, after the physical hands and weapon handling settled this tick.
pub(crate) unsafe fn tick(inputs: &FinishInputs) {
    let enabled = scheme::is_controllers()
        && !crate::is_shutting_down()
        && Config::lock_query(|config| {
            config.controllers.enabled && config.controllers.physical_hands.enabled
        });
    let held = physical_hands::held_bodies();
    let now = Instant::now();
    let mut previous = std::mem::take(&mut *CONTROLLED.lock());
    let mut next = Vec::new();
    let Some(local) =
        (unsafe { Character::GetLocalPlayerCharacter().as_mut() }).filter(|_| enabled)
    else {
        unsafe { release_all(previous) };
        return;
    };
    let seen: Vec<(usize, usize, Weak)> = SEEN
        .lock()
        .iter()
        .map(|record| (record.object, record.control, record.weak.clone()))
        .collect();
    for (object, control, weak) in seen {
        let Some(alive) = weak
            .lock()
            .filter(|alive| alive.object() as usize == object)
        else {
            continue;
        };
        let weapon_ptr = crate::controllers::weapons::engine::weapon_at(alive.object());
        let weapon = unsafe { &mut *weapon_ptr };
        if weapon.m_State != WeaponState::E_WS_MOUNTED
            || !weapon.m_User[0].is_null()
            || native::lifecycle_pending(weapon)
        {
            continue;
        }
        let matches = held.map(|held| held.filter(|held| held.object.get() == object));
        let hands = Hand::BOTH.map(|hand| {
            let i = hand.index();
            matches[i].is_some() && inputs.grips[i].held() && !inputs.unavailable[i]
        });
        if !hands.contains(&true) {
            continue;
        }
        let Some(body) = matches.into_iter().flatten().next() else {
            continue;
        };
        let identity = Identity {
            weapon: weapon_ptr as usize,
            control,
            object: body.object.get(),
            body: body.body,
        };
        let existing = previous
            .iter()
            .position(|record| record.identity == identity);
        let mut record = if let Some(index) = existing {
            previous.remove(index)
        } else {
            let shooter = unsafe { Weak::of(&local.m_WeakThis) };
            Controlled {
                identity,
                weak,
                shooter,
                input: None,
                motors: unsafe { native::motors(weapon) },
                prompt: unsafe { native::prompt(weapon) },
                tear: [Tear::default(); 2],
            }
        };
        record.input = Some(Input {
            identity,
            hands,
            fire: inputs.triggers.map(|trigger| trigger.held()),
            at: now,
        });
        for motor in &record.motors {
            unsafe { motor.disable() };
        }
        if let Some(prompt) = &mut record.prompt {
            unsafe { prompt.suppress() };
        }
        let tear = Hand::BOTH.into_iter().any(|hand| {
            let i = hand.index();
            if !hands[i] {
                record.tear[i] = Tear::default();
                return false;
            }
            record.tear[i].step(inputs.real_dt, matches[i].and_then(|held| held.report))
        });
        if tear && weapon.m_MountedCanBeDetached {
            let hand = Hand::BOTH
                .into_iter()
                .find(|hand| hands[hand.index()])
                .unwrap();
            match unsafe { pickup::preflight(&*local, weapon, hand, false) } {
                Ok(admitted) => {
                    unsafe { record.restore() };
                    unsafe { armed::release_fire(weapon) };
                    unsafe { weapon.DetachMountedGun() };
                    if weapon.m_State == WeaponState::E_WS_DROPPED {
                        let result = unsafe {
                            pickup::deliver(
                                local,
                                hand,
                                weapon,
                                admitted,
                                pickup::TakeOptions {
                                    slide_s: 0.15,
                                    source: TakeSource::World,
                                    delivery: Delivery::Local,
                                },
                            )
                        };
                        if let Err(error) = result {
                            tracing::warn!(%error, "mounted gun: portable pickup refused after detach");
                        }
                    }
                    record.input = None;
                    next.push(record);
                    continue;
                }
                Err(error) => {
                    tracing::debug!(%error, "mounted gun: tear-off pickup admission refused")
                }
            }
        }
        if !weapon.m_RequiresPostSimUpdates {
            weapon.m_RequiresPostSimUpdates = true;
            unsafe { (*object_of(weapon)).AddToUpdate() };
        }
        next.push(record);
    }
    for mut record in previous {
        let alive = record
            .weak
            .lock()
            .filter(|alive| alive.object() as usize == record.identity.object);
        if record.input.take().is_some() {
            unsafe { record.restore() };
            if alive.is_some() {
                let weapon = unsafe { &mut *(record.identity.weapon as *mut WeaponBase) };
                if weapon.m_User[0].is_null() {
                    unsafe { armed::release_fire(weapon) };
                }
            }
        }
        if alive.is_some() {
            let weapon = unsafe { &*(record.identity.weapon as *const WeaponBase) };
            if weapon.m_DeferredPhysicalRecoil || weapon.m_DeferredAwarenessBroadcast {
                next.push(record);
            }
        }
    }
    *CONTROLLED.lock() = next;
    SEEN.lock().retain(|record| record.weak.lock().is_some());
}

/// Release every retained gun, for a paused game, a mode change, or a shutdown.
pub(crate) unsafe fn stand_down() {
    let records = std::mem::take(&mut *CONTROLLED.lock());
    unsafe { release_all(records) };
}

/// The retained grips on `weapon`, if this module controls it.
pub(crate) fn input(weapon: &WeaponBase) -> Option<Input> {
    let address = weapon as *const WeaponBase as usize;
    let control = weapon.m_WeakThis.pn as usize;
    CONTROLLED
        .lock()
        .iter()
        .find(|record| record.identity.weapon == address && record.identity.control == control)
        .and_then(|record| record.input)
}

/// The temporary shooter for `weapon`, and whether the hands still grip it.
pub(crate) fn shooter(weapon: &WeaponBase) -> Option<(Weak, bool)> {
    let address = weapon as *const WeaponBase as usize;
    let control = weapon.m_WeakThis.pn as usize;
    CONTROLLED
        .lock()
        .iter()
        .find(|record| record.identity.weapon == address && record.identity.control == control)
        .map(|record| (record.shooter.clone(), record.input.is_some()))
}

/// Run `run` with the local player as `weapon`'s temporary user, then put the weapon's user fields
/// back. `None` when the shooter no longer resolves to the local player, the weapon already has a
/// user, or a deferred lifecycle operation is about to run.
///
/// # Safety
/// The weapon is live on the game thread, and `run` may act on it.
pub(crate) unsafe fn with_shooter<R>(
    weapon: *mut WeaponBase,
    shooter: &Weak,
    run: impl FnOnce() -> R,
) -> Option<R> {
    let alive = shooter.lock()?;
    let character = alive
        .object()
        .cast::<u8>()
        .wrapping_sub(Character::GAME_OBJECT_BASE_OFFSET as usize)
        .cast::<Character>();
    if character_object(character) != alive.object()
        || character != unsafe { Character::GetLocalPlayerCharacter() }
    {
        return None;
    }
    let weapon_ref = unsafe { &mut *weapon };
    if !weapon_ref.m_User[0].is_null() || native::lifecycle_pending(weapon_ref) {
        return None;
    }
    let saved_user = weapon_ref.m_User;
    let saved_local = weapon_ref.m_IsUserLocalPlayer;
    let saved_player = weapon_ref.m_IsUserPlayer;
    weapon_ref.m_User = [character.cast::<c_void>(), shooter.raw().pn];
    weapon_ref.m_IsUserLocalPlayer = true;
    weapon_ref.m_IsUserPlayer = true;
    let result = run();
    let weapon_ref = unsafe { &mut *weapon };
    weapon_ref.m_User = saved_user;
    weapon_ref.m_IsUserLocalPlayer = saved_local;
    weapon_ref.m_IsUserPlayer = saved_player;
    Some(result)
}

/// Restore every record's mount and prompt, release its fire, and keep only the ones whose
/// deferred recoil or awareness is still pending, so their next pre-simulation update still has a
/// shooter.
unsafe fn release_all(records: Vec<Controlled>) {
    let mut draining = Vec::new();
    for mut record in records {
        unsafe { record.restore() };
        record.input = None;
        let alive = record
            .weak
            .lock()
            .filter(|alive| alive.object() as usize == record.identity.object);
        if alive.is_none() {
            continue;
        }
        let weapon = unsafe { &mut *(record.identity.weapon as *mut WeaponBase) };
        if weapon.m_User[0].is_null() {
            unsafe { armed::release_fire(weapon) };
        }
        if weapon.m_DeferredPhysicalRecoil || weapon.m_DeferredAwarenessBroadcast {
            draining.push(record);
        }
    }
    *CONTROLLED.lock() = draining;
}

/// The most mounted weapons the late weapon update retains between input ticks.
const MAX_SEEN: usize = 256;

static SEEN: Mutex<Vec<Seen>> = Mutex::new(Vec::new());
static CONTROLLED: Mutex<Vec<Controlled>> = Mutex::new(Vec::new());

struct Seen {
    object: usize,
    control: usize,
    weak: Weak,
}

struct Controlled {
    identity: Identity,
    weak: Weak,
    shooter: Weak,
    input: Option<Input>,
    motors: Vec<Motor>,
    prompt: Option<Prompt>,
    tear: [Tear; 2],
}

impl Controlled {
    unsafe fn restore(&mut self) {
        if let Some(prompt) = &mut self.prompt {
            unsafe { prompt.restore() };
        }
        for motor in &self.motors {
            unsafe { motor.restore() };
        }
        self.motors.clear();
        self.prompt = None;
    }
}
