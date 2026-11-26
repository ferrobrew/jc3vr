//! The IK body at the solve gate: which state the character is in and why the IK stands down when
//! it does, the feet's contacts in the world, and the IK pose seeded into Rico's posture before the
//! torso is solved on it.
//!
//! The procedural body's gate ([`crate::controllers::body::procedural`]) calls [`prepare`] with the
//! clip's pose before it solves the torso, solves the torso on the pose [`prepare`] returns, writes
//! it, and then calls [`crate::controllers::body::ik_body::effectors::queue`] to hold the seeded
//! pelvis and place the feet through HumanIK's effectors for the solve that follows.

use std::time::{Duration, Instant};

use body_rig::{
    contact::{
        ContactEvent, ContactFrame, FootContacts, FootPose, FootTarget, Handover, Probe, Side,
        Stance, StepRate, SupportFrame, SupportId, Surroundings,
    },
    fk::PelvisSource,
    ik_weight::{IkBlend, IkState, IkWeights},
    math::angle::{line_yaw, wrap, yaw_of},
};
use glam::{Mat4, Vec2, Vec3};
use jc3vr_view::headpose::{teleport, xr};

use crate::controllers::{
    body::{
        firm_ragdoll::{
            self,
            target::{IkLegs, TargetBones},
        },
        ik_body::{
            climb::{ClimbFeet, ClimbLegs, ModelBrace},
            config::IkBodyConfig,
            fk::{self, FkPose},
            probe,
            seed::{self, ModelBody, Proportions},
            support,
            tuning::IK_BODY,
        },
        procedural::{
            authority::Case,
            rig::{Rig, RigBone},
            solver::pose::RigPose,
            tuning::PROCEDURAL,
        },
        rig::{grounded, tuning::RIG},
    },
    locomotion::LocomotionStatus,
    scheme,
    telemetry::report,
};

/// Why the IK body leaves the body to the game.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StandDown {
    /// The gamepad scheme: the estimator does not run.
    Gamepad,
    /// The firm ragdoll owns the legs.
    FirmRagdoll,
    /// No fresh estimate: the FK pelvis is the rig's waist, which cannot place the legs.
    NoEstimate,
    /// The stick moves Rico, or the game's walk does.
    Walking,
    /// The feet handed the legs to the game's walk, and Rico has not stood still long enough since.
    HandedOver(Handover),
    /// On foot in a state outside the standing idles: a landing, a turn, a weapon's act.
    NotIdle,
    /// The movement task is not running on foot.
    NoMovementTask,
    /// A case the table leaves to the game.
    Case(Case),
    /// The procedural body did not write a pose this tick.
    NotWritten,
}

impl StandDown {
    pub fn label(self) -> &'static str {
        match self {
            StandDown::Gamepad => "the gamepad scheme",
            StandDown::FirmRagdoll => "the firm ragdoll owns the legs",
            StandDown::NoEstimate => "no fresh estimate",
            StandDown::Walking => "walking",
            StandDown::HandedOver(_) => "handed to the walk",
            StandDown::NotIdle => "not a standing idle",
            StandDown::NoMovementTask => "the movement task is not running",
            StandDown::Case(case) => case.label(),
            StandDown::NotWritten => "no pose written",
        }
    }
}

/// What the debug UI shows of a foot.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum FootLabel {
    #[default]
    None,
    Planted,
    Swinging,
    Unsupported,
}

impl FootLabel {
    pub fn label(self) -> &'static str {
        match self {
            FootLabel::None => "-",
            FootLabel::Planted => "planted",
            FootLabel::Swinging => "swinging",
            FootLabel::Unsupported => "unsupported",
        }
    }

    fn of(target: &FootTarget) -> Self {
        match target {
            FootTarget::Planted(_) => FootLabel::Planted,
            FootTarget::Swinging { .. } => FootLabel::Swinging,
            FootTarget::Unsupported => FootLabel::Unsupported,
        }
    }
}

