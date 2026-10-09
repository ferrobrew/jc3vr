//! Vehicle doors by hand: a grip on a closed door's handle area takes hold of the door, a pull
//! outward undoes its latch, and the hand then holds the door on its hinge as it holds any loose
//! object, so that turning or moving the hand swings it. A shove swings it shut, and the game's own
//! door controller latches a door swung shut hard. Hoods, trunks, hatches, and the other opening
//! parts in a vehicle's door controller work the same way. The design is in
//! `docs/mod/input/vehicle-doors.md`, the engine side in `docs/engine/gameplay/vehicle-doors.md`.
//!
//! - [`config`]: the settings;
//! - [`geometry`]: a door's box, its handle area, and the pull on its latch, pure;
//! - [`engine`]: a vehicle's doors as its door controller keeps them, and the latch;
//! - [`scan`]: the doors near the hands, offered by their handles whatever the reach's rays hit.
//!
//! The pickup's reach ([`crate::controllers::physical_hands::pickup::reach`]) asks [`reach`] about
//! every ray hit on a vehicle or a loosened vehicle part. A hit on an open door's own body takes the
//! door as a loose object; a palm at a closed door's handle area, found from the vehicle's door
//! boxes, takes it as latched. The pickup tick ([`crate::controllers::physical_hands::pickup::tick`])
//! keeps a latched grip ([`Latch`]): the hand stays at the grab point on the vehicle ([`LatchStep::Pin`])
//! until it has pulled outward past [`DoorsConfig::unlatch_pull_m`], when the latch gives
//! ([`engine::unlatch`]) and the hold becomes an ordinary one of the door's own body.

pub mod config;
pub mod engine;
pub mod geometry;
pub(crate) mod scan;

use std::num::NonZeroUsize;

pub use config::DoorsConfig;
use glam::{Mat4, Vec3};
use jc3gi::vehicle::vehicle::DoorSlot;

use crate::controllers::physical_hands::{
    contact::SurfaceHit,
    doors::{engine::DoorError, geometry::pull},
    grasp::{Reach, TargetKind},
};

/// A door a hand can take.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct DoorTarget {
    /// The vehicle the door belongs to.
    pub vehicle: NonZeroUsize,
    /// The vehicle's body the reach found, which proves the vehicle alive while the hand grips a
    /// latched door; `None` for a hit on an open door's own body.
    pub vehicle_body: Option<u32>,
    pub part_index: u32,
    pub slot: DoorSlot,
    /// Whether the door is merged into the vehicle's body, latched shut.
    pub latched: bool,
    /// Which way is out of the vehicle at the door, world space.
    pub outward: Vec3,
}

/// What a hand reaches of a vehicle through the ray hit `hit` on `object`'s body `body`, from a
/// palm centred at `palm`: an open door it touches, or a closed door whose handle area the palm is
/// at. `None` when the hit is on neither, so that the pickup treats it as anything else.
///
/// # Safety
///
/// The game thread, outside the physics step; `object` and `body` must come from a cast this tick.
pub(crate) unsafe fn reach(
    object: NonZeroUsize,
    body: u32,
    hit: &SurfaceHit,
    palm: Vec3,
    reach_m: f32,
    config: &DoorsConfig,
) -> Option<Reach> {
    if !config.enabled {
        return None;
    }
    let vehicle = unsafe { engine::vehicle_of(object) }?;
    let doors = unsafe { engine::doors(vehicle) };
    let origin = unsafe { engine::vehicle_frame(vehicle) }.w_axis.truncate();
    if unsafe { engine::is_vehicle_part(object) } {
        // An open door's own body: the door is held where the hand touched it.
        let door = doors
            .iter()
            .find(|d| d.body.is_some_and(|(owner, _)| owner == object))?;
        let (owner, door_body) = door.body?;
        return Some(Reach {
            object: owner,
            body: door_body,
            point: hit.position,
            distance: hit.position.distance(palm),
            normal: hit.normal,
            kind: TargetKind::Door(DoorTarget {
                vehicle,
                vehicle_body: None,
                part_index: door.part_index,
                slot: door.slot,
                latched: false,
                outward: door.door_box.map_or(Vec3::ZERO, |b| b.outward(origin)),
            }),
        });
    }
    // The vehicle's own body: a closed door, taken at its handle area, or an open one the ray
    // reported as the vehicle, taken anywhere.
    doors
        .iter()
        .filter_map(|d| {
            let door_box = d.door_box?;
            let r = door_box.reach(palm, reach_m)?;
            let target = DoorTarget {
                vehicle,
                vehicle_body: Some(body),
                part_index: d.part_index,
                slot: d.slot,
                latched: d.latched,
                outward: door_box.outward(origin),
            };
            let (object, body) = match d.body {
                None if d.latched && r.handle >= config.handle_share.clamp(0.0, 1.0) => {
                    (vehicle, body)
                }
                Some(loose) if !d.latched => loose,
                _ => return None,
            };
            Some(Reach {
                object,
                body,
                point: r.point,
                distance: r.distance,
                normal: target.outward,
                kind: TargetKind::Door(target),
            })
        })
        .min_by(|a, b| a.distance.total_cmp(&b.distance))
}

