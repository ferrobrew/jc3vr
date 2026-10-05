//! The raycast fallback for the world markers' surface placement: markers the render thread could
//! not sample the scene depth for (no recent depth result) get a physics raycast from the head
//! toward their world point instead.
//!
//! The casts run on the game thread in the player aim's simulation update, outside the physics
//! step, as the player aim's own raycast does ([`crate::controllers::aim::raycast`]); the render
//! thread only publishes which markers need one and reads the results.

use std::{collections::HashMap, time::Instant};

use glam::Vec3;
use jc3gi::character::character::Character;
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::aim::{
        ray::Ray,
        raycast::{CastOptions, cast},
    },
    hud::world_markers::{depth::Timed, layout::Sight},
};

/// Cast the pending markers' rays. Call from the player aim control's simulation update for the
/// local player character.
///
/// # Safety
/// `owner` must be the live local player character, on the simulation thread outside the physics
/// step.
pub(crate) unsafe fn resolve_rays(owner: &Character) {
    let cfg = Config::lock_query(|c| c.hud.world_markers);
    if !(cfg.enabled && cfg.surface_placement && cfg.ray_fallback) {
        return;
    }
    let requests = std::mem::take(&mut *REQUESTS.lock());
    if requests.is_empty() {
        return;
    }
    let head = crate::headpose::query().position;
    // SAFETY: the owner is the live local player; its vehicle pointer is live or null.
    let vehicle = unsafe { owner.GetVehiclePtr() } as usize;
    let options = CastOptions {
        ignore: [owner as *const Character as usize, vehicle],
        see_through: true,
    };
    let now = Instant::now();
    let mut results = Vec::with_capacity(requests.len().min(MAX_CASTS_PER_TICK));
    for (id, world) in requests.into_iter().take(MAX_CASTS_PER_TICK) {
        let offset = world - head;
        let distance = offset.length();
        let Some(ray) = Ray::new(head, offset) else {
            continue;
        };
        // SAFETY: forwarded from the caller; the options name the live owner and its vehicle.
        let sight = match unsafe { cast(&ray, distance, &options) } {
            Ok(Some(hit)) => Sight::Surface(hit.distance),
            Ok(None) => Sight::Clear,
            Err(_) => continue,
        };
        results.push((id, Timed { sight, at: now }));
    }
    let mut table = RESULTS.lock();
    table.get_or_insert_with(HashMap::new).extend(results);
}

/// Replace the markers awaiting a raycast (render thread).
pub(super) fn request(markers: Vec<(u32, Vec3)>) {
    *REQUESTS.lock() = markers;
}

/// The latest raycast result for each marker (render thread).
pub(super) fn result(id: u32) -> Option<Timed> {
    RESULTS.lock().as_ref()?.get(&id).copied()
}

/// Forget results older than `ttl_s` seconds (render thread).
pub(super) fn expire(ttl_s: f32) {
    if let Some(table) = RESULTS.lock().as_mut() {
        table.retain(|_, timed| timed.at.elapsed().as_secs_f32() < ttl_s);
    }
}

/// The most rays cast per simulation update; the rest wait for a later update.
const MAX_CASTS_PER_TICK: usize = 8;

/// The markers awaiting a raycast: id and world point.
static REQUESTS: Mutex<Vec<(u32, Vec3)>> = Mutex::new(Vec::new());

/// The raycast results by marker id.
static RESULTS: Mutex<Option<HashMap<u32, Timed>>> = Mutex::new(None);