/// What the IK body did at its last gate, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct IkStatus {
    pub at: Option<Instant>,
    pub state: IkState,
    pub stand_down: Option<StandDown>,
    pub case: Case,
    pub weights: IkWeights,
    pub source: PelvisSource,
    pub feet: [FootLabel; 2],
    pub planted: usize,
    pub steps: u64,
    pub steps_per_minute: f32,
    pub handovers: u64,
    pub missed: u64,
    /// The last step's foot and why it released.
    pub last_step: Option<(Side, body_rig::contact::Release)>,
    /// How far the pelvis was lowered for the feet at the last gate, and at most this episode,
    /// metres.
    pub pelvis_drop_m: f32,
    pub max_drop_m: f32,
    /// How many HumanIK targets the last gate queued.
    pub targets_queued: usize,
    /// How far the seeded hips stand from the clip's, metres.
    pub hips_moved_m: f32,
    /// Whether each foot holds a brace on the face while climbing.
    pub braced: [bool; 2],
    /// How many times a climbing foot has braced or moved to a new brace.
    pub brace_moves: u64,
    /// Whether the planted feet stand on a moving body rather than the world.
    pub on_moving_support: bool,
    /// How many times the feet have come to stand on a moving body.
    pub moving_landings: u64,
    /// How many moving supports are tracked.
    pub moving_supports: usize,
}

/// What the IK body keeps between gates.
pub(crate) struct GateState {
    pub blend: IkBlend,
    pub contacts: FootContacts,
    pub rate: StepRate,
    pub hold: Option<Hold>,
    /// The feet's last world targets, faded out with the feet's weight after the idle.
    pub last_feet: [Option<FootPose>; 2],
    pub targets: Option<Targets>,
    pub status: IkStatus,
    /// The state and reason last logged.
    pub logged: Option<(IkState, Option<StandDown>)>,
    pub in_idle: bool,
    pub episode: Episode,
    pub climb: ClimbFeet,
    pub epoch: u64,
    pub snaps: u64,
}

/// One spell of the standing body, for its summary in the log.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(crate) struct Episode {
    pub began: Option<Instant>,
    pub steps: u64,
    pub handovers: u64,
    /// When the spell's state was last logged.
    pub logged_at: Option<Instant>,
}

/// A hand-over to the walk, held until Rico stands still again.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Hold {
    why: Handover,
    /// How long Rico has stood still since, seconds.
    still_s: f32,
}

impl GateState {
    pub(crate) const fn new() -> Self {
        Self {
            blend: IkBlend::new(),
            contacts: FootContacts::new(),
            rate: StepRate::new(),
            hold: None,
            last_feet: [None; 2],
            targets: None,
            status: IkStatus {
                at: None,
                state: IkState::Unchanged,
                stand_down: None,
                case: Case::NotInHeadset,
                weights: IkWeights::ZERO,
                source: PelvisSource::Fallback,
                feet: [FootLabel::None; 2],
                planted: 0,
                steps: 0,
                steps_per_minute: 0.0,
                handovers: 0,
                missed: 0,
                last_step: None,
                pelvis_drop_m: 0.0,
                max_drop_m: 0.0,
                targets_queued: 0,
                hips_moved_m: 0.0,
                braced: [false; 2],
                brace_moves: 0,
                on_moving_support: false,
                moving_landings: 0,
                moving_supports: 0,
            },
            logged: None,
            in_idle: false,
            climb: ClimbFeet::new(),
            episode: Episode {
                began: None,
                steps: 0,
                handovers: 0,
                logged_at: None,
            },
            epoch: 0,
            snaps: 0,
        }
    }

    /// Drop the contacts on a teleport or a recenter.
    fn check_resets(&mut self, now: Instant) {
        let epoch = teleport::epoch();
        let snaps = grounded::status().snaps;
        if epoch == self.epoch && snaps == self.snaps {
            return;
        }
        let teleported = epoch != self.epoch;
        self.epoch = epoch;
        self.snaps = snaps;
        if teleported {
            probe::forget();
        }
        if self.in_idle {
            report("ik body", None, "the feet's contacts are dropped", || {
                Some(if teleported { "teleport" } else { "recenter" }.to_owned())
            });
            self.end_episode(now);
        }
        self.drop_contacts();
    }

    pub(crate) fn drop_contacts(&mut self) {
        self.contacts.reset();
        self.last_feet = [None; 2];
    }

