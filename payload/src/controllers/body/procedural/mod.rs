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
//! - [`ground`]: the scene rays that find the ground under the footholds.
//! - [`state`]: what the character is doing, for the hand-over table, and whether the animation
//!   moves the body.
//! - [`signals`]: the grapple's ledge climb and the hit reactions, which the character does not
//!   keep.
//! - [`config`]: the mode and the tuning.
//!
//! [`at_solve_gate`] runs the mode: `PassThrough` writes the animated pose back unchanged and logs
//! how it reads back, the proof that the write path is sound; `Full` builds the body, handing each
//! region (the legs, the torso) between it and the animation by the table in [`authority`], and
//! zeroes the game's own IK targets on the bones it owns. While it has any weight, [`owns_body`] tells the
//! pre-solve IK (the body IK's torso, the roomscale crouch, the squared stance) to stand down.

pub mod authority;
pub mod config;
pub mod ground;
pub mod joints;
pub mod rig;
pub mod signals;
pub mod skeleton;
pub mod solver;
pub mod state;

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use glam::{Mat4, Vec2, Vec3};
use jc3gi::{
    animation::ik::{Effector, HumanIK},
    character::character::{AnimationController, Character},
};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        body::{
            alignment::torso_turn,
            procedural::{
                authority::{Case, RegionBlend, Weights, classify},
                config::{ProceduralBodyConfig, ProceduralMode},
                ground::{GroundCaster, GroundLog, GroundTally},
                joints::{LiveRig, RigCache},
                rig::{Rig, RigBone, RigError},
                solver::{
                    DropFollower, SolveInput,
                    air::{AirFollower, AirTuning, DipTuning, LandingDip, air_feet, mix_feet},
                    arms::{Side, TorsoKeepOut},
                    clavicles::{self, ClavicleFollower, ClavicleTuning, ClavicleTurn},
                    feet::{Feet, FeetInput, FootPose, StepTuning},
                    gait::{self, NeutralStance, StanceFoot},
                    hips_height,
                    pose::RigPose,
                    reach_drop, solve,
                },
                state::StateTracker,
            },
        },
        hands::ik::deferred_wrist_targets,
        locomotion::{LocomotionStatus, status as locomotion_status},
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
}

/// What the controllers' arm targets are queued against while the procedural body poses the
/// torso: published by the gate for the hand IK it runs next.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ArmContext {
    /// The torso the wrist and elbow targets are kept out of.
    pub keep_out: Option<TorsoKeepOut>,
    /// The elbow hints' reach, already scaled by the procedural pose's weight.
    pub elbow_reach: Option<f32>,
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

/// Whether the procedural body poses the torso and legs: the pre-solve IK that would pose them
/// stands down while it does.
pub fn owns_body() -> bool {
    OWNS_BODY.load(Ordering::Relaxed)
}

/// Run the pose authority for the local player, at the `MAIN` pass's solve gate. `dt` is the
/// pass's step, which paces every follower, the blend, the walk, and the vertical speed, so the
/// body keeps the simulation's time rather than the wall clock's.
///
/// # Safety
///
/// `character` must be the live local character, inside its pose-finalization pass, on the thread
/// running it.
pub(crate) unsafe fn at_solve_gate(character: &mut Character, dt: f32) {
    // SAFETY: forwarded from the caller.
    unsafe { run_gate(character, dt) };
    let status = STATE.lock().status;
    *PUBLISHED.lock() = status;
}

