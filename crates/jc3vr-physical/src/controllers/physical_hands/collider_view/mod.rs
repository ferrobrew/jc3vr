//! The collider view: the physics world's bodies near the player drawn in the world-space
//! overlay, for seeing what the hand bodies, the held guns, and the player's capsule meet. A
//! diagnostic, off by default; the design is in `docs/mod/input/collider-view.md`.
//!
//! - [`scan`]: the read of the world after a step, on the game thread outside the step;
//! - [`model`]: the snapshot it fills, and the pure geometry over it;
//! - [`draw`]: the primitives within a budget;
//! - [`nearby`]: the reads the tracking recorder's near set makes, a slice of the walk at a time;
//! - [`config`]: the settings;
//! - the tuning, in [`crate::controllers::physical_hands::views`].
//!
//! Geometry discovery runs once per physics step, read off the step clock
//! ([`jc3vr_core::published::Step`]). After the engine's render update, retained body
//! transforms are refreshed and drawn at their current collision poses, independently of the mesh's
//! render interpolation. The same-frame mesh/body comparison reports that difference rather than
//! moving the outline to conceal it. The view's state is a game-thread lock; the overlay store and
//! status use leaf locks, and the status may be read from any thread.

pub mod config;
pub mod draw;
pub mod model;
pub mod nearby;
mod scan;

use std::sync::atomic::{AtomicBool, Ordering};

use glam::Vec3;
use jc3vr_body::controllers::held::guns;
use jc3vr_core::published::{Stamp, Step};
use jc3vr_view::{
    controllers::overlay::{
        primitive::{Primitive, Source},
        store,
    },
    headpose,
};
use parking_lot::Mutex;

pub use self::{config::ColliderViewConfig, scan::ScanError};
use self::{
    draw::{DrawInputs, Drawn, Flash},
    model::{Aabb, BodyKind, JointKind, MotionKind, Snapshot, gazed},
    scan::ModIds,
};
use crate::controllers::physical_hands::{
    config::PHYSICAL_HANDS_CONFIG,
    physics,
    views::{ColliderViewTuning, VIEWS},
};

/// The view's readout for the debug UI and the status rows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ColliderViewStatus {
    /// Whether the view is switched on.
    pub enabled: bool,
    /// The physics steps begun since the payload loaded, on the step clock ([`Step`]).
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
    /// The body under the gaze at the last frame.
    pub gaze: Option<GazeReadout>,
    pub last_error: Option<String>,
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
    let mut config = PHYSICAL_HANDS_CONFIG.edit();
    let view = &mut config.collider_view;
    view.enabled = !view.enabled;
    tracing::info!(enabled = view.enabled, "collider view: toggled");
}

/// Discover geometry after a step and submit current collision poses after the mesh render updates.
/// The render fraction identifies the mesh sample; collision geometry is not interpolated by it.
///
/// # Safety
///
/// The game thread, after the engine's render update and outside the physics step.
pub unsafe fn render_tick(dtf: f32) {
    let enabled = PHYSICAL_HANDS_CONFIG.query(|c| c.collider_view.enabled);
    if !enabled || !jc3vr_core::hooks::in_gameplay() {
        if WAS_ACTIVE.swap(false, Ordering::Relaxed) {
            store::clear(Source::ColliderView);
            *STATE.lock() = State::EMPTY;
            let mut status = STATUS.lock();
            *status = ColliderViewStatus {
                enabled,
                steps: status.steps,
                reads: status.reads,
                ..ColliderViewStatus::default()
            };
        }
        return;
    }
    WAS_ACTIVE.store(true, Ordering::Relaxed);

    let tuning = &VIEWS.get().collider_view;
    let steps = Step::now().count();
    let head = headpose::query();
    let read = {
        let state = STATE.lock();
        state.read_at != Some(steps) || state.current.is_none()
    };
    let outcome = read.then(|| {
        let mods = mod_ids();
        let started = Stamp::now();
        // SAFETY: forwarded from the caller.
        let scanned = unsafe {
            scan::scan(
                head.position,
                tuning.radius_m,
                tuning.max_bodies,
                true,
                &mods,
            )
        };
        (scanned, started.elapsed())
    });
    let flashes = take_flashes(tuning);

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
    let gaze = gazed(
        &current.bodies,
        head.position,
        head.orientation * Vec3::NEG_Z,
        tuning.radius_m * tuning.gaze_reach_per_radius,
    );
    let drawn = draw::build(
        &DrawInputs {
            current,
            previous: None,
            dtf,
            budget: tuning.max_primitives,
            tuning: &tuning.draw,
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
    store::submit(Source::ColliderView, &mut state.primitives, tuning.ttl());

    let mut status = STATUS.lock();
    status.enabled = true;
    status.steps = steps;
    status.scanned = scanned;
    status.in_range = in_range;
    status.drawn = drawn;
    status.primitives = primitives;
    status.gaze = gaze;
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
    // SAFETY: forwarded from the caller.
    unsafe { scan::scan(centre, radius_m, max_bodies as usize, false, &mod_ids()) }
}

/// The ids and boxes (game space) of the dynamic bodies whose boxes `keep` accepts, at most
/// `max_bodies` of them, whether or not the view is on. Reads no shapes, so it is cheap enough to
/// run every tick over the whole world.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub(crate) unsafe fn dynamic_boxes(
    keep: impl FnMut(&Aabb) -> bool,
    max_bodies: usize,
) -> Result<Vec<(u32, Aabb)>, ScanError> {
    // SAFETY: forwarded from the caller.
    unsafe { scan::dynamic_boxes(keep, max_bodies) }
}

/// Flash a contact a hand body or a held gun's body started: where the body was before the step
/// (game space), the other body, and the contact's impulse. Called on the game thread as the
/// contacts are delivered.
pub(crate) fn note_contact(at: Vec3, other_body: u32, impulse_ns: f32) {
    if !WAS_ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    FLASHES
        .lock()
        .push((Stamp::now(), at, other_body, impulse_ns));
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
    gaze: None,
    last_error: None,
});

/// The contacts reported lately, oldest first: when, where, the other body, and the impulse.
static FLASHES: Mutex<Vec<(Stamp, Vec3, u32, f32)>> = Mutex::new(Vec::new());

/// Whether the view drew last frame, so that switching it off clears its slot once.
static WAS_ACTIVE: AtomicBool = AtomicBool::new(false);

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

/// The newest contacts still flashing, at most `tuning`'s number of them, with their ages; the
/// spent and the older ones are dropped. The cap is applied here, once a frame, rather than as
/// each contact is noted, in the physics' per-contact loop.
fn take_flashes(tuning: &ColliderViewTuning) -> Vec<Flash> {
    let now = Stamp::now();
    let flash_s = tuning.draw.flash_s;
    let mut flashes = FLASHES.lock();
    flashes.retain(|(at, ..)| now.saturating_duration_since(*at).as_secs_f32() < flash_s);
    let over = flashes.len().saturating_sub(tuning.max_flashes);
    flashes.drain(..over);
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
