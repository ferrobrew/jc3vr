//! The character's skeleton after the pose-finalization pass's solves: the hip and shoulder joints
//! the holsters can ride ([`skeleton_in_cockpit`]), and the divergence between them and the torso
//! estimate, published for the debug UI and logged once a second.

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use glam::{Mat4, Vec3};
use jc3gi::{
    animation::ik::Effector,
    character::character::{AnimationController, Character, Joint, SafeBoneIndex},
};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::body::rig::{
        self,
        frames::{Divergence, Frames, SkeletonLandmarks, divergence, upright},
    },
    headpose, hooks,
    hooks::character::body_ik::bone_for_effector,
};

/// The tracing target for the divergence's once-a-second log. Public so that the debug UI can turn
/// it up to `DEBUG`.
pub const SKELETON_TARGET: &str = "body_alignment";

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
    STATE.lock().volume_anchor = Some(anchor);
}

/// The skeleton's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct SkeletonStatus {
    /// The latest divergence between the estimate and the skeleton after the solves.
    pub divergence: Option<Divergence>,
    /// When the skeleton was last read.
    pub at: Option<Instant>,
    /// Which body the holsters rode on the latest input tick.
    pub volume_anchor: Option<&'static str>,
}

pub fn status() -> SkeletonStatus {
    let state = STATE.lock();
    SkeletonStatus {
        divergence: state.divergence,
        at: state.skeleton.map(|(_, _, at)| at),
        volume_anchor: state.volume_anchor,
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
    if headpose::source() != headpose::Source::Vr || !hooks::in_gameplay() {
        STATE.lock().skeleton = None;
        return;
    }
    let skeleton = unsafe { read_skeleton(character) };
    let world = Mat4::from(character.m_WorldMatrixT1);
    let now = Instant::now();
    let measured = match (skeleton, rig::estimate(), headpose::xr::cockpit_pose()) {
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
        state.divergence = measured;
    }
    log(measured);
}

/// How old the skeleton may be and still place the holsters.
const SKELETON_FRESH: Duration = Duration::from_millis(250);

/// The shortest time between two log lines.
const LOG_INTERVAL: Duration = Duration::from_secs(1);

struct State {
    /// The skeleton in model space with the character's world transform it was read under.
    skeleton: Option<(SkeletonLandmarks, Mat4, Instant)>,
    divergence: Option<Divergence>,
    volume_anchor: Option<&'static str>,
    last_log: Option<Instant>,
}

static STATE: Mutex<State> = Mutex::new(State {
    skeleton: None,
    divergence: None,
    volume_anchor: None,
    last_log: None,
});

static NOTHING_LOGGED: AtomicBool = AtomicBool::new(false);

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
    if !tracing::enabled!(target: SKELETON_TARGET, tracing::Level::DEBUG) {
        return;
    }
    let estimate = rig::estimate();
    let anchor = {
        let mut state = STATE.lock();
        let now = Instant::now();
        if state
            .last_log
            .is_some_and(|at| now.saturating_duration_since(at) < LOG_INTERVAL)
        {
            return;
        }
        state.last_log = Some(now);
        state.volume_anchor
    };
    let degrees = |turn: Option<f32>| turn.map(|t| format!("{:+.1}", t.to_degrees()));
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z).length();
    match (estimate, measured) {
        (Some(estimate), Some(d)) => tracing::debug!(
            target: SKELETON_TARGET,
            torso_yaw_deg = estimate.yaw.to_degrees(),
            neck_twist_deg = estimate.neck_twist.to_degrees(),
            hands_weight = estimate.hands_weight,
            root_turn_deg = d.root_turn.to_degrees(),
            chest_divergence_deg = ?degrees(d.chest_turn),
            pelvis_divergence_deg = ?degrees(d.pelvis_turn),
            chest_offset = ?d.chest,
            pelvis_offset = ?d.pelvis,
            shoulder_offsets_flat_m = ?[flat(d.shoulders[0]), flat(d.shoulders[1])],
            hip_offsets_flat_m = ?[flat(d.hips[0]), flat(d.hips[1])],
            volume_anchor = anchor.unwrap_or("none"),
            "body alignment"
        ),
        (estimate, _) => {
            if !NOTHING_LOGGED.swap(true, Ordering::Relaxed) {
                tracing::debug!(
                    target: SKELETON_TARGET,
                    estimate = estimate.is_some(),
                    "body alignment: nothing to measure (no estimate or no skeleton)"
                );
            }
            return;
        }
    }
    NOTHING_LOGGED.store(false, Ordering::Relaxed);
}