/// The gate itself; [`at_solve_gate`] publishes its status after it.
///
/// # Safety
///
/// As [`at_solve_gate`].
unsafe fn run_gate(character: &mut Character, dt: f32) {
    let config = Config::lock_query(|c| c.controllers.body.procedural);
    let now = Instant::now();
    let mut state = STATE.lock();
    let dt = if dt.is_finite() {
        dt.clamp(0.0, MAX_STEP_S)
    } else {
        0.0
    };
    state.status.at = Some(now);
    state.status.mode = config.mode;
    state.arms = None;
    if config.mode == ProceduralMode::Off {
        state.blend.reset();
        state.status.weight = 0.0;
        state.status.legs_weight = 0.0;
        OWNS_BODY.store(false, Ordering::Relaxed);
        return;
    }
    let Some(controller) = (unsafe { character.m_AnimatedModel.m_AnimationController.as_mut() })
    else {
        return;
    };
    let Some(live) = (unsafe { state.rig_for(controller) }) else {
        OWNS_BODY.store(false, Ordering::Relaxed);
        return;
    };
    let rig = &live.rig;
    let animated = unsafe { live.read(controller) };
    let locomotion = locomotion_status();
    state.observe_stance(&animated, &locomotion, dt);
    match config.mode {
        ProceduralMode::Off => {}
        ProceduralMode::PassThrough => {
            OWNS_BODY.store(false, Ordering::Relaxed);
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
        }
        ProceduralMode::Full => {
            let jump_grace_s = Config::lock_query(|c| c.controllers.locomotion.view.jump_grace_s);
            let case = classify(&unsafe {
                state
                    .tracker
                    .read(character, &locomotion, now, dt, jump_grace_s, &config)
            });
            let previous = state.blend.weights();
            let weights = state.blend.step(case, dt, &config);
            state.log_case(case, &weights, &config);
            OWNS_BODY.store(weights.torso > 0.0, Ordering::Relaxed);
            state.status.case = case;
            state.status.weight = weights.torso;
            state.status.legs_weight = weights.legs;
            let world = Mat4::from(character.m_WorldMatrixT1);
            let previous_world = Mat4::from(character.m_WorldMatrixT0);
            let vertical = state.vertical_speed(previous_world.w_axis.y, world.w_axis.y, dt);
            state.track_landing(case, vertical, now, &config);
            if !weights.any() {
                state.feet.reset();
                state.drop.reset();
                state.clavicles.reset();
                state.air.reset();
                state.dip.reset();
                state.takeoff_feet = None;
                state.status.clavicles = [ClavicleTurn::ZERO; 2];
                return;
            }
            let velocity = root_velocity(&previous_world, &world, dt);
            let commanded = commanded_velocity(&locomotion);
            let mut caster = GroundCaster::new(character, world.w_axis.y, &config);
            // The legs come back from the animation with the feet where it left them, and step
            // from there; while the animation has them, the feet are its own.
            let legs_procedural = weights.legs > 0.0 || case.targets(config.air_legs)[0] > 0.0;
            let airborne =
                config.air_legs && matches!(case, Case::Jump | Case::HandClimb | Case::Flight);
            let feet = if legs_procedural {
                let request = FeetRequest {
                    world: &world,
                    rig,
                    animated: &animated,
                    still: locomotion.standing_still(),
                    velocity,
                    commanded,
                    dt,
                };
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
                    let hips = hips_height(rig, head_in_model(&world).y, &config);
                    let stance = state.stance_feet(&request);
                    let air = air_feet(rig, stance, hips, vertical, &tuning);
                    mix_feet(ground_feet, air, air_weight)
                } else {
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
                return;
            };
            // The torso comes back from the animation with the hips where it left them, and rises
            // from there at the reach drop's pace.
            if previous.torso <= 0.0 {
                let seed =
                    hips_height(rig, input.head.y, &config) - animated.position(RigBone::Hips).y;
                state.drop.seed(seed.min(hips_room(rig, &config)));
            }
            let need = reach_drop(rig, &input, &config);
            let reach = state.drop.step(need, dt, config.reach_drop_s);
            let dip = state.dip.step(dt, config.landing_dip_s);
            state.status.reach_drop_m = reach;
            state.status.landing_dip_m = dip;
            // The dip is the hips' alone: the view is anchored to the root, not to them.
            input.hips_drop = (reach + dip).min(hips_room(rig, &config));
            let mut solved = solve(rig, &input, &config);
            let turns = unsafe { state.turn_clavicles(character, rig, &solved, dt, &config) };
            for side in Side::BOTH {
                clavicles::apply(&mut solved, rig, side, turns[side.index()]);
            }
            let pose = animated.blend_regions(&solved, weights.legs, weights.torso);
            unsafe { live.write(controller, &pose) };
            state.arms = (weights.torso > 0.0).then(|| ArmContext {
                keep_out: config.keep_out.then(|| {
                    TorsoKeepOut::around(
                        &pose,
                        config.chest_radius_m,
                        config.abdomen_radius_m,
                        config.hand_margin_m,
                    )
                }),
                elbow_reach: config
                    .elbow_hints
                    .then_some(config.elbow_hint_reach * weights.torso),
            });
            if config.neutralize_targets {
                neutralize_owned_targets(&mut character.m_HIK, &weights);
            }
            state.status.hips_drop_m =
                animated.position(RigBone::Hips).y - pose.position(RigBone::Hips).y;
            let neck_target = input.head
                + input.head_rotation
                    * (animated.position(RigBone::Neck) - animated.position(RigBone::Head));
            state.status.neck_error_m = solved.position(RigBone::Neck).distance(neck_target);
            state.log_gait(now, &config);
        }
    }
}

