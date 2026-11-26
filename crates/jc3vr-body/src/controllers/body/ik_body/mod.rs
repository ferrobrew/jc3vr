//! The IK-first on-foot body: while Rico stands on foot, his pelvis goes where the learned
//! estimate's forward kinematics puts the player's, his feet stay planted on the ground by HumanIK
//! and move only by explicit steps, and his root follows the floor point under the pelvis; in the
//! air the pelvis hangs under the head with the legs relaxed toward a light air pose; on the walk
//! a share of the pose turns the clip's pelvis. See `docs/mod/body/ik-body.md`.
//!
//! - [`fk`]: the FK pelvis, advanced once per rendered frame from the root's frame half in the
//!   rig's room frame ([`body_rig::fk`]).
//! - [`gate`]: at the procedural body's solve gate, the state and why the IK stands down, the feet's
//!   contacts in the world ([`body_rig::contact`]), and the IK pose seeded into Rico's posture
//!   before the torso is solved on it.
//! - [`climb`]: the feet braced on the face while the hands climb ([`body_rig::brace`]), from the
//!   casts [`brace`] makes where the probes cast.
//! - [`effectors`]: the HumanIK targets that hold the seeded pelvis and place the feet, owned
//!   against the game's locomotion IK task.
//! - [`probe`]: the feet's ground probes, cast where the head probe casts.
//! - [`support`]: the bodies the feet stand on, snapshotted where the probes cast and carried
//!   forward to the gate, so a moving deck carries the planted feet.
//! - [`seed`]: the geometry of the seeded pose (pure).
//! - [`config`]: the firm ragdoll's target switch and the contacts' tuning.
//!
//! The rest of the mod reads [`owns_idle`] (the act filter drops the game's turn-in-place acts
//! while the IK turns the feet itself) and [`root_target`] (the grounded rig's drive follows the FK
//! pelvis rather than committing steps after the neck).

pub(crate) mod brace;
pub(crate) mod climb;
pub mod config;
pub(crate) mod effectors;
pub mod fk;
pub mod gate;
pub(crate) mod probe;
pub(crate) mod seed;
pub(crate) mod support;

use std::time::{Duration, Instant};

use body_rig::{PlayerBody, body::EYES_TO_NECK, fk::FkTuning, ik_weight::IkState};
use glam::{Quat, Vec2, Vec3};
use parking_lot::Mutex;

pub use crate::controllers::body::ik_body::{
    config::IkBodyConfig,
    gate::{IkStatus, StandDown},
};
pub(crate) use crate::controllers::body::ik_body::{
    effectors::queue,
    gate::{GateInput, prepare},
};
use crate::controllers::body::{
    BodyDimensions, estimator,
    ik_body::{fk::FkInput, gate::GateState},
    rig::grounded,
};

/// Whether the IK body owns the standing body now: its feet are planted by IK, so the game's
/// turn-in-place acts must not turn the clip's.
pub fn owns_idle() -> bool {
    let state = STATE.lock();
    fresh(&state.status) && state.status.state == IkState::Idle && state.status.weights.feet > 0.0
}

/// The floor point under the FK pelvis, room `(x, z)`, while the IK body owns the root: the
/// grounded rig's drive follows it.
pub fn root_target() -> Option<Vec2> {
    let owns = {
        let state = STATE.lock();
        fresh(&state.status)
            && state.status.state == IkState::Idle
            && state.status.weights.pelvis_position >= ROOT_WEIGHT
    };
    if !owns {
        return None;
    }
    fk::latest().map(|pose| pose.pelvis_floor())
}

/// The last gate's status.
pub fn status() -> IkStatus {
    STATE.lock().status
}

/// Advance the FK pelvis by one rendered frame: the root's frame half calls it before the grounded
/// rig. `eyes` and `head` are the room frame's, as the rig takes them.
pub(crate) fn frame(eyes: Vec3, head: Quat, dimensions: &BodyDimensions, dt: f32, now: Instant) {
    let diagnostics = grounded::diagnostics();
    let input = FkInput {
        eyes,
        head,
        estimate: estimator::latest(),
        body: PlayerBody::from_heights(dimensions.height, dimensions.eye_height),
        standing_neck: grounded::standing_eye_height() + EYES_TO_NECK.y,
        waist: diagnostics.map(|d| d.waist),
        yaw: grounded::body_yaw().unwrap_or(0.0),
        tuning: FkTuning::new(),
        dt,
        now,
    };
    fk::frame(&input);
}

/// The procedural body wrote no pose this tick, or its gate did not run: nothing holds the seeded
/// pelvis, so the targets are dropped and the blend starts over.
pub(crate) fn not_written() {
    let mut state = STATE.lock();
    state.targets = None;
    state.blend.reset();
    if state.status.state != IkState::Unchanged {
        state.status.stand_down = Some(StandDown::NotWritten);
    }
    state.status.weights = body_rig::ik_weight::IkWeights::ZERO;
    state.drop_contacts();
}

/// Cast the feet's ground probes, from the player aim control's post-simulation update.
///
/// # Safety
///
/// `character` must be the live local character, in the simulation phase outside the physics
/// step.
pub(crate) unsafe fn probe_tick(character: &jc3gi::character::character::Character) {
    // SAFETY: forwarded from the caller.
    unsafe { probe::tick(character) };
}

/// Forget the followed body and the feet, for a teleport.
pub(crate) fn forget() {
    fk::forget();
    probe::forget();
    brace::forget();
    STATE.lock().drop_contacts();
}

/// Whether the last gate ran recently.
fn fresh(status: &IkStatus) -> bool {
    status
        .at
        .is_some_and(|at| Instant::now().saturating_duration_since(at) <= GATE_FRESH)
}

/// The pelvis position's weight from which the root follows the FK pelvis: past half way into the
/// standing body, so the step commit keeps the root through the start of the blend.
const ROOT_WEIGHT: f32 = 0.5;

/// How long after the gate last ran its status still stands: a few ticks.
const GATE_FRESH: Duration = Duration::from_millis(250);

/// The gate's state. Taken by the gate, the effectors, and the readers of the status; nothing is
/// locked under it.
pub(crate) static STATE: Mutex<GateState> = Mutex::new(GateState::new());
