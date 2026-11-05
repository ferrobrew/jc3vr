//! The hinges of the doors the hands opened: what keeps a loose door where it is left.
//!
//! The door controller leaves a loose door on a bare limited hinge, with no friction, and latches it
//! once it swings shut within a couple of degrees at a speed a rebound easily reaches
//! ([`DoorInfo`](jc3gi::vehicle::vehicle::DoorInfo)). So every door the hands opened has the
//! controller's latch switched off ([`engine::set_native_latch`]), and this module stands in for it
//! each palm tick:
//!
//! - a door a hand holds never latches, so the hold keeps it wherever the hand has it;
//! - a free side door is damped, so a door thrown open slows and settles rather than bouncing back
//!   off its stop;
//! - a free lid (a hood, a trunk, a hatch) above [`HingeConfig::strut_fraction`] of its travel is
//!   held where it is against its weight, as a gas strut holds it, and falls freely below it;
//! - a free door latches only when it swings shut faster than [`HingeConfig::latch_speed_deg_s`], or
//!   when it rests shut for [`HingeConfig::settle_s`], through the controller's own merge
//!   ([`engine::latch`]).
//!
//! A door the game turns with its motor (an entry or exit animation, the AI, a script) is the
//! game's again: it leaves the registry with the controller's latch restored.

use std::num::NonZeroUsize;

use glam::Vec3;
use jc3gi::vehicle::vehicle::{DoorInfo, DoorSlot};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::controllers::physical_hands::{bodies, doors::engine, rigid::Velocities};

/// The hinges of the doors the hands opened (see [`crate::controllers::physical_hands::doors::hinge`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HingeConfig {
    pub enabled: bool,
    /// The closing speed past which a free door within the controller's latch angle latches,
    /// degrees per second. The controller's own is a tenth of a typical shove.
    pub latch_speed_deg_s: f32,
    /// How long a free door must rest within the latch angle before it latches, seconds.
    pub settle_s: f32,
    /// The free side door's damping, per second: its velocities fall by `e` each `1 / damping_per_s`
    /// seconds.
    pub damping_per_s: f32,
    /// The share of its travel above which a free lid is held where it is left, `0..=1`.
    pub strut_fraction: f32,
}

impl HingeConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            latch_speed_deg_s: 60.0,
            settle_s: 1.0,
            damping_per_s: 1.5,
            strut_fraction: 0.3,
        }
    }
}

impl Default for HingeConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Note that a hand undid the latch of the door at part index `part_index` of `vehicle`, whose body
/// `vehicle_body` proves it alive.
pub(crate) fn opened(vehicle: NonZeroUsize, vehicle_body: u32, part_index: u32, slot: DoorSlot) {
    let mut doors = DOORS.lock();
    if doors
        .iter()
        .any(|d| d.vehicle == vehicle && d.part_index == part_index)
    {
        return;
    }
    doors.push(Opened {
        vehicle,
        vehicle_body,
        part_index,
        slot,
        angle_rad: None,
        rested_s: 0.0,
        strut: false,
    });
}

/// Keep the doors the hands opened, for a palm tick of `dt` game seconds in which the hands hold
/// the bodies `held`. With `config` off, every door gets the controller's latch back.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub(crate) unsafe fn tick(dt: f32, held: &[u32], config: &HingeConfig, gravity_mps2: f32) {
    let mut doors = DOORS.lock();
    if !config.enabled {
        for door in doors.drain(..) {
            // SAFETY: the vehicle is proven alive by its body before it is touched.
            if unsafe { engine::alive(door.vehicle, door.vehicle_body) } {
                unsafe { engine::set_native_latch(door.vehicle, door.part_index, true) };
            }
        }
        return;
    }
    doors.retain_mut(|door| unsafe { keep(door, dt, held, config, gravity_mps2) });
}

/// What a door's hinge does this tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) enum Swing {
    /// A hand holds it: it goes where the hand takes it, and never latches.
    Held,
    /// It swings freely.
    Free,
    /// Its velocities fall by this factor.
    Damped(f32),
    /// It is held where it is against its weight.
    Strut,
    /// It latches.
    Latch(LatchCause),
}

/// Why a free door latched.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum LatchCause {
    /// It swung shut fast.
    Shoved,
    /// It rested shut.
    Settled,
}

impl LatchCause {
    const fn label(self) -> &'static str {
        match self {
            LatchCause::Shoved => "it swung shut hard",
            LatchCause::Settled => "it came to rest shut",
        }
    }
}

/// One tick of a door's hinge, as [`swing`] reads it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct HingeTick {
    pub held: bool,
    /// Whether the door is a lid, hinged across the vehicle.
    pub lid: bool,
    /// The door's angle above the hinge's smallest, radians.
    pub angle_rad: f32,
    /// The hinge's travel, radians.
    pub range_rad: f32,
    /// The angle's rate, radians per second, opening positive; `None` on the first tick.
    pub rate_rad_s: Option<f32>,
    /// How long the door has rested within the latch angle, seconds.
    pub rested_s: f32,
    pub dt: f32,
}

