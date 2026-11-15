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
//! The native root and attachment remain authoritative. A standing stunt position is the on-foot
//! animation (the vehicle's interaction node carries the character), so its torso is procedural on
//! the clip's hips; other cases, including the helicopter hang, retain the animated hips. The torso
//! (the spine, neck, head, and clavicles) follows the player wherever the hand-over table gives it
//! to them ([`authority`]).
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
//! - [`hips`]: the clip's hips against Rico's facing, which the yaw follow squares.
//! - [`stance`]: the feet squared under the hips while Rico stands idle.
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
pub mod hips;
pub mod joints;
pub mod rig;
pub mod signals;
pub mod skeleton;
pub mod solver;
pub mod stance;
pub mod state;
pub mod targets;
pub mod twist;
pub mod wings;

use std::{
    sync::{
        LazyLock,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

use body_rig::angle::line_yaw;
use glam::{Mat4, Quat, Vec3};
use jc3gi::{
    animation::ik::{Effector, HumanIK},
    character::character::Character,
};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        body::{
            firm_ragdoll,
            procedural::{
                authority::{Case, WeightFollower, classify_locked, relaxes_legs},
                config::{ProceduralBodyConfig, ProceduralMode},
                gate_state::{STATE, State, relaxed_legs},
                hips::square_pose,
                rig::{Rig, RigBone},
                solver::{
                    SolveInput,
                    anchored::{clear_clavicle_offset, solve_anchored, upright_share},
                    arms::{Side, TorsoKeepOut},
                    clavicles::{self, ClavicleTurn},
                    pose::RigPose,
                },
                stance::{place_feet, square_stance},
                targets::{Fade, Slot, TargetFade},
                wings::{WingRequest, read_player_arms},
            },
            rig::{grounded, torso::relative_heading, torso_turn},
        },
        cockpit::CockpitTransform,
        hands::{active_hands_raw, arm_remaps},
        locomotion::{anchor::JUMP_GRACE_S, status as locomotion_status},
        mode::{AirborneVariant, Mode, current as current_mode},
    },
    grapple::physical::lock,
    hash::StateSet,
    headpose::{body_relative_rotation, xr},
    hooks::animation::active_state_type_id,
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
    /// The square stance's weight against the clip's feet.
    pub stance_weight: f32,
    /// How many foot targets the square stance moved or queued at the last gate.
    pub stance_targets: usize,
    /// Which hands the wingsuit's arms placed a target for, left then right.
    pub wing_hands: [bool; 2],
    /// Which hands the wingsuit's arms queued a target for, left then right.
    pub wing_queued: [bool; 2],
}

