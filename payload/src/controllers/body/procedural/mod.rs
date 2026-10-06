//! The procedural body: Rico's skeleton posed by the mod from the player's tracked head and body,
//! rather than by the game's animation. See `docs/mod/body/procedural-body.md`.
//!
//! The pose is written at the HumanIK solve gate: a pre-call detour on
//! [`HumanIK::HasTargets`](jc3gi::animation::ik::HumanIK::HasTargets) for the local player's
//! solver and the `MAIN` pass (`hooks::character::gate`). That call has one caller, inside the
//! pose-finalization pass, after every game writer of IK targets and before the solve, the weapons'
//! attachment refresh, the model-space sync, the hit shapes, the AI's view, and the effects, so all
//! of them see the pose written there (`docs/engine/character/skeleton.md`).
//!
//! - [`skeleton`]: reading the skeleton, and logging it once per session.
//! - [`rig`]: the bones the body drives, found by name.
//! - [`solver`]: the standing body under the player's head (pure).
//! - [`config`]: the mode and the tuning.
//!
//! [`at_solve_gate`] runs the mode: `PassThrough` writes the animated pose back unchanged and logs
//! how it reads back, the proof that the write path is sound; `Body` builds the standing body while
//! Rico stands on foot, blending in and out with the animation, and zeroes the game's own IK
//! targets on the bones it owns. While it has any weight, [`owns_body`] tells the pre-solve IK
//! (the body IK's torso, the roomscale crouch, the squared stance) to stand down.

pub mod config;
pub mod rig;
pub mod skeleton;
pub mod solver;

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use glam::{Mat4, Quat, Vec3};
use jc3gi::{
    animation::ik::{Effector, HumanIK},
    character::character::{AnimationController, Character, Joint},
};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        body::{
            alignment::torso_turn,
            procedural::{
                config::{ProceduralBodyConfig, ProceduralMode},
                rig::{Rig, RigBone, RigError},
                skeleton::Transform,
                solver::{SolveInput, pose::RigPose, solve},
            },
        },
        locomotion::{anchor::status as anchor_status, status as locomotion_status},
    },
    headpose::{Source, body_relative_rotation, query, source},
    hooks::in_gameplay,
};

/// What the procedural body did at its last gate.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ProceduralStatus {
    pub at: Option<Instant>,
    pub mode: ProceduralMode,
    /// The weight of the procedural pose against the animation.
    pub weight: f32,
    /// Why the skeleton could not be rigged, if it could not.
    pub rig_error: Option<RigError>,
    /// The pass-through's largest read-back error over the driven bones: metres and degrees.
    pub round_trip: Option<(f32, f32)>,
    /// How far the solved hips sit below the animated ones, metres.
    pub hips_drop_m: f32,
    /// How far the solved neck sits from its target, metres.
    pub neck_error_m: f32,
}

/// The last gate's status.
pub fn status() -> ProceduralStatus {
    STATE.lock().status
}

/// Whether the procedural body poses the torso and legs: the pre-solve IK that would pose them
/// stands down while it does.
pub fn owns_body() -> bool {
    OWNS_BODY.load(Ordering::Relaxed)
}

