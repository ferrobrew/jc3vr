//! Keeping the character's body aligned with the player's (`docs/mod/body/alignment.md`).
//!
//! - The **torso estimate** ([`torso`](crate::controllers::body::torso)) runs once per rendered VR
//!   frame ([`frame_tick`]) from the cockpit-frame head and the raw controller grips, and is
//!   published for the rest of the mod.
//! - The **body turning under the view** ([`crate::headpose::follow`]) reads the estimate's heading
//!   on the input tick through [`follow_settings`].
//! - The **chest turn** ([`chest_turn`]) is the torso's heading relative to the character's facing,
//!   which the body IK turns the chest by and the squared stance turns the pelvis and feet by.
//! - The **skeleton** is read after the pose-finalization pass's solves ([`after_solve`]): the hip
//!   and shoulder joints the holsters can ride ([`skeleton_in_cockpit`]), and the divergence between
//!   them and the estimate, published for the debug UI and logged once a second.
//!
//! [`config`] holds the settings and [`measure`] the pure frames and divergence.

pub mod config;
pub mod measure;

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use glam::{Mat4, Quat, Vec3};
use jc3gi::{
    animation::ik::Effector,
    character::character::{AnimationController, Character, Joint, SafeBoneIndex},
};
use parking_lot::Mutex;

pub use crate::controllers::body::alignment::config::AlignmentConfig;
use crate::{
    config::Config,
    controllers::{
        body::{
            self,
            alignment::measure::{Divergence, Frames, SkeletonLandmarks, divergence, upright},
            torso::{TorsoEstimate, TorsoEstimator, TorsoInputs, wrap, yaw_forward, yaw_of},
        },
        frame::{self, Hand, Pose},
    },
    headpose::{self, xr::FollowSettings},
    hooks::{self, character::body_ik::bone_for_effector},
};

/// The tracing target for the alignment's once-a-second log. Public so that the debug UI can turn
/// it up to `DEBUG`.
pub const ALIGNMENT_TARGET: &str = "body_alignment";

/// Advance the torso estimate from this frame's cockpit-frame head pose and the latest controller
/// grips. Called once per rendered VR frame on the game thread, from the pose composition.
pub fn frame_tick(head: Pose) {
    let config = Config::lock_query(|c| c.controllers.body.alignment);
    let now = Instant::now();
    if !config.estimate {
        let mut state = STATE.lock();
        state.estimator.reset();
        state.estimate = None;
        state.last_tick = None;
        return;
    }
    let hands = frame::with_latest(|frame| {
        Hand::BOTH.map(|hand| {
            let h = frame.hand(hand);
            (frame.focused && h.active && h.grip.position_valid).then_some(h.grip.pose.position)
        })
    })
    .unwrap_or([None; 2]);
    let dims = body::player();
    let inputs = TorsoInputs {
        head_position: head.position,
        head_orientation: head.orientation,
        hands,
    };
    let mut state = STATE.lock();
    let dt = state.last_tick.map_or(0.0, |last| {
        now.saturating_duration_since(last).as_secs_f32()
    });
    state.last_tick = Some(now);
    if dt > ESTIMATE_GAP.as_secs_f32() {
        state.estimator.reset();
    }
    let estimate = state.estimator.update(&inputs, dt, &dims, &config.torso);
    state.estimate = estimate.map(|estimate| (estimate, now));
}

/// The latest torso estimate, when it is fresh.
pub fn estimate() -> Option<TorsoEstimate> {
    fresh_estimate(&STATE.lock())
}

/// How the body follows the torso on this input tick: on with the setting and the estimate, in
/// gameplay, while the headset drives the view.
pub fn follow_settings() -> FollowSettings {
    let config = Config::lock_query(|c| c.controllers.body.alignment);
    if !config.root_follow
        || !config.estimate
        || headpose::source() != headpose::Source::Vr
        || !hooks::in_gameplay()
    {
        return FollowSettings::OFF;
    }
    FollowSettings {
        enabled: true,
        torso: estimate().map(|e| e.yaw),
        deadband_deg: config.follow_deadband_deg,
    }
}