    /// Log a change of the state or the reason the IK stands down.
    fn log_state(&mut self, state: IkState, reason: Option<StandDown>, case: Case) {
        if self.logged == Some((state, reason)) {
            return;
        }
        let first = self.logged.is_none();
        self.logged = Some((state, reason));
        if first && state == IkState::Unchanged {
            return;
        }
        let what = match state {
            IkState::Idle => "takes the standing body",
            IkState::Airborne => "takes the airborne body",
            IkState::Climb => "takes the climbing legs",
            IkState::Walk => "blends onto the walk",
            IkState::Clip | IkState::Unchanged => "stands down",
        };
        report("ik body", None, what, || {
            Some(format!(
                "{} ({})",
                reason.map_or("the estimate is fresh", StandDown::label),
                case.label()
            ))
        });
    }

    fn begin_episode(&mut self, now: Instant) {
        self.in_idle = true;
        self.episode = Episode {
            began: Some(now),
            steps: 0,
            handovers: 0,
            logged_at: Some(now),
        };
        self.status.max_drop_m = 0.0;
    }

    /// Log the standing body's state every [`STANDING_LOG`], for judging the controlled stand
    /// from the session log.
    fn log_standing(&mut self, now: Instant, estimate_weight: f32) {
        if self
            .episode
            .logged_at
            .is_some_and(|at| now.saturating_duration_since(at) < STANDING_LOG)
        {
            return;
        }
        self.episode.logged_at = Some(now);
        let s = &self.status;
        tracing::info!(
            target: "body",
            left = s.feet[0].label(),
            right = s.feet[1].label(),
            planted = s.planted,
            steps = self.episode.steps,
            steps_per_minute = s.steps_per_minute,
            feet_weight = s.weights.feet,
            pelvis_drop_m = s.pelvis_drop_m,
            hips_moved_m = s.hips_moved_m,
            targets = s.targets_queued,
            estimate_weight,
            moving_support = s.on_moving_support,
            pelvis_speed_mps = self.contacts.pelvis_velocity().length(),
            "ik body: standing",
        );
    }

    fn end_episode(&mut self, now: Instant) {
        self.in_idle = false;
        let Some(began) = self.episode.began.take() else {
            return;
        };
        let seconds = now.saturating_duration_since(began).as_secs_f32();
        tracing::info!(
            target: "body",
            seconds,
            steps = self.episode.steps,
            steps_per_minute = if seconds > 0.0 {
                self.episode.steps as f32 * 60.0 / seconds
            } else {
                0.0
            },
            handovers = self.episode.handovers,
            max_pelvis_drop_m = self.status.max_drop_m,
            "ik body: the standing body ends",
        );
    }

    /// Step the feet's contacts in the world, and return each foot's world target.
    fn step_contacts(
        &mut self,
        input: &GateInput<'_>,
        geometry: &Geometry,
        fk: &FkPose,
    ) -> [Option<FootPose>; 2] {
        let to_world = |p: Vec3| input.world.transform_point3(p);
        let body = &geometry.body;
        let pelvis = to_world(body.pelvis);
        let dt = input.dt;
        let hips = body.hips.map(to_world);
        let pelvis_yaw = line_yaw(hips[0], hips[1]).unwrap_or(geometry.world_yaw);
        let animated = [RigBone::LeftFoot, RigBone::RightFoot]
            .into_iter()
            .zip([RigBone::LeftToeBase, RigBone::RightToeBase])
            .map(|(ankle, toe)| {
                let a = to_world(input.animated.position(ankle));
                let t = to_world(input.animated.position(toe));
                FootPose {
                    ankle: a,
                    yaw: yaw_of(t - a).unwrap_or(pelvis_yaw),
                }
            });
        let animated: Vec<FootPose> = animated.collect();
        let frame = ContactFrame {
            pelvis,
            pelvis_yaw,
            neck: to_world(input.model_neck),
            hips,
            leg_m: geometry.rico.leg,
            ankle_height_m: geometry.rico.ankle_height,
            root: input.world.w_axis.truncate(),
            stance: Stance {
                width_m: PROCEDURAL.get().stance.width_m,
                toe_out_deg: PROCEDURAL.get().stance.toe_out_deg,
            },
            animated: [animated[0], animated[1]],
            dt,
        };
        let out = self.contacts.update(
            &frame,
            &mut GameWorld { now: input.now },
            &input.config.contact,
            &RIG.get().core.contact,
        );
        let mut stepped = 0;
        for event in &out.events {
            self.note(event, fk, &mut stepped);
        }
        self.rate.update(dt, stepped);
        self.status.steps = self.contacts.steps();
        self.status.steps_per_minute = self.rate.per_minute();
        if let Some(why) = out.handover {
            self.status.handovers += 1;
            self.episode.handovers += 1;
            self.hold = Some(Hold { why, still_s: 0.0 });
            report("ik body", None, "hands the legs to the walk", || {
                Some(format!(
                    "{}; the pelvis at {:.2} m/s",
                    why.label(),
                    self.contacts.pelvis_velocity().length()
                ))
            });
        }
        self.status.feet = out.feet.map(|f| FootLabel::of(&f));
        self.status.planted = out.planted();
        self.note_carrier();
        let feet = out.feet.map(|f| f.pose());
        if out.handover.is_none() {
            self.last_feet = feet;
        }
        feet
    }