/// Run the pose authority for the local player, at the `MAIN` pass's solve gate.
///
/// # Safety
///
/// `character` must be the live local character, inside its pose-finalization pass, on the thread
/// running it.
pub(crate) unsafe fn at_solve_gate(character: &mut Character) {
    let config = Config::lock_query(|c| c.controllers.body.procedural);
    let now = Instant::now();
    let mut state = STATE.lock();
    let dt = state
        .status
        .at
        .map_or(0.0, |at| now.saturating_duration_since(at).as_secs_f32())
        .min(MAX_STEP_S);
    state.status.at = Some(now);
    state.status.mode = config.mode;
    if config.mode == ProceduralMode::Off {
        state.progress = 0.0;
        state.status.weight = 0.0;
        OWNS_BODY.store(false, Ordering::Relaxed);
        return;
    }
    let Some(controller) = (unsafe { character.m_AnimatedModel.m_AnimationController.as_mut() })
    else {
        return;
    };
    let Some(rig) = (unsafe { state.rig_for(controller) }) else {
        OWNS_BODY.store(false, Ordering::Relaxed);
        return;
    };
    let animated = unsafe { read_pose(controller, &rig) };
    match config.mode {
        ProceduralMode::Off => {}
        ProceduralMode::PassThrough => {
            OWNS_BODY.store(false, Ordering::Relaxed);
            unsafe { write_pose(controller, &rig, &animated) };
            let read_back = unsafe { read_pose(controller, &rig) };
            let error = round_trip_error(&animated, &read_back);
            state.status.round_trip = Some(error);
            if state
                .logged_at
                .is_none_or(|at| now.saturating_duration_since(at) >= PASS_THROUGH_LOG_INTERVAL)
            {
                state.logged_at = Some(now);
                tracing::info!(
                    target: "body",
                    bones = RigBone::COUNT,
                    max_error_m = error.0,
                    max_error_deg = error.1,
                    "procedural body: pass-through wrote the animated pose back",
                );
            }
        }
        ProceduralMode::Body => {
            let eligible = eligible(character, now);
            let weight = state.step_weight(eligible, dt, &config);
            OWNS_BODY.store(weight > 0.0, Ordering::Relaxed);
            state.status.weight = weight;
            if weight <= 0.0 {
                return;
            }
            let Some(input) = solve_input(character, &animated) else {
                return;
            };
            let solved = solve(&rig, &input, &config);
            let pose = animated.blend(&solved, weight);
            unsafe { write_pose(controller, &rig, &pose) };
            if config.neutralize_targets {
                neutralize_owned_targets(&mut character.m_HIK, weight);
            }
            state.status.hips_drop_m =
                animated.position(RigBone::Hips).y - pose.position(RigBone::Hips).y;
            let neck_target = input.head
                + input.head_rotation
                    * (animated.position(RigBone::Neck) - animated.position(RigBone::Head));
            state.status.neck_error_m = solved.position(RigBone::Neck).distance(neck_target);
        }
    }
}

/// The longest step the blend takes, seconds: a gap is not a long blend.
const MAX_STEP_S: f32 = 0.1;

/// How often the pass-through logs its read-back.
const PASS_THROUGH_LOG_INTERVAL: Duration = Duration::from_secs(5);

/// The effectors whose bones the procedural body owns: everything below the neck but the arms.
const OWNED_EFFECTORS: [Effector; 13] = [
    Effector::HIPS,
    Effector::WAIST,
    Effector::CHEST_END,
    Effector::LEFT_SHOULDER,
    Effector::RIGHT_SHOULDER,
    Effector::LEFT_HIP,
    Effector::RIGHT_HIP,
    Effector::LEFT_KNEE,
    Effector::RIGHT_KNEE,
    Effector::LEFT_ANKLE,
    Effector::RIGHT_ANKLE,
    Effector::LEFT_FOOT,
    Effector::RIGHT_FOOT,
];

static OWNS_BODY: AtomicBool = AtomicBool::new(false);

struct State {
    /// The rig, for the animation controller it was built from; `None` beside the key when that
    /// controller's skeleton could not be rigged.
    rig: Option<(usize, Option<Rig>)>,
    /// The blend's linear progress toward the procedural pose.
    progress: f32,
    logged_at: Option<Instant>,
    status: ProceduralStatus,
}

impl State {
    /// The rig of `controller`'s skeleton, built once per controller. A skeleton that cannot be
    /// rigged is reported once.
    unsafe fn rig_for(&mut self, controller: &AnimationController) -> Option<Rig> {
        let key = controller as *const AnimationController as usize;
        if self.rig.as_ref().is_none_or(|(k, _)| *k != key) {
            self.status.rig_error = None;
            let rig = match unsafe { skeleton::read(controller) } {
                Ok(snapshot) => match Rig::from_skeleton(&snapshot) {
                    Ok(rig) => {
                        tracing::info!(target: "body", "procedural body: rigged the skeleton");
                        Some(rig)
                    }
                    Err(error) => {
                        tracing::warn!(target: "body", "procedural body: {error}");
                        self.status.rig_error = Some(error);
                        None
                    }
                },
                Err(error) => {
                    tracing::warn!(target: "body", "procedural body: {error}");
                    None
                }
            };
            self.rig = Some((key, rig));
        }
        self.rig.as_ref().and_then(|(_, rig)| rig.clone())
    }

