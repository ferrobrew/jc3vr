use std::num::NonZeroUsize;

use glam::{Mat4, Vec3};
use jc3gi::{
    physics::{GameMaterialId, PhysicsSystem},
    vehicle::vehicle::DoorSlot,
};

use crate::controllers::{
    aim::{
        ray::Ray,
        raycast::{self, CastOptions, RaycastError},
    },
    glass::bullets::names_hidden_pane,
};

mod diagnostics;

pub use crate::controllers::physical_hands::doors::pressure::diagnostics::{Readout, Reason};
use crate::controllers::physical_hands::{
    bodies,
    contact::{Surface, SurfaceHit},
    doors::{
        DoorTarget, UNLATCH_PULL_M,
        engine::{self, DoorError, DoorRead},
        hinge, panel_hit,
        pressure::diagnostics::Diagnostics,
        table,
    },
    engine as contact_engine,
    grasp::{Reach, TargetKind},
    palm::PalmBox,
    state::STATE,
};

#[derive(Debug, Copy, Clone, Default)]
pub(crate) struct Press {
    contact: Option<Contact>,
    open: Option<OpenPress>,
    diagnostics: Diagnostics,
    contact_cue: bool,
}

pub(crate) struct Input<'a> {
    pub(crate) reach: Option<Reach>,
    pub(crate) hit: Option<SurfaceHit>,
    pub(crate) palm: Vec3,
    pub(crate) dt: f32,
    pub(crate) world_scale: f32,
    pub(crate) options: &'a CastOptions,
}

#[derive(Debug, Copy, Clone)]
pub(crate) enum Probe {
    Miss,
    Visible(SurfaceHit),
    Hidden,
}

pub fn status() -> [Readout; 2] {
    STATE
        .lock()
        .door_press
        .map(|press| press.diagnostics.readout)
}

pub(crate) unsafe fn probe(
    shoulder: Vec3,
    palm: &PalmBox,
    options: &CastOptions,
) -> Result<Probe, RaycastError> {
    let mut result = Probe::Miss;
    for point in palm.probe_points() {
        result = prefer_probe(
            result,
            unsafe { raw_probe(shoulder, point, options) }?,
            palm.centre,
        );
        if matches!(result, Probe::Hidden) {
            break;
        }
    }
    Ok(result)
}

impl Press {
    pub(crate) const fn new() -> Self {
        Self {
            contact: None,
            open: None,
            diagnostics: Diagnostics::new(),
            contact_cue: false,
        }
    }

    pub(crate) fn observe_probe(&mut self, probe: Probe) -> Option<SurfaceHit> {
        match probe {
            Probe::Visible(hit) => Some(hit),
            Probe::Miss => None,
            Probe::Hidden => {
                self.reject(Reason::GlassRejected);
                None
            }
        }
    }

    pub(crate) fn engaged(&self) -> bool {
        self.contact.is_some()
    }

    pub(crate) fn take_contact_cue(&mut self) -> bool {
        std::mem::take(&mut self.contact_cue)
    }

    pub(crate) fn reset(&mut self) {
        self.contact_cue = false;
        let lost = self.contact.is_some() || self.open.is_some();
        self.contact = None;
        self.open = None;
        self.diagnostics.idle(if lost {
            Reason::ContactLost
        } else {
            Reason::NoContact
        });
    }

    fn reject(&mut self, reason: Reason) {
        self.contact_cue = false;
        self.contact = None;
        self.open = None;
        self.diagnostics.reason(reason);
    }

    fn missing_contact(&mut self, reach: Option<Reach>, hit: Option<SurfaceHit>) {
        let reason = if hit.is_some_and(glass_hit) {
            Some(Reason::GlassRejected)
        } else if hit.is_some() {
            Some(Reason::NoPanel)
        } else {
            None
        };
        if let Some(reason) = reason {
            self.reject(reason);
            if !reach.is_some_and(|reach| matches!(reach.kind, TargetKind::Door(_))) {
                self.diagnostics.idle(reason);
            }
        } else {
            self.reset();
        }
    }

    fn native_failure(&mut self, error: DoorError) -> DoorError {
        self.diagnostics.failed(error, None);
        error
    }

    fn body_failure(&mut self, error: bodies::BodyError, mapped: DoorError) -> DoorError {
        self.diagnostics.failed(mapped, Some(error));
        mapped
    }

