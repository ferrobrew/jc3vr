//! The runtime behind grip-driven mounted weapons: observe mounted weapons, retain the ones the
//! physical hands grip by their body, holder, or handle, give them a temporary shooter, and tear
//! them off on a sustained pull, a vehicle's gun included. A gun the player mans from a vehicle's
//! gunner seat is left to the seat's native use, which keeps the player in the seat, until a pull
//! on its handles leaves the seat and tears it off; the player steps into an empty gunner seat by
//! standing behind its gun ([`gunner_seat`]).

use std::{
    ffi::c_void,
    num::NonZeroUsize,
    time::{Duration, Instant},
};

use glam::Vec3;
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::{WeaponBase, WeaponState},
    input::input_action_map::Action,
};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        bindings::hold::{Desired, MapKind, Target, Want},
        cockpit::CockpitTransform,
        frame::Hand,
        hands,
        loadout::{Delivery, TakeSource},
        physical_hands::{self, doors::engine::vehicle_of as resolve_vehicle, physics_object_of},
        scheme,
        vehicles::{
            is_mounted_weapon,
            mounted::{
                Held, Mount, MountedGun,
                control::{Identity, Input, Tear},
                gunner_seat, handle, mount_matches,
                native::{self, Motor, Prompt},
                pivot_and_barrel,
                vehicle_hold::VehicleHold,
            },
            summary as vehicles_summary,
        },
        weapons::{
            FinishInputs,
            engine::{character_object, object_of, weapon_at},
            hold::{armed, button_pickup, pickup, refs::Weak},
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
pub(crate) unsafe fn tick(inputs: &FinishInputs, desired: &mut Vec<Desired>) {
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
        gunner_seat::reset();
        return;
    };
    // The virtual mounted handles are the grip the player takes on a gun without the use prompt.
    // The native mounted state places the same handles, so its hands are kept when the runtime
    // stops that use, and the gun does not fall out of the hands on the same tick.
    let summary = vehicles_summary();
    let handle_holds = summary.mounted_holds;
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
    let guns: Vec<(usize, Weak)> = seen
        .iter()
        .map(|(object, _, weak)| (*object, weak.clone()))
        .collect();
    unsafe { gunner_seat::step(local, xf.as_ref().map(|xf| xf.head_world), &guns, desired) };
    // In a vehicle's gunner seat the native use is what holds the player in the seat: stopping it
    // would leave the seat, so the seat's gun is aimed by the handles through the native state.
    let seated = unsafe { local.IsAttachedToVehicle() };
    unsafe { finish_seat_tear(local, desired) };
    let seat_inputs = SeatTearInputs {
        handle_holds,
        hands_world: Hand::BOTH.map(|hand| {
            active.as_ref().zip(xf.as_ref()).and_then(|(active, xf)| {
                active.hands[hand.index()].map(|hand| xf.point_to_world(hand.grip.position))
            })
        }),
        dt: inputs.real_dt,
    };
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
        let holder = holder_ref.as_ref().and_then(|alive| unsafe {
            physics_object_of(NonZeroUsize::new(alive.object() as usize)?)
        });
        let mount = Mount {
            object: weapon_ptr as usize,
            holder: holder.map(NonZeroUsize::get),
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
        if native && seated {
            unsafe { seat_tear(&seat_inputs, weapon, &weak, desired) };
            continue;
        }
        // A native use places both hands on the gun's handles; the virtual handles hold only the
        // hands that took them, and only for the gun the input tick selected.
        let selected = is_mounted_weapon(weapon_ptr as usize);
        let native_hands = if native {
            [true; 2]
        } else if selected {
            handle_holds
        } else {
            [false; 2]
        };
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
                object: weapon_ptr as usize,
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
        // A physical hold or a handle hold is already a held grip; the native takeover keeps the
        // hands the game placed, whether or not the player's grip button is down.
        let hands = Hand::BOTH.map(|hand| {
            let i = hand.index();
            (physical[i].is_some() || takeover_hands[i]) && !inputs.unavailable[i]
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
                // A pull takes the hand farther from the gun's pivot than the handle rests; aiming
                // swings the hand around the pivot at the handle's radius, which is not a pull.
                let lag = hand_world
                    .zip(handle_point)
                    .map(|(hand, handle)| hand.distance(pivot) - handle.distance(pivot));
                let Some(lag) = lag.filter(|lag| lag.is_finite()) else {
                    record.tear[i] = Tear::default();
                    return false;
                };
                return record.tear[i].step_pull(inputs.real_dt, lag.max(0.0), 1.0);
            }
            record.tear[i] = Tear::default();
            false
        });
        // A vehicle's gun comes off as the vehicle's explosion lets it go; any other gun only where
        // the game lets it be detached.
        let hold = unsafe { VehicleHold::of(weapon) };
        let detachable = weapon.m_MountedCanBeDetached || hold.is_some();
        if tear && detachable && hold.is_none_or(|hold| unsafe { hold.ready() }) {
            let hand = Hand::BOTH
                .into_iter()
                .find(|hand| hands[hand.index()])
                .unwrap();
            unsafe { record.restore() };
            unsafe { tear_off(local, weapon, hand, hold, desired) };
            record.input = None;
            record.takeover = None;
            next.push(record);
            continue;
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

/// Tear `weapon` off its mount into `hand`: release its fire, detach it through the weapon's own
/// path, release the vehicle's hold on it where a vehicle carries it, and take it into the hand,
/// or, for a gun the table cannot take (a heavy gun from the dropped state), through the game's own
/// use-item pickup, which carries it in the heavy slot.
///
/// # Safety
/// The game thread, outside the world update; the character and the weapon are live, the weapon
/// mounted and without a user.
unsafe fn tear_off(
    local: &mut Character,
    weapon: &mut WeaponBase,
    hand: Hand,
    hold: Option<VehicleHold>,
    desired: &mut Vec<Desired>,
) {
    let admitted = unsafe { pickup::preflight(&*local, &*weapon, hand, false) };
    unsafe { armed::release_fire(weapon) };
    unsafe { weapon.DetachMountedGun() };
    if let Some(hold) = hold
        && let Err(error) = unsafe { hold.release(weapon) }
    {
        tracing::warn!(
            target: "controllers",
            reason = error.label(),
            "mounted gun: the vehicle's hold on the torn gun was not released"
        );
    }
    tracing::info!(
        target: "controllers",
        from_vehicle = hold.is_some(),
        "mounted gun: the pull tore the gun off"
    );
    if weapon.m_State != WeaponState::E_WS_DROPPED {
        return;
    }
    let delivered = admitted
        .map_err(|error| error.to_string())
        .and_then(|admitted| {
            unsafe {
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
            }
            .map_err(|error| error.to_string())
        });
    if let Err(error) = delivered {
        tracing::info!(%error, "mounted gun: the table did not take the torn gun");
        // A heavy gun cannot enter the table from the dropped state; the game's own use-item
        // pickup takes it instead, as a direct pickup does.
        button_pickup::grant();
        desired.push(Desired {
            target: Target {
                map: MapKind::Player,
                action: Action::USE_ITEM,
            },
            want: Want::Click,
        });
        tracing::info!(
            target: "controllers",
            "mounted gun: the detached gun is picked up through the use-item action"
        );
    }
}

/// A tear from a vehicle's gunner seat in progress: the pull that leaves the seat, and once the
/// player has left it, the gun to tear off.
struct SeatTear {
    weapon: usize,
    weak: Weak,
    pulls: [Tear; 2],
    /// When the exit was asked for, the hand that tore, and whether the exit was asked again.
    exit: Option<(Instant, Hand, bool)>,
}

/// Step the tear of the gun the player mans from a vehicle's gunner seat: a sustained pull on its
/// handles, the same as off the seat, asks for the game's own exit from the seat.
///
/// # Safety
/// The game thread; `weapon` is the live gun `local` mans natively from a vehicle seat.
unsafe fn seat_tear(
    inputs: &SeatTearInputs,
    weapon: &WeaponBase,
    weak: &Weak,
    desired: &mut Vec<Desired>,
) {
    if unsafe { VehicleHold::of(weapon) }.is_none() {
        return;
    }
    let address = weapon as *const WeaponBase as usize;
    let mut slot = SEAT_TEAR.lock();
    if slot.as_ref().is_none_or(|tear| tear.weapon != address) {
        *slot = Some(SeatTear {
            weapon: address,
            weak: weak.clone(),
            pulls: [Tear::default(); 2],
            exit: None,
        });
    }
    let Some(tear) = slot.as_mut() else {
        return;
    };
    if tear.exit.is_some() {
        return;
    }
    let Some((pivot, _)) = (unsafe { pivot_and_barrel(weapon) }) else {
        return;
    };
    let torn = Hand::BOTH.into_iter().find(|&hand| {
        let i = hand.index();
        let pull = inputs.handle_holds[i]
            .then(|| unsafe { handle(weapon, hand) })
            .flatten()
            .zip(inputs.hands_world[i])
            .map(|(handle, hand)| hand.distance(pivot) - handle.distance(pivot))
            .filter(|lag| lag.is_finite());
        match pull {
            Some(lag) => tear.pulls[i].step_pull(inputs.dt, lag.max(0.0), 1.0),
            None => {
                tear.pulls[i] = Tear::default();
                false
            }
        }
    });
    if let Some(hand) = torn {
        tear.exit = Some((Instant::now(), hand, false));
        desired.push(exit_click());
        tracing::info!(
            target: "controllers",
            hand = hand.label(),
            "mounted gun: the pull tore at the gun from its gunner seat; leaving the seat"
        );
    }
}

/// Finish a tear from the gunner seat once the player has left the seat: tear the gun off into the
/// hand that pulled. The exit is asked again once if the player is still seated after
/// [`SEAT_EXIT_RETRY`], and the tear is given up after [`SEAT_EXIT_TIMEOUT`].
///
/// # Safety
/// The game thread, outside the world update; `local` is the live local player.
unsafe fn finish_seat_tear(local: &mut Character, desired: &mut Vec<Desired>) {
    let mut slot = SEAT_TEAR.lock();
    let seated = unsafe { local.IsAttachedToVehicle() || local.IsInMountedGunState() };
    let Some((asked, hand, retried)) = slot.as_ref().and_then(|tear| tear.exit) else {
        // A pull that has not torn yet ends with the seat.
        if !seated {
            *slot = None;
        }
        return;
    };
    if seated {
        if asked.elapsed() >= SEAT_EXIT_TIMEOUT {
            *slot = None;
            tracing::info!(
                target: "controllers",
                "mounted gun: the player did not leave the gunner seat; the tear is given up"
            );
        } else if !retried && asked.elapsed() >= SEAT_EXIT_RETRY {
            if let Some(tear) = slot.as_mut() {
                tear.exit = Some((asked, hand, true));
            }
            desired.push(exit_click());
        }
        return;
    }
    let Some(tear) = slot.take() else {
        return;
    };
    drop(slot);
    let Some(alive) = tear
        .weak
        .lock()
        .filter(|alive| weapon_at(alive.object()) as usize == tear.weapon)
    else {
        return;
    };
    let weapon = unsafe { &mut *weapon_at(alive.object()) };
    if weapon.m_State != WeaponState::E_WS_MOUNTED || !weapon.m_User[0].is_null() {
        tracing::info!(
            target: "controllers",
            "mounted gun: the gun left the mount or has a user after the seat exit; no tear"
        );
        return;
    }
    let Some(hold) = (unsafe { VehicleHold::of(weapon) }) else {
        return;
    };
    if !unsafe { hold.ready() } {
        // Try again next tick, once the vehicle is out of its parallel update.
        *SEAT_TEAR.lock() = Some(tear);
        return;
    }
    unsafe { tear_off(local, weapon, hand, Some(hold), desired) };
}

/// What the seat's tear reads this tick.
struct SeatTearInputs {
    handle_holds: [bool; 2],
    hands_world: [Option<Vec3>; 2],
    dt: f32,
}

/// A click of the game's own vehicle exit.
fn exit_click() -> Desired {
    Desired {
        target: Target {
            map: MapKind::Player,
            action: Action::EXIT_VEHICLE,
        },
        want: Want::Click,
    }
}

static SEAT_TEAR: Mutex<Option<SeatTear>> = Mutex::new(None);

/// How long after asking for the seat exit it is asked again if the player is still seated.
const SEAT_EXIT_RETRY: Duration = Duration::from_millis(1500);
/// How long a tear from the seat waits for the player to leave it.
const SEAT_EXIT_TIMEOUT: Duration = Duration::from_secs(4);

/// Release every retained gun, for a paused game, a mode change, or a shutdown.
pub(crate) unsafe fn stand_down() {
    gunner_seat::reset();
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

/// The mounted gun whose handles or pivot are nearest `hands`, within `reach_m`, for a gun the
/// player is not in the native mounted state of: its handles can be taken and the runtime can
/// drive it. The nearest gun wins; a gun with no readable handles uses its pivot.
///
/// # Safety
///
/// The game thread, before the world update; the hands are world-space points.
pub(crate) unsafe fn nearby(hands: [Option<Vec3>; 2], reach_m: f32) -> Option<MountedGun> {
    let reach_m = reach_m.max(0.0);
    if !reach_m.is_finite() {
        return None;
    }
    let seen: Vec<(usize, Weak)> = SEEN
        .lock()
        .iter()
        .map(|record| (record.object, record.weak.clone()))
        .collect();
    let mut best: Option<(f32, MountedGun)> = None;
    for (object, weak) in seen {
        let Some(alive) = weak
            .lock()
            .filter(|alive| alive.object() as usize == object)
        else {
            continue;
        };
        let weapon = unsafe { &*(weapon_at(alive.object())) };
        if weapon.m_State != WeaponState::E_WS_MOUNTED || native::lifecycle_pending(weapon) {
            continue;
        }
        let Some((pivot, barrel)) = (unsafe { pivot_and_barrel(weapon) }) else {
            continue;
        };
        let handles = Hand::BOTH.map(|hand| unsafe { handle(weapon, hand) });
        let points: Vec<Vec3> = handles
            .iter()
            .flatten()
            .copied()
            .chain(std::iter::once(pivot))
            .collect();
        let distance = hands
            .iter()
            .flatten()
            .flat_map(|hand| points.iter().map(move |point| hand.distance(*point)))
            .fold(f32::INFINITY, f32::min);
        if distance <= reach_m && best.is_none_or(|(best, _)| distance < best) {
            best = Some((
                distance,
                MountedGun {
                    address: weapon as *const WeaponBase as usize,
                    pivot,
                    barrel,
                    handles,
                },
            ));
        }
    }
    best.map(|(_, gun)| gun)
}

/// Whether the runtime holds `weapon`, so the game's native start-using must not run.
pub(crate) fn suppress_start_using(weapon: &WeaponBase) -> bool {
    // Entering a vehicle's gunner seat starts the native use, which the seat needs.
    // SAFETY: the game thread, where the native start-using runs.
    if unsafe { Character::GetLocalPlayerCharacter().as_ref() }
        .is_some_and(|local| unsafe { local.IsAttachedToVehicle() })
    {
        return false;
    }
    let address = weapon as *const WeaponBase as usize;
    let control = weapon.m_WeakThis.pn as usize;
    // The gun the input tick selected is the one the hands are on: the use prompt must not start
    // the native state the runtime would only take over. A retained gun is refused as well.
    is_mounted_weapon(address)
        || CONTROLLED.lock().iter().any(|record| {
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
