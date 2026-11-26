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
use jc3vr_body::controllers::{
    hands,
    held::native::weapon_at,
    intent::hold::{Desired, MapKind, Target, Want},
    physics::{bodies::physics_object_of, refs::Weak},
    scheme,
};
use jc3vr_input::frame::Hand;
use jc3vr_view::controllers::cockpit::CockpitTransform;
use parking_lot::Mutex;

use crate::controllers::{
    loadout::{Delivery, TakeSource},
    physical_hands,
    physical_hands::{config::PHYSICAL_HANDS_CONFIG, doors::engine::vehicle_of as resolve_vehicle},
    vehicles::{
        is_mounted_weapon,
        mounted::{
            Held, Mount, MountedGun,
            control::{Identity, Input, Tear},
            gunner_seat, handle, mount_matches, native,
            native::{Motor, Prompt},
            pivot_and_barrel,
            vehicle_hold::VehicleHold,
        },
        summary as vehicles_summary,
    },
    weapons::{
        FinishInputs,
        engine::{character_object, object_of},
        hold::{armed, pickup},
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
        // A gun forgotten keeps no hidden prompt.
        if let Some(prompt) = &mut seen.remove(0).prompt {
            unsafe { prompt.restore() };
        }
    }
    seen.push(Seen {
        object,
        control,
        weak: unsafe { Weak::of(&weapon.m_WeakThis) },
        prompt: None,
    });
}