/// Turn the estimate with the cockpit frame, for a recenter that turned the frame by `turn` (new
/// frame from old), so that the torso stays where it is in the room.
pub fn on_recenter(turn: Quat) {
    let mut state = STATE.lock();
    state.estimator.rotate(turn);
    // A recentre may move the cockpit's origin to the head and its height with it; the feet start
    // again under the head.
    state.estimator.reset_stance();
    state.estimate = None;
}

/// The torso's heading in the character's model space, relative to his facing, for a character
/// whose world rotation is `character_rotation`: what the chest turns by to face the player's
/// torso. `None` without a fresh estimate or under the flatscreen source.
pub fn torso_turn(character_rotation: Quat) -> Option<f32> {
    if headpose::source() != headpose::Source::Vr {
        return None;
    }
    let estimate = estimate()?;
    let view = headpose::xr::body_rotation();
    yaw_of(character_rotation.inverse() * (view * yaw_forward(estimate.yaw)))
}

/// The chest's turn toward the player's torso for the body IK, radians in model space, when the
/// setting drives the chest; `None` leaves the body IK to its head-yaw share.
pub fn chest_turn(character_rotation: Quat) -> Option<f32> {
    let config = Config::lock_query(|c| c.controllers.body.alignment);
    if !config.drive_chest || !config.estimate {
        record_chest(None);
        return None;
    }
    let limit = config.chest_limit_deg.max(0.0).to_radians();
    let turn = torso_turn(character_rotation)
        .map(|turn| (wrap(turn) * config.chest_weight.clamp(0.0, 1.0)).clamp(-limit, limit));
    record_chest(turn);
    turn
}

/// The squared stance's extra turn of the pelvis and feet toward the player's torso, radians in
/// model space; zero unless the chest is driven.
pub fn stance_turn(character_rotation: Quat) -> f32 {
    let config = Config::lock_query(|c| c.controllers.body.alignment);
    if !config.drive_chest || !config.estimate {
        return 0.0;
    }
    let limit = config.chest_limit_deg.max(0.0).to_radians();
    torso_turn(character_rotation).map_or(0.0, |turn| {
        (wrap(turn) * config.stance_turn_share.clamp(0.0, 1.0)).clamp(-limit, limit)
    })
}

/// The torso's heading for the hands' estimated shoulders, when the setting asks for it.
pub fn shoulder_heading() -> Option<Quat> {
    let config = Config::lock_query(|c| c.controllers.body.alignment);
    (config.torso_shoulders && config.estimate)
        .then(estimate)
        .flatten()
        .map(|e| e.rotation())
}

/// The character's skeleton in the cockpit frame, when it is fresh and upright: what the holsters
/// ride under [`HolsterAnchor::Rico`](crate::controllers::weapons::config::HolsterAnchor::Rico).
/// `head_cockpit` is the head's cockpit position and `world_scale` the VR world scale the cockpit
/// composes with.
pub fn skeleton_in_cockpit(head_cockpit: Vec3, world_scale: f32) -> Option<SkeletonLandmarks> {
    let (skeleton, character) = {
        let state = STATE.lock();
        let (skeleton, character, at) = state.skeleton?;
        (at.elapsed() <= SKELETON_FRESH).then_some((skeleton, character))?
    };
    let frames = Frames {
        head_world: headpose::query().position,
        head_cockpit,
        view: headpose::xr::body_rotation(),
        world_scale,
        character,
    };
    if !frames.is_usable() {
        return None;
    }
    let landmarks = skeleton.map(|p| frames.model_to_cockpit(p));
    upright(&landmarks).then_some(landmarks)
}

/// Record which body the holsters rode on the latest input tick, for the readout.
pub fn record_volume_anchor(anchor: &'static str) {
    STATE.lock().status.volume_anchor = Some(anchor);
}

