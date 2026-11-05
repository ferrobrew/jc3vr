//! The character's skeleton after the pose-finalization pass's solves: the hip and shoulder joints
//! the holsters can ride ([`skeleton_in_cockpit`]), and the divergence between them and the torso
//! estimate, published for the debug UI and logged once a second.
//!
//! A read that is not a plausible body ([`frames::implausibility`]) does not take the volumes away:
//! they ride the last plausible skeleton, in the character's model space, carried by the
//! character's current transform, so they stay on the body as it moves ([`SkeletonMemory`]).

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
        frames::{self, Divergence, Frames, Implausible, SkeletonLandmarks, divergence},
    },
    headpose, hooks,
    hooks::character::body_ik::bone_for_effector,
};

/// The tracing target for the divergence's once-a-second log. Public so that the debug UI can turn
/// it up to `DEBUG`.
pub const SKELETON_TARGET: &str = "body_divergence";

/// The character's skeleton in the cockpit frame, while the character's pose is fresh: what the
/// body's grab volumes ride ([`crate::controllers::weapons::volumes`]), with where it came from.
/// `head_cockpit` is the head's cockpit position and `world_scale` the VR world scale the cockpit
/// composes with.
pub fn skeleton_in_cockpit(
    head_cockpit: Vec3,
    world_scale: f32,
) -> Option<(SkeletonLandmarks, SkeletonSource)> {
    let (skeleton, character, source) = STATE.lock().memory.body(Instant::now())?;
    let frames = Frames {
        head_world: headpose::query().position,
        head_cockpit,
        view: headpose::xr::body_rotation(),
        world_scale,
        character,
    };
    frames
        .is_usable()
        .then(|| (skeleton.map(|p| frames.model_to_cockpit(p)), source))
}

/// Which skeleton the volumes ride.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SkeletonSource {
    /// The skeleton read after the latest solve.
    Latest,
    /// The last plausible skeleton, carried by the character's current transform, because the
    /// latest read was not a plausible body.
    LastGood,
}

impl SkeletonSource {
    /// The readout's name for the body the volumes ride.
    pub fn anchor(self) -> &'static str {
        match self {
            SkeletonSource::Latest => "the character's skeleton",
            SkeletonSource::LastGood => "the character's last plausible skeleton",
        }
    }
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

impl SkeletonStatus {
    /// Whether the skeleton was read recently enough to place the holsters, which is when its
    /// divergence describes the body now.
    pub fn fresh(&self, now: Instant) -> bool {
        self.at
            .is_some_and(|at| now.saturating_duration_since(at) <= SKELETON_FRESH)
    }
}

pub fn status() -> SkeletonStatus {
    let state = STATE.lock();
    SkeletonStatus {
        divergence: state.divergence,
        at: state.memory.latest.as_ref().map(|read| read.at),
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
        STATE.lock().memory.latest = None;
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
    let unusable = {
        let mut state = STATE.lock();
        let unusable = state.memory.record(skeleton, world, now);
        state.divergence = measured;
        let changed = state.unusable != unusable;
        state.unusable = unusable;
        changed.then_some(unusable)
    };
    if let Some(unusable) = unusable {
        log_unusable(unusable);
    }
    log(measured);
}

/// How old the character's pose may be and still place the holsters.
const SKELETON_FRESH: Duration = Duration::from_millis(250);

/// The shortest time between two log lines.
const LOG_INTERVAL: Duration = Duration::from_secs(1);

struct State {
    memory: SkeletonMemory,
    /// Why the latest read could not place the volumes, for logging each change once.
    unusable: Option<Unusable>,
    divergence: Option<Divergence>,
    volume_anchor: Option<&'static str>,
    last_log: Option<Instant>,
}

static STATE: Mutex<State> = Mutex::new(State {
    memory: SkeletonMemory {
        latest: None,
        last_good: None,
    },
    unusable: None,
    divergence: None,
    volume_anchor: None,
    last_log: None,
});

/// The latest skeleton read, and the last one that was a plausible body. Pure.
#[derive(Debug, Clone, PartialEq)]
struct SkeletonMemory {
    latest: Option<SkeletonRead>,
    /// The last plausible skeleton, in the character's model space.
    last_good: Option<SkeletonLandmarks>,
}

/// One read of the skeleton after a solve.
#[derive(Debug, Copy, Clone, PartialEq)]
struct SkeletonRead {
    /// The landmarks in model space, or `None` when the bones could not be read.
    landmarks: Option<SkeletonLandmarks>,
    /// The character's model-to-world transform at the read.
    character: Mat4,
    at: Instant,
}

/// Why a read cannot place the volumes itself.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Unusable {
    /// The bones could not be read.
    Unread,
    /// The bones are not a plausible body.
    Implausible(Implausible),
}