/// What the hinge of a door does this tick.
pub(crate) fn swing(tick: &HingeTick, config: &HingeConfig) -> Swing {
    if tick.held {
        return Swing::Held;
    }
    let latch_angle = DoorInfo::LATCH_ANGLE_DEG.to_radians();
    if tick.angle_rad < latch_angle {
        let closing = tick.rate_rad_s.map_or(0.0, |rate| -rate);
        if closing > config.latch_speed_deg_s.max(0.0).to_radians() {
            return Swing::Latch(LatchCause::Shoved);
        }
        if tick.rested_s >= config.settle_s.max(0.0) {
            return Swing::Latch(LatchCause::Settled);
        }
    }
    let damping = (-config.damping_per_s.max(0.0) * tick.dt.max(0.0)).exp();
    if !tick.lid {
        return Swing::Damped(damping);
    }
    let strut_angle = config.strut_fraction.clamp(0.0, 1.0) * tick.range_rad;
    if tick.angle_rad < strut_angle {
        return Swing::Free;
    }
    // A lid thrown open carries on up, damped; one at rest or coming down stays where it is.
    if tick
        .rate_rad_s
        .is_some_and(|rate| rate > STRUT_RISING_RAD_S)
    {
        Swing::Damped(damping)
    } else {
        Swing::Strut
    }
}

/// Whether a slot is a lid, hinged across the vehicle so that its weight swings it shut.
pub(crate) const fn is_lid(slot: DoorSlot) -> bool {
    matches!(
        slot,
        DoorSlot::HOOD
            | DoorSlot::TRUNK
            | DoorSlot::REAR_DOOR
            | DoorSlot::TURRET_DOOR
            | DoorSlot::CARGO_BAY_DOOR
            | DoorSlot::MISSILE_HATCH
    )
}

/// A door the hands opened.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Opened {
    vehicle: NonZeroUsize,
    vehicle_body: u32,
    part_index: u32,
    slot: DoorSlot,
    /// The angle on the last tick, radians.
    angle_rad: Option<f32>,
    /// How long it has rested within the latch angle, seconds.
    rested_s: f32,
    /// Whether the strut held it on the last tick.
    strut: bool,
}

static DOORS: Mutex<Vec<Opened>> = Mutex::new(Vec::new());

/// The fastest a lid may open and still be held by the strut, radians per second; faster, it was
/// thrown open and carries on.
const STRUT_RISING_RAD_S: f32 = 0.2;

/// The slowest a door within the latch angle may swing and still count as resting, radians per
/// second.
const RESTING_RAD_S: f32 = 0.05;

/// Keep one door for a tick; `false` drops it from the registry.
///
/// # Safety
///
/// The game thread, outside the physics step.
unsafe fn keep(door: &mut Opened, dt: f32, held: &[u32], config: &HingeConfig, g: f32) -> bool {
    if !unsafe { engine::alive(door.vehicle, door.vehicle_body) } {
        return false;
    }
    let Some(hinge) = (unsafe { engine::hinge(door.vehicle, door.part_index) }) else {
        // Latched or broken off by something else; a part loosened again later, such as by a
        // crash, must find the controller's latch in place.
        unsafe { engine::set_native_latch(door.vehicle, door.part_index, true) };
        return false;
    };
    if !hinge.resting_open {
        // The game turns it with its motor now; its latch is its own again.
        unsafe { engine::set_native_latch(door.vehicle, door.part_index, true) };
        return false;
    }
    unsafe { engine::set_native_latch(door.vehicle, door.part_index, false) };
    let angle = unsafe { engine::open_angle(door.vehicle, door.part_index) };
    let rate = door
        .angle_rad
        .filter(|_| dt > 0.0)
        .map(|previous| (angle - previous) / dt);
    door.angle_rad = Some(angle);
    let resting = angle < DoorInfo::LATCH_ANGLE_DEG.to_radians()
        && rate.is_some_and(|rate| rate.abs() < RESTING_RAD_S);
    door.rested_s = if resting { door.rested_s + dt } else { 0.0 };
    let body = hinge.body;
    let tick = HingeTick {
        held: body.is_some_and(|(_, body)| held.contains(&body)),
        lid: is_lid(door.slot),
        angle_rad: angle,
        range_rad: hinge.range_rad,
        rate_rad_s: rate,
        rested_s: door.rested_s,
        dt,
    };
    let swing = swing(&tick, config);
    let strut = swing == Swing::Strut;
    if strut != door.strut {
        tracing::debug!(
            target: "controllers",
            slot = ?door.slot,
            angle_deg = angle.to_degrees(),
            "physical hands: doors: {}",
            if strut { "the strut holds a lid" } else { "the strut let a lid go" }
        );
        door.strut = strut;
    }
    match swing {
        Swing::Held | Swing::Free => true,
        Swing::Latch(cause) => {
            tracing::info!(
                target: "controllers",
                slot = ?door.slot,
                part = door.part_index,
                speed_deg_s = rate.map(|r| -r.to_degrees()),
                "physical hands: doors: a free door latched: {}",
                cause.label()
            );
            unsafe { engine::latch(door.vehicle, door.part_index) };
            false
        }
        Swing::Damped(factor) => {
            if let Some((owner, body)) = body {
                unsafe { scale_velocities(owner, body, factor) };
            }
            true
        }
        Swing::Strut => {
            if let Some((owner, body)) = body {
                // Still after the step: the step's gravity is taken back before it acts.
                let still = Velocities {
                    linear: Vec3::Y * g.max(0.0) * dt.max(0.0),
                    angular: Vec3::ZERO,
                };
                // A body that cannot be driven this tick is left as it is.
                let _ = unsafe { bodies::drive(body, owner, still) };
            }
            true
        }
    }
}