/// What the alignment did and measured on its latest frames, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct AlignmentStatus {
    /// The latest torso estimate, when fresh.
    pub estimate: Option<TorsoEstimate>,
    /// Whether the body follows the torso, and its target heading relative to the view, radians.
    pub following: bool,
    pub follow_target: f32,
    /// The body's heading relative to the view, radians.
    pub follow_offset: f32,
    /// The chest turn the body IK took on its latest tick, radians.
    pub chest_turn: Option<f32>,
    /// The latest divergence between the estimate and the skeleton after the solves.
    pub divergence: Option<Divergence>,
    /// When the skeleton was last read.
    pub skeleton_at: Option<Instant>,
    /// Which body the holsters rode on the latest input tick.
    pub volume_anchor: Option<&'static str>,
}

/// The latest status.
pub fn status() -> AlignmentStatus {
    let (following, follow_target) = headpose::xr::follow_status();
    let follow_offset = headpose::xr::follow_offset(true);
    let state = STATE.lock();
    AlignmentStatus {
        estimate: fresh_estimate(&state),
        following,
        follow_target,
        follow_offset,
        skeleton_at: state.skeleton.map(|(_, _, at)| at),
        ..state.status
    }
}

/// Read the local player's skeleton after the pose-finalization pass's solves, measure its
/// divergence from the torso estimate, and publish both. Called from the end of the pass.
///
/// # Safety
///
/// `character` must be null or a live character at the end of its pose-finalization pass.
pub unsafe fn after_solve(character: *mut Character) {
    let Some(character) = (unsafe { character.as_mut() }).filter(|c| c.m_IsLocalCharacter) else {
        return;
    };
    let config = Config::lock_query(|c| c.controllers.body.alignment);
    if !config.estimate || headpose::source() != headpose::Source::Vr || !hooks::in_gameplay() {
        STATE.lock().skeleton = None;
        return;
    }
    let skeleton = unsafe { read_skeleton(character) };
    let world = Mat4::from(character.m_WorldMatrixT1);
    let now = Instant::now();
    let measured = match (skeleton, estimate(), headpose::xr::cockpit_pose()) {
        (Some(skeleton), Some(estimate), Some(cockpit)) => {
            let frames = Frames {
                head_world: headpose::query().position,
                head_cockpit: cockpit.position,
                view: headpose::xr::body_rotation(),
                world_scale: Config::lock_query(|c| c.vr.world_scale),
                character: world,
            };
            divergence(&estimate, &skeleton, &frames)
        }
        _ => None,
    };
    {
        let mut state = STATE.lock();
        state.skeleton = skeleton.map(|s| (s, world, now));
        state.status.divergence = measured;
    }
    log(measured);
}

/// The status of the chest turn, for the readout.
fn record_chest(turn: Option<f32>) {
    STATE.lock().status.chest_turn = turn;
}

/// Read the hip and shoulder joints and the head from the solved pose, in model space.
///
/// # Safety
///
/// `character` must be a live character at the end of its pose-finalization pass.
unsafe fn read_skeleton(character: &mut Character) -> Option<SkeletonLandmarks> {
    let controller = unsafe { character.m_AnimatedModel.m_AnimationController.as_mut() }?;
    let hik = &character.m_HIK;
    let effector = |effector: Effector| unsafe { bone_for_effector(hik, effector as i32) };
    let bones = (
        effector(Effector::HIPS)?,
        effector(Effector::LEFT_HIP)?,
        effector(Effector::RIGHT_HIP)?,
        effector(Effector::LEFT_SHOULDER)?,
        effector(Effector::RIGHT_SHOULDER)?,
    );
    let head = unsafe { character.GetSafeIndex(SafeBoneIndex::HEAD) };
    let mut at = |bone: u32| unsafe { joint_position(controller, bone) };
    let skeleton = SkeletonLandmarks {
        head: at(head),
        pelvis: at(bones.0),
        hips: [at(bones.1), at(bones.2)],
        shoulders: [at(bones.3), at(bones.4)],
    };
    skeleton.is_finite().then_some(skeleton)
}

unsafe fn joint_position(controller: &mut AnimationController, bone: u32) -> Vec3 {
    let mut joint = Joint::default();
    unsafe { controller.GetJoint(bone, &mut joint) };
    Vec3::from_array(joint.m_Translation.data)
}