    pub(crate) unsafe fn step(&mut self, input: Input<'_>) -> Result<Option<DoorSlot>, DoorError> {
        let Input {
            reach,
            hit,
            palm,
            dt,
            world_scale,
            options,
        } = input;
        if !valid_timing(dt, world_scale) || !palm.is_finite() {
            self.reject(Reason::InvalidSample);
            return Ok(None);
        }
        if hit.is_some_and(|hit| {
            !hit.position.is_finite() || hit.position.distance(palm) > REACH_M * world_scale
        }) {
            self.reject(Reason::NoPanel);
            return Ok(None);
        }
        if let Some(Reach {
            kind: TargetKind::Door(target),
            ..
        }) = reach
        {
            self.diagnostics.target(target);
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
        let Some((reach, hit, target, vehicle_body)) = candidate(reach, hit) else {
            if reach.is_none()
                && hit.is_none()
                && unsafe { self.retain(palm, dt, world_scale, options) }
            {
                return Ok(None);
            }
            self.missing_contact(reach, hit);
            return Ok(None);
        };
        self.open = None;
        if !unsafe { engine::alive(target.vehicle, vehicle_body) } {
            self.reject(Reason::NativeFailure(DoorError::VehicleGone));
            return Err(self.native_failure(DoorError::VehicleGone));
        }
        let Some(door) = (unsafe { engine::door(target.vehicle, target.part_index) }) else {
            self.reject(Reason::NativeFailure(DoorError::NoDoor));
            return Err(self.native_failure(DoorError::NoDoor));
        };
        let world = unsafe { engine::vehicle_frame(target.vehicle) };
        if !panel_hit(&door, &hit) {
            self.reject(panel_rejection(door, hit));
            return Ok(None);
        }
        if unsafe { engine::vehicle_body(target.vehicle) } != Some(vehicle_body) {
            self.reject(Reason::InvalidSample);
            return Ok(None);
        }
        let sample = Sample::new(reach, hit, door, world, palm);
        let Some(opening) = self.intent(sample, dt, world_scale) else {
            return Ok(None);
        };
        let (owner, body) = unsafe { engine::unlatch(target.vehicle, target.part_index) }
            .map_err(|error| self.native_failure(error))?;
        let current = unsafe { engine::door(target.vehicle, target.part_index) }
            .ok_or_else(|| self.native_failure(DoorError::NoDoor))?;
        if !door_body(current.body, owner, body, target.vehicle, vehicle_body) {
            return Err(self.native_failure(DoorError::NoLooseBody));
        }
        let state = unsafe { bodies::read(body, owner) }
            .map_err(|error| self.body_failure(error, DoorError::NoLooseBody))?;
        let impulse = opening_impulse(opening.outward, state.mass_kg, opening.speed);
        if impulse != Vec3::ZERO {
            unsafe { bodies::shove(body, owner, impulse, opening.point) }
                .map_err(|error| self.body_failure(error, DoorError::NoLooseBody))?;
        }
        self.diagnostics.opened();
        Ok(Some(target.slot))
    }

    unsafe fn retain(&mut self, palm: Vec3, dt: f32, scale: f32, options: &CastOptions) -> bool {
        let Some(contact) = self.contact else {
            return false;
        };
        let target = contact.target;
        if !unsafe { engine::alive(target.vehicle, target.body) }
            || unsafe { engine::vehicle_body(target.vehicle) } != Some(target.body)
        {
            return false;
        }
        let Some(door) = (unsafe { engine::door(target.vehicle, target.part) }) else {
            return false;
        };
        let world = unsafe { engine::vehicle_frame(target.vehicle) };
        let local = world.inverse().transform_point3(palm);
        if !contact.near(local, scale) || !valid_frame(world) {
            return false;
        }
        if !door
            .door_box
            .is_some_and(|bounds| bounds.reach(palm, REACH_M * scale).is_some())
        {
            return false;
        }
        let projected = local - contact.outward * (local - contact.point).dot(contact.outward);
        let from = world.transform_point3(projected - contact.outward * PROOF_DEPTH_M * scale);
        let to = world.transform_point3(projected + contact.outward * PROOF_DEPTH_M * scale);
        let Ok(Probe::Visible(hit)) = (unsafe { raw_probe(from, to, options) }) else {
            return false;
        };
        let reach = Reach {
            object: target.vehicle,
            body: target.body,
            point: hit.position,
            distance: hit.position.distance(palm),
            normal: hit.normal,
            kind: TargetKind::Door(DoorTarget {
                vehicle: target.vehicle,
                vehicle_body: Some(target.body),
                part_index: target.part,
                slot: target.slot,
                latched: true,
                outward: world.transform_vector3(contact.outward),
            }),
        };
        self.retain_sample(Sample::new(reach, hit, door, world, palm), dt, scale)
    }

    fn retain_sample(&mut self, sample: Option<Sample>, dt: f32, scale: f32) -> bool {
        let Some(sample) = sample else {
            return false;
        };
        let Some(contact) = self.contact.as_mut() else {
            return false;
        };
        let local = sample.world.inverse().transform_point3(sample.palm);
        if !valid_timing(dt, scale)
            || contact.target != sample.target
            || contact.fired
            || !contact.near(local, scale)
            || contact.gap_s + dt > CONTACT_GRACE_S
            || sample.hit.material.is_none()
            || glass_hit(sample.hit)
            || sample.hit.normal.normalize_or_zero().dot(sample.outward) >= -0.25
            || sample.hit.position.distance(sample.palm) > REACH_M * scale
        {
            return false;
        }
        contact.gap_s += dt;
        self.diagnostics.reason(Reason::ContactLost);
        true
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
            self.reject(Reason::NoPanel);
            return Ok(None);
        }
        let Some(vehicle_body) = (unsafe { engine::vehicle_body(target.vehicle) }) else {
            self.open = None;
            return Err(self.native_failure(DoorError::VehicleGone));
        };
        let Some(door) = (unsafe { engine::door(target.vehicle, target.part_index) }) else {
            self.open = None;
            return Err(self.native_failure(DoorError::NoDoor));
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
            self.reject(panel_rejection(door, hit));
            return Ok(None);
        }
        self.diagnostics.reason(Reason::OpenPanel);
        self.diagnostics.movement(0.0, scale);
        let world = unsafe { engine::vehicle_frame(target.vehicle) };
        let local = world.inverse().transform_point3(palm);
        if !local.is_finite() || !hit.normal.is_finite() {
            self.reject(Reason::InvalidSample);
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
        self.diagnostics
            .movement(-displacement.dot(hit.normal.normalize_or_zero()), scale);
        let speed = push_speed(displacement, hit.normal, dt, scale);
        if speed == 0.0 {
            return Ok(None);
        }
        let state = unsafe { bodies::read(reach.body, reach.object) }
            .map_err(|error| self.body_failure(error, DoorError::NoLooseBody))?;
        let normal = hit.normal.normalize_or_zero();
        let depth = (hit.position - palm).dot(normal);
        if depth < -0.06 * scale {
            return Ok(None);
        }
        let chassis = unsafe { bodies::read(vehicle_body, target.vehicle) }
            .map_err(|error| self.body_failure(error, DoorError::VehicleGone))?;
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
                .map_err(|error| self.body_failure(error, DoorError::NoLooseBody))?;
            hinge::yield_support(target.vehicle, target.part_index);
        }
        Ok(None)
    }

