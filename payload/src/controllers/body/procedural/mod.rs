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
//! - [`joints`]: the rig for the skeleton the controller holds, and the bounds-checked joint reads
//!   and writes.
//! - [`solver`]: the body under the player's head, the feet, the walk, and the clavicles (pure).
//! - [`gate_state`]: what the gate keeps between ticks, and the steps of the gate that update it.
//! - [`ground`]: the scene rays that find the ground under the footholds.
//! - [`state`]: what the character is doing, for the hand-over table, and whether the animation
//!   moves the body.
//! - [`signals`]: the grapple's ledge climb and the hit reactions, which the character does not
//!   keep.
//! - [`targets`]: the fade of the HumanIK targets on the owned bones.
//! - [`wings`]: Rico's arms in the wingsuit, carried over from the player's.
//! - [`config`]: the mode and the tuning.
//!
//! [`at_solve_gate`] runs the mode: `PassThrough` writes the animated pose back unchanged and logs
//! how it reads back, the proof that the write path is sound; `Full` builds the body, handing each
//! region (the legs, the torso) between it and the animation by the table in [`authority`], and
//! zeroes the game's own IK targets on the bones it owns. While it has any weight, [`owns_body`] tells the
//! pre-solve IK (the body IK's torso, the roomscale crouch, the squared stance) to stand down.
//! Where the animation keeps the hips (a vehicle seat), the torso is solved on the clip's hips
//! instead ([`solver::anchored`]); see `docs/mod/body/procedural-states.md`.

pub mod authority;
pub mod config;
mod gate_state;
pub mod ground;
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
    time::{Duration, Instant},
};

use glam::{Mat4, Vec2, Vec3};
use jc3gi::{
    animation::ik::{Effector, HumanIK},
    character::character::{AimState, Character},
};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        body::{
            alignment::torso_turn,
            procedural::{
                authority::{Case, Weights, classify},
                config::{ProceduralBodyConfig, ProceduralMode},
                gate_state::{FeetRequest, Landing, LowerRequest, STATE, State},
                ground::{GroundCaster, GroundTally},
                rig::{Rig, RigBone, RigError},
                solver::{
                    SolveInput,
                    air::{AirTuning, air_feet, mix_feet},
                    anchored::{solve_anchored, upright_share},
                    arms::{Side, TorsoKeepOut},
                    clavicles::{self, ClavicleTurn},
                    crouch_depth,
                    feet::FootPose,
                    gait::STOP_SHARE,
                    hips_height, hips_horizontal, hips_room, line_yaw,
                    lower::LowerStatus,
                    pose::{RigPose, SolveKind},
                    reach_drop, solve, step_offset,
                },
                targets::{Fade, Slot, TargetFade},
                wings::{WingRequest, read_player_arms},
            },
        },
        frame::Hand,
        locomotion::{
            LocomotionStatus, anchor::status as anchor_status, status as locomotion_status,
        },
        mode::{AirborneVariant, Mode, current as current_mode},
        roomscale::{frame_shared, posture::EffectivePosture},
    },
    headpose::{body_relative_rotation, query},
};

