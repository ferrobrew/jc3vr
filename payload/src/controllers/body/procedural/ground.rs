//! The ground under the procedural body's footholds: a scene ray straight down through each
//! foothold a step heads for, cast from the solve gate.
//!
//! The gate runs inside the pose-finalization pass, which the engine queues for its parallel
//! update of every character. The engine casts its own ground ray from that pass
//! ([`Character::CalculateHeightOverGround`](jc3gi::character::character::Character::CalculateHeightOverGround),
//! through [`Character::UpdateGroundInfo`](jc3gi::character::character::Character::UpdateGroundInfo)),
//! with the same query ([`PhysicsSystem::CastRay`](jc3gi::physics::PhysicsSystem::CastRay)), a hit
//! record and a filter built on the stack, and the same layer as [`raycast::cast`]. The physics
//! step has finished by then, so the casts read a settled world, as the engine's own do.
//!
//! The first cast of a session and a tally every [`LOG_INTERVAL`] are logged, so a session's log
//! shows that the casts run and what they find.

use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

use glam::{Mat4, Quat, Vec3};
use jc3gi::{
    character::character::Character,
    physics::{PhysicsSystem, hknpBody, hknpCharacterSurfaceInfo},
};

use crate::controllers::{
    aim::{
        ray::Ray,
        raycast::{self, CastOptions},
    },
    body::{
        procedural::config::ProceduralBodyConfig,
        rig::{
            estimate::{Support, SupportSample, Terrain},
            legs::swing::{FootPose, Ground},
        },
    },
    physical_hands::physics::havok_affine,
};

/// The casts of one gate, and the running tally the log reports.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct GroundTally {
    pub casts: u32,
    pub hits: u32,
    /// Hits too steep to stand on.
    pub steep: u32,
    /// Hits on a game object rather than the world: a prop, a held object, a body, a vehicle.
    pub objects: u32,
    pub errors: u32,
    /// The largest height of a hit above or below the root, metres, signed.
    pub largest_offset_m: f32,
}

impl GroundTally {
    pub const fn new() -> Self {
        Self {
            casts: 0,
            hits: 0,
            steep: 0,
            objects: 0,
            errors: 0,
            largest_offset_m: 0.0,
        }
    }

    fn add(&mut self, other: &Self) {
        self.casts += other.casts;
        self.hits += other.hits;
        self.steep += other.steep;
        self.objects += other.objects;
        self.errors += other.errors;
        if other.largest_offset_m.abs() > self.largest_offset_m.abs() {
            self.largest_offset_m = other.largest_offset_m;
        }
    }
}

#[cfg(test)]
mod tests;

/// Current scene geometry paired with the proxy's last completed pre-physics support record.
pub(crate) struct FitGround<'a> {
    caster: &'a mut GroundCaster,
    pub sample: SupportSample,
    body: u32,
    moving: bool,
    floor: f32,
}

impl<'a> FitGround<'a> {
    /// # Safety
    ///
    /// The local character is live at the native HumanIK solve gate, outside physics.
    pub(crate) unsafe fn new(
        character: &Character,
        caster: &'a mut GroundCaster,
        point: Vec3,
        now: Instant,
    ) -> Option<Self> {
        let proxy = unsafe { character.m_PfxCharacter.px.as_ref() }?;
        let info = &proxy.m_GroundInformation;
        let normal = Vec3::from_array(info.m_surfaceNormal.data);
        if info.m_supportedState != hknpCharacterSurfaceInfo::SUPPORTED
            || !normal.is_finite()
            || normal.y < MIN_GROUND_NORMAL_Y
            || !unsafe { proxy.IsGroundBodyValid() }
        {
            return None;
        }
        let physics = unsafe { PhysicsSystem::get() }?;
        let world = unsafe { physics.m_World.as_ref() }?;
        let body = proxy.m_GroundBodyId;
        if world.m_bodies.is_null() || body > world.m_peakBodyIndex {
            return None;
        }
        let slot = unsafe { &*world.m_bodies.add(body as usize) };
        let moving = slot.m_flags & hknpBody::IS_STATIC == 0;
        let transform = if moving {
            let mut transform = Mat4::from(havok_affine(&slot.m_transform));
            transform.w_axis += Vec3::from_array(physics.m_AccumulatedShift.data).extend(0.0);
            transform
        } else {
            Mat4::IDENTITY
        };
        let hit = unsafe { caster.fit_hit(point) }?;
        if !accepts_body(moving, body, hit.body, hit.object.is_some()) {
            return None;
        }
        Some(Self {
            caster,
            sample: SupportSample {
                identity: if moving { u64::from(body) + 1 } else { 0 },
                generation: if moving {
                    slot.m_shape as usize as u64 ^ u64::from(slot.m_motionId)
                } else {
                    0
                },
                transform,
                at: now,
            },
            body,
            moving,
            floor: hit.position.y,
        })
    }
}