/// The longest step the body takes, seconds: a hitch is not a long blend.
const MAX_STEP_S: f32 = 0.1;

/// How often the pass-through logs its read-back.
const PASS_THROUGH_LOG_INTERVAL: Duration = Duration::from_secs(5);

/// How often the walk is logged while walking.
const GAIT_LOG_INTERVAL: Duration = Duration::from_secs(2);

/// The fastest the root moves vertically between two gates and still counts as moving, metres per
/// second; anything faster is a teleport.
const MAX_VERTICAL_MPS: f32 = 60.0;

/// The fastest the root moves horizontally between two ticks and still counts as moving, metres
/// per second; anything faster is a teleport.
const MAX_HORIZONTAL_MPS: f32 = 40.0;

/// How long after a jump ends its landing may still dip the hips.
const LANDING_WINDOW: Duration = Duration::from_millis(300);

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

static OWNS_BODY: AtomicBool = AtomicBool::new(false);

/// The status [`published_status`] returns. Taken only after the gate releases [`STATE`], and
/// nothing is locked under it.
static PUBLISHED: Mutex<ProceduralStatus> = Mutex::new(INITIAL_STATUS);

struct State {
    /// The rig of the skeleton the animation controller holds.
    rig: RigCache,
    /// The regions' weights against the animation.
    blend: RegionBlend,
    /// What the hand-over table reads from the character, with the history it keeps.
    tracker: StateTracker,
    /// Whether the hand-over case has been logged once.
    case_logged: bool,
    /// The planted feet, world space.
    feet: Feet,
    /// The neutral stance the feet stand and walk around, for the rigged skeleton.
    stance: NeutralStance,
    /// The hips' drop for the legs' reach.
    drop: DropFollower,
    /// The clavicles' turns toward the hands.
    clavicles: ClavicleFollower,
    /// The legs' mix of the air's feet against the ground's.
    air: AirFollower,
    /// The hips' landing dip.
    dip: LandingDip,
    /// The feet the last gate placed, model space.
    last_feet: Option<[FootPose; 2]>,
    /// The feet as they left the ground, model space, while the legs are in the air.
    takeoff_feet: Option<[FootPose; 2]>,
    /// The fastest fall of the jump under way, metres per second down, and when the jump ended
    /// if it has.
    fall: Option<(f32, Option<Instant>)>,
    ground_log: GroundLog,
    /// This tick's arm context, for the hand IK the gate runs next.
    arms: Option<ArmContext>,
    logged_at: Option<Instant>,
    gait_logged_at: Option<Instant>,
    status: ProceduralStatus,
}

/// What the feet are placed from, besides the settings and the ground.
struct FeetRequest<'a> {
    /// The root's world transform.
    world: &'a Mat4,
    rig: &'a Rig,
    animated: &'a RigPose,
    /// Whether Rico stands still: the animated feet stand for the stance until it is measured.
    still: bool,
    /// The root's horizontal velocity over the pass, world space: what the walk keeps pace with.
    velocity: Vec3,
    /// The velocity the body is told to move at, world space: where the next footholds lead.
    commanded: Vec3,
    dt: f32,
}

impl State {
    /// The rig of `controller`'s skeleton, checked against it. A new skeleton forgets the stance
    /// measured on the last one.
    unsafe fn rig_for(&mut self, controller: &AnimationController) -> Option<LiveRig> {
        let lookup = unsafe { self.rig.get(controller) };
        if lookup.rebuilt {
            self.status.rig_error = lookup.error;
            self.stance = NeutralStance::new();
        }
        lookup.rig
    }

    /// Measure the neutral stance from the animated feet once Rico has stood still for a moment.
    fn observe_stance(&mut self, animated: &RigPose, locomotion: &LocomotionStatus, dt: f32) {
        let feet = [RigBone::LeftFoot, RigBone::RightFoot].map(|bone| StanceFoot {
            position: animated.position(bone),
            rotation: animated.get(bone).rotation,
        });
        if let Some(stance) = self.stance.observe(feet, locomotion.standing_still(), dt) {
            tracing::info!(
                target: "body",
                left = ?stance[0].position,
                right = ?stance[1].position,
                "procedural body: measured the neutral stance (model space)",
            );
        }
        self.status.stance_measured = self.stance.get().is_some();
    }

