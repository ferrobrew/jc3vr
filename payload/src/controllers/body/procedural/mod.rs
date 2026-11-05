//! The procedural body: Rico's torso posed by the mod from the player's tracked head and hands on
//! the hips the game's animation keeps. See `docs/mod/body/procedural-body.md`.
//!
//! The pose is written at the HumanIK solve gate: a pre-call detour on
//! [`HumanIK::HasTargets`](jc3gi::animation::ik::HumanIK::HasTargets) for the local player's
//! solver and the `MAIN` pass (`hooks::character::gate`). That call has one caller, inside the
//! pose-finalization pass, after every game writer of IK targets and before the solve, the weapons'
//! attachment refresh, the model-space sync, the hit shapes, the AI's view, and the effects, so all
//! of them see the pose written there (`docs/engine/character/skeleton.md`).
//!
//! The game's animation keeps the hips, the pelvis, and the legs in every case; the body rig moves
//! Rico by driving the game's own movement (`docs/mod/body/rig.md`). The torso (the spine, the
//! neck, the head, and the clavicles) follows the player wherever the hand-over table gives it to
//! them ([`authority`]).
//!
//! - [`skeleton`]: reading the skeleton, and logging it once per session.
//! - [`rig`]: the bones the body drives, found by name.
//! - [`joints`]: the rig for the skeleton the controller holds, and the bounds-checked joint reads
//!   and writes.
//! - [`solver`]: the torso on the animation's hips, the clavicles, and the arms' keep-out (pure).
//! - [`gate_state`]: what the gate keeps between ticks.
//! - [`head`]: the player's head as the torso reads it, at the neck pivot.
//! - [`state`]: what the character is doing, for the hand-over table, and whether the animation
//!   moves the body.
//! - [`signals`]: the grapple's ledge climb and the hit reactions, which the character does not
//!   keep.
//! - [`targets`]: the fade of the HumanIK targets on the owned bones.
//! - [`wings`]: Rico's arms in the wingsuit, carried over from the player's.
//! - [`twist`]: the arms' twist bones.
//! - [`config`]: the mode and the tuning.
//!
//! [`at_solve_gate`] runs the mode: `Full` poses the torso, handing it between the player and the
//! animation by the table in [`authority`], and fades the game's own IK targets on the bones it
//! owns. While it has any weight, [`owns_body`] tells the pre-solve IK (the body IK's torso) to
//! stand down.

pub mod authority;
pub mod config;
mod gate_state;
pub mod head;
pub mod joints;
pub mod rig;
pub mod signals;
pub mod skeleton;
pub mod solver;
pub mod state;
pub mod targets;
pub mod twist;
pub mod wings;

use std::{
    sync::atomic::{AtomicU32, Ordering},
    time::Instant,
};

use body_rig::angle::line_yaw;
use glam::Mat4;
use jc3gi::{
    animation::ik::{Effector, HumanIK},
    character::character::Character,
};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        body::{
            procedural::{
                authority::{Case, classify},
                config::{ProceduralBodyConfig, ProceduralMode},
                gate_state::{STATE, State},
                rig::{Rig, RigBone},
                solver::{
                    SolveInput,
                    anchored::{solve_anchored, upright_share},
                    arms::{Side, TorsoKeepOut},
                    clavicles::{self, ClavicleTurn},
                    pose::RigPose,
                },
                targets::{Fade, Slot, TargetFade},
                wings::{WingRequest, read_player_arms},
            },
            rig::torso_turn,
        },
        locomotion::{anchor::JUMP_GRACE_S, status as locomotion_status},
        mode::{AirborneVariant, Mode, current as current_mode},
    },
    headpose::{body_relative_rotation, eye_arm},
};

/// What the procedural body did at its last gate.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ProceduralStatus {
    pub at: Option<Instant>,
    pub mode: ProceduralMode,
    /// The torso's weight of the procedural pose against the animation.
    pub weight: f32,
    /// The hand-over table's case at the last gate.
    pub case: Case,
    /// Why the skeleton could not be rigged, if it could not.
    pub rig_error: Option<rig::RigError>,
    /// How far the solved neck sits from its target, metres.
    pub neck_error_m: f32,
    /// The clavicles' turns toward the hands, left then right.
    pub clavicles: [ClavicleTurn; 2],
    /// The wingsuit's arms' weight against the animation.
    pub wing_weight: f32,
    /// Which hands the wingsuit's arms placed a target for, left then right.
    pub wing_hands: [bool; 2],
    /// Which hands the wingsuit's arms queued a target for, left then right.
    pub wing_queued: [bool; 2],
}

