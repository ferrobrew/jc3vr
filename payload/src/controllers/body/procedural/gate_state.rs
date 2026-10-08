//! What the solve gate keeps between ticks ([`State`]): the rig, the region blend, the planted
//! feet, the followers, and the status, behind one lock, with the steps of the gate that read and
//! update them.

use std::time::{Duration, Instant};

use glam::{Mat4, Vec3};
use jc3gi::character::character::{AnimationController, Character};
use parking_lot::Mutex;

use crate::controllers::{
    body::procedural::{
        ArmContext, INITIAL_STATUS, MAX_VERTICAL_MPS, ProceduralStatus, animated_feet,
        authority::{Case, RegionBlend, Weights},
        config::ProceduralBodyConfig,
        ground::{GroundCaster, GroundLog},
        joints::{LiveRig, RigCache},
        rig::{Rig, RigBone},
        root_speed,
        solver::{
            DropFollower, HipsFollower,
            air::{AirFollower, DipTuning, LandingDip},
            arms::Side,
            clavicles::{self, ClavicleFollower, ClavicleTuning, ClavicleTurn},
            feet::{Feet, FeetInput, FootPose, StepTuning},
            gait::{self, NeutralStance, StanceFoot},
            pose::RigPose,
        },
        state::StateTracker,
        targets::TargetFade,
    },
    hands::ik::deferred_wrist_targets,
};

/// How often the walk is logged while walking.
const GAIT_LOG_INTERVAL: Duration = Duration::from_secs(2);

/// How long after a jump ends its landing may still dip the hips.
const LANDING_WINDOW: Duration = Duration::from_millis(300);

pub(super) struct State {
    /// The rig of the skeleton the animation controller holds.
    pub(super) rig: RigCache,
    /// The regions' weights against the animation.
    pub(super) blend: RegionBlend,
    /// What the hand-over table reads from the character, with the history it keeps.
    pub(super) tracker: StateTracker,
    /// Whether the hand-over case has been logged once.
    pub(super) case_logged: bool,
    /// The planted feet, world space.
    pub(super) feet: Feet,
    /// The neutral stance the feet stand and walk around, for the rigged skeleton.
    pub(super) stance: NeutralStance,
    /// The hips' drop for the legs' reach.
    pub(super) drop: DropFollower,
    /// The hips' place over the stance.
    pub(super) hips: HipsFollower,
    /// The clavicles' turns toward the hands.
    pub(super) clavicles: ClavicleFollower,
    /// The legs' mix of the air's feet against the ground's.
    pub(super) air: AirFollower,
    /// The hips' landing dip.
    pub(super) dip: LandingDip,
    /// The fade of the owned bones' IK targets.
    pub(super) fade: TargetFade,
    /// The feet the last gate placed, model space.
    pub(super) last_feet: Option<[FootPose; 2]>,
    /// The feet as they left the ground, model space, while the legs are in the air.
    pub(super) takeoff_feet: Option<[FootPose; 2]>,
    /// The fastest fall of the jump under way, metres per second down, and when the jump ended
    /// if it has.
    pub(super) fall: Option<(f32, Option<Instant>)>,
    pub(super) ground_log: GroundLog,
    /// This tick's arm context, for the hand IK the gate runs next.
    pub(super) arms: Option<ArmContext>,
    pub(super) logged_at: Option<Instant>,
    pub(super) gait_logged_at: Option<Instant>,
    pub(super) status: ProceduralStatus,
}

/// What the feet are placed from, besides the settings and the ground.
pub(super) struct FeetRequest<'a> {
    /// The root's world transform.
    pub(super) world: &'a Mat4,
    pub(super) rig: &'a Rig,
    pub(super) animated: &'a RigPose,
    /// Whether Rico stands still: the animated feet stand for the stance until it is measured.
    pub(super) still: bool,
    /// The root's horizontal velocity over the pass, world space: what the walk keeps pace with.
    pub(super) velocity: Vec3,
    /// The velocity the body is told to move at, world space: where the next footholds lead.
    pub(super) commanded: Vec3,
    pub(super) dt: f32,
}

impl State {
    /// The rig of `controller`'s skeleton, checked against it. A new skeleton forgets the stance
    /// measured on the last one.
    pub(super) unsafe fn rig_for(&mut self, controller: &AnimationController) -> Option<LiveRig> {
        let lookup = unsafe { self.rig.get(controller) };
        if lookup.rebuilt {
            self.status.rig_error = lookup.error;
            self.stance = NeutralStance::new();
        }
        lookup.rig
    }

    /// Measure the neutral stance from the animated feet once Rico has stood still, not aiming,
    /// for a moment.
    pub(super) fn observe_stance(&mut self, animated: &RigPose, rig: &Rig, still: bool, dt: f32) {
        let bones = [RigBone::LeftFoot, RigBone::RightFoot];
        let feet = bones.map(|bone| StanceFoot {
            position: animated.position(bone),
            rotation: animated.get(bone).rotation,
        });
        let reference = bones.map(|bone| rig.reference(bone).rotation);
        if let Some(stance) = self.stance.observe(feet, reference, still, dt) {
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
    pub(super) fn place_feet(
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
            gait: gait::tuning(velocity.length(), self.feet.phase().is_some(), config),
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
    pub(super) unsafe fn turn_clavicles(
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
    pub(super) fn stance_feet(&self, request: &FeetRequest<'_>) -> [FootPose; 2] {
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
    pub(super) fn vertical_speed(&mut self, previous: f32, current: f32, dt: f32) -> f32 {
        let speed = root_speed(current - previous, dt, MAX_VERTICAL_MPS);
        self.status.vertical_mps = speed;
        speed
    }

    /// Follow a jump's fall, and dip the hips when it lands on foot: the fastest fall of the jump
    /// kicks the dip when the body is back on the ground within [`LANDING_WINDOW`] of the jump
    /// ending.
    pub(super) fn track_landing(
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
    pub(super) fn log_case(
        &mut self,
        case: Case,
        weights: &Weights,
        config: &ProceduralBodyConfig,
    ) {
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
    pub(super) fn log_gait(&mut self, now: Instant, config: &ProceduralBodyConfig) {
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
        let tuning = gait::tuning(speed, true, config);
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

pub(super) static STATE: Mutex<State> = Mutex::new(State {
    rig: RigCache::new(),
    blend: RegionBlend::new(),
    tracker: StateTracker::new(),
    case_logged: false,
    feet: Feet::new(),
    stance: NeutralStance::new(),
    drop: DropFollower::new(),
    hips: HipsFollower::new(),
    clavicles: ClavicleFollower::new(),
    air: AirFollower::new(),
    dip: LandingDip::new(),
    fade: TargetFade::new(),
    last_feet: None,
    takeoff_feet: None,
    fall: None,
    ground_log: GroundLog::new(),
    arms: None,
    logged_at: None,
    gait_logged_at: None,
    status: INITIAL_STATUS,
});
