//! The stable view anchor's engine side: decides when the on-foot view hangs from Rico's root, feeds
//! the neutral capture, and composes the anchors the headpose receives. Called from the pre-solve
//! anchor capture at the entry of the pose-finalization pass (`hooks::character::body_ik`), once
//! per sim tick, with the animated anchors it measured. The pure parts are in
//! [`crate::controllers::locomotion::view`].

use std::time::Instant;

use glam::{Mat4, Vec3};
use jc3gi::character::character::{AimState, Character};
use parking_lot::Mutex;

use crate::{
    controllers::{
        body::procedural::{
            authority::{Case, classify},
            state::StateTracker,
        },
        config::CONTROLLERS_CONFIG,
        locomotion::{
            status as locomotion_status,
            view::{
                Eligibility, Handover, HeightFilter, MOVEMENT_TASK_GRACE_S, Neutral,
                NeutralCapture, blend, face_clear, face_offset, neck_model_anchors, stable_anchors,
            },
            view_follow::{Escape, HeadFollower, HeldSource, escape_distance},
        },
        mode::{AirborneVariant, Mode, OnFootVariant, current as current_mode},
    },
    headpose::{
        Anchors, Source,
        sim::{HeadMode, mode as head_mode},
        source,
    },
    hooks::in_gameplay,
    vr::VR_CONFIG,
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
    /// Whether Rico is off the ground on foot, from the jump's grace on: a jump, a step off a
    /// ledge, or a fall the mode detection has not reported yet. Read only in the headset with the
    /// controllers on.
    pub airborne: bool,
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
) -> Stabilized {
    let (enabled, controllers_enabled, head_on_pivot) = CONTROLLERS_CONFIG.query(|c| {
        (
            c.locomotion.view.enabled,
            c.enabled,
            c.locomotion.view.head_on_pivot,
        )
    });
    let world_scale = VR_CONFIG.lock().world_scale;
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
    let mut airborne = false;
    let case = (controllers_enabled && source() == Source::Vr).then(|| {
        let config = CONTROLLERS_CONFIG.lock().body.procedural;
        let step = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        // SAFETY: the pass's entry, on the thread running it.
        let mut read =
            unsafe { tracker.read(character, &locomotion, now, step, JUMP_GRACE_S, &config) };
        // A few ticks' jitter in the task's stamp does not hand the view over and back.
        read.movement_task = locomotion.at.is_some_and(|at| {
            now.saturating_duration_since(at).as_secs_f32() <= MOVEMENT_TASK_GRACE_S
        });
        airborne = read.jump || read.flight;
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
    let head_may_lie = case.is_none_or(Case::head_may_lie);
    let clear = state.face_clear.step(posed, dt, BLEND_S);
    let cleared = face_clear(&animated, &model, FACE_CLEARANCE_M * clear);
    state.status = AnchorStatus {
        weight,
        eligible,
        on_ground: on_ground && grounded && !ragdoll,
        airborne,
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
    let stable_target = state
        .neutral
        .filter(|_| weight > 0.0)
        .map(|neutral| stable_anchors(&neutral, world_t0, world_t1, height));
    if source() == Source::Vr {
        // A ragdoll, a get-up, a death, and a landing that carries the head well away from the
        // root hang the held view from the followed animated head rather than over the root.
        let distance =
            stable_target.map_or(0.0, |stable| escape_distance(animated.head - stable.head));
        let may_escape = case.is_some_and(Case::view_may_escape);
        // A handover from a case that held the view on the animated head (a wingsuit, a vehicle)
        // with that head far from the root's starts on it, and the view returns to the root once
        // Rico's head is over it, rather than cutting through the body as the weight comes in.
        // Likewise, a get-up or a death that ends with the head still away from the root (the case
        // reads as on foot again) keeps the view on the head until it comes back over the root,
        // rather than sliding the view back at once.
        let left_follow =
            state.held == Some(HeldSource::Head) && !case.is_some_and(Case::view_follows_head);
        if stable_target.is_some() && may_escape && (state.held.is_none() || left_follow) {
            state.escape.seed(distance);
        }
        let escaped = state.escape.step(weight > 0.0 && may_escape, distance, dt);
        let held = if case.is_some_and(Case::view_follows_head) || escaped {
            HeldSource::Head
        } else {
            HeldSource::Root
        };
        if state.held != Some(held) && stable_target.is_some() {
            tracing::debug!(
                target: "controllers",
                ?held,
                escaped,
                distance,
                "locomotion: the held view changes its head",
            );
        }
        let stable_target = match stable_target {
            Some(stable) => {
                state.held = Some(held);
                let pair = state.follower.step(held, stable.head, animated.head, dt);
                Some(Anchors {
                    head: pair.current,
                    head_prev: pair.previous,
                    neck: pair.current + (stable.neck - stable.head),
                    eye_arm: stable.eye_arm,
                })
            }
            None => {
                state.held = None;
                state.follower.reset();
                None
            }
        };
        // The headset's view takes only the camera's anchors: the body's pose and IK keep the
        // animated head, so the head bone stays on the animation while the view does not bob.
        // Under the neck model the eyes hang from the neutral head as the body's head target
        // assumes, so the head bone lands on the camera's neck pivot.
        return Stabilized {
            anatomy: animated,
            camera: stable_target.as_ref().map(|stable| {
                let stable = if head_on_pivot {
                    neck_model_anchors(stable, world_scale)
                } else {
                    *stable
                };
                blend(&animated, &stable, weight)
            }),
            weight,
            head_may_lie,
            face_offset: face_offset(&model, FACE_CLEARANCE_M * clear),
        };
    }
    let Some(stable) = stable_target else {
        return Stabilized {
            anatomy: cleared,
            camera: None,
            weight,
            head_may_lie,
            face_offset: Vec3::ZERO,
        };
    };
    Stabilized {
        anatomy: blend(&cleared, &stable, weight),
        camera: None,
        weight,
        head_may_lie,
        face_offset: Vec3::ZERO,
    }
}

/// The anchors the pose publishes: the anatomy's, and the camera's while the headset's view hangs
/// from the root instead of the animated head.
pub(crate) struct Stabilized {
    pub anatomy: Anchors,
    /// The camera's anchors, `None` where the camera uses [`anatomy`](Self::anatomy).
    pub camera: Option<Anchors>,
    /// The root-held anchor's weight against the animated head, from zero to one.
    pub weight: f32,
    /// Whether the case may carry Rico's head down to the ground ([`Case::head_may_lie`]), where
    /// the view's floor applies. Always outside the headset's cases.
    pub head_may_lie: bool,
    /// In the headset, the eyes' clearance out in front of the animated face, model space, still to
    /// be added to the eye arm once the arm is expressed in the filtered head frame (the headset's
    /// eye arm is rebuilt in that frame after this returns, so the clearance cannot ride
    /// [`anatomy`](Self::anatomy) as it does elsewhere). Zero outside the headset, where
    /// [`anatomy`](Self::anatomy) already carries it.
    pub face_offset: Vec3,
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
    /// The held view's head, over the root or following the animated head.
    follower: HeadFollower,
    /// Whether a landing or a one-off has carried the head away long enough to follow it.
    escape: Escape,
    /// The head the held view hung from last tick, while it was held.
    held: Option<HeldSource>,
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
    follower: HeadFollower::new(),
    escape: Escape::new(),
    held: None,
    status: AnchorStatus {
        weight: 0.0,
        eligible: false,
        on_ground: false,
        airborne: false,
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