/// Scale the velocities of `owner`'s body `body` by `factor`.
///
/// # Safety
///
/// The game thread, outside the physics step.
unsafe fn scale_velocities(owner: NonZeroUsize, body: u32, factor: f32) {
    let Ok(state) = (unsafe { bodies::read(body, owner) }) else {
        return;
    };
    let damped = Velocities {
        linear: state.velocities.linear * factor,
        angular: state.velocities.angular * factor,
    };
    // A body that cannot be driven this tick is left as it is.
    let _ = unsafe { bodies::drive(body, owner, damped) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tick() -> HingeTick {
        HingeTick {
            held: false,
            lid: false,
            angle_rad: 1.0,
            range_rad: 1.2,
            rate_rad_s: Some(0.0),
            rested_s: 0.0,
            dt: 1.0 / 30.0,
        }
    }

    const CONFIG: HingeConfig = HingeConfig::new();

    #[test]
    fn a_held_door_never_latches() {
        let held = HingeTick {
            held: true,
            angle_rad: 0.0,
            rate_rad_s: Some(-10.0),
            rested_s: 10.0,
            ..tick()
        };
        assert_eq!(swing(&held, &CONFIG), Swing::Held);
    }

    #[test]
    fn a_rebound_does_not_latch_but_a_shove_does() {
        // The controller's own latch would take a door closing at a tenth of this.
        let rebound = HingeTick {
            angle_rad: 0.01,
            rate_rad_s: Some(-(CONFIG.latch_speed_deg_s * 0.5).to_radians()),
            ..tick()
        };
        assert!(matches!(swing(&rebound, &CONFIG), Swing::Damped(_)));
        let shove = HingeTick {
            rate_rad_s: Some(-(CONFIG.latch_speed_deg_s * 2.0).to_radians()),
            ..rebound
        };
        assert_eq!(swing(&shove, &CONFIG), Swing::Latch(LatchCause::Shoved));
        // Fast but well open is just a swing.
        let swinging = HingeTick {
            angle_rad: 0.5,
            ..shove
        };
        assert!(matches!(swing(&swinging, &CONFIG), Swing::Damped(_)));
    }

    #[test]
    fn a_door_resting_shut_latches_after_a_while() {
        let resting = HingeTick {
            angle_rad: 0.01,
            rested_s: CONFIG.settle_s * 0.5,
            ..tick()
        };
        assert!(matches!(swing(&resting, &CONFIG), Swing::Damped(_)));
        let settled = HingeTick {
            rested_s: CONFIG.settle_s,
            ..resting
        };
        assert_eq!(swing(&settled, &CONFIG), Swing::Latch(LatchCause::Settled));
    }

    #[test]
    fn a_side_door_is_damped() {
        let Swing::Damped(factor) = swing(&tick(), &CONFIG) else {
            panic!("a free side door is damped");
        };
        assert!(factor > 0.9 && factor < 1.0, "{factor}");
    }

    #[test]
    fn a_lid_stays_up_above_the_strut_and_falls_below_it() {
        let lid = HingeTick {
            lid: true,
            ..tick()
        };
        assert_eq!(swing(&lid, &CONFIG), Swing::Strut);
        let falling = HingeTick {
            rate_rad_s: Some(-1.0),
            ..lid
        };
        assert_eq!(swing(&falling, &CONFIG), Swing::Strut);
        let thrown = HingeTick {
            rate_rad_s: Some(2.0),
            ..lid
        };
        assert!(matches!(swing(&thrown, &CONFIG), Swing::Damped(_)));
        let low = HingeTick {
            angle_rad: CONFIG.strut_fraction * lid.range_rad * 0.5,
            ..lid
        };
        assert_eq!(swing(&low, &CONFIG), Swing::Free);
    }

    #[test]
    fn the_lids_are_the_slots_hinged_across_the_vehicle() {
        assert!(is_lid(DoorSlot::HOOD));
        assert!(is_lid(DoorSlot::TRUNK));
        assert!(!is_lid(DoorSlot::FRONT_LEFT));
        assert!(!is_lid(DoorSlot::REAR_RIGHT));
    }
}