    /// Note the support carrying the feet, and log a change between the world and a moving body.
    fn note_carrier(&mut self) {
        let moving = self
            .contacts
            .carrier()
            .is_some_and(|support| support != SupportId::WORLD);
        self.status.moving_supports = support::tracked();
        if moving != self.status.on_moving_support {
            self.status.on_moving_support = moving;
            if moving {
                self.status.moving_landings += 1;
            }
            report("ik body", None, "the feet's support changes", || {
                Some(if moving { "a moving body" } else { "the world" }.to_owned())
            });
        }
    }

    /// Log a contact event, and count a step in `stepped`.
    fn note(&mut self, event: &ContactEvent, fk: &FkPose, stepped: &mut usize) {
        match *event {
            ContactEvent::Released {
                side,
                reason,
                length_m,
            } => {
                *stepped += 1;
                self.episode.steps += 1;
                self.status.last_step = Some((side, reason));
                report("ik body", None, "a foot steps", || {
                    Some(format!(
                        "{} foot, {}, {length_m:.2} m; estimate weight {:.2}",
                        side.label(),
                        reason.label(),
                        fk.estimate_weight
                    ))
                });
            }
            ContactEvent::Missed { side } => {
                self.status.missed += 1;
                report("ik body", None, "a step's landing has no ground", || {
                    Some(format!("{} foot: unsupported", side.label()))
                });
            }
            ContactEvent::Lost { side } => {
                report("ik body", None, "a foot's support is gone", || {
                    Some(format!("{} foot: unsupported", side.label()))
                });
            }
            ContactEvent::Landed { side } => {
                tracing::debug!(target: "body", foot = side.label(), "ik body: a foot lands");
            }
            ContactEvent::Acquired { side } => {
                tracing::debug!(target: "body", foot = side.label(), "ik body: a foot is planted");
            }
        }
    }
}

/// What the gate passes in.
pub(crate) struct GateInput<'a> {
    /// The clip's pose this tick, with the hips squared.
    pub animated: &'a RigPose,
    pub rig: &'a Rig,
    pub case: Case,
    /// Whether the active state is one of the standing idles.
    pub idle_state: bool,
    pub locomotion: LocomotionStatus,
    /// The player's neck pivot in Rico's model space: where the head bone goes.
    pub model_neck: Vec3,
    /// The character's world transform.
    pub world: Mat4,
    /// The weight the procedural torso is headed for: the seeded pose is written only while the
    /// torso is procedural, so the IK hands back with it.
    pub torso_target: f32,
    pub config: IkBodyConfig,
    pub dt: f32,
    pub now: Instant,
}

/// What [`prepare`] hands back.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Prepared {
    /// The posture to solve the torso on, while the IK has any weight.
    pub pose: Option<RigPose>,
    /// The planted feet's weight against the clip's: the square stance yields by it.
    pub feet_weight: f32,
}

/// The targets [`prepare`] leaves for the effectors, in model space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Targets {
    pub weights: IkWeights,
    /// Each foot's ankle target and heading, while it has one.
    pub feet: [Option<FootPose>; 2],
    /// The knees' hints.
    pub knees: [Vec3; 2],
    /// The airborne legs' ankles, while in the air, and the hanging legs' while climbing.
    pub air_ankles: Option<[Vec3; 2]>,
    /// The braced feet's targets while climbing, with each brace's share against the hanging leg.
    pub braces: [Option<ModelBrace>; 2],
    /// The toe's height over the ankle's ground, and its reach ahead of the ankle, metres.
    pub toe_height_m: f32,
    pub toe_ahead_m: f32,
    /// The ankle's height over the ground, metres.
    pub ankle_height_m: f32,
}

