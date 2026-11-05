//! The stable view anchor's engine side: decides when the on-foot view hangs from Rico's root, feeds
//! the neutral capture, and composes the anchors the headpose receives. Called from the pre-solve
//! anchor capture at the entry of the pose-finalization pass (`hooks::character::body_ik`), once
//! per sim tick, with the animated anchors it measured. The pure parts are in
//! [`crate::controllers::locomotion::view`].

use std::time::Instant;

use glam::Mat4;
use jc3gi::character::character::{AimState, Character};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        body::procedural::{
            authority::{Case, classify},
            state::StateTracker,
        },
        locomotion::{
            status as locomotion_status,
            view::{
                Eligibility, Handover, HeightFilter, MOVEMENT_TASK_GRACE_S, Neutral,
                NeutralCapture, blend, face_clear, stable_anchors,
            },
        },
        mode::{AirborneVariant, Mode, OnFootVariant, current as current_mode},
    },
    headpose::{
        Anchors, Source,
        sim::{HeadMode, mode as head_mode},
        source,
    },
    hooks::in_gameplay,
};

/// What the stable anchor did on its last tick, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct AnchorStatus {
    /// The weight of the root-fixed anchor against the animated head: one on foot once the neutral
    /// head is measured, zero in a vehicle, a long fall, a ragdoll, or a swim.
    pub weight: f32,
    /// Whether the view should hang from the root this tick.
    pub eligible: bool,
    /// Whether Rico is on foot on the ground, out of a ragdoll and the water, whatever the
    /// settings say.
    pub on_ground: bool,
    /// The measured neutral head, neck, and eyes, model space.
    pub neutral: Option<Neutral>,
    /// The seconds of standing still sampled toward the measurement.
    pub capture_progress: f32,
    /// The smoothed root height minus the raw one, metres: what the height smoothing hides.
    pub height_offset: f32,
}

/// The last tick's status.
pub fn status() -> AnchorStatus {
    STATE.lock().status
}

/// Forget the measured neutral head so that the next stretch of standing still measures it again.
pub fn remeasure() {
    let mut state = STATE.lock();
    state.neutral = None;
    state.capture.interrupt();
}