    fn step_weight(&mut self, eligible: bool, dt: f32, config: &ProceduralBodyConfig) -> f32 {
        let target = if eligible { 1.0 } else { 0.0 };
        if config.blend_s <= 0.0 {
            self.progress = target;
        } else {
            let step = dt / config.blend_s;
            self.progress += (target - self.progress).clamp(-step, step);
        }
        let t = self.progress.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    rig: None,
    progress: 0.0,
    logged_at: None,
    status: ProceduralStatus {
        at: None,
        mode: ProceduralMode::Off,
        weight: 0.0,
        rig_error: None,
        round_trip: None,
        hips_drop_m: 0.0,
        neck_error_m: 0.0,
    },
});

/// The animated model-space pose of the rig's bones.
unsafe fn read_pose(controller: &AnimationController, rig: &Rig) -> RigPose {
    RigPose {
        bones: RigBone::ALL.map(|bone| {
            let mut joint = Joint::default();
            unsafe { controller.GetJoint(rig.skeleton_index(bone) as u32, &mut joint) };
            from_joint(&joint)
        }),
    }
}

/// Write the rig's bones, parents first, so each write carries the children that follow.
unsafe fn write_pose(controller: &mut AnimationController, rig: &Rig, pose: &RigPose) {
    if !pose.is_finite() {
        return;
    }
    for bone in RigBone::ALL {
        let mut joint = to_joint(pose.get(bone));
        unsafe { controller.SetJoint(rig.skeleton_index(bone) as u32, &mut joint) };
    }
}

fn from_joint(joint: &Joint) -> Transform {
    let [x, y, z, w] = joint.m_Orientation.data;
    Transform {
        translation: Vec3::from_array(joint.m_Translation.data),
        rotation: Quat::from_xyzw(x, y, z, w),
        scale: Vec3::from_array(joint.m_Scale.data),
    }
}

fn to_joint(transform: &Transform) -> Joint {
    let mut joint = Joint::default();
    joint.m_Translation.data = transform.translation.to_array();
    joint.m_Orientation.data = transform.rotation.to_array();
    joint.m_Scale.data = transform.scale.to_array();
    joint
}

/// The largest position (metres) and rotation (degrees) difference between two poses.
fn round_trip_error(written: &RigPose, read: &RigPose) -> (f32, f32) {
    written
        .bones
        .iter()
        .zip(&read.bones)
        .fold((0.0f32, 0.0f32), |(m, d), (a, b)| {
            (
                m.max(a.translation.distance(b.translation)),
                d.max(a.rotation.angle_between(b.rotation).to_degrees()),
            )
        })
}

/// Whether Rico stands on foot with the headset driving the view: the procedural body's only case
/// for now. Walking, the air, vehicles, and every authored state keep the animation.
fn eligible(character: &Character, now: Instant) -> bool {
    let anchor = anchor_status();
    let locomotion = locomotion_status();
    source() == Source::Vr
        && in_gameplay()
        && anchor.on_ground
        && locomotion.fresh(now)
        && locomotion.standing_still()
        && character.m_CurrentMotionState == Character::MOTION_STATE_TASK_DEFINED
}

/// The solve's inputs from the headpose and the torso estimate, in model space.
fn solve_input(character: &Character, animated: &RigPose) -> Option<SolveInput> {
    let world = Mat4::from(character.m_WorldMatrixT1);
    let (_, rotation, _) = world.to_scale_rotation_translation();
    let head = world.inverse().transform_point3(query().position);
    head.is_finite().then(|| SolveInput {
        animated: *animated,
        head,
        head_rotation: body_relative_rotation(),
        torso_yaw: torso_turn(rotation),
        feet: [
            animated.position(RigBone::LeftFoot),
            animated.position(RigBone::RightFoot),
        ],
    })
}

/// Fade the game's (and the mod's) IK targets on the bones the procedural body owns, so the solve
/// after the gate leaves them where the pose put them.
fn neutralize_owned_targets(hik: &mut HumanIK, weight: f32) {
    let keep = (1.0 - weight).clamp(0.0, 1.0);
    for effector in OWNED_EFFECTORS {
        let i = effector as usize;
        hik.m_TargetReachT[i] *= keep;
        hik.m_TargetReachR[i] *= keep;
        hik.m_ReachT[i] *= keep;
        hik.m_ReachR[i] *= keep;
        hik.m_TargetPull[i] *= keep;
        hik.m_Pull[i] *= keep;
    }
}