/// A door slot's name, for the readouts and the log.
pub fn slot_label(slot: DoorSlot) -> &'static str {
    match slot {
        DoorSlot::FRONT_LEFT => "front left",
        DoorSlot::FRONT_RIGHT => "front right",
        DoorSlot::REAR_LEFT => "rear left",
        DoorSlot::REAR_RIGHT => "rear right",
        DoorSlot::CENTER_LEFT => "centre left",
        DoorSlot::CENTER_RIGHT => "centre right",
        DoorSlot::TURRET_DOOR => "turret",
        DoorSlot::TRUNK => "trunk",
        DoorSlot::HOOD => "hood",
        DoorSlot::REAR_DOOR => "rear",
        DoorSlot::CARGO_BAY_DOOR => "cargo bay",
        DoorSlot::FOLDING_WING => "folding wing",
        DoorSlot::MISSILE_HATCH => "missile hatch",
    }
}

/// A hand gripping a latched door, before the latch gives.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Latch {
    pub door: DoorTarget,
    /// The vehicle's body that proves it alive.
    pub vehicle_body: u32,
    /// The grab point in the vehicle's frame.
    pub grab: Vec3,
    /// Which way is out at the door, in the vehicle's frame.
    pub outward: Vec3,
}

impl Latch {
    /// The latched grip of a hand that took `door` at the world point `point`. `None` for a door
    /// found through an open door's body rather than the vehicle's.
    ///
    /// # Safety
    ///
    /// The game thread, outside the physics step; the door's vehicle must be live.
    pub unsafe fn take(door: DoorTarget, point: Vec3) -> Option<Self> {
        let vehicle_body = door.vehicle_body?;
        let to_vehicle = unsafe { engine::vehicle_frame(door.vehicle) }.inverse();
        let grab = to_vehicle.transform_point3(point);
        let outward = to_vehicle
            .transform_vector3(door.outward)
            .normalize_or_zero();
        (grab.is_finite() && outward != Vec3::ZERO).then_some(Self {
            door,
            vehicle_body,
            grab,
            outward,
        })
    }

    /// What the latch does this tick with the palm at `palm`, world space, the latch giving to a
    /// pull of `unlatch_pull_m` outward.
    ///
    /// # Safety
    ///
    /// The game thread, outside the physics step.
    pub unsafe fn step(&self, palm: Vec3, unlatch_pull_m: f32) -> LatchStep {
        let vehicle = self.door.vehicle;
        if !unsafe { engine::alive(vehicle, self.vehicle_body) } {
            return LatchStep::Lost(DoorError::VehicleGone);
        }
        let Some(door) = (unsafe { engine::door(vehicle, self.door.part_index) }) else {
            return LatchStep::Lost(DoorError::NoDoor);
        };
        let world = unsafe { engine::vehicle_frame(vehicle) };
        let decision = latch_decision(world, self, palm, unlatch_pull_m, door.latched);
        match decision {
            LatchDecision::Hold(pin) => LatchStep::Pin(pin),
            LatchDecision::Give(grab) => {
                // SAFETY: forwarded from the caller; the vehicle was checked alive above.
                match unsafe { engine::unlatch(vehicle, self.door.part_index) } {
                    Ok((object, body)) => LatchStep::Opened { object, body, grab },
                    Err(e) => LatchStep::Lost(e),
                }
            }
        }
    }
}