impl SkeletonMemory {
    /// Take a read of `landmarks` under the transform `character` at `at`, and say why it cannot
    /// place the volumes itself, if it cannot. A plausible read becomes the last good one.
    fn record(
        &mut self,
        landmarks: Option<SkeletonLandmarks>,
        character: Mat4,
        at: Instant,
    ) -> Option<Unusable> {
        self.latest = Some(SkeletonRead {
            landmarks,
            character,
            at,
        });
        let Some(landmarks) = landmarks else {
            return Some(Unusable::Unread);
        };
        match frames::implausibility(&landmarks) {
            Some(why) => Some(Unusable::Implausible(why)),
            None => {
                self.last_good = Some(landmarks);
                None
            }
        }
    }

    /// The skeleton the volumes ride at `now`, in model space, with the character's transform that
    /// carries it: the latest read when it is plausible, else the last plausible one, carried by
    /// the latest read's transform. `None` while the latest read is older than [`SKELETON_FRESH`],
    /// since then the character's pose is not being solved, or before any plausible read.
    fn body(&self, now: Instant) -> Option<(SkeletonLandmarks, Mat4, SkeletonSource)> {
        let latest = self.latest.as_ref()?;
        if now.saturating_duration_since(latest.at) > SKELETON_FRESH {
            return None;
        }
        match latest.landmarks.filter(frames::plausible) {
            Some(landmarks) => Some((landmarks, latest.character, SkeletonSource::Latest)),
            None => Some((self.last_good?, latest.character, SkeletonSource::LastGood)),
        }
    }
}

/// Log a change in whether the reads place the volumes, naming the gate a read failed.
fn log_unusable(unusable: Option<Unusable>) {
    match unusable {
        None => tracing::info!(target: "body", "body frame: the skeleton reads as a body again"),
        Some(Unusable::Unread) => tracing::info!(
            target: "body",
            "body frame: the skeleton's bones could not be read; the volumes ride the last plausible skeleton"
        ),
        Some(Unusable::Implausible(why)) => tracing::info!(
            target: "body",
            reason = %why,
            "body frame: the skeleton is not a plausible body; the volumes ride the last plausible skeleton"
        ),
    }
}

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
            "body divergence"
        ),
        (estimate, _) => {
            if !NOTHING_LOGGED.swap(true, Ordering::Relaxed) {
                tracing::debug!(
                    target: SKELETON_TARGET,
                    estimate = estimate.is_some(),
                    "body divergence: nothing to measure (no estimate or no skeleton)"
                );
            }
            return;
        }
    }
    NOTHING_LOGGED.store(false, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    fn standing() -> SkeletonLandmarks {
        SkeletonLandmarks {
            head: Vec3::new(0.0, 1.63, 0.01),
            pelvis: Vec3::new(0.0, 1.04, 0.0),
            hips: [Vec3::new(-0.09, 0.98, -0.03), Vec3::new(0.09, 0.98, -0.03)],
            shoulders: [Vec3::new(-0.19, 1.45, 0.04), Vec3::new(0.19, 1.45, 0.04)],
        }
    }

    fn memory() -> SkeletonMemory {
        SkeletonMemory {
            latest: None,
            last_good: None,
        }
    }

    #[test]
    fn a_plausible_read_places_the_volumes() {
        let mut memory = memory();
        let now = Instant::now();
        let character = Mat4::from_translation(Vec3::new(5.0, 0.0, 2.0));
        assert_eq!(memory.record(Some(standing()), character, now), None);
        assert_eq!(
            memory.body(now),
            Some((standing(), character, SkeletonSource::Latest))
        );
    }

    #[test]
    fn a_bad_read_falls_back_to_the_last_good_skeleton_carried_by_the_body() {
        let mut memory = memory();
        let now = Instant::now();
        memory.record(Some(standing()), Mat4::IDENTITY, now);
        // The character has walked on and turned since the good read.
        let moved =
            Mat4::from_rotation_translation(Quat::from_rotation_y(1.0), Vec3::new(3.0, 0.5, -1.0));
        let torn = SkeletonLandmarks {
            head: Vec3::new(3.0, 1.6, 0.0),
            ..standing()
        };
        let later = now + Duration::from_millis(30);
        assert_eq!(
            memory.record(Some(torn), moved, later),
            Some(Unusable::Implausible(Implausible::Head))
        );
        assert_eq!(
            memory.body(later),
            Some((standing(), moved, SkeletonSource::LastGood))
        );
        assert_eq!(memory.record(None, moved, later), Some(Unusable::Unread));
        assert_eq!(
            memory.body(later),
            Some((standing(), moved, SkeletonSource::LastGood))
        );
    }

    #[test]
    fn nothing_is_placed_before_a_good_read_or_once_the_pose_goes_stale() {
        let mut memory = memory();
        let now = Instant::now();
        assert_eq!(memory.body(now), None);
        memory.record(None, Mat4::IDENTITY, now);
        assert_eq!(memory.body(now), None);
        memory.record(Some(standing()), Mat4::IDENTITY, now);
        assert!(memory.body(now + SKELETON_FRESH).is_some());
        assert_eq!(memory.body(now + SKELETON_FRESH * 2), None);
    }
}
