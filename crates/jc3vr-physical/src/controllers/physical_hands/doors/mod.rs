//! Vehicle doors by hand: a grip on a closed door's handle area takes hold of the door, a pull
//! outward undoes its latch, and the hand then holds the door on its hinge as it holds any loose
//! object, so that turning or moving the hand swings it. A shove swings it shut, and the game's own
//! door controller latches a door swung shut hard. Hoods, trunks, hatches, and the other opening
//! parts in a vehicle's door controller work the same way. The design is in
//! `docs/mod/input/vehicle-doors.md`, the engine side in `docs/engine/gameplay/vehicle-doors.md`.
//!
//! - [`tuning`]: the tuning, from the shipped `tuning/doors.toml`;
//! - [`geometry`]: a door's box, its handle area, its handle, and the pull on its latch, pure;
//! - [`table`]: the door handle table derived offline from the game's files;
//! - [`engine`]: a vehicle's doors as its door controller keeps them, and the latch;
//! - [`acquisition`]: what a palm takes of a door panel a ray on the vehicle's body reported, pure;
//! - [`hinge`]: what keeps a door the hands opened where it is left, and when it latches;
//! - [`scan`]: the doors near the hands, offered by their handles whatever the reach's rays hit;
//! - [`pressure`]: a palm pushing a door open from inside, unlatching a closed one or shoving an
//!   open one;
//! - [`tearing`]: a deliberate tear of a held door off its hinge.
//!
//! The pickup's reach ([`crate::controllers::physical_hands::pickup::reach`]) asks
//! [`observed_reach`] about every ray hit on a vehicle or a loosened vehicle part. A hit on an open
//! door's own body takes the door as a loose object; a palm at a closed door, found from the
//! vehicle's door boxes, takes it as latched, closing on its handle when within the handle reach.
//! The pickup tick ([`crate::controllers::physical_hands::pickup::tick`]) keeps a latched grip
//! ([`Latch`]): the hand stays at the grab point on the vehicle ([`LatchStep::Pin`]) until it has
//! pulled outward past the tuning's `grip.unlatch_pull_m`, when the latch gives
//! ([`engine::unlatch`]) and the hold becomes an ordinary one of the door's own body.

pub(crate) mod acquisition;
pub mod engine;
pub mod geometry;
pub(crate) mod hinge;
pub mod pressure;
pub(crate) mod scan;
pub mod table;
pub(crate) mod tearing;
pub mod tuning;

use std::num::NonZeroUsize;

use glam::{Mat4, Vec3};
use jc3gi::{physics::GameMaterialId, vehicle::vehicle::DoorSlot};
use jc3vr_body::controllers::{
    grab::world_reach::{DoorTarget, Reach, TargetKind},
    physics::vehicles,
};
use jc3vr_core::published::Stamp;
use jc3vr_input::frame::Pose;

use crate::controllers::physical_hands::{
    contact::SurfaceHit,
    doors::{
        acquisition::{PanelBody, PanelReach},
        engine::DoorError,
        geometry::pull,
        tuning::DOORS,
    },
    pickup::diagnostics::scan::{Branch, Scan, ScanEntry, ScanResult},
};

/// What a hand reaches of a vehicle through the ray hit `hit` on `object`'s body `body`, from a
/// palm centred at `palm`: an open door it touches within `reach`'s box reach, or a closed door
/// the palm reaches within `reach`'s box reach, at the handle when the palm is within the handle
/// reach of it ([`acquisition::panel`]). `None` when the hit is on neither, so that the pickup
/// treats it as anything else.
///
/// # Safety
///
/// The game thread, outside the physics step; `object` and `body` must come from a cast this tick.
pub(crate) unsafe fn reach(
    object: NonZeroUsize,
    body: u32,
    hit: &SurfaceHit,
    palm: Vec3,
    reach: PanelReach,
) -> Option<Reach> {
    // SAFETY: forwarded from the caller; the scan record is discarded.
    unsafe { observed_reach(object, body, hit, palm, reach, &mut Scan::new()) }
}