/// Classify the tick, step the contacts, and seed the IK pose into `input.animated`.
pub(crate) fn prepare(input: &GateInput<'_>) -> Prepared {
    let fk = fk::latest();
    let mut state = crate::controllers::body::ik_body::STATE.lock();
    let state = &mut *state;
    state.check_resets(input.now);
    let (target, reason) = classify(input, fk.as_ref(), &mut state.hold);
    state.log_state(target, reason, input.case);
    if reason == Some(StandDown::FirmRagdoll) && input.config.ragdoll_target {
        firm_ragdoll::publish_source(
            TargetBones::of(input.rig),
            fk.as_ref().and_then(|fk| ragdoll_legs(input, fk)),
            input.now,
        );
    }
    let tuning = &IK_BODY.get().gate;
    let weights = state
        .blend
        .step(target.weights(tuning.walk_weight), input.dt, tuning.blend_s);
    state.status.state = target;
    state.status.stand_down = reason;
    state.status.weights = weights;
    state.status.case = input.case;
    state.status.at = Some(input.now);
    let leaving = target != IkState::Idle;
    if leaving && state.in_idle {
        state.end_episode(input.now);
    }
    let Some(fk) = fk.filter(|_| weights.max() > 0.0) else {
        state.climb.stop(input.now);
        state.status.braced = [false; 2];
        state.drop_contacts();
        state.targets = None;
        state.status.planted = 0;
        state.status.feet = [FootLabel::None; 2];
        return Prepared {
            pose: None,
            feet_weight: 0.0,
        };
    };
    let geometry = Geometry::of(input, &fk);
    // The braces last through the blend out of the climb, so the feet fade off the face.
    let climbing = target == IkState::Climb
        || (state.climb.active() && weights.feet > 0.0 && target != IkState::Idle);
    if !climbing {
        state.climb.stop(input.now);
    }
    let mut feet = state.last_feet;
    if target == IkState::Idle {
        if !state.in_idle {
            state.begin_episode(input.now);
        }
        feet = state.step_contacts(input, &geometry, &fk);
    } else if climbing || weights.feet <= 0.0 {
        state.drop_contacts();
        feet = [None; 2];
    }
    let to_model = input.world.inverse();
    let model_feet = feet.map(|foot| {
        foot.map(|pose| FootPose {
            ankle: to_model.transform_point3(pose.ankle),
            yaw: wrap(pose.yaw - geometry.world_yaw),
        })
    });
    let unseeded = seed::seed(input.animated, &geometry.body, &weights, 0.0);
    let drop = seed::pelvis_drop(
        [
            unseeded.position(RigBone::LeftUpLeg),
            unseeded.position(RigBone::RightUpLeg),
        ],
        model_feet.map(|foot| foot.map(|pose| pose.ankle)),
        geometry.rico.leg,
        tuning.max_pelvis_drop_m,
    );
    let pose = seed::seed(input.animated, &geometry.body, &weights, drop);
    state.status.pelvis_drop_m = drop * weights.feet;
    state.status.max_drop_m = state.status.max_drop_m.max(state.status.pelvis_drop_m);
    state.status.source = fk.source;
    state.status.hips_moved_m = pose
        .position(RigBone::Hips)
        .distance(input.animated.position(RigBone::Hips));
    if state.in_idle {
        state.log_standing(input.now, fk.estimate_weight);
    }
    let seeded_hips = [
        pose.position(RigBone::LeftUpLeg),
        pose.position(RigBone::RightUpLeg),
    ];
    let air = (weights.air_legs > 0.0)
        .then(|| seed::ik_frame(&geometry.body))
        .flatten()
        .map(|frame| seed::air_legs(&geometry.body, frame, geometry.rico.leg, tuning.air_relax));
    // The knees hint along the estimate's thighs, or the air pose's in the air, from the seeded
    // hips at Rico's thigh length.
    let thigh_knees = air.map_or(geometry.body.knees, |[_, knees]| knees);
    let knees = [0, 1].map(|i| {
        let thigh = thigh_knees[i] - geometry.body.hips[i];
        seeded_hips[i] + thigh.normalize_or(Vec3::NEG_Y) * geometry.rico.thigh
    });
    // The air pose hangs from the seeded hips, which are the clip's where the IK does not move the
    // pelvis.
    let air_ankles =
        air.map(|[ankles, _]| [0, 1].map(|i| seeded_hips[i] + (ankles[i] - geometry.body.hips[i])));
    let braces = if climbing {
        let legs = ClimbLegs {
            world: &input.world,
            world_yaw: geometry.world_yaw,
            hips: seeded_hips,
            ankles: [
                pose.position(RigBone::LeftFoot),
                pose.position(RigBone::RightFoot),
            ],
            leg_m: geometry.rico.leg,
            thigh_m: geometry.rico.thigh,
            foot_m: geometry.rico.foot,
            ankle_height_m: geometry.rico.ankle_height,
            toe_height_m: geometry.rico.toe_height,
        };
        state
            .climb
            .step(&legs, &IK_BODY.get().brace, input.dt, input.now)
    } else {
        [None; 2]
    };
    state.status.braced = state.climb.braced();
    state.status.brace_moves = state.climb.moves;
    state.targets = Some(Targets {
        weights,
        feet: model_feet,
        knees,
        air_ankles,
        braces,
        toe_height_m: geometry.rico.toe_height,
        toe_ahead_m: geometry.rico.foot,
        ankle_height_m: geometry.rico.ankle_height,
    });
    Prepared {
        pose: Some(pose),
        feet_weight: weights.feet,
    }
}

