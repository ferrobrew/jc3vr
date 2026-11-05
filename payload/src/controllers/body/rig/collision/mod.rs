//! What the head runs into: the head-through-geometry probe ([`probe`]), run every simulation tick
//! in every state ([`tick`]), so the comfort fade darkens a view that has gone into the world
//! whether Rico is on foot, in the wingsuit, under the parachute, in a vehicle, or in a ragdoll.
//!
//! The probe casts from the player aim control's post-simulation update, where the game casts its
//! own aim ray, for the local player, while the headset drives the view in gameplay. The eyes are
//! the camera's: the head pose's position carried to the eyes as the camera hangs them, through
//! the neck and the eye arm. The vehicle Rico rides in is ignored along with Rico himself, since
//! between two ticks of a fast car the head's path runs through its cabin.

pub mod probe;

use std::time::Instant;

use glam::Vec3;
use jc3gi::character::character::Character;
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        body::rig::collision::probe::{HeadProbe, TimedProbe},
        roomscale::teleport,
    },
    headpose::{self, Source},
    hooks,
};

/// Probe the head for one simulation tick and publish the result ([`latest`]).
///
/// # Safety
///
/// `character` must be the live local character, in the simulation phase outside the physics
/// step.
pub(crate) unsafe fn tick(character: &Character) {
    let enabled = Config::lock_query(|c| c.controllers.roomscale.head_fade.enabled);
    let mut state = STATE.lock();
    let epoch = teleport::epoch();
    if !enabled || headpose::source() != Source::Vr || !hooks::in_gameplay() || epoch != state.epoch
    {
        state.epoch = epoch;
        state.probe.reset();
        state.latest = None;
        return;
    }
    let pose = headpose::query();
    let body = headpose::xr::body_rotation();
    let eyes = pose.position + headpose::neck_delta() + body * headpose::eye_arm();
    let anchor = headpose::anchor().unwrap_or(pose.position);
    let right = pose.orientation * Vec3::X;
    let right = Vec3::new(right.x, 0.0, right.z).normalize_or_zero();
    let vehicle = unsafe { character.GetVehiclePtr() } as usize;
    let ignore = [std::ptr::from_ref(character) as usize, vehicle];
    let scene = state.probe.step(eyes, anchor, right, |from, to| {
        // SAFETY: the simulation phase; the ignored objects are the live local character and the
        // vehicle it rides, or zero.
        unsafe { probe::scene_cast(from, to, ignore) }.unwrap_or_else(|error| {
            tracing::debug!(target: "controllers", "rig: head probe: {error}");
            None
        })
    });
    let terrain = probe::below_terrain(eyes, |from, to| {
        // SAFETY: the simulation phase.
        unsafe { probe::terrain_cast(from, to) }.unwrap_or_else(|error| {
            tracing::debug!(target: "controllers", "rig: head probe's terrain check: {error}");
            None
        })
    });
    let result = probe::with_terrain(scene, terrain);
    state.latest = Some(TimedProbe {
        result,
        at: Instant::now(),
    });
}

/// The head probe's latest result, while it runs.
pub fn latest() -> Option<TimedProbe> {
    STATE.lock().latest
}

struct State {
    probe: HeadProbe,
    /// The teleport epoch last acted on: a teleport forgets where the head was last in the open.
    epoch: u64,
    latest: Option<TimedProbe>,
}

static STATE: Mutex<State> = Mutex::new(State {
    probe: HeadProbe::new(),
    epoch: 0,
    latest: None,
});