/// [`reach`], with each door panel it evaluates, and why it refused one, recorded in `scan`.
///
/// # Safety
///
/// The game thread, outside the physics step; the hit identifies a live body this tick.
pub(crate) unsafe fn observed_reach(
    object: NonZeroUsize,
    body: u32,
    hit: &SurfaceHit,
    palm: Vec3,
    reach: PanelReach,
    scan: &mut Scan,
) -> Option<Reach> {
    if hit.position.distance(palm) > reach.box_m.max(reach.handle_m) {
        return None;
    }
    let vehicle = unsafe { vehicles::vehicle_of(object) }?;
    let doors = unsafe { engine::doors(vehicle) };
    let origin = unsafe { vehicles::vehicle_frame(vehicle) }
        .w_axis
        .truncate();
    let tuning = &DOORS.get().grip;
    if unsafe { vehicles::is_vehicle_part(object) } {
        // An open door's own body: the door is held where the hand touched it.
        let door = doors
            .iter()
            .find(|d| d.body.is_some_and(|(owner, _)| owner == object))?;
        if !panel_hit(door, hit, tuning.panel_slack_m) {
            return None;
        }
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
    // The vehicle's own body: a door, closed or open, taken anywhere the hand reaches it.
    doors
        .iter()
        .filter(|d| panel_hit(d, hit, tuning.panel_slack_m))
        .filter_map(|d| {
            let mut acquired = acquisition::observed_panel(
                d,
                palm,
                reach,
                tuning.handle_share,
                origin,
                ScanEntry {
                    branch: Branch::DoorPanel,
                    object: Some(vehicle),
                    body: Some(body),
                    door: Some((vehicle, d.slot, d.part_index)),
                    point: None,
                    distance: None,
                    mass_kg: None,
                    result: ScanResult::Offered,
                },
                scan,
            )
            .ok()?;
            if table::is_canopy(d.part_hash) || (palm - hit.position).dot(acquired.outward) < 0.0 {
                acquired.reach.point = hit.position;
                acquired.reach.distance = palm.distance(hit.position);
            }
            let target = DoorTarget {
                vehicle,
                vehicle_body: Some(body),
                part_index: d.part_index,
                slot: d.slot,
                latched: d.latched,
                outward: acquired.outward,
            };
            let (object, body) = match acquired.body {
                PanelBody::LatchedHandle => (vehicle, body),
                PanelBody::OpenPanel { object, body } => (object, body),
            };
            Some(Reach {
                object,
                body,
                point: acquired.reach.point,
                distance: acquired.reach.distance,
                normal: target.outward,
                kind: TargetKind::Door(target),
            })
        })
        .min_by(|a, b| a.distance.total_cmp(&b.distance))
}

/// Whether `hit` is on `door`'s panel: within `slack_m` of its box, and not on glass unless the
/// door is a canopy.
pub(crate) fn panel_hit(door: &engine::DoorRead, hit: &SurfaceHit, slack_m: f32) -> bool {
    (hit.material != Some(GameMaterialId::EGameMaterialId_Glass as u32)
        || table::is_canopy(door.part_hash))
        && door
            .door_box
            .is_some_and(|bounds| bounds.reach(hit.position, slack_m).is_some())
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
    /// The palm at acquisition, in the vehicle's frame, before its opening pull.
    pub palm: Pose,
    /// The first native unlatch attempt, bounding a body-publication retry.
    pub(crate) opening_at: Option<Stamp>,
}

impl Latch {
    /// The latched grip of a hand that took `door` at the world point `point`. `None` for a door
    /// found through an open door's body rather than the vehicle's.
    ///
    /// # Safety
    ///
    /// The game thread, outside the physics step; the door's vehicle must be live.
    pub unsafe fn take(door: DoorTarget, point: Vec3, palm: Pose) -> Option<Self> {
        let vehicle_body = door.vehicle_body?;
        if !unsafe { vehicles::owns_body(door.vehicle, vehicle_body) } {
            return None;
        }
        let world = unsafe { vehicles::vehicle_frame(door.vehicle) };
        Self::at(door, vehicle_body, point, palm, world)
    }

    fn at(
        door: DoorTarget,
        vehicle_body: u32,
        point: Vec3,
        palm: Pose,
        world: Mat4,
    ) -> Option<Self> {
        let to_vehicle = world.inverse();
        let grab = to_vehicle.transform_point3(point);
        let outward = to_vehicle
            .transform_vector3(door.outward)
            .normalize_or_zero();
        let palm = Pose {
            position: to_vehicle.transform_point3(palm.position),
            orientation: (to_vehicle.to_scale_rotation_translation().1 * palm.orientation)
                .normalize(),
        };
        (grab.is_finite()
            && outward != Vec3::ZERO
            && palm.position.is_finite()
            && palm.orientation.is_finite())
        .then_some(Self {
            door,
            vehicle_body,
            grab,
            outward,
            palm,
            opening_at: None,
        })
    }

    /// The acquisition palm carried by the vehicle, without absorbing the opening pull.
    pub(crate) fn handoff_palm(&self, world: Mat4) -> Pose {
        Pose {
            position: world.transform_point3(self.palm.position),
            orientation: (world.to_scale_rotation_translation().1 * self.palm.orientation)
                .normalize(),
        }
    }

    /// The palm closed on the handle, carried by the vehicle: the acquisition palm's orientation at
    /// the grab point. While the latch holds, the hand is held here, so the hand that took the
    /// handle from a few centimetres off closes on it; once the latch gives, the door is held with
    /// its grab point in the palm, so the handle comes to the hand.
    pub(crate) fn handle_palm(&self, world: Mat4) -> Pose {
        Pose {
            position: world.transform_point3(self.grab),
            ..self.handoff_palm(world)
        }
    }

    /// What the latch does this tick with the palm at `palm`, world space, the latch giving to a
    /// pull of `unlatch_pull_m` outward.
    ///
    /// # Safety
    ///
    /// The game thread, outside the physics step.
    pub unsafe fn step(&mut self, palm: Vec3, unlatch_pull_m: f32) -> LatchStep {
        let vehicle = self.door.vehicle;
        if !unsafe { vehicles::owns_body(vehicle, self.vehicle_body) } {
            return LatchStep::Lost(DoorError::VehicleGone);
        }
        let Some(door) = (unsafe { engine::door(vehicle, self.door.part_index) }) else {
            return LatchStep::Lost(DoorError::NoDoor);
        };
        let world = unsafe { vehicles::vehicle_frame(vehicle) };
        let decision = latch_decision(world, self, palm, unlatch_pull_m, door.latched);
        match decision {
            LatchDecision::Hold(pin) => LatchStep::Pin(pin),
            LatchDecision::Give(grab) => {
                let now = Stamp::now();
                let opening_at = *self.opening_at.get_or_insert(now);
                // SAFETY: forwarded from the caller; the vehicle was checked alive above.
                let result = unsafe { engine::unlatch(vehicle, self.door.part_index) };
                match opened_step(
                    result,
                    grab,
                    now.saturating_duration_since(opening_at).as_secs_f32(),
                    DOORS.get().grip.body_wait_s,
                ) {
                    LatchStep::Pin(_) => LatchStep::Pin(self.handle_palm(world).position),
                    step => step,
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

/// What a latch that gave with `result` does, `elapsed_s` after the first attempt, waiting up to
/// `body_wait_s` for the door's loose body.
fn opened_step(
    result: Result<(NonZeroUsize, u32), DoorError>,
    grab: Vec3,
    elapsed_s: f32,
    body_wait_s: f32,
) -> LatchStep {
    match result {
        Ok((object, body)) => LatchStep::Opened { object, body, grab },
        Err(DoorError::NoLooseBody) if elapsed_s < body_wait_s => LatchStep::Pin(grab),
        Err(error) => LatchStep::Lost(error),
    }
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
/// gives at once, and a latched one once the palm has pulled `unlatch_pull_m` outward from its
/// acquisition position.
fn latch_decision(
    world: Mat4,
    latch: &Latch,
    palm: Vec3,
    unlatch_pull_m: f32,
    latched: bool,
) -> LatchDecision {
    let grab = world.transform_point3(latch.grab);
    let acquired_palm = latch.handoff_palm(world).position;
    let outward = world.transform_vector3(latch.outward).normalize_or_zero();
    if !latched || pull(acquired_palm, palm, outward) >= unlatch_pull_m.max(0.0) {
        LatchDecision::Give(grab)
    } else {
        LatchDecision::Hold(grab)
    }
}

#[cfg(test)]
mod tests {
    use glam::Quat;
    use jc3vr_body::controllers::physics::rigid::BodyPose;

    use super::*;
    use crate::controllers::physical_hands::grasp::Grip;

    #[test]
    fn panel_evidence_rejects_car_glass_and_hits_outside_the_door() {
        let mut door = engine::DoorRead {
            slot: DoorSlot::FRONT_LEFT,
            part_index: 1,
            latched: true,
            door_box: geometry::DoorBox::new(Mat4::IDENTITY, Mat4::IDENTITY, Vec3::ZERO, Vec3::ONE),
            body: None,
            part_hash: None,
        };
        let mut hit = SurfaceHit {
            position: Vec3::splat(0.5),
            normal: Vec3::X,
            surface: crate::controllers::physical_hands::contact::Surface::Solid {
                object: NonZeroUsize::new(8),
                body: 3,
            },
            material: None,
        };
        let slack_m = DOORS.get().grip.panel_slack_m;
        assert!(panel_hit(&door, &hit, slack_m));
        hit.material = Some(GameMaterialId::EGameMaterialId_Glass as u32);
        assert!(!panel_hit(&door, &hit, slack_m));
        door.part_hash = table::parse(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tools/door_handles/door_handles.json"
        )))
        .unwrap()
        .into_iter()
        .find(|entry| entry.part == "fighterjet_door_f")
        .map(|entry| entry.part_hash);
        assert!(panel_hit(&door, &hit, slack_m));
        hit.position = Vec3::splat(2.0);
        assert!(!panel_hit(&door, &hit, slack_m));
    }

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
            palm: Pose {
                position: Vec3::new(-0.95, 1.0, 0.0),
                orientation: Quat::IDENTITY,
            },
            opening_at: None,
        }
    }

    #[test]
    fn an_existing_handle_reach_gap_does_not_count_as_an_opening_pull() {
        let mut acquired = latch();
        acquired.palm.position += Vec3::NEG_X * 0.12;
        assert_eq!(
            latch_decision(
                Mat4::IDENTITY,
                &acquired,
                acquired.palm.position,
                0.05,
                true
            ),
            LatchDecision::Hold(acquired.grab)
        );
        // The hand is held on the handle, not where it took the handle from, and the door it
        // opens is held with the handle in the palm.
        let handle = acquired.handle_palm(Mat4::IDENTITY);
        assert_eq!(handle.position, acquired.grab);
        let body = BodyPose {
            centre: acquired.grab + Vec3::Z * 0.4,
            orientation: Quat::IDENTITY,
        };
        let grip = Grip::of(handle, body, acquired.grab);
        let palm = Pose {
            position: Vec3::new(-1.2, 1.1, 0.2),
            orientation: Quat::IDENTITY,
        };
        assert!(grip.target(palm).point.distance(palm.position) < 1e-5);
        assert!(matches!(
            latch_decision(
                Mat4::IDENTITY,
                &acquired,
                acquired.palm.position + Vec3::NEG_X * 0.06,
                0.05,
                true
            ),
            LatchDecision::Give(_)
        ));
    }

    #[test]
    fn a_native_body_wait_is_bounded_and_a_real_unlatch_refusal_is_immediate() {
        let grab = Vec3::X;
        let wait_s = DOORS.get().grip.body_wait_s;
        assert_eq!(
            opened_step(Err(DoorError::NoLooseBody), grab, 0.0, wait_s),
            LatchStep::Pin(grab)
        );
        assert_eq!(
            opened_step(Err(DoorError::NoLooseBody), grab, wait_s, wait_s),
            LatchStep::Lost(DoorError::NoLooseBody)
        );
        assert_eq!(
            opened_step(Err(DoorError::StayedLatched), grab, 0.0, wait_s),
            LatchStep::Lost(DoorError::StayedLatched)
        );
        assert_eq!(
            opened_step(Ok((NonZeroUsize::new(16).unwrap(), 7)), grab, 0.1, wait_s),
            LatchStep::Opened {
                object: NonZeroUsize::new(16).unwrap(),
                body: 7,
                grab
            }
        );
    }

    #[test]
    fn the_loose_door_handoff_retains_the_pull_after_vehicle_transport() {
        let acquired = latch();
        let world =
            Mat4::from_rotation_translation(Quat::from_rotation_y(0.7), Vec3::new(8.0, 2.0, -3.0));
        let grab = world.transform_point3(acquired.grab);
        let palm = acquired.handle_palm(world);
        let body = BodyPose {
            centre: grab + Vec3::Z * 0.3,
            orientation: Quat::IDENTITY,
        };
        let grip = Grip::of(palm, body, grab);
        let pulled = Pose {
            position: palm.position + world.transform_vector3(Vec3::NEG_X) * 0.08,
            ..palm
        };
        assert!(grip.target(pulled).point.distance(grab) > 0.079);
        assert!(matches!(
            latch_decision(world, &acquired, pulled.position, 0.05, true),
            LatchDecision::Give(_)
        ));
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
