//! What the snatch does with the target it fired at: frees a gun from a character, fires the hook,
//! opens a door the hook took hold of, pulls a weapon or an object in along an arc, and hands it to
//! the hand on arrival; and the toss a tap of the grapple gives a gun.

use std::num::NonZeroUsize;

use glam::Vec3;
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::WeaponBase,
};

use crate::controllers::{
    aim::{
        ray::Ray,
        raycast::{self, CastOptions},
    },
    frame::Hand,
    hands::ik,
    haptics::{self, Sustain},
    physical_hands::{
        self,
        doors::{DoorTarget, engine::vehicle_frame},
        grasp::GripTarget as HoldPoint,
        hold::{self, HandHold, HoldTuning},
        remote,
    },
    snatch::{
        FinishInput, Refusal, engine,
        rules::{self, PullLimits, StallWatch, Verdict},
        target::{Offer, OfferKind},
    },
    weapons::hold::{
        Cause,
        pickup::{self, TakeOptions},
        refs::{Strong, Weak},
        steal,
    },
};

/// What the snatch fired at, held alive from the fire to the end.
pub(super) struct Grabbed {
    /// The physics game object the hook goes to.
    pub object: NonZeroUsize,
    pub kind: Pulled,
    _alive: Strong,
}

/// How a grabbed thing comes.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) enum Pulled {
    /// A weapon lying in the world, into the hand through the table.
    Weapon,
    /// A loose object, body `body` of `mass_kg`, into the hand's ordinary hold.
    Prop { body: u32, mass_kg: f32 },
    /// A vehicle's closed door, pulled open at its handle, `handle` in the vehicle's frame.
    Door { door: DoorTarget, handle: Vec3 },
}

/// What a fire at an armed target started.
pub(super) enum Fired {
    /// A character let go of its gun; the hook fires once the gun lies in the world.
    Freeing(Grabbed),
    /// The hook flies.
    HookOut(Grabbed, NonZeroUsize),
}

/// Fire at the armed target: free a gun from a character's hands or holster first, or fire the
/// hook at a weapon, an object, or a door.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(super) unsafe fn fire_at(hand: Hand, offer: Offer) -> Result<Fired, Refusal> {
    let freed = match offer.kind {
        OfferKind::Ground { weapon, weak, .. } => {
            let grabbed = Grabbed {
                object: weapon,
                kind: Pulled::Weapon,
                _alive: weak.lock().ok_or(Refusal::Gone)?,
            };
            let (dropped, _) = unsafe { engine::dropped(weapon) }?;
            // What the hand could not take is refused before the hook flies.
            let player = unsafe { Character::GetLocalPlayerCharacter().as_ref() }
                .ok_or(Refusal::NoPlayer)?;
            unsafe { pickup::takeable(player, dropped) }.map_err(Refusal::Take)?;
            unsafe { pickup::route(dropped, Some(hand)) }.map_err(Refusal::Take)?;
            let wire = unsafe { fire_hook(&grabbed) }?;
            return Ok(Fired::HookOut(grabbed, wire));
        }
        OfferKind::Prop {
            object,
            body,
            mass_kg,
            weak,
        } => {
            let grabbed = Grabbed {
                object,
                kind: Pulled::Prop { body, mass_kg },
                _alive: weak.lock().ok_or(Refusal::Gone)?,
            };
            let wire = unsafe { fire_hook(&grabbed) }?;
            return Ok(Fired::HookOut(grabbed, wire));
        }
        OfferKind::Door { door, handle, weak } => {
            let alive = weak.lock().ok_or(Refusal::Gone)?;
            let local = unsafe { vehicle_frame(door.vehicle) }
                .inverse()
                .transform_point3(handle);
            let grabbed = Grabbed {
                object: door.vehicle,
                kind: Pulled::Door {
                    door,
                    handle: local,
                },
                _alive: alive,
            };
            let wire = unsafe { fire_hook(&grabbed) }?;
            return Ok(Fired::HookOut(grabbed, wire));
        }
        OfferKind::Held(target) => {
            unsafe { steal::release(hand, &target) }.map_err(Refusal::Free)?
        }
        OfferKind::Holstered(target) => {
            unsafe { steal::release_holstered(hand, &target) }.map_err(Refusal::Free)?
        }
    };
    let object = NonZeroUsize::new(freed.weapon as usize).ok_or(Refusal::Gone)?;
    tracing::info!(target: "controllers", hand = hand.label(), "snatch: the character let go of its gun");
    Ok(Fired::Freeing(Grabbed {
        object,
        kind: Pulled::Weapon,
        _alive: freed.alive,
    }))
}

