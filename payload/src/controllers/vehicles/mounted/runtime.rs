//! The runtime behind grip-driven mounted weapons: observe mounted weapons, retain the ones the
//! physical hands grip by their body, holder, or handle, give them a temporary shooter, and tear
//! them off on a sustained pull.

use std::{ffi::c_void, num::NonZeroUsize, time::Instant};

use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::{WeaponBase, WeaponState},
};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        cockpit::CockpitTransform,
        frame::Hand,
        hands,
        loadout::{Delivery, TakeSource},
        physical_hands::{self, doors::engine::vehicle_of as resolve_vehicle},
        scheme,
        vehicles::{
            mounted::{
                Held, Mount,
                control::{Identity, Input, Tear},
                handle, mount_matches,
                native::{self, Motor, Prompt},
                pivot_and_barrel,
            },
            summary as vehicles_summary,
        },
        weapons::{
            FinishInputs,
            engine::{character_object, object_of, weapon_at},
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
    // The native mounted state's handles are the only grip on a gun the player entered by the use
    // prompt. Keep the hands that held them when the runtime stops that use, so the gun does not
    // fall out of the hands on the same tick.
    let native_holds = if unsafe { local.IsInMountedGunState() } {
        vehicles_summary().mounted_holds
    } else {
        [false; 2]
    };
    let active = hands::active_hands_raw();
    let xf = active
        .as_ref()
        .map(|active| CockpitTransform::current(active.head.position, active.world_scale));
    let resolve = |object: usize| unsafe {
        NonZeroUsize::new(object)
            .and_then(|object| resolve_vehicle(object))
            .map(NonZeroUsize::get)
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
        let weapon_ptr = weapon_at(alive.object());
        let weapon = unsafe { &mut *weapon_ptr };
        if weapon.m_State != WeaponState::E_WS_MOUNTED || native::lifecycle_pending(weapon) {
            continue;
        }
        let Some((pivot, _)) = (unsafe { pivot_and_barrel(weapon) }) else {
            continue;
        };
        let holder_ref = if weapon.m_Holder.pn.is_null() {
            None
        } else {
            unsafe { Weak::of(&weapon.m_Holder) }.lock()
        };
        let holder = holder_ref.as_ref().map(|alive| alive.object() as usize);
        let mount = Mount {
            object,
            holder,
            centre: pivot,
        };
        let physical = held.map(|held| {
            held.filter(|held| {
                mount_matches(
                    mount,
                    Held {
                        object: held.object.get(),
                        centre: held.report.map(|report| report.centre),
                    },
                    resolve,
                )
            })
        });
        let native = unsafe { native_mounted(local, weapon) };
        let native_hands = if native { native_holds } else { [false; 2] };
        let physical_body = physical.iter().flatten().next().copied();
        let identity = match physical_body {
            Some(body) => Identity {
                weapon: weapon_ptr as usize,
                control,
                object: body.object.get(),
                body: body.body,
            },
            None => Identity {
                weapon: weapon_ptr as usize,
                control,
                object,
                body: NATIVE_HANDLE_BODY,
            },
        };
        let existing = previous.iter().position(|record| {
            record.identity.weapon == weapon_ptr as usize && record.identity.control == control
        });
        let previous_takeover = existing.and_then(|index| previous[index].takeover);
        let takeover_hands = Hand::BOTH.map(|hand| {
            let i = hand.index();
            (native_hands[i] || previous_takeover.is_some_and(|takeover| takeover.hands[i]))
                && physical[i].is_none()
        });
        let hands = Hand::BOTH.map(|hand| {
            let i = hand.index();
            (physical[i].is_some() || takeover_hands[i])
                && inputs.grips[i].held()
                && !inputs.unavailable[i]
        });
        if !hands.contains(&true) {
            continue;
        }
        // A hand that lets go ends its native handle hold: taking the grip again needs a new
        // handle or holder, as it would on the native handles.
        let takeover_hands = Hand::BOTH.map(|hand| {
            let i = hand.index();
            takeover_hands[i] && hands[i]
        });
        let user = weapon.m_User[0];
        if !user.is_null() {
            if !std::ptr::eq(user.cast::<Character>(), local as *const Character) {
                continue;
            }
            unsafe { weapon.StopUsing() };
            if !weapon.m_User[0].is_null() || weapon.m_State != WeaponState::E_WS_MOUNTED {
                continue;
            }
            tracing::info!(
                target: "controllers",
                "mounted gun: stopped the native use and took the gun"
            );
        }
        let created = existing.is_none();
        let mut record = if let Some(index) = existing {
            let mut record = previous.remove(index);
            if record.identity != identity {
                record.tear = [Tear::default(); 2];
            }
            record
        } else {
            let shooter = unsafe { Weak::of(&local.m_WeakThis) };
            Controlled {
                identity,
                weapon_object: object,
                weak,
                shooter,
                input: None,
                motors: unsafe { native::motors(weapon) },
                prompt: unsafe { native::prompt(weapon) },
                tear: [Tear::default(); 2],
                takeover: None,
            }
        };
        record.identity = identity;
        let takeover = takeover_hands.contains(&true);
        record.takeover = takeover.then_some(Takeover {
            hands: takeover_hands,
        });
        record.input = Some(Input {
            identity,
            hands,
            fire: inputs.triggers.map(|trigger| trigger.held()),
            at: now,
        });
        for motor in &record.motors {
            unsafe { motor.disable() };
        }
        if record.prompt.is_none() {
            record.prompt = unsafe { native::prompt(weapon) };
        }
        if let Some(prompt) = &mut record.prompt {
            unsafe { prompt.suppress() };
        }
        if created {
            tracing::info!(
                target: "controllers",
                hands = ?hands,
                "mounted gun: the hands took the gun"
            );
        }
        let tear = Hand::BOTH.into_iter().any(|hand| {
            let i = hand.index();
            if !hands[i] {
                record.tear[i] = Tear::default();
                return false;
            }
            if let Some(report) = physical[i].and_then(|held| held.report) {
                return record.tear[i].step(inputs.real_dt, Some(report));
            }
            if takeover_hands[i] {
                let handle_point = unsafe { handle(weapon, hand) };
                let hand_world = active.as_ref().zip(xf.as_ref()).and_then(|(active, xf)| {
                    active.hands[i].map(|hand| xf.point_to_world(hand.grip.position))
                });
                let lag = hand_world
                    .zip(handle_point)
                    .map(|(hand, handle)| hand.distance(handle));
                let Some(lag) = lag else {
                    record.tear[i] = Tear::default();
                    return false;
                };
                return record.tear[i].step_pull(inputs.real_dt, lag, 1.0);
            }
            record.tear[i] = Tear::default();
            false
        });
        if tear && weapon.m_MountedCanBeDetached {
            let hand = Hand::BOTH
                .into_iter()
                .find(|hand| hands[hand.index()])
                .unwrap();
            match unsafe { pickup::preflight(&*local, &*weapon, hand, false) } {
                Ok(admitted) => {
                    unsafe { record.restore() };
                    unsafe { armed::release_fire(weapon) };
                    unsafe { weapon.DetachMountedGun() };
                    tracing::info!(target: "controllers", "mounted gun: the pull tore the gun off");
                    if weapon.m_State == WeaponState::E_WS_DROPPED {
                        let result = unsafe {
                            pickup::deliver(
                                &mut *local,
                                hand,
                                &mut *weapon,
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
                    record.takeover = None;
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
            .filter(|alive| alive.object() as usize == record.weapon_object);
        if record.input.take().is_some() {
            unsafe { record.restore() };
            if alive.is_some() {
                let weapon = unsafe { &mut *(record.identity.weapon as *mut WeaponBase) };
                if weapon.m_User[0].is_null() {
                    unsafe { armed::release_fire(weapon) };
                }
            }
            tracing::info!(
                target: "controllers",
                alive = alive.is_some(),
                "mounted gun: the runtime released the gun"
            );
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

/// Whether the runtime holds `weapon`, so the game's native start-using must not run.
pub(crate) fn suppress_start_using(weapon: &WeaponBase) -> bool {
    let address = weapon as *const WeaponBase as usize;
    let control = weapon.m_WeakThis.pn as usize;
    CONTROLLED.lock().iter().any(|record| {
        record.identity.weapon == address
            && record.identity.control == control
            && record.input.is_some()
    })
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

/// Whether `weapon` is the gun `local` mans in the native mounted state.
///
/// # Safety
/// The character and weapon are live on the game thread.
unsafe fn native_mounted(local: &Character, weapon: &WeaponBase) -> bool {
    if !unsafe { local.IsInMountedGunState() } {
        return false;
    }
    unsafe {
        local
            .m_Inventory
            .m_Weapons
            .get(WeaponSlot::E_SLOT_HEAVY as usize)
            .and_then(|slot| slot.px.as_ref())
            .is_some_and(|held| std::ptr::eq(held, weapon))
    }
}

/// Restore every record's mount and prompt, release its fire, and keep only the ones whose
/// deferred recoil or awareness is still pending, so their next pre-simulation update still has a
/// shooter.
unsafe fn release_all(records: Vec<Controlled>) {
    let mut draining = Vec::new();
    for mut record in records {
        unsafe { record.restore() };
        record.input = None;
        record.takeover = None;
        let alive = record
            .weak
            .lock()
            .filter(|alive| alive.object() as usize == record.weapon_object);
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

/// The body of a hand that holds a handle the native mounted state placed, not a physical body.
const NATIVE_HANDLE_BODY: u32 = u32::MAX;

static SEEN: Mutex<Vec<Seen>> = Mutex::new(Vec::new());
static CONTROLLED: Mutex<Vec<Controlled>> = Mutex::new(Vec::new());

struct Seen {
    object: usize,
    control: usize,
    weak: Weak,
}

struct Controlled {
    identity: Identity,
    /// The weapon's own game object, which its weak reference names. A held body of its holder
    /// must not be mistaken for a dead weapon.
    weapon_object: usize,
    weak: Weak,
    shooter: Weak,
    input: Option<Input>,
    motors: Vec<Motor>,
    prompt: Option<Prompt>,
    tear: [Tear; 2],
    /// The hands the native mounted state gave the gun when the runtime stopped that use.
    takeover: Option<Takeover>,
}

/// The hands the native mounted state gave the gun when the runtime stopped that use.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct Takeover {
    hands: [bool; 2],
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