/// What the procedural body did at its last gate.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ProceduralStatus {
    pub at: Option<Instant>,
    pub mode: ProceduralMode,
    /// The torso's weight of the procedural pose against the animation.
    pub weight: f32,
    /// The legs' weight of the procedural pose against the animation.
    pub legs_weight: f32,
    /// The hand-over table's case at the last gate.
    pub case: Case,
    /// Why the skeleton could not be rigged, if it could not.
    pub rig_error: Option<RigError>,
    /// The pass-through's largest read-back error over the driven bones: metres and degrees.
    pub round_trip: Option<(f32, f32)>,
    /// How far the solved hips sit below the animated ones, metres.
    pub hips_drop_m: f32,
    /// How far the hips drop for the legs to reach the feet, metres.
    pub reach_drop_m: f32,
    /// How far the solved neck sits from its target, metres.
    pub neck_error_m: f32,
    /// Whether a planted foot is stepping.
    pub stepping: bool,
    /// The body's horizontal speed, metres per second.
    pub speed_mps: f32,
    /// The walk's phase while walking.
    pub gait_phase: Option<f32>,
    /// How far the head has taken the hips toward their lowest, `0..=1`: what the crouch gait
    /// scales the steps by.
    pub crouch_depth: f32,
    /// The lower body's pose and what it was picked from.
    pub lower: LowerStatus,
    /// How far the stance has stepped under the player's head, metres.
    pub step_offset_m: f32,
    /// How far the falling legs have reached for the ground below, `0..=1`.
    pub landing_reach: f32,
    /// Whether the neutral stance the feet stand around has been measured.
    pub stance_measured: bool,
    /// The last gate's ground casts.
    pub ground: GroundTally,
    /// The clavicles' turns toward the hands, left then right.
    pub clavicles: [ClavicleTurn; 2],
    /// The legs' weight of the air's feet against the ground's.
    pub air_weight: f32,
    /// The root's vertical speed, metres per second, up positive.
    pub vertical_mps: f32,
    /// The landing dip's drop of the hips, metres.
    pub landing_dip_m: f32,
    /// Whether the last pose written was the torso on the animation's hips rather than the body
    /// under the head.
    pub on_animated_hips: bool,
    /// Whether a switch between the two solves is still fading.
    pub switch_fading: bool,
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
    pub keep_out: Option<TorsoKeepOut>,
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
/// transform): `None` when the procedural body did not pose the torso this tick, or posed it with
/// the keep-out switched off. A caller that keeps its own carried target out of the player's body,
/// such as the physical hands' held-object keep-out, queries this instead of rebuilding the torso's
/// shape, so the two keep-outs never drift apart.
pub fn torso_keep_out(to_world: Mat4) -> Option<TorsoKeepOut> {
    arm_context()?
        .keep_out
        .map(|keep_out| keep_out.to_world(to_world))
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

/// Whether the body sits on the ground, or is still getting up from it, as of a gate within the
/// last few ticks: the stick stands it up rather than walking it, so the stick locomotion holds
/// its velocity at zero. A leaf lock, so any thread may ask; a gate that has stopped running
/// stops holding the stick.
pub fn sat_on_ground() -> bool {
    let status = *PUBLISHED.lock();
    status.at.is_some_and(|at| at.elapsed() <= SAT_STALE_AFTER) && status.lower.holds_stick()
}

/// Whether the procedural body poses the torso at all: the wrist targets wait for the gate while
/// it does, so they are queued on the torso it writes.
pub fn owns_body() -> bool {
    torso_weight() > 0.0
}

/// The torso's weight of the procedural pose at the last gate, `0..=1`. The pre-solve IK that
/// would pose the same bones (the body IK's torso, the roomscale crouch, the squared stance) is
/// queued while it is below one, and the gate fades the reach and pull of those targets by what is
/// left of it ([`neutralize_owned_targets`]), so the hand-over is as gradual as the blend.
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
    state.status.legs_weight = 0.0;
}