    /// Where the feet go this tick, model space: planted in the world, stepping standing and
    /// walking moving, when `config.plant_feet`; where the animation puts them otherwise.
    fn place_feet(
        &mut self,
        request: &FeetRequest<'_>,
        config: &ProceduralBodyConfig,
        caster: &mut GroundCaster,
    ) -> [FootPose; 2] {
        let FeetRequest {
            world,
            animated,
            velocity,
            dt,
            ..
        } = *request;
        if !config.plant_feet {
            self.feet.reset();
            return animated_feet(animated);
        }
        let model = self.stance_feet(request);
        let (_, rotation, _) = world.to_scale_rotation_translation();
        let inverse = world.inverse();
        let wanted = model.map(|f| FootPose {
            position: world.transform_point3(f.position),
            rotation: rotation * f.rotation,
        });
        let tuning = StepTuning {
            threshold_m: config.step_threshold_m,
            threshold_rad: config.step_turn_deg.to_radians(),
            duration_s: config.step_duration_s,
            height_m: config.step_height_m,
            tilt_limit_rad: config.ankle_tilt_limit_deg.to_radians(),
        };
        let input = FeetInput {
            wanted,
            root_height: world.w_axis.y,
            velocity,
            commanded: request.commanded,
            gait: gait::tuning(velocity.length(), config),
            dt,
        };
        // SAFETY: the gate runs with the local character live.
        let mut ground = |point| unsafe { caster.ground(point) };
        self.feet
            .step(&input, &tuning, &mut ground)
            .map(|f| FootPose {
                position: inverse.transform_point3(f.position),
                rotation: (rotation.inverse() * f.rotation).normalize(),
            })
    }

    /// The clavicles' turns this tick: toward the wrist targets the hand IK queues next, eased.
    ///
    /// # Safety
    ///
    /// As [`at_solve_gate`].
    unsafe fn turn_clavicles(
        &mut self,
        character: &Character,
        rig: &Rig,
        pose: &RigPose,
        dt: f32,
        config: &ProceduralBodyConfig,
    ) -> [ClavicleTurn; 2] {
        let wanted = if config.clavicles {
            let targets = unsafe { deferred_wrist_targets(character) };
            let tuning = ClavicleTuning {
                elevation: config.clavicle_elevation_deg.to_radians(),
                depression: config.clavicle_depression_deg.to_radians(),
                protraction: config.clavicle_protraction_deg.to_radians(),
                reach_start: config.clavicle_reach_start,
            };
            Side::BOTH.map(|side| {
                targets[side.index()].map_or(ClavicleTurn::ZERO, |(target, weight)| {
                    clavicles::wanted(pose, rig, side, target, weight, &tuning)
                })
            })
        } else {
            [ClavicleTurn::ZERO; 2]
        };
        let turns = self.clavicles.step(wanted, dt, config.clavicle_follow_s);
        self.status.clavicles = turns;
        turns
    }

    /// The neutral stance's feet, model space. Until the stance is measured, standing still keeps
    /// the animation's feet and moving takes the reference pose's.
    fn stance_feet(&self, request: &FeetRequest<'_>) -> [FootPose; 2] {
        match self.stance.get() {
            Some(stance) => stance.map(|f| FootPose {
                position: f.position,
                rotation: f.rotation,
            }),
            None if request.still => animated_feet(request.animated),
            None => [RigBone::LeftFoot, RigBone::RightFoot].map(|bone| FootPose {
                position: request.rig.reference(bone).translation,
                rotation: request.rig.reference(bone).rotation,
            }),
        }
    }

    /// The root's vertical speed over the pass, metres per second, up positive: from its height at
    /// the previous tick (`m_WorldMatrixT0`) to this one's (`m_WorldMatrixT1`).
    fn vertical_speed(&mut self, previous: f32, current: f32, dt: f32) -> f32 {
        let speed = root_speed(current - previous, dt, MAX_VERTICAL_MPS);
        self.status.vertical_mps = speed;
        speed
    }

