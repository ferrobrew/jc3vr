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
//! - a free lid (a hood, a trunk, a hatch) above [`HingeTuning::strut_fraction`] of its travel is
//!   held where it is against its weight, as a gas strut holds it, and falls freely below it;
//! - a free door latches only when it swings shut faster than [`HingeTuning::latch_speed_deg_s`], or
//!   when it rests shut for [`HingeTuning::settle_s`], through the controller's own merge
//!   ([`engine::latch`]).
//!
//! A door the game turns with its motor (an entry or exit animation, the AI, a script) is the
//! game's again: it leaves the registry with the controller's latch restored.

use std::num::NonZeroUsize;

use glam::Vec3;
use jc3gi::vehicle::vehicle::{DoorInfo, DoorSlot};
use jc3vr_body::controllers::physics::{
    bodies,
    bodies::BodyState,
    rigid::{BodyPose, Velocities},
    vehicles,
};
use parking_lot::Mutex;

use crate::controllers::physical_hands::doors::{
    engine, table,
    tuning::{CanopyTuning, DOORS, HingeTuning},
};

/// Note that a hand undid the latch of the door at part index `part_index` of `vehicle`, whose body
/// `vehicle_body` proves it alive.
pub(crate) fn opened(vehicle: NonZeroUsize, vehicle_body: u32, part_index: u32, slot: DoorSlot) {
    let mut doors = OPENED.lock();
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
        lid: None,
        canopy: false,
        support: CanopySupport::Free,
        support_body: None,
        angle_rad: None,
        rested_s: 0.0,
        strut: false,
    });
}

pub(crate) fn yield_support(vehicle: NonZeroUsize, part_index: u32) {
    if let Some(door) = OPENED
        .lock()
        .iter_mut()
        .find(|door| door.vehicle == vehicle && door.part_index == part_index)
    {
        door.support.yield_for_input(&DOORS.get().hinge.canopy);
    }
}

/// Keep the doors the hands opened, for a palm tick of `dt` game seconds in which the hands hold
/// the bodies `held`. While the hands are not `active`, every door gets the controller's latch back.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub(crate) unsafe fn tick(dt: f32, held: &[u32], active: bool, gravity_mps2: f32) {
    let mut doors = OPENED.lock();
    if !active {
        for door in doors.drain(..) {
            // SAFETY: the vehicle is proven alive by its body before it is touched.
            if unsafe { vehicles::owns_body(door.vehicle, door.vehicle_body) } {
                unsafe { engine::set_native_latch(door.vehicle, door.part_index, true) };
            }
        }
        return;
    }
    let config = &DOORS.get().hinge;
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
    pub(crate) lid: bool,
    /// The door's angle above the hinge's smallest, radians.
    pub angle_rad: f32,
    /// The hinge's travel, radians.
    pub(crate) range_rad: f32,
    /// The angle's rate, radians per second, opening positive; `None` on the first tick.
    pub(crate) rate_rad_s: Option<f32>,
    /// How long the door has rested within the latch angle, seconds.
    pub(crate) rested_s: f32,
    pub dt: f32,
}

/// What the hinge of a door does this tick.
pub(crate) fn swing(tick: &HingeTick, config: &HingeTuning) -> Swing {
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
        .is_some_and(|rate| rate > config.strut_rising_rad_s)
    {
        Swing::Damped(damping)
    } else {
        Swing::Strut
    }
}