/// Run the pose authority for the local player, at the `MAIN` pass's solve gate. `dt` is the
/// pass's step, which paces every follower, the blend, the walk, and the vertical speed, so the
/// body keeps the simulation's time rather than the wall clock's.
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
        // A gate that wrote no pose holds no pose of the legs either: a sit must not keep holding
        // the stick through a pass that never reached the solve.
        if !wrote {
            state.lower.reset();
            state.status.lower = state.lower.status();
        }
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
    let (config, off_hand) = Config::lock_query(|c| {
        (
            c.controllers.body.procedural,
            c.controllers.acting_hand().other(),
        )
    });
    let off_side = match off_hand {
        Hand::Left => Side::Left,
        Hand::Right => Side::Right,
    };
    let now = Instant::now();
    // Read before the gate's own state: the roomscale frame half publishes it from the game thread.
    let standing_play = frame_shared()
        .filter(|frame| frame.fresh(now))
        .is_some_and(|frame| frame.posture != EffectivePosture::Seated);
    // Read before the gate's own state too: it takes the configuration and the controller frame.
    let player_arms = (current_mode().mode == Mode::Airborne(AirborneVariant::Wingsuit))
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
    if config.mode == ProceduralMode::Off && !state.blend.weights().any() {
        state.blend.reset();
        state.status.weight = 0.0;
        state.status.legs_weight = 0.0;
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
    let aiming = character
        .m_AimFlags
        .intersects(AimState::m_AimingWeapon | AimState::m_AimingGrapple);
    state.observe_stance(&animated, rig, locomotion.standing_still() && !aiming, dt);
    match config.mode {
        ProceduralMode::PassThrough => {
            unsafe { live.write(controller, &animated) };
            let read_back = unsafe { live.read(controller) };
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
            false
        }
        ProceduralMode::Off | ProceduralMode::Full => {
            let jump_grace_s = Config::lock_query(|c| c.controllers.locomotion.view.jump_grace_s);
            let read = unsafe {
                state
                    .tracker
                    .read(character, &locomotion, now, dt, jump_grace_s, &config)
            };
            let case = if config.mode == ProceduralMode::Off {
                Case::SwitchedOff
            } else {
                classify(&read)
            };
            let previous = state.blend.weights();
            let weights = state.blend.step(case, dt, &config);
            state.log_case(case, &weights, &config);
            state.status.case = case;
            state.status.on_animated_hips = case.anchored() && weights.any();
            state.status.switch_fading = false;
            state.status.weight = weights.torso;
            state.status.legs_weight = weights.legs;
            let world = Mat4::from(character.m_WorldMatrixT1);
            let previous_world = Mat4::from(character.m_WorldMatrixT0);
            let vertical = state.vertical_speed(previous_world.w_axis.y, world.w_axis.y, dt);
            state.track_landing(case, vertical, now, &config);
            let wings = WingRequest {
                case,
                player: player_arms.as_ref(),
                rig,
                animated: &animated,
                world: &world,
                dt,
            };
            state.wing_queue = state.step_wings(&wings, &config);
            if !weights.any() {
                state.reset_under_head();
                state.clavicles.reset();
                state.switch.reset();
                state.on_hips_logged = None;
                state.status.clavicles = [ClavicleTurn::ZERO; 2];
                state.fade.forget();
                return false;
            }
            if case.anchored() {
                let gate = AnchoredGate {
                    character,
                    animated: &animated,
                    case,
                    weights,
                    dt,
                };
                let Some(pose) =
                    (unsafe { pose_on_animated_hips(&mut state, rig, &gate, &config) })
                else {
                    return false;
                };
                unsafe { live.write(controller, &pose) };
                finish_write(&mut state, character, &pose, &weights, &config);
                return true;
            }
            state.on_hips_logged = None;
            let velocity = root_velocity(&previous_world, &world, dt);
            let commanded = commanded_velocity(&locomotion);
            let mut caster = GroundCaster::new(character, world.w_axis.y, &config);
            // The legs come back from the animation with the feet where it left them, and step
            // from there; while the animation has them, the feet are its own.
            let legs_procedural = weights.legs > 0.0 || case.targets(&config)[0] > 0.0;
            let airborne = config.air_legs
                && matches!(
                    case,
                    Case::Jump | Case::HandClimb | Case::Flight | Case::Wingsuit | Case::Parachute
                );
            let head = head_in_model(&world);
            let crouch = crouch_depth(rig, head.y, &config);
            state.status.crouch_depth = crouch;
            let mut request = FeetRequest {
                world: &world,
                rig,
                animated: &animated,
                still: locomotion.standing_still(),
                velocity,
                commanded,
                crouch,
                step_offset: Vec2::ZERO,
                dt,
            };
            let stance = state.stance_feet(&request);
            // The stance steps under a head that has gone further than a lean, so the feet follow
            // a player walking about the room instead of the body stretching after the head.
            let wanted_step = if config.step_under_head {
                let (_, rotation, _) = world.to_scale_rotation_translation();
                let limit = config.pelvis_turn_limit_deg.max(0.0).to_radians();
                let yaw = torso_turn(rotation).map_or(0.0, |y| y.clamp(-limit, limit));
                let middle = (stance[0].position + stance[1].position) * 0.5;
                step_offset(rig, head, middle, yaw, config.lean_allowance_m)
            } else {
                Vec2::ZERO
            };
            request.step_offset = state.step.step(wanted_step, dt, STEP_UNDER_S);
            state.status.step_offset_m = request.step_offset.length();
            let feet = if legs_procedural {
                let ground_feet = if airborne {
                    // The feet leave the ground with the body; the landing plants them afresh
                    // where they are wanted.
                    state.feet.reset();
                    let takeoff = state.last_feet.unwrap_or_else(|| animated_feet(&animated));
                    *state.takeoff_feet.get_or_insert(takeoff)
                } else {
                    let landed = state.takeoff_feet.take().is_some();
                    if !state.feet.placed() && !landed {
                        state
                            .feet
                            .plant(animated_feet(&animated).map(|f| to_world(&world, &f)));
                    }
                    state.place_feet(&request, &config, &mut caster)
                };
                let air_weight = state.air.step(airborne, dt);
                state.status.air_weight = air_weight;
                if air_weight > 0.0 {
                    let tuning = AirTuning {
                        tuck: config.air_tuck,
                        hang: config.air_hang,
                        fall_speed_mps: config.air_fall_speed_mps,
                        toe_drop: config.air_toe_drop_deg.to_radians(),
                    };
                    let hips = hips_height(rig, head.y, &config);
                    let air = air_feet(rig, stance, hips, vertical, &tuning);
                    let air = if config.landing_reach && airborne && vertical < 0.0 {
                        let landing = Landing {
                            world: &world,
                            rig,
                            stance,
                            hips,
                            vertical,
                        };
                        unsafe { state.reach_for_landing(&landing, air, &mut caster, &config) }
                    } else {
                        state.status.landing_reach = 0.0;
                        air
                    };
                    mix_feet(ground_feet, air, air_weight)
                } else {
                    state.status.landing_reach = 0.0;
                    ground_feet
                }
            } else {
                state.feet.reset();
                state.air.reset();
                state.takeoff_feet = None;
                state.status.air_weight = 0.0;
                animated_feet(&animated)
            };
            state.last_feet = Some(feet);
            state.status.ground = caster.tally;
            state.ground_log.record(&caster.tally, now);
            state.status.stepping = state.feet.stepping();
            state.status.speed_mps = velocity.length();
            state.status.gait_phase = state.feet.phase();
            let Some(mut input) = solve_input(character, &animated, feet) else {
                return false;
            };
            input.support = (stance[0].position + stance[1].position) * 0.5
                + Vec3::new(request.step_offset.x, 0.0, request.step_offset.y);
            (input.planted, input.heel_lifts) = if legs_procedural && !airborne {
                (state.feet.planted(), state.feet.heel_lifts())
            } else {
                ([true; 2], [0.0; 2])
            };
            // The torso comes back from the animation with the hips where it left them, and rises
            // from there at the reach drop's pace and moves over the stance at the follower's.
            if previous.torso <= 0.0 {
                let hips = animated.position(RigBone::Hips);
                state.hips.seed(Vec2::new(hips.x, hips.z));
                let seed =
                    hips_height(rig, input.head.y, &config) - animated.position(RigBone::Hips).y;
                state
                    .drop
                    .seed(seed.min(hips_room(rig, input.head.y, &config)));
            }
            let place = hips_horizontal(rig, &input, &config);
            input.hips_xz = Some(state.hips.step(place, dt, config.hips_follow_s));
            let lower = LowerRequest {
                world: &world,
                rig,
                stance,
                crouch,
                grounded: case == Case::OnFoot && legs_procedural && !airborne,
                moving: commanded.length() > config.gait_speed_mps * STOP_SHARE
                    || locomotion.stick.length() > MOVING_STICK,
                stick: locomotion.stick.length() > MOVING_STICK,
                standing_play,
                head_world: query().position,
                off_side,
                dt,
            };
            if let Some(targets) = state.step_lower(&lower, &input, &config) {
                input.hold(targets);
            }
            let need = reach_drop(rig, &input, &config);
            let reach = state.drop.step(need, dt, config.reach_drop_s);
            let dip = state.dip.step(dt, config.landing_dip_s);
            state.status.reach_drop_m = reach;
            state.status.landing_dip_m = dip;
            // The dip is the hips' alone: the view is anchored to the root, not to them.
            input.hips_drop = (reach + dip).min(hips_room(rig, input.head.y, &config));
            let mut solved = solve(rig, &input, &config);
            let turns = unsafe { state.turn_clavicles(character, rig, &solved, dt, &config) };
            for side in Side::BOTH {
                clavicles::apply(&mut solved, rig, side, turns[side.index()]);
            }
            let pose = animated.blend_regions(&solved, weights.legs, weights.torso);
            let pose = state
                .switch
                .step(SolveKind::UnderHead, pose, dt, config.blend_s);
            unsafe { live.write(controller, &pose) };
            finish_write(&mut state, character, &pose, &weights, &config);
            state.status.hips_drop_m =
                animated.position(RigBone::Hips).y - pose.position(RigBone::Hips).y;
            let neck_target = input.head
                + input.head_rotation
                    * (animated.position(RigBone::Neck) - animated.position(RigBone::Head));
            state.status.neck_error_m = solved.position(RigBone::Neck).distance(neck_target);
            state.log_gait(now, &config);
            true
        }
    }
}