/// Retain the mounted weapons the physical hands grip, and release the rest.
///
/// # Safety
/// The game thread, after the physical hands and weapon handling settled this tick.
pub(crate) unsafe fn tick(inputs: &FinishInputs, desired: &mut Vec<Desired>) {
    let enabled = scheme::is_controllers()
        && !jc3vr_core::is_shutting_down()
        && scheme::enabled()
        && PHYSICAL_HANDS_CONFIG.query(|config| config.enabled);
    let held = physical_hands::held_bodies();
    let now = Instant::now();
    let mut previous = std::mem::take(&mut *CONTROLLED.lock());
    let mut next = Vec::new();
    let Some(local) =
        (unsafe { Character::GetLocalPlayerCharacter().as_mut() }).filter(|_| enabled)
    else {
        unsafe { release_all(previous) };
        unsafe { hide_standalone_prompts(false) };
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
        let native_hands = if selected { handle_holds } else { [false; 2] };
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
        let grips = grip_state(
            GripInputs {
                native,
                handles: native_hands,
                physical: physical.map(|held| held.is_some()),
                gripping: inputs.grips.map(|grip| grip.held()),
                unavailable: inputs.unavailable,
            },
            previous_takeover,
        );
        let Some(input) = grips.input(identity, inputs.triggers.map(|trigger| trigger.held()), now)
        else {
            continue;
        };
        let hands = input.hands;
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
        record.takeover = grips.takeover;
        record.input = Some(input);
        // A hand holding the gun's own body swings it on its joints, which the mount's motors
        // would fight. A grip on the handles aims the gun through those motors, which the mounted
        // aim drives toward the handles' aim point: disabled for it, nothing turns the gun at all.
        for motor in &record.motors {
            if physical_body.is_some() {
                unsafe { motor.disable() };
            } else {
                unsafe { motor.restore() };
            }
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
            if grips.handles[i] {
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
            unsafe { tear_off(local, weapon, hand, hold) };
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
        record.takeover = None;
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
    unsafe { hide_standalone_prompts(true) };
}

/// Hide the use prompt of every standalone mounted gun seen, or put back the ones this hid: under
/// the controllers the hands take a gun by its handles, and the prompt's native use is a state
/// that is hard to leave. A vehicle's gun keeps its prompt, as the gunner seat needs the native
/// use, and the gamepad scheme keeps every prompt.
///
/// # Safety
/// The game thread, after the runtime's tick.
unsafe fn hide_standalone_prompts(hidden: bool) {
    let mut seen = SEEN.lock();
    for record in seen.iter_mut() {
        let Some(alive) = record
            .weak
            .lock()
            .filter(|alive| alive.object() as usize == record.object)
        else {
            continue;
        };
        if !hidden {
            if let Some(prompt) = &mut record.prompt {
                unsafe { prompt.restore() };
            }
            continue;
        }
        let standalone = NonZeroUsize::new(record.object)
            .is_none_or(|object| unsafe { resolve_vehicle(object) }.is_none());
        if !standalone {
            continue;
        }
        if record.prompt.is_none() {
            let weapon = unsafe { &mut *weapon_at(alive.object()) };
            record.prompt = unsafe { native::prompt(weapon) };
        }
        if let Some(prompt) = &mut record.prompt {
            unsafe { prompt.suppress() };
        }
    }
}

/// Tear `weapon` off its mount into `hand`: release its fire, detach it through the weapon's own
/// path, free it from its mount's hinges, release the vehicle's hold on it where a vehicle carries
/// it, and take it into the hand. A gun the table refuses falls where the tear left it, with its
/// pickup offered.
///
/// # Safety
/// The game thread, outside the world update; the character and the weapon are live, the weapon
/// mounted and without a user.
unsafe fn tear_off(
    local: &mut Character,
    weapon: &mut WeaponBase,
    hand: Hand,
    hold: Option<VehicleHold>,
) {
    let admitted = unsafe { pickup::preflight(&*local, &*weapon, hand, false) };
    unsafe { armed::release_fire(weapon) };
    let constraints = unsafe { native::mount_constraints(weapon) };
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
    // The drop leaves the mount's hinges enabled, which kept a torn gun on its tripod; with them
    // disabled it falls. The drop also skips the pickup off a mount, which the dropped gun gets
    // below.
    let freed = unsafe { native::free_from_mount(&constraints) };
    tracing::info!(
        target: "controllers",
        from_vehicle = hold.is_some(),
        freed,
        "mounted gun: the pull tore the gun off"
    );
    if weapon.m_State != WeaponState::E_WS_DROPPED {
        return;
    }
    unsafe { weapon.PickupEnable() };
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
        // The game's use-item action is not a fallback: it picks up the nearest pickup, not the
        // torn gun, and its slot swap ejected the player's own weapons on the 14 November take.
        // The gun stays where the tear left it.
        tracing::info!(
            target: "controllers",
            %error,
            "mounted gun: the table did not take the torn gun, which stays where it is"
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
    unsafe { tear_off(local, weapon, hand, Some(hold)) };
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
    /// The use prompt, once looked up: a standalone gun's is hidden under the controllers.
    prompt: Option<Prompt>,
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

struct GripInputs {
    native: bool,
    handles: [bool; 2],
    physical: [bool; 2],
    gripping: [bool; 2],
    unavailable: [bool; 2],
}

struct GripState {
    hands: [bool; 2],
    handles: [bool; 2],
    takeover: Option<Takeover>,
}

impl GripState {
    fn input(&self, identity: Identity, fire: [bool; 2], at: Instant) -> Option<Input> {
        self.hands.contains(&true).then_some(Input {
            identity,
            hands: self.hands,
            fire,
            at,
        })
    }
}

fn grip_state(inputs: GripInputs, previous: Option<Takeover>) -> GripState {
    let takeover = Hand::BOTH.map(|hand| {
        let i = hand.index();
        (inputs.native || previous.is_some_and(|takeover| takeover.hands[i]))
            && !inputs.physical[i]
            && !inputs.unavailable[i]
    });
    let handles = Hand::BOTH.map(|hand| {
        let i = hand.index();
        (takeover[i] || (inputs.handles[i] && inputs.gripping[i]))
            && !inputs.physical[i]
            && !inputs.unavailable[i]
    });
    GripState {
        hands: Hand::BOTH.map(|hand| {
            let i = hand.index();
            (inputs.physical[i] || handles[i]) && !inputs.unavailable[i]
        }),
        handles,
        takeover: takeover
            .contains(&true)
            .then_some(Takeover { hands: takeover }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn virtual_grip(handles: [bool; 2], gripping: [bool; 2]) -> GripInputs {
        GripInputs {
            native: false,
            handles,
            physical: [false; 2],
            gripping,
            unavailable: [false; 2],
        }
    }

    #[test]
    fn virtual_handles_release_without_firing_or_tearing() {
        let held = grip_state(virtual_grip([true, false], [true, false]), None);
        assert_eq!(held.hands, [true, false]);
        assert_eq!(held.handles, [true, false]);
        assert_eq!(held.takeover, None);
        for (handles, gripping) in [([false; 2], [true; 2]), ([true; 2], [false; 2])] {
            let released = grip_state(virtual_grip(handles, gripping), held.takeover);
            assert_eq!(released.handles, [false; 2]);
            assert_eq!(released.takeover, None);
            assert!(
                released
                    .input(
                        Identity {
                            weapon: 1,
                            control: 2,
                            object: 3,
                            body: 4
                        },
                        [true; 2],
                        Instant::now(),
                    )
                    .is_none()
            );
        }
    }

    #[test]
    fn native_takeover_survives_open_grips_but_not_unavailable_hands() {
        let native = grip_state(
            GripInputs {
                native: true,
                ..virtual_grip([false; 2], [false; 2])
            },
            None,
        );
        let retained = grip_state(virtual_grip([false; 2], [false; 2]), native.takeover);
        assert_eq!(retained.hands, [true; 2]);
        let released = grip_state(
            GripInputs {
                unavailable: [true; 2],
                ..virtual_grip([false; 2], [false; 2])
            },
            retained.takeover,
        );
        assert_eq!(released.hands, [false; 2]);
        assert_eq!(released.takeover, None);
        assert_eq!(
            grip_state(virtual_grip([false; 2], [false; 2]), released.takeover).hands,
            [false; 2]
        );
    }
}