impl Terrain for FitGround<'_> {
    fn floor_height(&self, _support: Support) -> f32 {
        self.floor
    }

    fn foothold(
        &mut self,
        support: Support,
        point: Vec3,
        heading: f32,
        ankle_height: f32,
    ) -> Option<FootPose> {
        // SAFETY: FitGround is constructed and consumed within one live solve gate.
        let hit = unsafe { self.caster.fit_hit(point) }?;
        if !accepts_body(self.moving, self.body, hit.body, hit.object.is_some()) {
            return None;
        }
        terrain_foothold(
            support,
            point,
            heading,
            ankle_height,
            &hit,
            self.moving.then_some(self.body),
        )
    }
}

/// Casts the ground under footholds for one gate.
pub struct GroundCaster {
    character: usize,
    root_height: f32,
    up_m: f32,
    down_m: f32,
    enabled: bool,
    pub tally: GroundTally,
}

impl GroundCaster {
    /// A caster for `character`'s footholds around `root_height`.
    pub fn new(character: &Character, root_height: f32, config: &ProceduralBodyConfig) -> Self {
        Self {
            character: character as *const Character as usize,
            root_height,
            up_m: config.max_step_up_m,
            down_m: config.max_step_down_m,
            enabled: config.ground_casts,
            tally: GroundTally::new(),
        }
    }

    /// A current hit for the fitted contacts, including the actual body identity.
    ///
    /// # Safety
    ///
    /// Called only inside the caster's live solve gate.
    unsafe fn fit_hit(&mut self, point: Vec3) -> Option<raycast::RayHit> {
        if !self.enabled || !point.is_finite() {
            return None;
        }
        let from = Vec3::new(point.x, self.root_height + self.up_m, point.z);
        let ray = Ray::new(from, Vec3::NEG_Y)?;
        let options = CastOptions {
            ignore: [self.character, 0],
            see_through: false,
            material: false,
        };
        self.tally.casts += 1;
        let hit = match unsafe { raycast::cast(&ray, self.up_m + self.down_m, &options) } {
            Ok(hit) => hit?,
            Err(_) => {
                self.tally.errors += 1;
                return None;
            }
        };
        if !hit.position.is_finite()
            || !hit.normal.is_finite()
            || hit.normal.y < MIN_GROUND_NORMAL_Y
        {
            self.tally.steep += 1;
            return None;
        }
        self.tally.hits += 1;
        Some(hit)
    }

    /// The ground under `point` (its horizontal place), between the highest step up and the
    /// lowest step down from the root, or `None` without ground there that a foot can stand on.
    ///
    /// # Safety
    ///
    /// Called from the solve gate, with the caster's character live.
    pub unsafe fn ground(&mut self, point: Vec3) -> Option<Ground> {
        let from = Vec3::new(point.x, self.root_height + self.up_m, point.z);
        // SAFETY: forwarded from the caller.
        unsafe { self.cast_down(from, self.up_m + self.down_m) }
    }

    /// The ground below `point`, as far as `depth` metres under it, or `None` without ground there
    /// that a foot can stand on: what a falling foot reaches for.
    ///
    /// # Safety
    ///
    /// As [`ground`](Self::ground).
    pub unsafe fn ground_below(&mut self, point: Vec3, depth: f32) -> Option<Ground> {
        if !(depth.is_finite() && depth > 0.0) {
            return None;
        }
        // SAFETY: forwarded from the caller.
        unsafe { self.cast_down(point, depth) }
    }