/// What the gate poses the torso on the animation's hips from.
struct AnchoredGate<'a> {
    character: &'a Character,
    animated: &'a RigPose,
    case: Case,
    weights: Weights,
    dt: f32,
}

/// The pose to write where the animation keeps the hips (a vehicle seat): the torso solved on the
/// clip's hips toward the player's head, the clavicles turned toward the hands, and the legs left
/// to the animation. `None` when the head is not known.
///
/// # Safety
///
/// As [`at_solve_gate`].
unsafe fn pose_on_animated_hips(
    state: &mut State,
    rig: &Rig,
    gate: &AnchoredGate<'_>,
    config: &ProceduralBodyConfig,
) -> Option<RigPose> {
    // The body under the head starts afresh from the animation when it comes back.
    state.reset_under_head();
    let input = solve_input(gate.character, gate.animated, animated_feet(gate.animated))?;
    let mut solved = solve_anchored(&input, config);
    let turns = unsafe { state.turn_clavicles(gate.character, rig, &solved, gate.dt, config) };
    for side in Side::BOTH {
        clavicles::apply(&mut solved, rig, side, turns[side.index()]);
    }
    let pose = gate
        .animated
        .blend_regions(&solved, gate.weights.legs, gate.weights.torso);
    let neck_target = input.head
        + input.head_rotation
            * (gate.animated.position(RigBone::Neck) - gate.animated.position(RigBone::Head));
    let neck_error_m = solved.position(RigBone::Neck).distance(neck_target);
    if state.on_hips_logged != Some(gate.case) {
        state.on_hips_logged = Some(gate.case);
        tracing::info!(
            target: "body",
            case = gate.case.label(),
            torso = gate.weights.torso,
            legs = gate.weights.legs,
            neck_error_m,
            "procedural body: the torso follows the head on the animation's hips",
        );
    }
    let pose = state
        .switch
        .step(SolveKind::OnAnimatedHips, pose, gate.dt, config.blend_s);
    state.status.hips_drop_m = 0.0;
    state.status.reach_drop_m = 0.0;
    state.status.landing_dip_m = 0.0;
    state.status.air_weight = 0.0;
    state.status.landing_reach = 0.0;
    state.status.stepping = false;
    state.status.gait_phase = None;
    state.status.step_offset_m = 0.0;
    state.status.neck_error_m = neck_error_m;
    Some(pose)
}