/// Log the divergence once a second at `DEBUG`, for reading a session back afterwards.
fn log(measured: Option<Divergence>) {
    if !tracing::enabled!(target: ALIGNMENT_TARGET, tracing::Level::DEBUG) {
        return;
    }
    let status = status();
    {
        let mut state = STATE.lock();
        let now = Instant::now();
        if state
            .last_log
            .is_some_and(|at| now.saturating_duration_since(at) < LOG_INTERVAL)
        {
            return;
        }
        state.last_log = Some(now);
    }
    let degrees = |turn: Option<f32>| turn.map(|t| format!("{:+.1}", t.to_degrees()));
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z).length();
    match (status.estimate, measured) {
        (Some(estimate), Some(d)) => tracing::debug!(
            target: ALIGNMENT_TARGET,
            torso_yaw_deg = estimate.yaw.to_degrees(),
            neck_twist_deg = estimate.neck_twist.to_degrees(),
            hands_confidence = estimate.hands_confidence,
            walk_confidence = estimate.walk_confidence,
            following = status.following,
            follow_offset_deg = status.follow_offset.to_degrees(),
            follow_target_deg = status.follow_target.to_degrees(),
            chest_turn_deg = ?degrees(status.chest_turn),
            root_turn_deg = d.root_turn.to_degrees(),
            chest_divergence_deg = ?degrees(d.chest_turn),
            pelvis_divergence_deg = ?degrees(d.pelvis_turn),
            chest_offset = ?d.chest,
            pelvis_offset = ?d.pelvis,
            shoulder_offsets_flat_m = ?[flat(d.shoulders[0]), flat(d.shoulders[1])],
            hip_offsets_flat_m = ?[flat(d.hips[0]), flat(d.hips[1])],
            volume_anchor = status.volume_anchor.unwrap_or("none"),
            "body alignment"
        ),
        (estimate, _) => {
            if !NOTHING_LOGGED.swap(true, Ordering::Relaxed) {
                tracing::debug!(
                    target: ALIGNMENT_TARGET,
                    estimate = estimate.is_some(),
                    "body alignment: nothing to measure (no estimate or no skeleton)"
                );
            }
            return;
        }
    }
    NOTHING_LOGGED.store(false, Ordering::Relaxed);
}

fn fresh_estimate(state: &State) -> Option<TorsoEstimate> {
    state
        .estimate
        .filter(|(_, at)| at.elapsed() <= ESTIMATE_FRESH)
        .map(|(estimate, _)| estimate)
}

/// How old the estimate may be and still stand for the player's torso. The frame loop publishes
/// one every rendered frame.
const ESTIMATE_FRESH: Duration = Duration::from_millis(250);

/// A gap between frames longer than this restarts the estimate rather than easing across it.
const ESTIMATE_GAP: Duration = Duration::from_millis(500);

/// How old the skeleton may be and still place the holsters.
const SKELETON_FRESH: Duration = Duration::from_millis(250);

/// The shortest time between two log lines.
const LOG_INTERVAL: Duration = Duration::from_secs(1);

struct State {
    estimator: TorsoEstimator,
    last_tick: Option<Instant>,
    estimate: Option<(TorsoEstimate, Instant)>,
    /// The skeleton in model space with the character's world transform it was read under.
    skeleton: Option<(SkeletonLandmarks, Mat4, Instant)>,
    status: AlignmentStatus,
    last_log: Option<Instant>,
}

static STATE: Mutex<State> = Mutex::new(State {
    estimator: TorsoEstimator::new(),
    last_tick: None,
    estimate: None,
    skeleton: None,
    status: AlignmentStatus {
        estimate: None,
        following: false,
        follow_target: 0.0,
        follow_offset: 0.0,
        chest_turn: None,
        divergence: None,
        skeleton_at: None,
        volume_anchor: None,
    },
    last_log: None,
});

static NOTHING_LOGGED: AtomicBool = AtomicBool::new(false);
