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

use glam::Vec3;
use jc3gi::character::character::Character;

use crate::controllers::{
    aim::{
        ray::Ray,
        raycast::{self, CastOptions},
    },
    body::{procedural::config::ProceduralBodyConfig, rig::legs::swing::Ground},
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

/// How often the running tally is logged.
pub const LOG_INTERVAL: Duration = Duration::from_secs(10);

/// The steepest a surface may be, as its normal's height, and still be stood on: about fifty
/// degrees of slope.
const MIN_GROUND_NORMAL_Y: f32 = 0.64;

static FIRST_LOGGED: AtomicBool = AtomicBool::new(false);
static ERROR_LOGGED: AtomicBool = AtomicBool::new(false);