/// The steps after a procedural pose is written: publish the torso's weight and the arm context
/// the hand IK queues the wrist targets against, and fade the owned bones' IK targets.
fn finish_write(
    state: &mut State,
    character: &mut Character,
    pose: &RigPose,
    weights: &Weights,
    config: &ProceduralBodyConfig,
) {
    publish_torso_weight(weights.torso);
    state.status.switch_fading = state.switch.fading();
    // The elbow hints point down and back from the chest, which only holds for a chest near
    // upright.
    let upright = (pose.position(RigBone::Neck) - pose.position(RigBone::Spine))
        .try_normalize()
        .is_some_and(|axis| upright_share(axis) > ELBOW_HINT_UPRIGHT);
    state.arms = (weights.torso > 0.0).then(|| ArmContext {
        keep_out: config.keep_out.then(|| {
            TorsoKeepOut::around(
                pose,
                config.chest_radius_m,
                config.abdomen_radius_m,
                config.hand_margin_m,
            )
        }),
        elbow_reach: (config.elbow_hints && upright)
            .then_some(config.elbow_hint_reach * weights.torso),
        chest_yaw: line_yaw(
            pose.position(RigBone::LeftShoulder),
            pose.position(RigBone::RightShoulder),
        )
        .unwrap_or(0.0),
    });
    if config.neutralize_targets {
        neutralize_owned_targets(&mut character.m_HIK, weights, &mut state.fade);
    } else {
        state.fade.forget();
    }
}