impl ProceduralStatus {
    /// Whether the local player's gate ran recently: the body is being solved now, not left as it
    /// was when the gate last ran.
    pub fn fresh(&self, now: Instant) -> bool {
        self.at
            .is_some_and(|at| now.saturating_duration_since(at) <= GATE_FRESH)
    }
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
    state.stance.reset();
    state.legs = WeightFollower::new();
    state.leg_weight = 0.0;
    state.leg_fade.forget();
    publish_leg_weight(0.0);
    state.wing_queue = None;
    state.last_wings = None;
    state.status.wing_hands = [false; 2];
    state.status.wing_queued = [false; 2];
    wings::clear();
    state.status.weight = 0.0;
    state.status.stance_weight = 0.0;
    state.status.stance_targets = 0;
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
        publish_leg_weight(0.0);
        let mut state = STATE.lock();
        state.legs = WeightFollower::new();
        state.leg_weight = 0.0;
        state.leg_fade.forget();
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
    let (config, follow, scale) = Config::lock_query(|c| {
        (
            c.controllers.body.procedural,
            c.controllers.body.rig.follow,
            c.vr.world_scale,
        )
    });
    let now = Instant::now();
    // Read before the gate's own state too: it takes the configuration and the controller frame.
    let mode = current_mode().mode;
    let player_arms = matches!(
        mode,
        Mode::Airborne(AirborneVariant::Wingsuit | AirborneVariant::Parachute)
    )
    .then(read_player_arms)
    .flatten();
    let visual_heading = (mode == Mode::Airborne(AirborneVariant::Parachute))
        .then(|| player_arms.as_ref().map(|arms| arms.torso))
        .flatten();
    let locked = lock::holds();
    let arm_shoulders = active_hands_raw().map_or([None; 2], |active| {
        let cockpit = CockpitTransform::current(active.head.position, active.world_scale);
        let to_model = Mat4::from(character.m_WorldMatrixT1).inverse();
        arm_remaps(
            &active.config,
            active.head,
            active.torso,
            active.world_scale,
        )
        .map(|remap| {
            remap
                .map(|remap| {
                    to_model.transform_point3(cockpit.point_to_world(remap.character_shoulder))
                })
                .filter(|point| point.is_finite())
        })
    });
    let visual_hips = if follow.enabled && follow.square_hips {
        let world = Mat4::from(character.m_WorldMatrixT1);
        let (_, rotation, _) = world.to_scale_rotation_translation();
        grounded::body_yaw().and_then(|yaw| {
            relative_heading(Quat::from_rotation_y(yaw), xr::body_rotation(), rotation)
        })
    } else {
        None
    };
    let mut state = STATE.lock();
    let dt = if dt.is_finite() {
        dt.clamp(0.0, MAX_STEP_S)
    } else {
        0.0
    };
    state.wing_queue = None;
    state.status.wing_hands = [false; 2];
    state.status.wing_weight = 0.0;
    state.status.at = Some(now);
    state.status.mode = config.mode;
    state.arms = None;
    // Switched off, the body blends back to the animation before the gate stops acting.
    if config.mode == ProceduralMode::Off && state.blend.weight() <= 0.0 && state.leg_weight <= 0.0
    {
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
        classify_locked(&read, locked)
    };
    state.hips.track(case, &animated, dt);
    // The square stance is for the standing idle only: a landing's or a get-up's clip that has
    // stopped the root still turns the hips and places the feet itself, and squared under it the
    // feet slid across a slope at their old height (6 of 18 fast-fall landings on the 14 November
    // take, which read as on foot once grounded).
    // SAFETY: the live local character, at the pose gate on the game thread.
    let idle = unsafe { active_state_type_id(character) }
        .is_some_and(|state| STANCE_IDLES.contains(state));
    let stance_authorized = (config.stance.enabled || (follow.enabled && follow.square_hips))
        && case == Case::OnFoot
        && idle
        && locomotion.fresh(now);
    let stance_weight = if stance_authorized {
        state
            .stance
            .step(locomotion.standing_still(), dt, config.stance.blend_s)
    } else {
        state.stance.reset();
        0.0
    };
    state.status.stance_weight = stance_weight;
    let animated = match visual_hips {
        Some(heading) => square_pose(&animated, heading, stance_weight),
        None => animated,
    };
    state.status.stance_targets = match visual_hips.or_else(|| state.hips.yaw()) {
        Some(heading) if config.stance.enabled && stance_weight > 0.0 => {
            let feet = square_stance(&animated, heading, &config.stance);
            unsafe { place_feet(&mut character.m_HIK, rig, &animated, &feet, stance_weight) }
        }
        _ => 0,
    };
    let weight = state.blend.step(case, dt, &config);
    let target = state.blend.target();
    state.log_case(case, target, weight);
    state.status.case = case;
    state.status.weight = weight;
    let world = Mat4::from(character.m_WorldMatrixT1);
    if state.leg_weight <= 0.0 {
        state.leg_swing.step(world.w_axis.truncate(), false, dt);
    }
    let wings = WingRequest {
        case,
        player: player_arms.as_ref(),
        rig,
        animated: &animated,
        world: &world,
        dt,
    };
    state.wing_queue = state.step_wings(&wings, &config);
    state.leg_weight = if (relaxes_legs(case)
        && !read.ledge_climb
        && !read.swimming
        && !read.wingsuit
        && !read.parachute
        && !read.hit_reaction
        && !read.hand_climb
        && !read.authored_motion)
        || case == Case::SwitchedOff
    {
        // The firm ragdoll's physics owns the legs on the line.
        state.legs.step(
            if case == Case::GrappleLock && !firm_ragdoll::owns_legs() {
                1.0
            } else {
                0.0
            },
            dt,
            config.blend_s,
            config.blend_out_s,
        )
    } else {
        state.legs = WeightFollower::new();
        0.0
    };
    let leg_weight = state.leg_weight;
    if config.neutralize_targets {
        fade_targets(
            &mut character.m_HIK,
            LEG_EFFECTORS
                .into_iter()
                .map(|e| (e as usize, leg_weight, Fade::All)),
            &mut state.leg_fade,
        );
    } else {
        state.leg_fade.forget();
    }
    let leg_down = state
        .leg_swing
        .step(world.w_axis.truncate(), leg_weight > 0.0, dt);
    if weight <= 0.0 && leg_weight <= 0.0 {
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
        visual_heading,
        arm_shoulders,
    };
    let mut pose = if weight > 0.0 {
        let Some(pose) = (unsafe { pose_torso(&mut state, rig, &gate, &config) }) else {
            return false;
        };
        pose
    } else {
        animated
    };
    let (_, world_rotation, _) = world.to_scale_rotation_translation();
    let legs = relaxed_legs(
        &animated,
        &rig.reference,
        leg_weight,
        world_rotation.inverse() * leg_down,
    );
    for bone in RigBone::ALL {
        if bone != RigBone::Pelvis && bone.descends_from(RigBone::Pelvis) {
            pose.bones[bone.index()] = legs.bones[bone.index()];
        }
    }
    unsafe { live.write(controller, &pose) };
    publish_leg_weight(leg_weight);
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
    visual_heading: Option<Quat>,
    arm_shoulders: [Option<Vec3>; 2],
}