/// Whether a slot is a lid, hinged across the vehicle so that its weight swings it shut.
pub(crate) fn is_lid(slot: DoorSlot, part_hash: Option<u32>) -> bool {
    table::is_canopy(part_hash)
        || matches!(
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
    lid: Option<bool>,
    canopy: bool,
    support: CanopySupport,
    support_body: Option<(NonZeroUsize, u32)>,
    /// The angle on the last tick, radians.
    angle_rad: Option<f32>,
    /// How long it has rested within the latch angle, seconds.
    rested_s: f32,
    /// Whether the strut held it on the last tick.
    strut: bool,
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum CanopySupport {
    Free,
    Supported { pose: Option<BodyPose> },
    Yielding { remaining_s: f32, quiet_s: f32 },
}

impl CanopySupport {
    fn yield_for_input(&mut self, canopy: &CanopyTuning) {
        *self = Self::Yielding {
            remaining_s: canopy.yield_s,
            quiet_s: 0.0,
        };
    }

    fn swing(&mut self, tick: &HingeTick, config: &HingeTuning) -> Swing {
        if tick.held {
            *self = Self::Free;
            return Swing::Held;
        }
        if let Self::Yielding {
            remaining_s,
            quiet_s,
        } = self
        {
            let dt = tick.dt.max(0.0);
            *remaining_s = (*remaining_s - dt).max(0.0);
            if tick
                .rate_rad_s
                .is_some_and(|rate| rate.abs() < config.resting_rad_s)
            {
                *quiet_s += dt;
            } else {
                *quiet_s = 0.0;
            }
            if *remaining_s > 0.0 || *quiet_s < config.canopy.quiet_s {
                return match swing(tick, config) {
                    latch @ Swing::Latch(_) => latch,
                    _ => Swing::Free,
                };
            }
            *self = Self::Free;
        }
        if matches!(self, Self::Supported { .. }) {
            if tick
                .rate_rad_s
                .is_some_and(|rate| rate < -config.strut_rising_rad_s)
            {
                self.yield_for_input(&config.canopy);
                return Swing::Free;
            }
            return Swing::Strut;
        }
        let result = swing(tick, config);
        if result == Swing::Strut {
            if tick
                .rate_rad_s
                .is_some_and(|rate| rate < -config.strut_rising_rad_s)
            {
                self.yield_for_input(&config.canopy);
                return Swing::Free;
            }
            *self = Self::Supported { pose: None };
        }
        result
    }

    fn observe_body(
        &mut self,
        previous: &mut Option<(NonZeroUsize, u32)>,
        current: Option<(NonZeroUsize, u32)>,
    ) -> bool {
        let changed = previous.is_some() && *previous != current;
        if changed {
            *self = Self::Free;
        }
        *previous = current;
        changed
    }

    fn velocities(
        &mut self,
        door: Option<&BodyState>,
        chassis: Option<&BodyState>,
        dt: f32,
        g: f32,
        canopy: &CanopyTuning,
    ) -> Option<Velocities> {
        let (Some(door), Some(chassis)) = (door, chassis) else {
            *self = Self::Free;
            return None;
        };
        let mut velocities = hinge_velocities(door, chassis, Swing::Strut, dt, g)?;
        let correction = self.correction(door, chassis, dt, canopy);
        velocities.linear += correction.linear;
        velocities.angular += correction.angular;
        Some(velocities)
    }

    fn correction(
        &mut self,
        door: &BodyState,
        chassis: &BodyState,
        dt: f32,
        canopy: &CanopyTuning,
    ) -> Velocities {
        let Self::Supported { pose } = self else {
            return Velocities::default();
        };
        let frame = chassis.pose.orientation;
        let target = pose.get_or_insert_with(|| BodyPose {
            centre: frame.inverse() * (door.pose.centre - chassis.pose.centre),
            orientation: frame.inverse() * door.pose.orientation,
        });
        if dt <= 0.0 {
            return Velocities::default();
        }
        let gain = canopy.restore_per_s.min(dt.recip());
        let displacement = chassis.pose.centre + frame * target.centre - door.pose.centre;
        let rotation = frame * target.orientation * door.pose.orientation.inverse();
        let rotation = if rotation.w < 0.0 {
            -rotation
        } else {
            rotation
        };
        Velocities {
            linear: (displacement * gain).clamp_length_max(canopy.restore_mps),
            angular: (rotation.to_scaled_axis() * gain).clamp_length_max(canopy.restore_rad_s),
        }
    }
}

/// The doors the hands opened.
static OPENED: Mutex<Vec<Opened>> = Mutex::new(Vec::new());

/// Keep one door for a tick; `false` drops it from the registry.
///
/// # Safety
///
/// The game thread, outside the physics step.
unsafe fn keep(door: &mut Opened, dt: f32, held: &[u32], config: &HingeTuning, g: f32) -> bool {
    if !unsafe { vehicles::owns_body(door.vehicle, door.vehicle_body) } {
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
    let body = hinge.body;
    if door.support.observe_body(&mut door.support_body, body) {
        door.angle_rad = None;
        door.rested_s = 0.0;
    }
    let angle = unsafe { engine::open_angle(door.vehicle, door.part_index) };
    let rate = door
        .angle_rad
        .filter(|_| dt > 0.0)
        .map(|previous| (angle - previous) / dt);
    door.angle_rad = Some(angle);
    let resting = angle < DoorInfo::LATCH_ANGLE_DEG.to_radians()
        && rate.is_some_and(|rate| rate.abs() < config.resting_rad_s);
    door.rested_s = if resting { door.rested_s + dt } else { 0.0 };
    if door.lid.is_none() {
        let part_hash =
            unsafe { engine::door(door.vehicle, door.part_index) }.and_then(|d| d.part_hash);
        door.canopy = table::is_canopy(part_hash);
        door.lid = Some(is_lid(door.slot, part_hash));
    }
    let tick = HingeTick {
        held: body.is_some_and(|(_, body)| held.contains(&body)),
        lid: door.lid.unwrap_or(false),
        angle_rad: angle,
        range_rad: hinge.range_rad,
        rate_rad_s: rate,
        rested_s: door.rested_s,
        dt,
    };
    let swing = if door.canopy {
        door.support.swing(&tick, config)
    } else {
        swing(&tick, config)
    };
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
        Swing::Damped(_) | Swing::Strut => {
            let Some((owner, body)) = body else {
                return true;
            };
            let read = |body, owner| {
                unsafe { bodies::read(body, owner) }
                    .inspect_err(|error| {
                        tracing::debug!(
                            target: "controllers",
                            slot = ?door.slot,
                            body,
                            %error,
                            "physical hands: doors: a swinging door's body could not be read"
                        );
                    })
                    .ok()
            };
            let chassis = read(door.vehicle_body, door.vehicle);
            let state = read(body, owner);
            let velocities = if door.canopy && swing == Swing::Strut {
                door.support
                    .velocities(state.as_ref(), chassis.as_ref(), dt, g, &config.canopy)
            } else {
                state
                    .as_ref()
                    .zip(chassis.as_ref())
                    .and_then(|(state, chassis)| hinge_velocities(state, chassis, swing, dt, g))
            };
            if let Some(velocities) = velocities
                && let Err(error) = unsafe { bodies::drive(body, owner, velocities) }
            {
                tracing::debug!(
                    target: "controllers",
                    slot = ?door.slot,
                    body,
                    %error,
                    "physical hands: doors: a swinging door could not be driven"
                );
            }
            true
        }
    }
}

fn hinge_velocities(
    door: &BodyState,
    chassis: &BodyState,
    swing: Swing,
    dt: f32,
    g: f32,
) -> Option<Velocities> {
    let (factor, gravity) = match swing {
        Swing::Damped(factor) => (factor, Vec3::ZERO),
        Swing::Strut => (0.0, Vec3::Y * g.max(0.0) * dt.max(0.0)),
        Swing::Held | Swing::Free | Swing::Latch(_) => return None,
    };
    let carrier = Velocities {
        linear: chassis.velocities.linear
            + chassis
                .velocities
                .angular
                .cross(door.pose.centre - chassis.pose.centre),
        angular: chassis.velocities.angular,
    };
    Some(Velocities {
        linear: carrier.linear + (door.velocities.linear - carrier.linear) * factor + gravity,
        angular: carrier.angular + (door.velocities.angular - carrier.angular) * factor,
    })
}

#[cfg(test)]
mod tests;