/// What the controllers' arm targets are queued against while the procedural body poses the
/// torso: published by the gate for the hand IK it runs next.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ArmContext {
    /// The torso the wrist and elbow targets are kept out of.
    pub keep_out: TorsoKeepOut,
    /// The elbow hints' reach, already scaled by the procedural pose's weight.
    pub elbow_reach: Option<f32>,
    /// The written chest's turn about +Y from Rico's facing, radians: the elbow hints turn with it.
    pub chest_yaw: f32,
}

/// The arm context of this tick's gate, `None` when the procedural body did not pose the torso.
pub fn arm_context() -> Option<ArmContext> {
    STATE.lock().arms
}

/// This tick's torso keep-out, carried into world space by `to_world` (the character's own world
/// transform): `None` when the procedural body did not pose the torso this tick. A caller that
/// keeps its own carried target out of the player's body, such as the physical hands' held-object
/// keep-out, queries this instead of rebuilding the torso's shape, so the two keep-outs never drift
/// apart.
pub fn torso_keep_out(to_world: Mat4) -> Option<TorsoKeepOut> {
    arm_context().map(|arms| arms.keep_out.to_world(to_world))
}

/// The last gate's status.
pub fn status() -> ProceduralStatus {
    STATE.lock().status
}

/// The last gate's status, copied out after the gate let go of its state, so a reader never waits
/// on a gate in progress. A leaf lock, so any thread may read it.
pub fn published_status() -> ProceduralStatus {
    *PUBLISHED.lock()
}

/// Whether the procedural body poses the torso at all: the wrist targets wait for the gate while
/// it does, so they are queued on the torso it writes.
pub fn owns_body() -> bool {
    torso_weight() > 0.0
}

/// The torso's weight of the procedural pose at the last gate, `0..=1`. The pre-solve IK that
/// would pose the same bones (the body IK's torso) is queued while it is below one, and the gate
/// fades the reach and pull of those targets by what is left of it ([`neutralize_owned_targets`]),
/// so the hand-over is as gradual as the blend.
pub fn torso_weight() -> f32 {
    f32::from_bits(TORSO_WEIGHT.load(Ordering::Relaxed))
}

/// Hand the body back after a pass whose solve gate did not run (the HumanIK switched off for the
/// character, or its reduced level of detail): nothing wrote the procedural pose, so the pre-solve
/// IK must not stand down for it, and the blend starts over when the gate runs again.
pub(crate) fn gate_skipped() {
    publish_torso_weight(0.0);
    let mut state = STATE.lock();
    state.blend.reset();
    state.status.weight = 0.0;
}

/// Run the pose authority for the local player, at the `MAIN` pass's solve gate. `dt` is the
/// pass's step, which paces the blend and the followers, so the body keeps the simulation's time
/// rather than the wall clock's.
///
/// # Safety
///
/// `character` must be the live local character, inside its pose-finalization pass, on the thread
/// running it.
/// Returns whether it wrote a procedural pose, which the weapons must be placed on.
pub(crate) unsafe fn at_solve_gate(character: &mut Character, dt: f32) -> bool {
    // SAFETY: forwarded from the caller.
    let wrote = unsafe { run_gate(character, dt) };
    if !wrote {
        publish_torso_weight(0.0);
    }
    let wing_queue = {
        let mut state = STATE.lock();
        state.wing_queue.take()
    };
    // Queued with the gate's state let go: the hand IK takes the configuration lock.
    let wing_queued = unsafe { wings::queue(character, wing_queue) };
    let status = {
        let mut state = STATE.lock();
        state.status.wing_queued = wing_queued;
        state.status
    };
    *PUBLISHED.lock() = status;
    wrote
}