/// Fire the hook at what `grabbed` holds.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(super) unsafe fn fire_hook(grabbed: &Grabbed) -> Result<NonZeroUsize, Refusal> {
    match grabbed.kind {
        Pulled::Weapon => {
            let (_, body) = unsafe { engine::dropped(grabbed.object) }?;
            let at = unsafe { body_centre(body, grabbed.object) }?;
            unsafe { engine::fire(grabbed.object, Vec3::ZERO, at) }
        }
        Pulled::Prop { body, .. } => {
            let at = unsafe { body_centre(body, grabbed.object) }?;
            unsafe { engine::fire(grabbed.object, Vec3::ZERO, at) }
        }
        Pulled::Door { door, handle } => {
            let at = unsafe { vehicle_frame(door.vehicle) }.transform_point3(handle);
            unsafe { engine::fire(grabbed.object, handle, at) }
        }
    }
}

/// What the hook taking hold of a grabbed thing does.
pub(super) enum Attached {
    /// The pull starts.
    Pull,
    /// The door came open, and the line's work is done.
    Opened,
}

/// The hook took hold of `grabbed`: a door comes open and swings outward; anything else is pulled.
///
/// # Safety
///
/// The main thread, outside the world update and the physics step.
pub(super) unsafe fn attached(grabbed: &Grabbed, swing_mps: f32) -> Result<Attached, Refusal> {
    let Pulled::Door { door, handle } = grabbed.kind else {
        return Ok(Attached::Pull);
    };
    let at = unsafe { vehicle_frame(door.vehicle) }.transform_point3(handle);
    unsafe { remote::swing_open(&door, at, swing_mps) }.map_err(Refusal::Door)?;
    Ok(Attached::Opened)
}

/// How a pulled thing arrived.
pub(super) enum Arrival {
    /// The weapon went into the hand, to this slot.
    Weapon(WeaponSlot),
    /// The object went into the hand's hold.
    Prop,
}

/// One tick of the pull: drive the target toward the hand, and hand it over once it arrives.
///
/// # Safety
///
/// The main thread, outside the world update and the physics step.
pub(super) unsafe fn pull(
    hand: Hand,
    grabbed: &Grabbed,
    wire: NonZeroUsize,
    stall: &mut StallWatch,
    input: &FinishInput,
) -> Result<Option<Arrival>, Refusal> {
    let config = &input.config.weapons.snatch;
    let (reading, line_m) = unsafe { engine::read_wire(wire) };
    if reading == engine::WireReading::Gone {
        return Err(Refusal::LineLost);
    }
    let body = match grabbed.kind {
        Pulled::Weapon => unsafe { engine::dropped(grabbed.object) }?.1,
        Pulled::Prop { body, .. } => body,
        Pulled::Door { .. } => return Err(Refusal::Gone),
    };
    let state =
        unsafe { physical_hands::body_state(body, grabbed.object) }.map_err(Refusal::Body)?;
    let Some(palm) = ik::tick_records().current[hand.index()].map(|t| t.wrist.position) else {
        // An untracked hand keeps the line where it is.
        return Ok(None);
    };
    let centre = state.pose.centre;
    let distance = centre.distance(palm);
    let stalled = stall.step(
        input.dt,
        centre,
        config.stall_window_s,
        config.stall_distance_m,
    );
    let limits = PullLimits {
        capture_radius_m: config.capture_radius_m,
        line_m: line_m.unwrap_or(config.range_m),
    };
    match rules::verdict(distance, stalled, &limits) {
        Verdict::Pulling => {}
        Verdict::Arrived => {
            unsafe { engine::cancel(wire) };
            return unsafe { arrive(hand, grabbed, body, centre, palm, input) }.map(Some);
        }
        Verdict::Stalled => return Err(Refusal::Stalled),
        Verdict::OutOfLine => return Err(Refusal::OutOfLine),
    }
    let mass = state.mass_kg.unwrap_or(1.0);
    let tuning = HoldTuning {
        stiffness_per_s: config.stiffness_per_s.max(0.0),
        angular_stiffness_per_s: 0.0,
        max_speed_mps: config.max_speed_mps.max(0.0),
        max_angular_speed_rps: 0.0,
        hand_force_n: mass * config.max_accel_mps2.max(0.0),
        hand_torque_nm: 0.0,
        two_hand_angular_share: 1.0,
        gravity: Vec3::new(0.0, -GRAVITY_MPS2, 0.0),
    };
    let rigid = state.rigid();
    let mut holds = [None; 2];
    holds[hand.index()] = Some(HandHold {
        anchor: centre,
        target: HoldPoint {
            point: rules::pull_point(centre, palm, config.arc),
            orientation: state.pose.orientation,
        },
        velocity: Vec3::ZERO,
        angular_velocity: rigid.velocities.angular,
    });
    let solution = hold::solve(&rigid, holds, input.dt, &tuning);
    unsafe {
        physical_hands::set_body_velocities(
            body,
            grabbed.object,
            solution.velocities.linear,
            solution.velocities.angular,
        )
    }
    .map_err(Refusal::Body)?;
    haptics::sustain(hand, Sustain::GrappleTension, solution.loads[hand.index()]);
    Ok(None)
}