/// The IK body's legs for the firm ragdoll's muscle target: the air pose the airborne body would
/// give them, in model space, while the estimate is fresh.
fn ragdoll_legs(input: &GateInput<'_>, fk: &FkPose) -> Option<IkLegs> {
    if fk.source != PelvisSource::Estimate {
        return None;
    }
    let geometry = Geometry::of(input, fk);
    let frame = seed::ik_frame(&geometry.body)?;
    let [ankles, knees] = seed::air_legs(
        &geometry.body,
        frame,
        geometry.rico.leg,
        IK_BODY.get().gate.air_relax,
    );
    Some(IkLegs {
        hips: geometry.body.hips,
        knees,
        ankles,
    })
}

/// The state the tick asks for, and why the IK stands down from the standing body if it does.
fn classify(
    input: &GateInput<'_>,
    fk: Option<&FkPose>,
    hold: &mut Option<Hold>,
) -> (IkState, Option<StandDown>) {
    let unchanged = |reason| (IkState::Unchanged, Some(reason));
    if !scheme::is_controllers() {
        return unchanged(StandDown::Gamepad);
    }
    if firm_ragdoll::owns_legs() {
        return unchanged(StandDown::FirmRagdoll);
    }
    // The braces need no estimate: without one the hanging legs hang straight from the fallback.
    if input.case == Case::HandClimb {
        *hold = None;
        return (IkState::Climb, None);
    }
    if !fk.is_some_and(|fk| fk.source == PelvisSource::Estimate) {
        return unchanged(StandDown::NoEstimate);
    }
    match input.case {
        Case::OnFoot => {
            let still = input.locomotion.standing_still();
            if let Some(held) = hold {
                if still && !grounded_stepping() {
                    held.still_s += input.dt;
                } else {
                    held.still_s = 0.0;
                }
                if held.still_s < IK_BODY.get().gate.resume_s {
                    return (IkState::Walk, Some(StandDown::HandedOver(held.why)));
                }
                *hold = None;
            }
            if !input.locomotion.fresh(input.now) {
                (IkState::Clip, Some(StandDown::NoMovementTask))
            } else if !still {
                (IkState::Walk, Some(StandDown::Walking))
            } else if !input.idle_state {
                (IkState::Clip, Some(StandDown::NotIdle))
            } else {
                (IkState::Idle, None)
            }
        }
        Case::Jump | Case::Flight if input.torso_target > 0.0 => {
            *hold = None;
            (IkState::Airborne, None)
        }
        Case::Jump | Case::Flight => (IkState::Clip, Some(StandDown::Case(input.case))),
        Case::AuthoredMotion | Case::HitReaction => {
            (IkState::Clip, Some(StandDown::Case(input.case)))
        }
        case => {
            *hold = None;
            unchanged(StandDown::Case(case))
        }
    }
}