/// The standing idle states of Rico's base graph, the only states the square stance squares the
/// feet in.
static STANCE_IDLES: LazyLock<StateSet> = LazyLock::new(|| {
    StateSet::new([
        "S_IDLE",
        "S_IDLE_AIM_FOCUS",
        "S_IDLE_PRECISION",
        "S_IDLE_TRANS_TO_AIM",
        "S_IDLE_TRANS_TO_NO_AIM",
        "S_IDLE_TRANS_TO_NO_AIM_SLOW",
        "S_IDLE_GRENADE_FIRE_0",
        "S_IDLE_GRENADE_FIRE_120L",
        "S_IDLE_GRENADE_FIRE_120R",
        "S_IDLE_GRPL_FIRE_0",
        "S_IDLE_GRPL_FIRE_120L",
        "S_IDLE_GRPL_FIRE_120R",
        "S_IDLE_GRPL_TETHER_FIRE_0",
        "S_IDLE_GRPL_TETHER_FIRE_120L",
        "S_IDLE_GRPL_TETHER_FIRE_120R",
    ])
});

/// The share of a gait clip's forward lean the torso takes out on the ground before following the
/// player's head.
const ON_FOOT_DELEAN: f32 = 0.8;

/// How long the de-lean takes to ease in or out across a change of case, seconds.
const DELEAN_BLEND_S: f32 = 0.25;