    /// Follow a jump's fall, and dip the hips when it lands on foot: the fastest fall of the jump
    /// kicks the dip when the body is back on the ground within [`LANDING_WINDOW`] of the jump
    /// ending.
    fn track_landing(
        &mut self,
        case: Case,
        vertical: f32,
        now: Instant,
        config: &ProceduralBodyConfig,
    ) {
        match case {
            Case::Jump => {
                let fastest = self.fall.map_or(0.0, |(fall, _)| fall).max(-vertical);
                self.fall = Some((fastest, None));
            }
            Case::OnFoot => {
                if let Some((fall, _)) = self.fall.take()
                    && config.landing_dip
                {
                    let tuning = DipTuning {
                        per_mps: config.landing_dip_per_mps,
                        max_m: config.landing_dip_max_m,
                        min_speed_mps: config.landing_dip_min_speed_mps,
                        time_s: config.landing_dip_s,
                    };
                    let depth = self.dip.land(fall, &tuning);
                    tracing::debug!(
                        target: "body",
                        fall_mps = fall,
                        dip_m = depth,
                        "procedural body: landed",
                    );
                }
            }
            // A landing can pass a tick or two with the movement task not yet running; anything
            // longer, or a fall into the air's other states, is not a jump's landing.
            _ => {
                if let Some((fall, ended)) = self.fall {
                    let ended = ended.unwrap_or(now);
                    self.fall = (case == Case::NoMovementTask
                        && now.saturating_duration_since(ended) <= LANDING_WINDOW)
                        .then_some((fall, Some(ended)));
                }
            }
        }
    }

    /// Log a change of the hand-over table's case, with the weights it leaves.
    fn log_case(&mut self, case: Case, weights: &Weights, config: &ProceduralBodyConfig) {
        if self.status.case == case && self.case_logged {
            return;
        }
        self.case_logged = true;
        tracing::info!(
            target: "body",
            from = self.status.case.label(),
            to = case.label(),
            legs = case.targets(config.air_legs)[0],
            torso = case.targets(config.air_legs)[1],
            legs_now = weights.legs,
            torso_now = weights.torso,
            "procedural body: the hand-over case changes",
        );
    }

    /// Log the walk every [`GAIT_LOG_INTERVAL`] while walking.
    fn log_gait(&mut self, now: Instant, config: &ProceduralBodyConfig) {
        let Some(phase) = self.status.gait_phase else {
            return;
        };
        if self
            .gait_logged_at
            .is_some_and(|at| now.saturating_duration_since(at) < GAIT_LOG_INTERVAL)
        {
            return;
        }
        self.gait_logged_at = Some(now);
        let speed = self.status.speed_mps;
        let tuning = gait::tuning(speed, config);
        tracing::debug!(
            target: "body",
            speed_mps = speed,
            stride_m = tuning.map_or(0.0, |t| t.stride_m),
            duty = tuning.map_or(0.0, |t| t.duty),
            phase,
            reach_drop_m = self.status.reach_drop_m,
            neck_error_m = self.status.neck_error_m,
            "procedural body: walking",
        );
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    rig: RigCache::new(),
    blend: RegionBlend::new(),
    tracker: StateTracker::new(),
    case_logged: false,
    feet: Feet::new(),
    stance: NeutralStance::new(),
    drop: DropFollower::new(),
    clavicles: ClavicleFollower::new(),
    air: AirFollower::new(),
    dip: LandingDip::new(),
    last_feet: None,
    takeoff_feet: None,
    fall: None,
    ground_log: GroundLog::new(),
    arms: None,
    logged_at: None,
    gait_logged_at: None,
    status: INITIAL_STATUS,
});

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
    stance_measured: false,
    ground: GroundTally::new(),
    clavicles: [ClavicleTurn::ZERO; 2],
    air_weight: 0.0,
    vertical_mps: 0.0,
    landing_dip_m: 0.0,
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
        hips_drop: 0.0,
    })
}

/// The player's head in the model space of the root at `world`.
fn head_in_model(world: &Mat4) -> Vec3 {
    world.inverse().transform_point3(query().position)
}

/// The most the hips may drop below where the head puts them: down to their lowest.
fn hips_room(rig: &Rig, config: &ProceduralBodyConfig) -> f32 {
    rig.reference(RigBone::Hips).translation.y * (1.0 - config.min_hips_height.clamp(0.1, 1.0))
}

/// Fade the game's (and the mod's) IK targets on the bones the procedural body owns, each region
/// by its weight, so the solve after the gate leaves them where the pose put them.
fn neutralize_owned_targets(hik: &mut HumanIK, weights: &Weights) {
    let torso = TORSO_EFFECTORS.map(|e| (e as usize, weights.torso));
    let legs = LEG_EFFECTORS.map(|e| (e as usize, weights.legs));
    for (i, weight) in torso.into_iter().chain(legs) {
        let keep = (1.0 - weight).clamp(0.0, 1.0);
        hik.m_TargetReachT[i] *= keep;
        hik.m_TargetReachR[i] *= keep;
        hik.m_ReachT[i] *= keep;
        hik.m_ReachR[i] *= keep;
        hik.m_TargetPull[i] *= keep;
        hik.m_Pull[i] *= keep;
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