/// How upright the written chest must be for the elbow hints, as [`upright_share`] measures it.
const ELBOW_HINT_UPRIGHT: f32 = 0.5;

/// The longest step the body takes, seconds: a hitch is not a long blend.
const MAX_STEP_S: f32 = 0.1;

/// How far the move stick must be pushed for the body to count as told to move, whatever the
/// velocity has reached yet.
const MOVING_STICK: f32 = 0.3;

/// How quickly the stance follows its step under the head, seconds: the time constant of its
/// follower, so a head that crosses the allowance for a moment does not jerk the feet's places.
const STEP_UNDER_S: f32 = 0.15;

/// How long a gate's sit holds the stick after it, at most: past this the gate has stopped
/// running, and the stick walks again.
const SAT_STALE_AFTER: Duration = Duration::from_millis(200);

/// How often the pass-through logs its read-back.
const PASS_THROUGH_LOG_INTERVAL: Duration = Duration::from_secs(5);

/// The fastest the root moves vertically between two gates and still counts as moving, metres per
/// second; anything faster is a teleport.
const MAX_VERTICAL_MPS: f32 = 60.0;

/// The fastest the root moves horizontally between two ticks and still counts as moving, metres
/// per second; anything faster is a teleport.
const MAX_HORIZONTAL_MPS: f32 = 40.0;

/// The effectors of the torso region's bones.
const TORSO_EFFECTORS: [Effector; 5] = [
    Effector::HIPS,
    Effector::WAIST,
    Effector::CHEST_END,
    Effector::LEFT_SHOULDER,
    Effector::RIGHT_SHOULDER,
];

/// The effectors of the legs region's bones.
const LEG_EFFECTORS: [Effector; 8] = [
    Effector::LEFT_HIP,
    Effector::RIGHT_HIP,
    Effector::LEFT_KNEE,
    Effector::RIGHT_KNEE,
    Effector::LEFT_ANKLE,
    Effector::RIGHT_ANKLE,
    Effector::LEFT_FOOT,
    Effector::RIGHT_FOOT,
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
    legs_weight: 0.0,
    case: Case::NotInHeadset,
    rig_error: None,
    round_trip: None,
    hips_drop_m: 0.0,
    reach_drop_m: 0.0,
    neck_error_m: 0.0,
    stepping: false,
    speed_mps: 0.0,
    gait_phase: None,
    crouch_depth: 0.0,
    lower: LowerStatus::new(),
    step_offset_m: 0.0,
    landing_reach: 0.0,
    stance_measured: false,
    ground: GroundTally::new(),
    clavicles: [ClavicleTurn::ZERO; 2],
    air_weight: 0.0,
    vertical_mps: 0.0,
    landing_dip_m: 0.0,
    on_animated_hips: false,
    switch_fading: false,
    wing_weight: 0.0,
    wing_hands: [false; 2],
    wing_queued: [false; 2],
};

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

/// The root's horizontal velocity over the pass, world space, from the previous tick's root to
/// this one's: whatever moved it (the stick, the roomscale chase, a push, a wall that stopped it).
/// A teleport reads as standing still.
fn root_velocity(previous: &Mat4, current: &Mat4, dt: f32) -> Vec3 {
    let moved = current.w_axis - previous.w_axis;
    let horizontal = Vec2::new(moved.x, moved.z);
    let speed = root_speed(horizontal.length(), dt, MAX_HORIZONTAL_MPS);
    let velocity = horizontal.normalize_or_zero() * speed;
    Vec3::new(velocity.x, 0.0, velocity.y)
}

/// The body's commanded horizontal velocity, world space: the stick's, or the movement task's
/// where the stick does not drive the body.
fn commanded_velocity(locomotion: &LocomotionStatus) -> Vec3 {
    let v: Vec2 = locomotion.velocity.unwrap_or(locomotion.task_velocity);
    let velocity = Vec3::new(v.x, 0.0, v.y);
    if velocity.is_finite() {
        velocity
    } else {
        Vec3::ZERO
    }
}