/// Turn the animated anchors into the ones the view hangs from. `model` is this tick's animated
/// head, neck, and eye midpoint in model space, which feeds the neutral measurement. `dt` is the
/// pass's step.
///
/// # Safety
///
/// `character` must be the live local character, at the entry of its pose-finalization pass.
pub(crate) unsafe fn stabilize(
    character: &Character,
    animated: Anchors,
    model: Neutral,
    dt: f32,
) -> Anchors {
    let (enabled, controllers_enabled) =
        Config::lock_query(|c| (c.controllers.locomotion.view.enabled, c.controllers.enabled));
    let now = Instant::now();
    let locomotion = locomotion_status();
    let grounded = locomotion.fresh(now);
    let detection = current_mode();
    let traversing = detection.traversal.is_some();
    let on_ground = if controllers_enabled {
        detection.mode == Mode::OnFoot(OnFootVariant::Ground) && !traversing
    } else {
        head_mode() == HeadMode::OnFoot
    };
    let falling = controllers_enabled
        && detection.mode == Mode::Airborne(AirborneVariant::Falling)
        && !traversing;
    // In the headset the procedural body's hand-over table says who moves the body, read with the
    // anchor's own history; the movement task's stamp it reads is written only then. Elsewhere the
    // mode alone decides, as it always has.
    let mut tracker = STATE.lock().tracker;
    let case = (controllers_enabled && source() == Source::Vr).then(|| {
        let config = Config::lock_query(|c| c.controllers.body.procedural);
        let step = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        // SAFETY: the pass's entry, on the thread running it.
        let mut read =
            unsafe { tracker.read(character, &locomotion, now, step, JUMP_GRACE_S, &config) };
        // A few ticks' jitter in the task's stamp does not hand the view over and back.
        read.movement_task = locomotion.at.is_some_and(|at| {
            now.saturating_duration_since(at).as_secs_f32() <= MOVEMENT_TASK_GRACE_S
        });
        classify(&read)
    });
    let ragdoll = character.m_CurrentMotionState == Character::MOTION_STATE_RAGDOLL
        || character.m_CurrentMotionState == Character::MOTION_STATE_SWIMMING;
    let aiming = character
        .m_AimFlags
        .intersects(AimState::m_AimingWeapon | AimState::m_AimingGrapple);
    let world_t0 = Mat4::from(character.m_WorldMatrixT0);
    let world_t1 = Mat4::from(character.m_WorldMatrixT1);

    let mut state = STATE.lock();
    state.tracker = tracker;
    if on_ground && grounded {
        state.grounded_at = Some(now);
    }
    let in_grace = state
        .grounded_at
        .is_some_and(|at| now.saturating_duration_since(at).as_secs_f32() <= JUMP_GRACE_S);
    let eligible = Eligibility {
        enabled: enabled && in_gameplay(),
        ragdoll,
        on_ground,
        falling_in_grace: falling && in_grace,
        case,
    }
    .root_fixed();

    let measuring = state.neutral.is_none()
        && eligible
        && on_ground
        && grounded
        && !aiming
        && locomotion.standing_still();
    if measuring {
        if let Some(neutral) = state.capture.push(model, dt, CAPTURE_S) {
            log_neutral(&neutral);
            state.neutral = Some(neutral);
        }
    } else {
        state.capture.interrupt();
    }

    // The smoothing holds only while the proxy stands on the ground this tick: the movement
    // task's stamp outlives a takeoff by a few ticks, and following a jump's rise for those would
    // hold the view down and then let it go at once.
    // SAFETY: the pass's entry, on the thread running it.
    let supported = on_ground && !unsafe { character.IsInAir() };
    let height = state
        .height
        .step(world_t1.w_axis.y, dt, eligible && supported);
    let measured = state.neutral.is_some();
    let weight = state.handover.step(eligible && measured, dt, BLEND_S);
    // Where a clip carries the head, the eyes are held out in front of its face.
    let posed = case.is_some_and(Case::game_poses_body);
    let clear = state.face_clear.step(posed, dt, BLEND_S);
    let animated = face_clear(&animated, &model, FACE_CLEARANCE_M * clear);
    state.status = AnchorStatus {
        weight,
        eligible,
        on_ground: on_ground && grounded && !ragdoll,
        neutral: state.neutral,
        capture_progress: state.capture.progress(),
        height_offset: height.current - world_t1.w_axis.y,
    };
    let stable = weight > 0.0;
    if stable != state.stable {
        state.stable = stable;
        tracing::debug!(
            target: "controllers",
            stable,
            "locomotion: the view anchor hands over",
        );
    }
    let Some(neutral) = state.neutral.filter(|_| weight > 0.0) else {
        return animated;
    };
    blend(
        &animated,
        &stable_anchors(&neutral, world_t0, world_t1, height),
        weight,
    )
}

/// How far in front of the animated face the eyes are held while a clip poses the body, metres: a
/// few centimetres past the skull's front, so a chest or a shoulder the pose folds toward the face
/// stays beyond the near plane.
const FACE_CLEARANCE_M: f32 = 0.06;

/// How long Rico must have stood still for his neutral head to be measured, seconds.
const CAPTURE_S: f32 = 1.5;

/// How long the anchor takes to hand over between the animated head and the root, seconds.
const BLEND_S: f32 = 0.25;

/// How long after leaving the ground the anchor stays on the root, seconds, so a jump keeps the view
/// steady while a long fall hands it back to the animation.
pub(crate) const JUMP_GRACE_S: f32 = 1.0;

struct State {
    neutral: Option<Neutral>,
    capture: NeutralCapture,
    height: HeightFilter,
    handover: Handover,
    /// The weight of the face clearance, in while a clip poses the body.
    face_clear: Handover,
    /// The hand-over table's history for the anchor's own reads, apart from the procedural body's.
    tracker: StateTracker,
    grounded_at: Option<Instant>,
    /// Whether the root-fixed anchor had any weight last tick, for the handover log.
    stable: bool,
    status: AnchorStatus,
}

static STATE: Mutex<State> = Mutex::new(State {
    neutral: None,
    capture: NeutralCapture::new(),
    height: HeightFilter::new(),
    handover: Handover::new(),
    face_clear: Handover::new(),
    tracker: StateTracker::new(),
    grounded_at: None,
    stable: false,
    status: AnchorStatus {
        weight: 0.0,
        eligible: false,
        on_ground: false,
        neutral: None,
        capture_progress: 0.0,
        height_offset: 0.0,
    },
});

fn log_neutral(neutral: &Neutral) {
    let eye_height = neutral.eyes.y;
    tracing::info!(
        target: "controllers",
        head = ?neutral.head,
        neck = ?neutral.neck,
        eyes = ?neutral.eyes,
        eye_height,
        "locomotion: measured Rico's neutral standing head (model space)",
    );
}