    /// Cast straight down from `from` for `length` metres.
    ///
    /// # Safety
    ///
    /// As [`ground`](Self::ground).
    unsafe fn cast_down(&mut self, from: Vec3, length: f32) -> Option<Ground> {
        if !self.enabled {
            return None;
        }
        let ray = Ray::new(from, Vec3::NEG_Y)?;
        let options = CastOptions {
            ignore: [self.character, 0],
            see_through: false,
            material: false,
        };
        self.tally.casts += 1;
        let hit = match unsafe { raycast::cast(&ray, length, &options) } {
            Ok(hit) => hit,
            Err(error) => {
                self.tally.errors += 1;
                if !ERROR_LOGGED.swap(true, Ordering::Relaxed) {
                    tracing::warn!(target: "body", "procedural body: ground cast: {error}");
                }
                return None;
            }
        };
        if !FIRST_LOGGED.swap(true, Ordering::Relaxed) {
            tracing::info!(
                target: "body",
                thread = ?thread::current().id(),
                thread_name = thread::current().name().unwrap_or("unnamed"),
                hit = ?hit.map(|h| (h.position, h.normal)),
                "procedural body: the first ground cast from the solve gate",
            );
        }
        let hit = hit?;
        if hit.normal.y < MIN_GROUND_NORMAL_Y {
            self.tally.steep += 1;
            return None;
        }
        // A foot stands on the world, not on whatever lies about on it: a prop the hands hold or
        // dropped, a body, or a crate would lift the foot onto it for a tick and drop it again as
        // the object moves. The foothold keeps the root's height instead, which is the ground the
        // proxy itself stands on, a vehicle's roof included.
        if hit.object.is_some() {
            self.tally.objects += 1;
            return None;
        }
        self.tally.hits += 1;
        let offset = hit.position.y - self.root_height;
        if offset.abs() > self.tally.largest_offset_m.abs() {
            self.tally.largest_offset_m = offset;
        }
        Some(Ground {
            height: hit.position.y,
            normal: hit.normal,
        })
    }
}

/// The running tally across gates, logged every [`LOG_INTERVAL`] while casts run.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct GroundLog {
    tally: GroundTally,
    since: Option<Instant>,
}

impl GroundLog {
    pub const fn new() -> Self {
        Self {
            tally: GroundTally::new(),
            since: None,
        }
    }

    /// Add a gate's tally, and log the running one when it is due.
    pub fn record(&mut self, tally: &GroundTally, now: Instant) {
        if tally.casts == 0 {
            return;
        }
        self.tally.add(tally);
        let since = *self.since.get_or_insert(now);
        if now.saturating_duration_since(since) >= LOG_INTERVAL {
            tracing::info!(
                target: "body",
                casts = self.tally.casts,
                hits = self.tally.hits,
                steep = self.tally.steep,
                objects = self.tally.objects,
                errors = self.tally.errors,
                largest_offset_m = self.tally.largest_offset_m,
                "procedural body: ground casts",
            );
            *self = Self::new();
        }
    }
}

fn accepts_body(moving: bool, ground: u32, hit: u32, object: bool) -> bool {
    if moving {
        hit == ground
    } else {
        !object || hit == ground
    }
}

pub(crate) fn terrain_foothold(
    support: Support,
    point: Vec3,
    heading: f32,
    ankle_height: f32,
    hit: &raycast::RayHit,
    accepted_body: Option<u32>,
) -> Option<FootPose> {
    if !accepted_body.map_or(hit.object.is_none(), |body| hit.body == body)
        || !point.is_finite()
        || !hit.position.is_finite()
        || !hit.normal.is_finite()
        || hit.normal.y < MIN_GROUND_NORMAL_Y
    {
        return None;
    }
    let normal = hit.normal.normalize();
    let forward = Quat::from_rotation_y(heading) * Vec3::NEG_Z;
    let forward = (forward - normal * forward.dot(normal)).try_normalize()?;
    let right = forward.cross(normal).normalize();
    let rotation = Quat::from_mat3(&glam::Mat3::from_cols(right, normal, -forward));
    let local = support.local(FootPose {
        position: Vec3::new(point.x, hit.position.y + ankle_height, point.z),
        rotation,
    });
    support.contains(local).then_some(local)
}

/// How often the running tally is logged.
pub const LOG_INTERVAL: Duration = Duration::from_secs(10);

/// The steepest a surface may be, as its normal's height, and still be stood on: about fifty
/// degrees of slope.
const MIN_GROUND_NORMAL_Y: f32 = 0.64;

static FIRST_LOGGED: AtomicBool = AtomicBool::new(false);
static ERROR_LOGGED: AtomicBool = AtomicBool::new(false);
