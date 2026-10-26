//! The collider view: the physics world's bodies near the player drawn in the world-space
//! overlay, for seeing what the hand bodies, the held guns, and the player's capsule meet. A
//! diagnostic, off by default; the design is in `docs/mod/input/collider-view.md`.
//!
//! - [`callback`]: a pre-simulation callback that counts the physics steps;
//! - [`scan`]: the read of the world after a step, on the game thread outside the step;
//! - [`model`]: the snapshot it fills, and the pure geometry over it;
//! - [`draw`]: the primitives within a budget;
//! - [`config`]: the settings.
//!
//! Geometry discovery runs once per physics step. After the engine's render update, retained body
//! transforms are refreshed and drawn at their current collision poses, independently of the mesh's
//! render interpolation. The same-frame mesh/body comparison reports that difference rather than
//! moving the outline to conceal it. The view's state is a game-thread lock; the overlay store and
//! status use leaf locks, and the status may be read from any thread.

mod callback;
pub mod config;
pub mod draw;
pub mod model;
mod scan;

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use glam::Vec3;
use parking_lot::Mutex;

pub use self::{config::ColliderViewConfig, scan::ScanError};
use self::{
    draw::{DrawInputs, Drawn, FLASH_S, Flash},
    model::{BodyKind, JointKind, MotionKind, Snapshot, gazed},
    scan::ModIds,
};
use crate::{
    config::Config,
    controllers::{
        frame::Hand,
        hands::late,
        overlay::{
            primitive::{Primitive, Source},
            store,
        },
        physical_hands::physics::{self, guns},
    },
    headpose, hooks,
};

/// The view's readout for the debug UI and the status rows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ColliderViewStatus {
    /// Whether the view is switched on.
    pub enabled: bool,
    /// Whether the step counter is registered with the physics system.
    pub registered: bool,
    /// The physics steps counted since the payload loaded.
    pub steps: u64,
    /// The reads of the world since the payload loaded.
    pub reads: u64,
    /// The body slots the last read went through.
    pub scanned: u32,
    /// The bodies within the radius at the last read.
    pub in_range: u32,
    /// What the last frame drew.
    pub drawn: Drawn,
    /// The primitives the last frame submitted.
    pub primitives: usize,
    /// How long the last read took, microseconds.
    pub read_us: u32,
    /// Render-time transform samples, including frames without a physics step.
    pub pose_reads: u64,
    /// The physics step identified by the current body-pose sample.
    pub pose_step: u64,
    /// The render fraction, for comparison with the separately interpolated visible meshes.
    pub render_fraction: f32,
    /// Time since the frame's body poses were sampled, microseconds.
    pub pose_age_us: u32,
    /// Mesh versus current collision pose for each hand's identified held gun.
    pub gun_pose_gaps: [Option<GunPoseGap>; 2],
    /// The body under the gaze at the last frame.
    pub gaze: Option<GazeReadout>,
    pub last_error: Option<String>,
}

/// A current collision body compared with the mesh submitted in this render frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GunPoseGap {
    pub weapon: usize,
    pub body: u32,
    pub translation_m: f32,
    pub rotation_deg: f32,
    pub mesh_age_us: u32,
    pub render_fraction: f32,
}

/// The body under the gaze, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GazeReadout {
    pub id: u32,
    pub kind: BodyKind,
    pub motion: MotionKind,
    /// The collision-filter word.
    pub filter_info: u32,
    pub mass_kg: Option<f32>,
    pub shape: &'static str,
    /// How far the body's box is from the player, metres.
    pub distance_m: f32,
}

/// The latest readout.
pub fn status() -> ColliderViewStatus {
    STATUS.lock().clone()
}

/// Switch the view on or off.
pub fn toggle() {
    let mut config = crate::config::edit();
    let view = &mut config.controllers.physical_hands.collider_view;
    view.enabled = !view.enabled;
    tracing::info!(enabled = view.enabled, "collider view: toggled");
}

/// Register the step counter while the view is on, and remove it while it is off. Called once
/// per frame on the game thread, ahead of the game's update.
pub(crate) fn frame_tick() {
    let wanted = !crate::is_shutting_down()
        && Config::lock_query(|c| c.controllers.physical_hands.collider_view.enabled);
    // SAFETY: the game thread, ahead of the game's update, which fires the callbacks.
    let registered = unsafe { callback::reconcile(wanted) };
    if registered && !CLEANUP_REGISTERED.swap(true, Ordering::AcqRel) {
        crate::lifecycle::on_cleanup(|_renderer| {
            // SAFETY: the shutdown cleanups run on the game thread, outside the game's update.
            unsafe { callback::unregister() };
        });
    }
    STATUS.lock().registered = registered;
}

