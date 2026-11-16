use std::num::NonZeroUsize;

use glam::{Mat4, Vec3};
use jc3gi::vehicle::vehicle::DoorSlot;

use crate::controllers::physical_hands::{
    bodies,
    contact::{Surface, SurfaceHit},
    doors::{
        DoorTarget, UNLATCH_PULL_M,
        engine::{self, DoorError, DoorRead},
        panel_hit,
    },
    grasp::{Reach, TargetKind},
};

#[derive(Debug, Copy, Clone, Default)]
pub(crate) struct Press {
    contact: Option<Contact>,
    open: Option<OpenPress>,
}

impl Press {
    pub(crate) const fn new() -> Self {
        Self {
            contact: None,
            open: None,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.contact = None;
        self.open = None;
    }

    pub(crate) unsafe fn step(
        &mut self,
        reach: Option<Reach>,
        hit: Option<SurfaceHit>,
        palm: Vec3,
        dt: f32,
        world_scale: f32,
    ) -> Result<Option<DoorSlot>, DoorError> {
        if !valid_timing(dt, world_scale) || !palm.is_finite() {
            self.reset();
            return Ok(None);
        }
        if let (Some(reach), Some(hit)) = (reach, hit)
            && let TargetKind::Door(target) = reach.kind
            && !target.latched
        {
            self.contact = None;
            return unsafe {
                self.push_open(OpenInput {
                    reach,
                    hit,
                    target,
                    palm,
                    dt,
                    scale: world_scale,
                })
            };
        }
        self.open = None;
        let Some((reach, hit, target, vehicle_body)) = candidate(reach, hit) else {
            self.reset();
            return Ok(None);
        };
        if !unsafe { engine::alive(target.vehicle, vehicle_body) } {
            self.reset();
            return Err(DoorError::VehicleGone);
        }
        let Some(door) = (unsafe { engine::door(target.vehicle, target.part_index) }) else {
            self.reset();
            return Err(DoorError::NoDoor);
        };
        let world = unsafe { engine::vehicle_frame(target.vehicle) };
        let sample = Sample::new(reach, hit, door, world, palm);
        let Some(opening) = self.intent(sample, dt, world_scale) else {
            return Ok(None);
        };
        let (owner, body) = unsafe { engine::unlatch(target.vehicle, target.part_index) }?;
        let current =
            unsafe { engine::door(target.vehicle, target.part_index) }.ok_or(DoorError::NoDoor)?;
        if !door_body(current.body, owner, body, target.vehicle, vehicle_body) {
            return Err(DoorError::NoLooseBody);
        }
        let state = unsafe { bodies::read(body, owner) }.map_err(|_| DoorError::NoLooseBody)?;
        let impulse = opening_impulse(opening.outward, state.mass_kg, opening.speed);
        if impulse != Vec3::ZERO {
            unsafe { bodies::shove(body, owner, impulse, opening.point) }
                .map_err(|_| DoorError::NoLooseBody)?;
        }
        Ok(Some(target.slot))
    }

    unsafe fn push_open(&mut self, input: OpenInput) -> Result<Option<DoorSlot>, DoorError> {
        let OpenInput {
            reach,
            hit,
            target,
            palm,
            dt,
            scale,
        } = input;
        if hit.surface
            != (Surface::Solid {
                object: Some(reach.object),
                body: reach.body,
            })
            || unsafe { bodies::owner_of(reach.body) } != Some(reach.object)
            || unsafe { engine::vehicle_of(reach.object) } != Some(target.vehicle)
        {
            self.open = None;
            return Ok(None);
        }
        let Some(vehicle_body) = (unsafe { engine::vehicle_body(target.vehicle) }) else {
            self.open = None;
            return Err(DoorError::VehicleGone);
        };
        let Some(door) = (unsafe { engine::door(target.vehicle, target.part_index) }) else {
            self.open = None;
            return Err(DoorError::NoDoor);
        };
        if door.latched
            || door.slot != target.slot
            || !panel_hit(&door, &hit)
            || !door_body(
                door.body,
                reach.object,
                reach.body,
                target.vehicle,
                vehicle_body,
            )
        {
            self.open = None;
            return Ok(None);
        }
        let world = unsafe { engine::vehicle_frame(target.vehicle) };
        let local = world.inverse().transform_point3(palm);
        if !local.is_finite() || !hit.normal.is_finite() {
            self.open = None;
            return Ok(None);
        }
        let next = OpenPress {
            vehicle: target.vehicle,
            part: target.part_index,
            owner: reach.object,
            body: reach.body,
            palm: local,
            scale,
        };
        let previous = self.open.replace(next);
        let Some(previous) = previous.filter(|previous| previous.same(next)) else {
            return Ok(None);
        };
        let displacement = world.transform_vector3(local - previous.palm);
        let speed = push_speed(displacement, hit.normal, dt, scale);
        if speed == 0.0 {
            return Ok(None);
        }
        let state = unsafe { bodies::read(reach.body, reach.object) }
            .map_err(|_| DoorError::NoLooseBody)?;
        let normal = hit.normal.normalize_or_zero();
        let depth = (hit.position - palm).dot(normal);
        if depth < -0.06 * scale {
            return Ok(None);
        }
        let chassis = unsafe { bodies::read(vehicle_body, target.vehicle) }
            .map_err(|_| DoorError::VehicleGone)?;
        let door_velocity = state.velocities.linear
            + state
                .velocities
                .angular
                .cross(hit.position - state.pose.centre);
        let chassis_velocity = chassis.velocities.linear
            + chassis
                .velocities
                .angular
                .cross(hit.position - chassis.pose.centre);
        let existing = (door_velocity - chassis_velocity).dot(-normal);
        let impulse = opening_impulse(
            -normal,
            state.mass_kg,
            (speed - existing).clamp(0.0, 0.25 * scale),
        );
        if impulse != Vec3::ZERO {
            unsafe { bodies::shove(reach.body, reach.object, impulse, hit.position) }
                .map_err(|_| DoorError::NoLooseBody)?;
        }
        Ok(None)
    }

    fn intent(&mut self, sample: Option<Sample>, dt: f32, scale: f32) -> Option<Opening> {
        let Some(sample) = sample.filter(|_| valid_timing(dt, scale)) else {
            self.reset();
            return None;
        };
        let local = sample.world.inverse().transform_point3(sample.palm);
        let same = self
            .contact
            .as_ref()
            .is_some_and(|contact| contact.target == sample.target && contact.scale == scale);
        if !same {
            self.reset();
            let outward = sample
                .world
                .inverse()
                .transform_vector3(sample.outward)
                .normalize_or_zero();
            if sample.hit.normal.normalize_or_zero().dot(sample.outward) >= -0.25
                || outward == Vec3::ZERO
            {
                return None;
            }
            self.contact = Some(Contact {
                target: sample.target,
                palm: local,
                previous: local,
                outward,
                point: sample.world.inverse().transform_point3(sample.hit.position),
                scale,
                fired: false,
            });
            return None;
        }
        let contact = self.contact.as_mut()?;
        let displacement = (local - contact.palm).dot(contact.outward);
        let speed = (local - contact.previous).dot(contact.outward) / (dt * scale);
        contact.previous = local;
        if contact.fired || displacement < UNLATCH_PULL_M * scale || speed <= 0.0 {
            return None;
        }
        contact.fired = true;
        Some(Opening {
            point: sample.world.transform_point3(contact.point),
            outward: sample
                .world
                .transform_vector3(contact.outward)
                .normalize_or_zero(),
            speed: speed.clamp(MIN_OPEN_MPS, MAX_OPEN_MPS) * scale,
        })
    }
}

pub(crate) const REACH_M: f32 = 0.18;

const MAX_DT_S: f32 = 0.25;
const MIN_OPEN_MPS: f32 = 0.2;
const MAX_OPEN_MPS: f32 = 1.0;
const MAX_IMPULSE_NS: f32 = 80.0;

struct OpenInput {
    reach: Reach,
    hit: SurfaceHit,
    target: DoorTarget,
    palm: Vec3,
    dt: f32,
    scale: f32,
}

#[derive(Debug, Copy, Clone)]
struct OpenPress {
    vehicle: NonZeroUsize,
    part: u32,
    owner: NonZeroUsize,
    body: u32,
    palm: Vec3,
    scale: f32,
}

impl OpenPress {
    fn same(self, other: Self) -> bool {
        self.vehicle == other.vehicle
            && self.part == other.part
            && self.owner == other.owner
            && self.body == other.body
            && self.scale == other.scale
    }
}

fn push_speed(displacement: Vec3, normal: Vec3, dt: f32, scale: f32) -> f32 {
    if !valid_timing(dt, scale) || !displacement.is_finite() || !normal.is_finite() {
        return 0.0;
    }
    (-displacement.dot(normal.normalize_or_zero()) / dt).clamp(0.0, 0.25 * scale)
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct Target {
    vehicle: NonZeroUsize,
    body: u32,
    part: u32,
    slot: DoorSlot,
}

#[derive(Debug, Copy, Clone)]
struct Contact {
    target: Target,
    palm: Vec3,
    previous: Vec3,
    outward: Vec3,
    point: Vec3,
    scale: f32,
    fired: bool,
}

#[derive(Debug, Copy, Clone)]
struct Opening {
    point: Vec3,
    outward: Vec3,
    speed: f32,
}

#[derive(Debug, Copy, Clone)]
struct Sample {
    target: Target,
    hit: SurfaceHit,
    world: Mat4,
    palm: Vec3,
    outward: Vec3,
}

impl Sample {
    fn new(reach: Reach, hit: SurfaceHit, door: DoorRead, world: Mat4, palm: Vec3) -> Option<Self> {
        let (_, _, target, body) = candidate(Some(reach), Some(hit))?;
        let inverse = world.inverse();
        let outward = target.outward.normalize_or_zero();
        (door.part_index == target.part_index
            && door.slot == target.slot
            && door.latched
            && panel_hit(&door, &hit)
            && world.is_finite()
            && inverse.is_finite()
            && inverse.transform_point3(palm).is_finite()
            && inverse.transform_point3(hit.position).is_finite()
            && hit.normal.is_finite()
            && outward.is_finite()
            && outward != Vec3::ZERO)
            .then_some(Self {
                target: Target {
                    vehicle: target.vehicle,
                    body,
                    part: target.part_index,
                    slot: target.slot,
                },
                hit,
                world,
                palm,
                outward,
            })
    }
}

fn valid_timing(dt: f32, scale: f32) -> bool {
    dt.is_finite() && dt > 0.0 && dt <= MAX_DT_S && scale.is_finite() && scale > 0.0
}

fn candidate(
    reach: Option<Reach>,
    hit: Option<SurfaceHit>,
) -> Option<(Reach, SurfaceHit, DoorTarget, u32)> {
    let reach = reach?;
    let hit = hit?;
    let TargetKind::Door(target) = reach.kind else {
        return None;
    };
    let vehicle_body = target.vehicle_body?;
    (target.latched
        && reach.object == target.vehicle
        && reach.body == vehicle_body
        && hit.surface
            == (Surface::Solid {
                object: Some(target.vehicle),
                body: vehicle_body,
            }))
    .then_some((reach, hit, target, vehicle_body))
}

fn door_body(
    current: Option<(NonZeroUsize, u32)>,
    owner: NonZeroUsize,
    body: u32,
    vehicle: NonZeroUsize,
    vehicle_body: u32,
) -> bool {
    body != vehicle_body && owner != vehicle && current == Some((owner, body))
}

fn opening_impulse(outward: Vec3, mass: Option<f32>, speed: f32) -> Vec3 {
    let Some(mass) = mass.filter(|mass| mass.is_finite() && *mass > 0.0) else {
        return Vec3::ZERO;
    };
    if !outward.is_finite() || !speed.is_finite() || speed <= 0.0 {
        return Vec3::ZERO;
    }
    outward.normalize_or_zero() * (mass * speed).min(MAX_IMPULSE_NS)
}

#[cfg(test)]
mod tests {
    use glam::Quat;
    use jc3gi::physics::GameMaterialId;

    use super::*;
    use crate::controllers::physical_hands::doors::{geometry::DoorBox, table};

    fn fixture() -> (Reach, SurfaceHit, DoorRead) {
        let vehicle = NonZeroUsize::new(64).unwrap();
        let target = DoorTarget {
            vehicle,
            vehicle_body: Some(7),
            part_index: 2,
            slot: DoorSlot::FRONT_LEFT,
            latched: true,
            outward: Vec3::X,
        };
        let hit = SurfaceHit {
            position: Vec3::X,
            normal: Vec3::NEG_X,
            surface: Surface::Solid {
                object: Some(vehicle),
                body: 7,
            },
            material: Some(0),
        };
        let reach = Reach {
            object: vehicle,
            body: 7,
            point: Vec3::new(1.0, 0.5, 0.5),
            distance: 0.0,
            normal: Vec3::X,
            kind: TargetKind::Door(target),
        };
        let door = DoorRead {
            slot: target.slot,
            part_index: target.part_index,
            latched: true,
            door_box: DoorBox::new(
                Mat4::IDENTITY,
                Mat4::IDENTITY,
                Vec3::new(0.98, -1.0, -1.0),
                Vec3::new(1.02, 1.0, 1.0),
            ),
            body: None,
            part_hash: None,
        };
        (reach, hit, door)
    }

    fn sample(palm: Vec3) -> Option<Sample> {
        let (reach, hit, door) = fixture();
        Sample::new(reach, hit, door, Mat4::IDENTITY, palm)
    }

    fn tick(press: &mut Press, palm: Vec3) -> Option<Opening> {
        press.intent(sample(palm), 0.01, 1.0)
    }

    #[test]
    fn an_inside_outward_press_opens_once_at_the_contact_not_the_handle() {
        let mut press = Press::new();
        assert!(tick(&mut press, Vec3::X).is_none());
        let opening = tick(&mut press, Vec3::X * 1.06).unwrap();
        assert_eq!(opening.point, Vec3::X);
        assert_eq!(opening.outward, Vec3::X);
        assert_eq!(opening.speed, MAX_OPEN_MPS);
        assert!(tick(&mut press, Vec3::X * 1.2).is_none());
        assert!(tick(&mut press, Vec3::X).is_none());
        assert!(tick(&mut press, Vec3::X * 1.2).is_none());
    }

    #[test]
    fn inward_and_tangential_displacement_do_not_open() {
        for displacement in [Vec3::NEG_X, Vec3::Y, Vec3::Z] {
            let mut press = Press::new();
            tick(&mut press, Vec3::X);
            assert!(tick(&mut press, Vec3::X + displacement).is_none());
        }
    }

    #[test]
    fn acquisition_does_not_spend_an_existing_gap_or_penetration() {
        for initial in [Vec3::X * 0.8, Vec3::X * 1.2] {
            let mut press = Press::new();
            assert!(tick(&mut press, initial).is_none());
            assert!(tick(&mut press, initial + Vec3::X * 0.04).is_none());
            assert!(tick(&mut press, initial + Vec3::X * 0.06).is_some());
        }
    }

    #[test]
    fn outside_contact_cannot_arm_a_push() {
        let mut press = Press::new();
        for palm in [Vec3::X, Vec3::X * 1.2] {
            let mut exterior = sample(palm).unwrap();
            exterior.hit.normal = Vec3::X;
            assert!(press.intent(Some(exterior), 0.01, 1.0).is_none());
        }
        assert!(press.contact.is_none());
    }

    #[test]
    fn vehicle_translation_and_rotation_do_not_spend_the_pull() {
        let mut press = Press::new();
        tick(&mut press, Vec3::X);
        for translation in [Vec3::new(50.0, -20.0, 90.0), Vec3::ZERO] {
            let world = Mat4::from_rotation_translation(Quat::from_rotation_y(1.7), translation);
            let mut moving = sample(Vec3::X).unwrap();
            moving.world = world;
            moving.palm = world.transform_point3(Vec3::X);
            moving.hit.position = world.transform_point3(Vec3::X);
            moving.hit.normal = world.transform_vector3(Vec3::NEG_X);
            moving.outward = world.transform_vector3(Vec3::X);
            assert!(press.intent(Some(moving), 0.01, 1.0).is_none());
            moving.palm = world.transform_point3(Vec3::X * 1.06);
            let opening = press.intent(Some(moving), 0.01, 1.0).unwrap();
            assert!(opening.point.distance(world.transform_point3(Vec3::X)) < 1e-4);
            assert!(opening.outward.distance(world.transform_vector3(Vec3::X)) < 1e-4);
            press.reset();
            tick(&mut press, Vec3::X);
        }
    }

    #[test]
    fn contact_loss_and_each_target_identity_change_rebase_the_pull() {
        let mut press = Press::new();
        tick(&mut press, Vec3::X);
        assert!(press.intent(None, 0.01, 1.0).is_none());
        assert!(tick(&mut press, Vec3::X * 1.2).is_none());
        assert!(tick(&mut press, Vec3::X * 1.26).is_some());
        for change in 0..4 {
            press.reset();
            tick(&mut press, Vec3::X);
            let mut swapped = sample(Vec3::X * 1.2).unwrap();
            match change {
                0 => swapped.target.vehicle = NonZeroUsize::new(128).unwrap(),
                1 => swapped.target.body += 1,
                2 => swapped.target.part += 1,
                _ => swapped.target.slot = DoorSlot::FRONT_RIGHT,
            }
            assert!(press.intent(Some(swapped), 0.01, 1.0).is_none());
            swapped.palm += Vec3::X * 0.06;
            assert!(press.intent(Some(swapped), 0.01, 1.0).is_some());
        }
    }

    #[test]
    fn invalid_timing_and_scale_reset_and_never_open() {
        for (dt, scale) in [
            (0.0, 1.0),
            (-0.01, 1.0),
            (0.3, 1.0),
            (f32::NAN, 1.0),
            (f32::INFINITY, 1.0),
            (0.01, 0.0),
            (0.01, -1.0),
            (0.01, f32::NAN),
            (0.01, f32::INFINITY),
        ] {
            let mut press = Press::new();
            tick(&mut press, Vec3::X);
            assert!(press.intent(sample(Vec3::X * 1.2), dt, scale).is_none());
            assert!(press.contact.is_none());
            assert!(tick(&mut press, Vec3::X * 1.2).is_none());
        }
    }

    #[test]
    fn scale_controls_the_threshold_and_changes_rebase() {
        let mut press = Press::new();
        assert!(press.intent(sample(Vec3::X), 0.01, 2.0).is_none());
        assert!(press.intent(sample(Vec3::X * 1.06), 0.01, 2.0).is_none());
        assert!(press.intent(sample(Vec3::X * 1.11), 0.01, 2.0).is_some());
        assert!(press.intent(sample(Vec3::X * 1.3), 0.01, 1.0).is_none());
    }

    #[test]
    fn only_real_owned_panel_contacts_are_candidates() {
        let (reach, hit, door) = fixture();
        assert!(candidate(Some(reach), None).is_none());
        assert!(candidate(None, Some(hit)).is_none());
        for surface in [
            Surface::Solid {
                object: None,
                body: 7,
            },
            Surface::Solid {
                object: Some(reach.object),
                body: 8,
            },
            Surface::Character {
                object: reach.object,
                body: 7,
            },
        ] {
            assert!(candidate(Some(reach), Some(SurfaceHit { surface, ..hit })).is_none());
        }
        let off_panel = SurfaceHit {
            position: Vec3::ZERO,
            ..hit
        };
        assert!(Sample::new(reach, off_panel, door, Mat4::IDENTITY, Vec3::X).is_none());
        assert!(Sample::new(reach, hit, door, Mat4::ZERO, Vec3::X).is_none());
        assert!(Sample::new(reach, hit, door, Mat4::IDENTITY, Vec3::NAN).is_none());
    }

    #[test]
    fn ordinary_glass_is_excluded_and_known_canopy_glass_is_allowed() {
        let (reach, mut hit, mut door) = fixture();
        hit.material = Some(GameMaterialId::EGameMaterialId_Glass as u32);
        assert!(Sample::new(reach, hit, door, Mat4::IDENTITY, Vec3::X).is_none());
        let entries = table::parse(include_str!(
            "../../../../../tools/door_handles/door_handles.json"
        ))
        .unwrap();
        let canopy = entries
            .iter()
            .find(|entry| table::is_canopy(Some(entry.part_hash)))
            .expect("the handle table contains a verified canopy");
        door.part_hash = Some(canopy.part_hash);
        let mut press = Press::new();
        assert!(
            press
                .intent(
                    Sample::new(reach, hit, door, Mat4::IDENTITY, Vec3::X),
                    0.01,
                    1.0
                )
                .is_none()
        );
        assert!(
            press
                .intent(
                    Sample::new(reach, hit, door, Mat4::IDENTITY, Vec3::X * 1.06),
                    0.01,
                    1.0
                )
                .is_some()
        );
    }

    #[test]
    fn open_panels_accept_only_bounded_pressure_into_the_panel() {
        assert_eq!(push_speed(Vec3::NEG_Y, Vec3::Y, 0.01, 1.0), 0.25);
        assert_eq!(push_speed(Vec3::Y, Vec3::Y, 0.01, 1.0), 0.0);
        assert_eq!(push_speed(Vec3::X, Vec3::Y, 0.01, 1.0), 0.0);
        assert_eq!(push_speed(Vec3::NEG_Y, Vec3::Y, 0.0, 1.0), 0.0);
        assert_eq!(push_speed(Vec3::NEG_Y, Vec3::Y, 0.01, f32::NAN), 0.0);
    }

    #[test]
    fn impulses_are_finite_bounded_and_never_target_the_chassis() {
        assert_eq!(opening_impulse(Vec3::X, Some(10.0), 0.5), Vec3::X * 5.0);
        assert_eq!(
            opening_impulse(Vec3::X, Some(f32::MAX), 10.0),
            Vec3::X * MAX_IMPULSE_NS
        );
        for mass in [
            None,
            Some(0.0),
            Some(-1.0),
            Some(f32::NAN),
            Some(f32::INFINITY),
        ] {
            assert_eq!(opening_impulse(Vec3::X, mass, 1.0), Vec3::ZERO);
        }
        for speed in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(opening_impulse(Vec3::X, Some(10.0), speed), Vec3::ZERO);
        }
        let vehicle = NonZeroUsize::new(64).unwrap();
        let owner = NonZeroUsize::new(128).unwrap();
        assert!(door_body(Some((owner, 8)), owner, 8, vehicle, 7));
        assert!(!door_body(Some((owner, 7)), owner, 7, vehicle, 7));
        assert!(!door_body(Some((vehicle, 8)), vehicle, 8, vehicle, 7));
        assert!(!door_body(Some((owner, 9)), owner, 8, vehicle, 7));
        assert!(!door_body(None, owner, 8, vehicle, 7));
    }
}