/// Gravity's acceleration, metres per second squared.
const GRAVITY_MPS2: f32 = 9.81;
/// Where a tossed gun lands, metres in front of the player toward where it came from.
const TOSS_LAND_M: f32 = 1.2;
/// The toss's speed over the ground, metres per second, and its shortest and longest flight,
/// seconds.
const TOSS_SPEED_MPS: f32 = 9.0;
const TOSS_FLIGHT_S: (f32, f32) = (0.45, 1.2);

/// Hand what arrived at the hand over: a weapon into the hand through the table, an object into
/// the hand's hold at its surface nearest the palm.
///
/// # Safety
///
/// The main thread, outside the world update and the physics step.
unsafe fn arrive(
    hand: Hand,
    grabbed: &Grabbed,
    body: u32,
    centre: Vec3,
    palm: Vec3,
    input: &FinishInput,
) -> Result<Arrival, Refusal> {
    match grabbed.kind {
        Pulled::Weapon => {
            let (weapon, _) = unsafe { engine::dropped(grabbed.object) }?;
            let hold = &input.config.weapons.hold;
            let options = TakeOptions {
                drop_replaced: hold.drop_replaced,
                slide_s: if hold.take_slide { hold.snap_s } else { 0.0 },
                cause: Cause::Snatch,
                touch: None,
            };
            unsafe { pickup::take(hand, weapon, options) }
                .map(Arrival::Weapon)
                .map_err(Refusal::Take)
        }
        Pulled::Prop { mass_kg, .. } => {
            let point = unsafe { surface_toward(grabbed.object, centre, palm) };
            remote::hold_remote(hand, grabbed.object, body, point, mass_kg);
            Ok(Arrival::Prop)
        }
        Pulled::Door { .. } => Err(Refusal::Gone),
    }
}

/// The point on `object`'s surface on the line from `palm` to its centre `centre`, or the centre
/// when the line does not find it.
///
/// # Safety
///
/// The main thread, outside the physics step.
unsafe fn surface_toward(object: NonZeroUsize, centre: Vec3, palm: Vec3) -> Vec3 {
    let player = unsafe { Character::GetLocalPlayerCharacter() } as usize;
    let options = CastOptions {
        ignore: [player, 0],
        see_through: false,
        material: false,
    };
    let distance = palm.distance(centre);
    Ray::new(palm, centre - palm)
        .and_then(|ray| {
            unsafe { raycast::cast(&ray, distance, &options) }
                .ok()
                .flatten()
        })
        .filter(|hit| hit.object == Some(object))
        .map_or(centre, |hit| hit.position)
}

/// The centre of mass of `object`'s body `body`, world space.
///
/// # Safety
///
/// The main thread, outside the physics step.
unsafe fn body_centre(body: u32, object: NonZeroUsize) -> Result<Vec3, Refusal> {
    unsafe { physical_hands::body_state(body, object) }
        .map(|state| state.pose.centre)
        .map_err(Refusal::Body)
}

/// Toss the weapon a tap of the grapple reeled in to land in front of the player.
///
/// # Safety
///
/// The main thread, outside the world update and the physics step.
pub(super) unsafe fn toss(weak: &Weak) -> Result<(), Refusal> {
    let alive = weak.lock().ok_or(Refusal::Gone)?;
    let address = alive.object() as usize - WeaponBase::GAME_OBJECT_BASE_OFFSET as usize;
    let weapon = NonZeroUsize::new(address).ok_or(Refusal::Gone)?;
    let (_, body) = unsafe { engine::dropped(weapon) }?;
    let from = unsafe { body_centre(body, weapon) }?;
    let player = unsafe { engine::player_position() }.ok_or(Refusal::NoPlayer)?;
    let toward = Vec3::new(from.x - player.x, 0.0, from.z - player.z)
        .try_normalize()
        .unwrap_or(Vec3::ZERO);
    let land = player + toward * TOSS_LAND_M;
    let flight = rules::toss_flight(
        from.distance(land),
        TOSS_SPEED_MPS,
        TOSS_FLIGHT_S.0,
        TOSS_FLIGHT_S.1,
    );
    let velocity = rules::toss_velocity(from, land, flight, Vec3::new(0.0, -GRAVITY_MPS2, 0.0));
    unsafe { physical_hands::set_body_velocities(body, weapon, velocity, Vec3::ZERO) }
        .map_err(Refusal::Body)
}