/// Discover geometry after a step and submit current collision poses after the mesh render updates.
/// The render fraction identifies the mesh sample; collision geometry is not interpolated by it.
///
/// # Safety
///
/// The game thread, after the engine's render update and outside the physics step.
pub(crate) unsafe fn render_tick(dtf: f32) {
    let config = Config::lock_query(|c| c.controllers.physical_hands.collider_view);
    if !config.enabled || !hooks::in_gameplay() {
        if WAS_ACTIVE.swap(false, Ordering::Relaxed) {
            store::clear(Source::ColliderView);
            *STATE.lock() = State::EMPTY;
            let mut status = STATUS.lock();
            *status = ColliderViewStatus {
                enabled: config.enabled,
                registered: status.registered,
                steps: status.steps,
                reads: status.reads,
                ..ColliderViewStatus::default()
            };
        }
        return;
    }
    WAS_ACTIVE.store(true, Ordering::Relaxed);

    let steps = callback::steps();
    let head = headpose::query();
    let read = {
        let state = STATE.lock();
        state.read_at != Some(steps) || state.current.is_none()
    };
    let outcome = read.then(|| {
        let mods = mod_ids();
        let started = Instant::now();
        // SAFETY: forwarded from the caller.
        let scanned = unsafe { scan::scan(head.position, &config, &mods) };
        (scanned, started.elapsed())
    });
    let flashes = take_flashes();

    let mut state = STATE.lock();
    let state = &mut *state;
    let mut error = None;
    let mut took = None;
    if let Some((scanned, elapsed)) = outcome {
        state.read_at = Some(steps);
        took = Some(elapsed);
        match scanned {
            Ok(snapshot) => state.previous = state.current.replace(snapshot),
            Err(e) => {
                state.previous = None;
                state.current = None;
                error = Some(e.to_string());
            }
        }
    }
    let sampled_at = Instant::now();
    if let Some(current) = state.current.as_mut() {
        // SAFETY: forwarded from the caller; only retained transforms and boxes are read here.
        if let Err(e) = unsafe { scan::refresh(current, head.position) } {
            state.previous = None;
            state.current = None;
            error = Some(e.to_string());
        }
    }
    let Some(current) = state.current.as_ref() else {
        store::clear(Source::ColliderView);
        let mut status = STATUS.lock();
        status.enabled = true;
        status.steps = steps;
        if error.is_some() {
            status.last_error = error;
        }
        return;
    };
    let gaze = config
        .gaze
        .then(|| {
            gazed(
                &current.bodies,
                head.position,
                head.orientation * Vec3::NEG_Z,
                config.radius_m.max(0.0) * 2.0,
            )
        })
        .flatten();
    let drawn = draw::build(
        &DrawInputs {
            current,
            previous: None,
            dtf,
            config: &config,
            flashes: &flashes,
            gaze: gaze.map(|b| b.id),
        },
        &mut state.primitives,
    );
    let primitives = state.primitives.len();
    let gaze = gaze.map(|b| GazeReadout {
        id: b.id,
        kind: b.kind,
        motion: b.motion,
        filter_info: b.filter_info,
        mass_kg: b.mass_kg,
        shape: b.geometry.label(),
        distance_m: b.distance_m,
    });
    let (scanned, in_range) = (current.scanned, current.in_range);
    let rendered = late::rendered_guns();
    let physical = guns::status();
    let gun_pose_gaps = Hand::BOTH.map(|hand| {
        let mesh = rendered[hand.index()]?;
        let gun = physical.hands[hand.index()].as_ref()?;
        if mesh.weapon != gun.weapon || !gun.in_world {
            return None;
        }
        let body = current.body(gun.body_id)?;
        let (_, rotation, position) = mesh.matrix.to_scale_rotation_translation();
        let (_, body_rotation, body_position) = body.transform.to_scale_rotation_translation();
        Some(GunPoseGap {
            weapon: mesh.weapon,
            body: gun.body_id,
            translation_m: position.distance(body_position),
            rotation_deg: rotation.angle_between(body_rotation).to_degrees(),
            mesh_age_us: u32::try_from(mesh.at.elapsed().as_micros()).unwrap_or(u32::MAX),
            render_fraction: mesh.fraction,
        })
    });
    store::submit(Source::ColliderView, &mut state.primitives, TTL);

    let mut status = STATUS.lock();
    status.enabled = true;
    status.steps = steps;
    status.scanned = scanned;
    status.in_range = in_range;
    status.drawn = drawn;
    status.primitives = primitives;
    status.gaze = gaze;
    status.pose_reads += 1;
    status.gun_pose_gaps = gun_pose_gaps;
    status.pose_step = steps;
    status.render_fraction = dtf;
    status.pose_age_us = u32::try_from(sampled_at.elapsed().as_micros()).unwrap_or(u32::MAX);
    if let Some(took) = took {
        status.reads += 1;
        status.read_us = u32::try_from(took.as_micros()).unwrap_or(u32::MAX);
    }
    if error.is_some() {
        status.last_error = error;
    }
}