/// What a latched grip does on a tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum LatchStep {
    /// The latch holds: the hand stays at this world point.
    Pin(Vec3),
    /// The latch gave, or the door was opened another way: the hand now holds the door's own body
    /// `body`, owned by `object`, at the world point `grab`.
    Opened {
        object: NonZeroUsize,
        body: u32,
        grab: Vec3,
    },
    /// The grip lost the door.
    Lost(DoorError),
}

/// Whether a latched grip holds or gives, pure.
#[derive(Debug, Copy, Clone, PartialEq)]
enum LatchDecision {
    /// The latch holds; the hand stays at this world point.
    Hold(Vec3),
    /// The latch gives; the grab point is here, world space.
    Give(Vec3),
}

/// Whether `latch` holds on a vehicle at `world` with the palm at `palm`: a door no longer latched
/// gives at once, and a latched one once the palm has pulled `unlatch_pull_m` outward from the grab
/// point.
fn latch_decision(
    world: Mat4,
    latch: &Latch,
    palm: Vec3,
    unlatch_pull_m: f32,
    latched: bool,
) -> LatchDecision {
    let grab = world.transform_point3(latch.grab);
    let outward = world.transform_vector3(latch.outward).normalize_or_zero();
    if !latched || pull(grab, palm, outward) >= unlatch_pull_m.max(0.0) {
        LatchDecision::Give(grab)
    } else {
        LatchDecision::Hold(grab)
    }
}

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    fn latch() -> Latch {
        Latch {
            door: DoorTarget {
                vehicle: NonZeroUsize::new(8).unwrap(),
                vehicle_body: Some(3),
                part_index: 4,
                slot: DoorSlot::FRONT_LEFT,
                latched: true,
                outward: Vec3::NEG_X,
            },
            vehicle_body: 3,
            grab: Vec3::new(-0.95, 1.0, 0.0),
            outward: Vec3::NEG_X,
        }
    }

    #[test]
    fn the_latch_holds_until_the_hand_pulls_out() {
        let world = Mat4::IDENTITY;
        let grab = Vec3::new(-0.95, 1.0, 0.0);
        assert_eq!(
            latch_decision(world, &latch(), grab + Vec3::NEG_X * 0.02, 0.05, true),
            LatchDecision::Hold(grab)
        );
        // Pushing in, or sliding along the door, never opens it.
        assert_eq!(
            latch_decision(world, &latch(), grab + Vec3::new(0.1, 0.0, 0.3), 0.05, true),
            LatchDecision::Hold(grab)
        );
        assert_eq!(
            latch_decision(world, &latch(), grab + Vec3::NEG_X * 0.06, 0.05, true),
            LatchDecision::Give(grab)
        );
    }

    #[test]
    fn a_door_opened_another_way_gives_at_once() {
        let grab = Vec3::new(-0.95, 1.0, 0.0);
        assert_eq!(
            latch_decision(Mat4::IDENTITY, &latch(), grab, 0.05, false),
            LatchDecision::Give(grab)
        );
    }

    #[test]
    fn the_pin_moves_with_the_vehicle() {
        let world = Mat4::from_rotation_translation(
            Quat::from_rotation_y(std::f32::consts::PI),
            Vec3::new(5.0, 0.0, 0.0),
        );
        // Turned half about +Y, the vehicle's left is world +X.
        let grab = world.transform_point3(Vec3::new(-0.95, 1.0, 0.0));
        assert_eq!(
            latch_decision(world, &latch(), grab, 0.05, true),
            LatchDecision::Hold(grab)
        );
        assert!(matches!(
            latch_decision(world, &latch(), grab + Vec3::X * 0.06, 0.05, true),
            LatchDecision::Give(_)
        ));
    }
}