/// Whether the grounded rig's step drive walks Rico after the neck: a physical walk.
fn grounded_stepping() -> bool {
    grounded::status()
        .latest
        .is_some_and(|(frame, _)| frame.output.phase.steps())
}

/// The tick's geometry: the FK body in model space and the measurements of Rico's skeleton.
struct Geometry {
    body: ModelBody,
    rico: RicoMeasures,
    /// The character's heading in the world.
    world_yaw: f32,
}

impl Geometry {
    fn of(input: &GateInput<'_>, fk: &FkPose) -> Self {
        let (_, rotation, _) = input.world.to_scale_rotation_translation();
        let model_from_room = (rotation.inverse() * xr::body_rotation()).normalize();
        let rico = RicoMeasures::of(input.rig);
        let ratio = |rico: f32, player: f32| {
            let r = rico / player;
            if r.is_finite() && r > 0.0 { r } else { 1.0 }
        };
        let proportions = Proportions {
            spine: ratio(rico.spine, fk.skeleton.spine_length()),
            legs: ratio(rico.leg, fk.skeleton.leg_length()),
        };
        Self {
            body: seed::to_model(&fk.body, input.model_neck, model_from_room, proportions),
            rico,
            world_yaw: yaw_of(rotation * Vec3::NEG_Z).unwrap_or(0.0),
        }
    }
}

/// Rico's skeleton's measurements, from its reference pose.
#[derive(Debug, Copy, Clone, PartialEq)]
struct RicoMeasures {
    /// From the hips to the head bone, metres.
    spine: f32,
    /// From the hip joint to the ankle with the knee straight, metres.
    leg: f32,
    /// From the hip joint to the knee, metres.
    thigh: f32,
    /// From the ankle to the toe's joint across the ground, metres.
    foot: f32,
    /// The ankle's height over the ground, metres.
    ankle_height: f32,
    /// The toe joint's height over the ground, metres.
    toe_height: f32,
}

impl RicoMeasures {
    fn of(rig: &Rig) -> Self {
        let at = |bone: RigBone| rig.reference[bone.index()].translation;
        let thigh = at(RigBone::LeftUpLeg).distance(at(RigBone::LeftLeg));
        let shin = at(RigBone::LeftLeg).distance(at(RigBone::LeftFoot));
        let ankle = at(RigBone::LeftFoot);
        let toe = at(RigBone::LeftToeBase);
        let foot = Vec2::new(toe.x - ankle.x, toe.z - ankle.z).length();
        let toe_height = IK_BODY.get().gate.toe_height_m;
        let valid = |v: f32, fallback: f32, min: f32, max: f32| {
            if v.is_finite() && (min..=max).contains(&v) {
                v
            } else {
                fallback
            }
        };
        Self {
            spine: valid(
                at(RigBone::Head).distance(at(RigBone::Hips)),
                0.62,
                0.3,
                1.2,
            ),
            leg: valid(thigh + shin, 0.85, 0.4, 1.4),
            thigh: valid(thigh, 0.42, 0.2, 0.7),
            foot: valid(foot, 0.14, 0.05, 0.3),
            ankle_height: valid(ankle.y - toe.y + toe_height, 0.09, 0.04, 0.16),
            toe_height,
        }
    }
}

/// How often the standing body's state is logged while it lasts.
const STANDING_LOG: Duration = Duration::from_secs(10);

/// The world as the contact model asks of it at `now`: each support's frame from its last snapshot
/// ([`support`]), and the ground probed through the mailbox ([`probe`]).
struct GameWorld {
    now: Instant,
}

impl Surroundings for GameWorld {
    fn frame(&self, support: SupportId) -> Option<SupportFrame> {
        support::frame(support, self.now)
    }

    fn probe(&mut self, side: Side, at: Vec2, near: f32) -> Probe {
        probe::ask(side, at, near)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stand_down_has_a_label() {
        for reason in [
            StandDown::Gamepad,
            StandDown::FirmRagdoll,
            StandDown::NoEstimate,
            StandDown::Walking,
            StandDown::HandedOver(Handover::BothFeet),
            StandDown::NotIdle,
            StandDown::NoMovementTask,
            StandDown::Case(Case::Swimming),
            StandDown::NotWritten,
        ] {
            assert!(!reason.label().is_empty());
        }
    }
}