/// Read the moving bodies whose boxes come within `radius_m` of `centre` (game space), the
/// nearest `max_bodies` of them, whether or not the view is on: the grab view's candidates
/// ([`crate::controllers::physical_hands::grab_view`]). Static bodies are left out, but for the
/// player's ground.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub(crate) unsafe fn scan_moving(
    centre: Vec3,
    radius_m: f32,
    max_bodies: u32,
) -> Result<Snapshot, ScanError> {
    let config = ColliderViewConfig {
        radius_m,
        max_bodies,
        static_bodies: false,
        ..ColliderViewConfig::new()
    };
    // SAFETY: forwarded from the caller.
    unsafe { scan::scan(centre, &config, &mod_ids()) }
}

/// Flash a contact a hand body or a held gun's body started: where the body was before the step
/// (game space), the other body, and the contact's impulse. Called on the game thread as the
/// contacts are delivered.
pub(crate) fn note_contact(at: Vec3, other_body: u32, impulse_ns: f32) {
    if !WAS_ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    let mut flashes = FLASHES.lock();
    if flashes.len() >= MAX_FLASHES {
        flashes.remove(0);
    }
    flashes.push((Instant::now(), at, other_body, impulse_ns));
}

/// The view's state across frames.
struct State {
    /// The physics step count at the last read.
    read_at: Option<u64>,
    /// The read before the last.
    previous: Option<Snapshot>,
    /// The last read.
    current: Option<Snapshot>,
    /// The frame's primitives, reused across submissions.
    primitives: Vec<Primitive>,
}

impl State {
    const EMPTY: State = State {
        read_at: None,
        previous: None,
        current: None,
        primitives: Vec::new(),
    };
}

static STATE: Mutex<State> = Mutex::new(State::EMPTY);

static STATUS: Mutex<ColliderViewStatus> = Mutex::new(ColliderViewStatus {
    enabled: false,
    registered: false,
    steps: 0,
    reads: 0,
    scanned: 0,
    in_range: 0,
    drawn: Drawn {
        bodies: 0,
        dropped: 0,
        kinds: [0; BodyKind::COUNT],
    },
    primitives: 0,
    read_us: 0,
    pose_reads: 0,
    pose_step: 0,
    render_fraction: 0.0,
    pose_age_us: 0,
    gun_pose_gaps: [None; 2],
    gaze: None,
    last_error: None,
});

/// The contacts reported lately: when, where, the other body, and the impulse.
static FLASHES: Mutex<Vec<(Instant, Vec3, u32, f32)>> = Mutex::new(Vec::new());

/// Whether the view drew last frame, so that switching it off clears its slot once.
static WAS_ACTIVE: AtomicBool = AtomicBool::new(false);

static CLEANUP_REGISTERED: AtomicBool = AtomicBool::new(false);

/// The view submits every rendered frame, so its slot lives a little past one.
const TTL: Duration = Duration::from_millis(100);

/// The most contacts kept flashing at once.
const MAX_FLASHES: usize = 32;

/// The mod's bodies and joints, from the hand bodies' and the held guns' readouts.
fn mod_ids() -> ModIds {
    let hands = physics::status();
    let guns = guns::status();
    let mut ids = ModIds::default();
    for hand in &hands.hands {
        ids.hands.extend(hand.body_id);
        ids.joints
            .extend(hand.joint.map(|constraint| (JointKind::Grip, constraint)));
    }
    for gun in guns.hands.iter().flatten() {
        ids.guns.push(gun.body_id);
        ids.joints.extend(
            gun.holder_joint
                .map(|constraint| (JointKind::GunGrip, constraint)),
        );
        ids.joints.extend(
            gun.brace_joint
                .map(|constraint| (JointKind::GunBrace, constraint)),
        );
    }
    ids
}

/// The contacts still flashing, with their ages; the spent ones are dropped.
fn take_flashes() -> Vec<Flash> {
    let now = Instant::now();
    let mut flashes = FLASHES.lock();
    flashes.retain(|(at, ..)| now.saturating_duration_since(*at).as_secs_f32() < FLASH_S);
    flashes
        .iter()
        .map(|&(born, at, body, impulse_ns)| Flash {
            at,
            body: Some(body),
            age_s: now.saturating_duration_since(born).as_secs_f32(),
            impulse_ns,
        })
        .collect()
}