/// The gate itself; [`at_solve_gate`] publishes its status after it. Returns whether it wrote a
/// procedural pose; every path that does not leaves the torso's published weight at zero.
///
/// # Safety
///
/// As [`at_solve_gate`].
unsafe fn run_gate(character: &mut Character, dt: f32) -> bool {
    let (config, scale) = Config::lock_query(|c| (c.controllers.body.procedural, c.vr.world_scale));
    let now = Instant::now();
    // Read before the gate's own state too: it takes the configuration and the controller frame.
    let mode = current_mode().mode;
    let player_arms = (mode == Mode::Airborne(AirborneVariant::Wingsuit))
        .then(read_player_arms)
        .flatten();
    let mut state = STATE.lock();
    let dt = if dt.is_finite() {
        dt.clamp(0.0, MAX_STEP_S)
    } else {
        0.0
    };
    state.status.at = Some(now);
    state.status.mode = config.mode;
    state.arms = None;
    // Switched off, the body blends back to the animation before the gate stops acting.
    if config.mode == ProceduralMode::Off && state.blend.weight() <= 0.0 {
        state.blend.reset();
        state.status.weight = 0.0;
        return false;
    }
    let Some(controller) = (unsafe { character.m_AnimatedModel.m_AnimationController.as_mut() })
    else {
        return false;
    };
    let Some(live) = (unsafe { state.rig_for(controller) }) else {
        return false;
    };
    let rig = &live.rig;
    let animated = unsafe { live.read(controller) };
    let locomotion = locomotion_status();
    let read = unsafe {
        state
            .tracker
            .read(character, &locomotion, now, dt, JUMP_GRACE_S, &config)
    };
    let case = if config.mode == ProceduralMode::Off {
        Case::SwitchedOff
    } else {
        classify(&read)
    };
    let weight = state.blend.step(case, dt, &config);
    state.log_case(case, weight);
    state.status.case = case;
    state.status.weight = weight;
    let world = Mat4::from(character.m_WorldMatrixT1);
    let wings = WingRequest {
        case,
        player: player_arms.as_ref(),
        rig,
        animated: &animated,
        world: &world,
        dt,
    };
    state.wing_queue = state.step_wings(&wings, &config);
    if weight <= 0.0 {
        state.clavicles.reset();
        state.on_hips_logged = None;
        state.status.clavicles = [ClavicleTurn::ZERO; 2];
        state.fade.forget();
        return false;
    }
    let gate = TorsoGate {
        character,
        animated: &animated,
        case,
        weight,
        scale,
        dt,
    };
    let Some(pose) = (unsafe { pose_torso(&mut state, rig, &gate, &config) }) else {
        return false;
    };
    unsafe { live.write(controller, &pose) };
    finish_write(&mut state, character, &pose, weight, &config);
    true
}

/// What the gate poses the torso from.
struct TorsoGate<'a> {
    character: &'a Character,
    animated: &'a RigPose,
    case: Case,
    /// The torso's weight against the animation.
    weight: f32,
    /// The VR world scale.
    scale: f32,
    dt: f32,
}

/// The pose to write: the torso solved on the clip's hips toward the player's head, the clavicles
/// turned toward the hands, and the hips, the pelvis, and the legs left to the animation. `None`
/// when the head is not known.
///
/// # Safety
///
/// As [`at_solve_gate`].
unsafe fn pose_torso(
    state: &mut State,
    rig: &Rig,
    gate: &TorsoGate<'_>,
    config: &ProceduralBodyConfig,
) -> Option<RigPose> {
    let input = solve_input(gate.character, gate.animated, gate.scale)?;
    let mut solved = solve_anchored(&input, config);
    let turns = unsafe { state.turn_clavicles(gate.character, rig, &solved, gate.dt, config) };
    for side in Side::BOTH {
        clavicles::apply(&mut solved, rig, side, turns[side.index()]);
    }
    let pose = gate.animated.blend_torso(&solved, gate.weight);
    let neck_target = input.head
        + input.head_rotation
            * (gate.animated.position(RigBone::Neck) - gate.animated.position(RigBone::Head));
    let neck_error_m = solved.position(RigBone::Neck).distance(neck_target);
    if state.on_hips_logged != Some(gate.case) {
        state.on_hips_logged = Some(gate.case);
        tracing::info!(
            target: "body",
            case = gate.case.label(),
            torso = gate.weight,
            neck_error_m,
            "procedural body: the torso follows the head on the animation's hips",
        );
    }
    state.status.neck_error_m = neck_error_m;
    Some(pose)
}

/// The steps after a procedural pose is written: publish the torso's weight and the arm context
/// the hand IK queues the wrist targets against, and fade the owned bones' IK targets.
fn finish_write(
    state: &mut State,
    character: &mut Character,
    pose: &RigPose,
    weight: f32,
    config: &ProceduralBodyConfig,
) {
    publish_torso_weight(weight);
    // The elbow hints point down and back from the chest, which only holds for a chest near
    // upright.
    let upright = (pose.position(RigBone::Neck) - pose.position(RigBone::Spine))
        .try_normalize()
        .is_some_and(|axis| upright_share(axis) > ELBOW_HINT_UPRIGHT);
    state.arms = (weight > 0.0).then(|| ArmContext {
        keep_out: TorsoKeepOut::around(
            pose,
            config.chest_radius_m,
            config.abdomen_radius_m,
            config.hand_margin_m,
        ),
        elbow_reach: (config.elbow_hints && upright).then_some(config.elbow_hint_reach * weight),
        chest_yaw: line_yaw(
            pose.position(RigBone::LeftShoulder),
            pose.position(RigBone::RightShoulder),
        )
        .unwrap_or(0.0),
    });
    if config.neutralize_targets {
        neutralize_owned_targets(&mut character.m_HIK, weight, &mut state.fade);
    } else {
        state.fade.forget();
    }
}