/// The solve's inputs from the headpose, the torso estimate, and the feet, in model space.
fn solve_input(
    character: &Character,
    animated: &RigPose,
    feet: [FootPose; 2],
) -> Option<SolveInput> {
    let world = Mat4::from(character.m_WorldMatrixT1);
    let (_, rotation, _) = world.to_scale_rotation_translation();
    let head = head_in_model(&world);
    head.is_finite().then(|| SolveInput {
        animated: *animated,
        head,
        head_rotation: body_relative_rotation(),
        torso_yaw: torso_turn(rotation),
        feet: feet.map(|f| f.position),
        foot_rotations: feet.map(|f| f.rotation),
        support: (feet[0].position + feet[1].position) * 0.5,
        planted: [true; 2],
        heel_lifts: [0.0; 2],
        hips_xz: None,
        hips_drop: 0.0,
        lower: None,
    })
}

/// The player's head in the model space of the root at `world`. The view hangs from the root at
/// its smoothed height on the ground ([`AnchorStatus::height_offset`](crate::controllers::locomotion::anchor::AnchorStatus::height_offset)), so on a stair the head
/// trails the raw root by what the smoothing hides; that is taken back out, so the solve measures
/// the head against the same root the view does, and a staircase does not read as a crouch.
fn head_in_model(world: &Mat4) -> Vec3 {
    let anchor = anchor_status();
    let hidden = anchor.height_offset * anchor.weight;
    let hidden = if hidden.is_finite() { hidden } else { 0.0 };
    world
        .inverse()
        .transform_point3(query().position - Vec3::Y * hidden)
}

/// Fade the game's (and the mod's) IK targets on the bones the procedural body owns, each region
/// by its weight, so the solve after the gate leaves them where the pose put them: in proportion
/// to the weight, whatever was queued this tick ([`TargetFade`]). The head's target keeps its
/// reach, so it still closes the gap to the player's head, but loses its pull by the torso's
/// weight, so it cannot drag the procedural spine and hips after it.
fn neutralize_owned_targets(hik: &mut HumanIK, weights: &Weights, fade: &mut TargetFade) {
    let torso = TORSO_EFFECTORS.map(|e| (e as usize, weights.torso, Fade::All));
    let legs = LEG_EFFECTORS.map(|e| (e as usize, weights.legs, Fade::All));
    let head = [(Effector::HEAD as usize, weights.torso, Fade::Pull)];
    for (i, weight, kind) in torso.into_iter().chain(legs).chain(head) {
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

/// The speed of a root that moved `distance` metres over `dt` seconds, metres per second: zero for
/// no step, and for anything faster than `max_mps`, which is a teleport rather than motion.
fn root_speed(distance: f32, dt: f32, max_mps: f32) -> f32 {
    if dt <= 0.0 {
        return 0.0;
    }
    let speed = distance / dt;
    if speed.is_finite() && speed.abs() <= max_mps {
        speed
    } else {
        0.0
    }
}

/// The animated feet, model space.
fn animated_feet(animated: &RigPose) -> [FootPose; 2] {
    [RigBone::LeftFoot, RigBone::RightFoot].map(|bone| FootPose {
        position: animated.position(bone),
        rotation: animated.get(bone).rotation,
    })
}

/// A model-space foot pose in the world.
fn to_world(world: &Mat4, foot: &FootPose) -> FootPose {
    let (_, rotation, _) = world.to_scale_rotation_translation();
    FootPose {
        position: world.transform_point3(foot.position),
        rotation: (rotation * foot.rotation).normalize(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_speed_ignores_a_teleport_and_an_empty_step() {
        assert_eq!(root_speed(0.1, 0.05, MAX_VERTICAL_MPS), 2.0);
        assert_eq!(root_speed(-0.1, 0.05, MAX_VERTICAL_MPS), -2.0);
        assert_eq!(root_speed(0.1, 0.0, MAX_VERTICAL_MPS), 0.0);
        assert_eq!(root_speed(50.0, 0.05, MAX_VERTICAL_MPS), 0.0);
        assert_eq!(root_speed(f32::NAN, 0.05, MAX_VERTICAL_MPS), 0.0);
    }
}