    fn intent(&mut self, sample: Option<Sample>, dt: f32, scale: f32) -> Option<Opening> {
        let Some(sample) = sample.filter(|_| valid_timing(dt, scale)) else {
            self.reject(Reason::InvalidSample);
            return None;
        };
        if sample.hit.normal.normalize_or_zero().dot(sample.outward) >= -0.25 {
            self.reject(Reason::ExteriorFace);
            return None;
        }
        self.diagnostics.readout.slot = Some(sample.target.slot);
        let local = sample.world.inverse().transform_point3(sample.palm);
        let same = self
            .contact
            .as_ref()
            .is_some_and(|contact| contact.target == sample.target && contact.scale == scale);
        if !same {
            self.contact = None;
            self.open = None;
            self.diagnostics.movement(0.0, scale);
            let outward = sample
                .world
                .inverse()
                .transform_vector3(sample.outward)
                .normalize_or_zero();
            if outward == Vec3::ZERO {
                self.diagnostics.reason(Reason::ExteriorFace);
                return None;
            }
            self.diagnostics.reason(Reason::InsufficientMovement);
            self.contact_cue = true;
            self.contact = Some(Contact {
                target: sample.target,
                palm: local,
                previous: local,
                outward,
                point: sample.world.inverse().transform_point3(sample.hit.position),
                scale,
                fired: false,
                gap_s: 0.0,
            });
            return None;
        }
        let contact = self.contact.as_mut()?;
        if contact.gap_s > 0.0 {
            if !contact.near(local, scale)
                || contact.gap_s + dt > CONTACT_GRACE_S
                || sample.hit.material.is_none()
                || glass_hit(sample.hit)
            {
                self.reject(Reason::ContactLost);
                return None;
            }
            contact.palm += local - contact.previous;
            contact.previous = local;
            contact.gap_s = 0.0;
            self.diagnostics.reason(Reason::InsufficientMovement);
            return None;
        }
        if (local - contact.previous).dot(contact.outward) < -0.005 * scale {
            self.reject(Reason::ContactLost);
            return None;
        }
        let displacement = (local - contact.palm).dot(contact.outward);
        let speed = (local - contact.previous).dot(contact.outward) / (dt * scale);
        contact.previous = local;
        self.diagnostics.movement(displacement, scale);
        if !contact.fired {
            self.diagnostics.reason(Reason::InsufficientMovement);
        }
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
const CONTACT_GRACE_S: f32 = 0.08;
const GAP_DRIFT_M: f32 = 0.025;
const PROOF_DEPTH_M: f32 = 0.04;
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
    gap_s: f32,
}

impl Contact {
    fn near(self, local: Vec3, scale: f32) -> bool {
        let drift = local - self.previous;
        self.scale == scale
            && local.is_finite()
            && drift.length() <= GAP_DRIFT_M * scale
            && drift.dot(self.outward) >= -0.005 * scale
    }
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
            && door.body.is_none()
            && panel_hit(&door, &hit)
            && valid_frame(world)
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

fn glass_hit(hit: SurfaceHit) -> bool {
    hit.material == Some(GameMaterialId::EGameMaterialId_Glass as u32)
}

fn panel_rejection(door: DoorRead, hit: SurfaceHit) -> Reason {
    if glass_hit(hit) && !table::is_canopy(door.part_hash) {
        Reason::GlassRejected
    } else {
        Reason::NoPanel
    }
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
mod tests;

unsafe fn raw_probe(from: Vec3, to: Vec3, options: &CastOptions) -> Result<Probe, RaycastError> {
    let ray = Ray::new(from, to - from).ok_or(RaycastError::InvalidDistance)?;
    let options = CastOptions {
        material: true,
        ..*options
    };
    let result = unsafe {
        raycast::cast_on_with(
            &ray,
            from.distance(to),
            &options,
            PhysicsSystem::QUERY_LAYER,
            |hit, info| {
                let surface = contact_engine::surface(&hit);
                classify_probe(surface, || {
                    info.base.m_HitObjectId != 0 && names_hidden_pane(&mut info.base)
                })
            },
        )
    }?;
    Ok(result.unwrap_or(Probe::Miss))
}

fn classify_probe(hit: SurfaceHit, hidden: impl FnOnce() -> bool) -> Probe {
    if glass_hit(hit) && hidden() {
        Probe::Hidden
    } else {
        Probe::Visible(hit)
    }
}

fn prefer_probe(current: Probe, next: Probe, palm: Vec3) -> Probe {
    match (current, next) {
        (Probe::Hidden, _) | (_, Probe::Hidden) => Probe::Hidden,
        (Probe::Miss, next) => next,
        (current, Probe::Miss) => current,
        (Probe::Visible(previous), Probe::Visible(hit)) => {
            if glass_hit(hit)
                || (!glass_hit(previous)
                    && hit.position.distance_squared(palm)
                        < previous.position.distance_squared(palm))
            {
                Probe::Visible(hit)
            } else {
                Probe::Visible(previous)
            }
        }
    }
}

fn valid_frame(world: Mat4) -> bool {
    world.is_finite()
        && world.inverse().is_finite()
        && (world.x_axis.truncate().length_squared() - 1.0).abs() < 0.001
        && (world.y_axis.truncate().length_squared() - 1.0).abs() < 0.001
        && (world.z_axis.truncate().length_squared() - 1.0).abs() < 0.001
        && (world.determinant() - 1.0).abs() < 0.001
}