/// How upright the written chest must be for the elbow hints, as [`upright_share`] measures it.
const ELBOW_HINT_UPRIGHT: f32 = 0.5;

/// The longest step the body takes, seconds: a hitch is not a long blend.
const MAX_STEP_S: f32 = 0.1;

/// The effectors of the torso's bones. The hips are the animation's.
const TORSO_EFFECTORS: [Effector; 4] = [
    Effector::WAIST,
    Effector::CHEST_END,
    Effector::LEFT_SHOULDER,
    Effector::RIGHT_SHOULDER,
];

/// The torso's weight [`torso_weight`] returns, as its bits.
static TORSO_WEIGHT: AtomicU32 = AtomicU32::new(0);

fn publish_torso_weight(weight: f32) {
    let weight = if weight.is_finite() {
        weight.clamp(0.0, 1.0)
    } else {
        0.0
    };
    TORSO_WEIGHT.store(weight.to_bits(), Ordering::Relaxed);
}

/// The status [`published_status`] returns. Taken only after the gate releases [`STATE`], and
/// nothing is locked under it.
static PUBLISHED: Mutex<ProceduralStatus> = Mutex::new(INITIAL_STATUS);

/// The status before the first gate.
const INITIAL_STATUS: ProceduralStatus = ProceduralStatus {
    at: None,
    mode: ProceduralMode::Off,
    weight: 0.0,
    case: Case::NotInHeadset,
    rig_error: None,
    neck_error_m: 0.0,
    clavicles: [ClavicleTurn::ZERO; 2],
    wing_weight: 0.0,
    wing_hands: [false; 2],
    wing_queued: [false; 2],
};

/// The solve's inputs from the headpose and the torso, in model space, at the VR world scale
/// `scale`.
fn solve_input(character: &Character, animated: &RigPose, scale: f32) -> Option<SolveInput> {
    let world = Mat4::from(character.m_WorldMatrixT1);
    let (_, rotation, _) = world.to_scale_rotation_translation();
    let head = head::read(&world, scale);
    let head_rotation = body_relative_rotation();
    let eye = head + head_rotation * eye_arm();
    (head.is_finite() && eye.is_finite()).then(|| SolveInput {
        animated: *animated,
        head,
        eye,
        head_rotation,
        torso_yaw: torso_turn(rotation),
    })
}

/// Fade the game's (and the mod's) IK targets on the torso's bones by its weight, so the solve
/// after the gate leaves them where the pose put them: in proportion to the weight, whatever was
/// queued this tick ([`TargetFade`]). The head's target keeps its reach, so it still closes the gap
/// to the player's head, but loses its pull by the torso's weight, so it cannot drag the
/// procedural spine after it.
fn neutralize_owned_targets(hik: &mut HumanIK, weight: f32, fade: &mut TargetFade) {
    let torso = TORSO_EFFECTORS.map(|e| (e as usize, weight, Fade::All));
    let head = [(Effector::HEAD as usize, weight, Fade::Pull)];
    for (i, weight, kind) in torso.into_iter().chain(head) {
        let slot = Slot {
            target_reach_t: hik.m_TargetReachT[i],
            target_reach_r: hik.m_TargetReachR[i],
            target_pull: hik.m_TargetPull[i],
            reach_t: hik.m_ReachT[i],
            reach_r: hik.m_ReachR[i],
            pull: hik.m_Pull[i],
        };
        let faded = fade.fade(i, slot, 1.0 - weight, kind);
        hik.m_TargetReachT[i] = faded.target_reach_t;
        hik.m_TargetReachR[i] = faded.target_reach_r;
        hik.m_TargetPull[i] = faded.target_pull;
        hik.m_ReachT[i] = faded.reach_t;
        hik.m_ReachR[i] = faded.reach_r;
        hik.m_Pull[i] = faded.pull;
    }
}