/// The pose to write: the torso follows the player's head on the clip's hips, including in a
/// standing stunt position, whose animation is the on-foot one. `None` without tracking.
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
    let mut input = solve_input(gate.character, gate.animated, gate.scale)?;
    if gate.case == Case::Parachute {
        let (_, character_rotation, _) =
            Mat4::from(gate.character.m_WorldMatrixT1).to_scale_rotation_translation();
        input.torso_yaw = gate
            .visual_heading
            .and_then(|heading| relative_heading(heading, xr::body_rotation(), character_rotation));
    }
    // On the ground the gait clips lean the whole torso 20 to 27 degrees forward; taken out before
    // the swing, the chest stays square over the pelvis rather than tipped back toward the eyes.
    // Eased in and out, so a jump's takeoff and landing do not move the torso in one tick.
    let grounded = matches!(gate.case, Case::OnFoot | Case::StuntStanding);
    input.delean = ON_FOOT_DELEAN
        * state.delean.step(
            if grounded { 1.0 } else { 0.0 },
            gate.dt,
            DELEAN_BLEND_S,
            DELEAN_BLEND_S,
        );
    let mut solved = solve_anchored(&input, config);
    if matches!(
        gate.case,
        Case::OnFoot | Case::StuntStanding | Case::Jump | Case::Parachute | Case::Flight
    ) {
        for side in Side::BOTH {
            if let Some(target) = gate.arm_shoulders[side.index()] {
                // The chest clearance ran before this, so the clavicle's own move toward the
                // camera is held to it as well.
                let clavicle = side.arm_bones().0;
                let joint = clavicles::shoulder_joint(&solved, rig, side);
                let offset = clear_clavicle_offset(
                    [solved.position(clavicle), joint],
                    target - joint,
                    &input,
                    config,
                );
                solved.translate(clavicle, offset);
            }
        }
    }
    let turns = unsafe { state.turn_clavicles(gate.character, rig, &solved, gate.dt, config) };
    for side in Side::BOTH {
        clavicles::apply(&mut solved, rig, side, turns[side.index()]);
    }
    let pose = gate.animated.blend_torso(&solved, gate.weight);
    let neck_target = input.head
        + input.head_rotation
            * (input.animated.position(RigBone::Neck) - input.animated.position(RigBone::Head));
    let neck_error_m = solved.position(RigBone::Neck).distance(neck_target);
    if state.on_hips_logged != Some(gate.case) {
        state.on_hips_logged = Some(gate.case);
        tracing::info!(
            target: "body",
            case = gate.case.label(),
            torso = gate.weight,
            neck_error_m,
            "procedural body: the torso follows the tracked head",
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

/// How long after the gate last ran the body still counts as solved: a few ticks.
const GATE_FRESH: Duration = Duration::from_millis(500);

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

const LEG_EFFECTORS: [Effector; 6] = [
    Effector::LEFT_ANKLE,
    Effector::RIGHT_ANKLE,
    Effector::LEFT_KNEE,
    Effector::RIGHT_KNEE,
    Effector::LEFT_FOOT,
    Effector::RIGHT_FOOT,
];

static LEG_WEIGHT: AtomicU32 = AtomicU32::new(0);

pub(super) fn leg_weight() -> f32 {
    f32::from_bits(LEG_WEIGHT.load(Ordering::Relaxed))
}

fn publish_leg_weight(weight: f32) {
    LEG_WEIGHT.store(weight.to_bits(), Ordering::Relaxed);
}

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
    stance_weight: 0.0,
    stance_targets: 0,
    wing_hands: [false; 2],
    wing_queued: [false; 2],
};

/// The solve's inputs from the headpose and the torso, in model space, at the VR world scale
/// `scale`.
fn solve_input(character: &Character, animated: &RigPose, scale: f32) -> Option<SolveInput> {
    let world = Mat4::from(character.m_WorldMatrixT1);
    let (_, rotation, _) = world.to_scale_rotation_translation();
    let head::ModelHead { head, eye } = head::read(&world, scale);
    let head_rotation = body_relative_rotation();
    (head.is_finite() && eye.is_finite()).then(|| SolveInput {
        animated: *animated,
        head,
        eye,
        head_rotation,
        torso_yaw: torso_turn(rotation),
        delean: 0.0,
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
    let hips = [(Effector::HIPS as usize, 0.0, Fade::All)];
    fade_targets(hik, torso.into_iter().chain(head).chain(hips), fade);
}

fn fade_targets(
    hik: &mut HumanIK,
    targets: impl IntoIterator<Item = (usize, f32, Fade)>,
    fade: &mut TargetFade,
) {
    for (i, weight, kind) in targets {
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
