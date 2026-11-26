//! Bilateral wingsuit deployment, braking, and closure from tracked cockpit poses.
//!
//! The binding owner supplies input availability, transition cancellation, and transient hand
//! claims. This module returns commands without writing game actions or claiming weapon fire.
//!
//! The recognizer runs while falling, under a canopy, in the suit, and on a grapple line that can
//! yield to deployment. A fall's start is a fresh start; a fall out of the suit needs the neutral
//! first, and a canopy the player opened needs another pose before the spread, so the arms that
//! opened it do not reopen the suit.
//!
//! The arms are measured against the way the player's body faces, which a player in flight shows
//! with the line between their hands rather than with their head: a flier looks around, down, and
//! along the turn while the arms stay spread across the chest. The palms are measured as each
//! wrist's roll about the fingers, the measure the arm steering's wrist tilt reads, so turning the
//! hands to face the wind reads the same whether the arms are level, raised, or swept back.

mod commands;

use std::{fmt, time::Instant};

use body_rig::{angle::yaw_of, rotation::level};
pub(crate) use commands::desired_for;
use glam::{Quat, Vec3};
use jc3vr_input::frame::{ControllerFrame, Hand, Pose};
pub(crate) use jc3vr_render::vr::input::flight_pose::{FlightPacket, GestureHead, GestureSample};

use crate::controllers::{
    body::{self, BodyDimensions},
    config::GripOffset,
    hands::pose::cockpit_hand,
    mode::{AirborneVariant, Mode, OnFootVariant},
    vehicles::{arm_steering::wrist_tilt, config::WingsuitGesturesConfig},
};

/// A single transition request. The binding owner emits it once, never as a held toggle.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum SuitTransition {
    Deploy,
    Close,
}

/// Commands produced from one observation. The canopy request always cancels these commands.
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub(crate) struct GestureCommands {
    pub transition: Option<SuitTransition>,
    pub brake: bool,
}

/// Current availability of the physical gesture sequence, independently of command pulses.
/// This describes the mod recognizer, not game-side permission to change the suit state.
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub(crate) enum GestureAvailability {
    #[default]
    Unavailable,
    Deploy,
    CloseAndBrake,
}

/// What the recognizer waits for, or holds, as of its last evaluated sample.
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub(crate) enum GesturePhase {
    #[default]
    Unavailable,
    /// Falling out of the suit, waiting for both arms lowered or drawn in.
    AwaitingNeutral,
    /// Falling, or in the suit, waiting for a stable flight pose.
    AwaitingFlight,
    /// In the suit, with closing armed.
    Armed,
    /// In the suit, holding the air brake.
    Braking,
}

impl fmt::Display for GesturePhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "unavailable",
            Self::AwaitingNeutral => "waiting for both arms lowered or drawn in",
            Self::AwaitingFlight => "waiting for a stable flight pose",
            Self::Armed => "armed",
            Self::Braking => "braking",
        })
    }
}

/// The first gate that keeps the gestures unavailable for an observation.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Blocked {
    /// The binding owner suspended gameplay input.
    Gameplay,
    CanopyRequested,
    Disabled,
    InvalidTuning,
    Claimed(Hand, Claim),
    /// The observation carried no valid pose.
    NoPose,
    /// The player is not falling, under a canopy, or in the suit.
    Mode,
}

/// Where the player is, as far as the gestures are concerned.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Stage {
    /// Falling, or off the ground on foot while the game still plays its jump: the game's jump
    /// states take the suit's opening as its falls do.
    Fall,
    Suit,
    /// Under a canopy. The parachute's states take the suit's opening act directly, so a flight
    /// pose deploys the suit from the canopy as it does from a fall.
    Parachute,
    /// Anywhere the gestures do not run.
    Other,
}

impl Stage {
    /// The stage of `mode`, with `airborne` telling a jump from standing on the ground.
    pub(crate) fn of(mode: Mode, airborne: bool) -> Self {
        match mode {
            Mode::Airborne(AirborneVariant::Falling) => Self::Fall,
            Mode::OnFoot(OnFootVariant::Ground) if airborne => Self::Fall,
            Mode::Airborne(AirborneVariant::Wingsuit) => Self::Suit,
            Mode::Airborne(AirborneVariant::Parachute) => Self::Parachute,
            _ => Self::Other,
        }
    }
}

impl fmt::Display for Blocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gameplay => f.write_str("gameplay input is suspended"),
            Self::CanopyRequested => f.write_str("a canopy is requested"),
            Self::Disabled => f.write_str("the gestures are off"),
            Self::InvalidTuning => f.write_str("the tuning is invalid"),
            Self::Claimed(hand, claim) => write!(f, "the {} hand {claim}", hand.label()),
            Self::NoPose => f.write_str("there is no valid arm pose"),
            Self::Mode => {
                f.write_str("the player is neither falling, under a canopy, nor in the suit")
            }
        }
    }
}

/// An interaction that claims a hand away from the gestures, as [`HandClaims`] reports it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Claim {
    Grip,
    Slot,
    Mechanical,
    Grapple,
}

impl fmt::Display for Claim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Grip => "grips something",
            Self::Slot => "squeezes its grip at a slot on the body",
            Self::Mechanical => "works a mechanism",
            Self::Grapple => "has the grapple",
        })
    }
}

/// Current physical input claims, indexed by `Hand::index`.
///
/// These describe actual interactions, not what a hand holds: a held gun, grenade, charge, or prop
/// rides along in the hand while the arm pose drives the suit. A support or reload hand reports
/// only the inputs its interaction owns. Grip release remains the owner's concern.
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub(crate) struct HandClaims {
    /// The grip works something other than a held item: a grab this tick, a support hold on the
    /// other hand's gun, or a hold on the world or a vehicle.
    pub grip: bool,
    /// The hand squeezes its grip within reach of a slot on the body (a socket or pouch): a draw
    /// is starting, so the arm drawn in is not a gesture. A hand that only rests within reach,
    /// with its grip open, claims nothing, since the lowered arms of the gestures' own neutral and
    /// closure put the hands at the hip sockets.
    pub slot: bool,
    pub mechanical: bool,
    pub grapple: bool,
}

/// Explicit pose inputs. The producer supplies a common epoch for one cockpit baseline.
///
/// Controller frames do not carry a head pose or baseline epoch. The integration must publish the
/// head and controller sample together; global freshness checks alone cannot establish coherence.
pub(crate) struct GestureObservation<'a> {
    pub sample: GestureSample,
    pub head: GestureHead,
    pub controllers: &'a ControllerFrame,
    pub grip_offset: &'a GripOffset,
    pub dimensions: &'a BodyDimensions,
    /// A torso estimate in the same cockpit baseline, or no estimate to use the supplied head yaw.
    pub torso: Option<Quat>,
    pub now: Instant,
}

/// Physical player dimensions and the configured controller grip-to-wrist transform.
/// `None` torso in the observation uses the packet's own head yaw rather than a global estimate.
pub(crate) struct FlightCalibration<'a> {
    pub grip_offset: &'a GripOffset,
    pub dimensions: &'a BodyDimensions,
}

/// Continuous pose measures in the calibrated shoulder frame, normalized by the arm span.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct FlightPose {
    /// Outward distance from each shoulder, positive on that hand's own side.
    pub lateral: [f32; 2],
    /// Distance from each shoulder to the wrist.
    pub reach: [f32; 2],
    /// Each wrist's height relative to its shoulder.
    pub height: [f32; 2],
    /// Each arm's angle away from its outward horizontal direction, degrees.
    pub angle_deg: [f32; 2],
    /// Each wrist's roll about the fingers' axis from palm-flat, degrees in `-180..=180`, positive
    /// as the palm turns to face forward (the leading edge up) and negative as it turns back.
    pub palm_roll_deg: [f32; 2],
}

impl FlightPose {
    /// Derive palms and shoulder-relative distances from `packet`, not a later snapshot.
    pub(crate) fn from_packet(
        packet: &FlightPacket,
        calibration: &FlightCalibration<'_>,
        now: Instant,
    ) -> Option<Self> {
        let head = packet.head();
        Self::from_tracking(&GestureObservation {
            sample: head.sample,
            head,
            controllers: packet.controllers(),
            grip_offset: calibration.grip_offset,
            dimensions: calibration.dimensions,
            torso: None,
            now,
        })
    }

    /// Measure raw wrists against calibrated shoulders from explicitly matched head/hand samples.
    /// The configured grip-to-wrist offset supplies the wrist frame; the aim pose is not a palm
    /// basis. Without a torso estimate, the body faces along [`flight_heading`].
    pub(crate) fn from_tracking(observation: &GestureObservation<'_>) -> Option<Self> {
        let GestureObservation {
            sample,
            head,
            controllers,
            grip_offset,
            dimensions,
            torso,
            now,
        } = observation;
        if *sample != head.sample
            || sample.sequence != controllers.sequence
            || sample.time_ns != controllers.display_time_ns
            || !controllers.focused
            || head.published_at != controllers.published_at
            || *now < head.published_at
            || !controllers.is_fresh_at(*now)
            || !head.pose.is_tracked()
            || !head.pose.pose.position.is_finite()
            || !valid_rotation(head.pose.pose.orientation)
            || dimensions.validate().is_err()
            || torso.is_some_and(|rotation| !valid_rotation(rotation))
        {
            return None;
        }
        let wrists = Hand::BOTH.map(|hand| {
            let tracked = controllers.hand(hand);
            if !tracked.active || !tracked.grip.is_tracked() {
                return None;
            }
            Some(cockpit_hand(hand, tracked.grip.pose, tracked.aim.pose, grip_offset, None).wrist)
        });
        let wrists = [wrists[0]?, wrists[1]?];
        let heading = torso.unwrap_or_else(|| flight_heading(wrists, head.pose.pose));
        let shoulders = body::shoulder_positions(head.pose.pose.position, heading, dimensions);
        Self::measure(wrists, shoulders, heading, dimensions.arm_span)
    }

    pub(crate) fn measure(
        wrists: [Pose; 2],
        shoulders: [Vec3; 2],
        heading: Quat,
        span: f32,
    ) -> Option<Self> {
        if !span.is_finite() || span < 0.5 || !valid_rotation(heading) {
            return None;
        }
        let right = heading * Vec3::X;
        let mut pose = Self {
            lateral: [0.0; 2],
            reach: [0.0; 2],
            height: [0.0; 2],
            angle_deg: [0.0; 2],
            palm_roll_deg: [0.0; 2],
        };
        for hand in Hand::BOTH {
            let i = hand.index();
            if !wrists[i].position.is_finite()
                || !shoulders[i].is_finite()
                || !valid_rotation(wrists[i].orientation)
            {
                return None;
            }
            let delta = wrists[i].position - shoulders[i];
            let direction = delta.try_normalize()?;
            let outward = if hand == Hand::Left { -right } else { right };
            pose.lateral[i] = delta.dot(outward) / span;
            pose.reach[i] = delta.length() / span;
            pose.height[i] = delta.y / span;
            pose.angle_deg[i] = direction.dot(outward).clamp(-1.0, 1.0).acos().to_degrees();
            let lead = if hand == Hand::Left { 1.0 } else { -1.0 };
            pose.palm_roll_deg[i] = -wrist_tilt(wrists[i].orientation, lead).to_degrees();
        }
        pose.valid().then_some(pose)
    }

    fn valid(self) -> bool {
        self.lateral
            .into_iter()
            .chain(self.reach)
            .chain(self.height)
            .chain(self.angle_deg)
            .chain(self.palm_roll_deg)
            .all(f32::is_finite)
            && self.reach.into_iter().all(|v| (0.0..=0.8).contains(&v))
            && self
                .angle_deg
                .into_iter()
                .all(|v| (0.0..=180.0).contains(&v))
            && self
                .palm_roll_deg
                .into_iter()
                .all(|v| (-180.001..=180.001).contains(&v))
    }

    /// Whether the pose is the flight pose: both arms spread and both palms flat.
    fn flight(self, config: &WingsuitGesturesConfig) -> bool {
        self.spread(config) && self.flat_palms(config.flight_roll_deg)
    }

    /// Whether both arms are spread out to the sides and near level, however the palms are turned.
    fn spread(self, config: &WingsuitGesturesConfig) -> bool {
        self.lateral
            .into_iter()
            .all(|v| v >= config.flight_lateral_span)
            && self
                .angle_deg
                .into_iter()
                .all(|v| v <= config.flight_angle_deg)
    }

    /// Whether the pose deploys the suit from `stage`. A fall takes the spread arms alone: a player
    /// spreading their arms to fly turns the palms any way, often forward into the wind, and nothing
    /// else in a fall reads the palms. A canopy takes the flight pose, since the canopy's own pose
    /// (the arms out with the hands turned up on the toggles) is a spread with rolled palms.
    fn deploys_from(self, stage: Stage, config: &WingsuitGesturesConfig) -> bool {
        match stage {
            Stage::Fall => self.spread(config),
            Stage::Parachute => self.flight(config),
            Stage::Suit | Stage::Other => false,
        }
    }

    /// Whether the pose enters the air brake: both arms spread wide and both palms turned to face
    /// the wind, within the brake's roll band. The band stops short of palm-up, where the roll
    /// wraps, so a palm turned past it does not enter the brake from the far side.
    fn enters_brake(self, config: &WingsuitGesturesConfig) -> bool {
        self.lateral
            .into_iter()
            .all(|v| v >= config.flight_lateral_span)
            && self
                .palm_roll_deg
                .into_iter()
                .all(|roll| roll >= config.brake_roll_deg && roll <= config.brake_roll_max_deg)
    }

    /// Whether the pose releases a held air brake: either arm drawn in past the spread's
    /// hysteresis, or both palms back within the flight pose's roll of flat. A held brake ignores
    /// the band's upper end, so a flare that turns the palms on toward palm-up, and through the
    /// roll's wrap, keeps braking.
    fn releases_brake(self, config: &WingsuitGesturesConfig) -> bool {
        self.lateral
            .into_iter()
            .any(|v| v < config.flight_lateral_span - config.pose_hysteresis_span)
            || self.flat_palms(config.flight_roll_deg)
    }

    /// Whether both palms are within `limit_deg` of flat, turned either way.
    fn flat_palms(self, limit_deg: f32) -> bool {
        self.palm_roll_deg
            .into_iter()
            .all(|roll| roll.abs() <= limit_deg)
    }

    fn withdrawn(self, config: &WingsuitGesturesConfig) -> bool {
        (0..2).all(|i| {
            self.lateral[i] >= -config.withdraw_lateral_span
                && self.lateral[i] <= config.withdraw_lateral_span
                && (self.height[i] <= -config.withdraw_lowered_span
                    || self.reach[i] <= config.withdraw_reach_span)
        })
    }
}

/// Pure observation. `gameplay` excludes menus, focus loss, action-set waits, and recovery masks.
#[derive(Debug, Copy, Clone)]
pub(crate) struct GestureInput {
    pub sample: GestureSample,
    pub mode: Mode,
    /// Rico is off the ground on foot, from the jump's grace on: a jump, or a step off a ledge,
    /// before the game reports a fall.
    pub airborne: bool,
    /// An active line can yield to deployment, including a reel-in holding the ground mode.
    /// The binding owner excludes hangs and seats; other hand claims still take precedence.
    pub deploy_grapple: bool,
    pub gameplay: bool,
    /// A contextual canopy request, including a held request, cancels pending suit gestures.
    pub canopy_requested: bool,
    pub pose: Option<FlightPose>,
    pub claims: [HandClaims; 2],
}

/// Stateful recognizer owned by the binding tick, independently of physical vehicles.
#[derive(Debug, Default)]
pub(crate) struct WingsuitGestures {
    last: Option<(GestureSample, Stage)>,
    current: GestureAvailability,
    config: Option<WingsuitGesturesConfig>,
    /// The stage last observed outside a menu, which tells a fall's start from its middle.
    stage: Option<Stage>,
    /// The flight pose may deploy the suit in this fall or canopy: from the fall's start, unless
    /// the fall came out of the suit or the canopy was opened by the player, where another pose
    /// sets it.
    ready: bool,
    /// Whether this active grapple has already supplied a fresh deployment intent.
    grapple_intent: bool,
    /// A canopy request seen before the canopy stage was observed. The canopy's entry consumes it
    /// so the arms that opened the canopy do not reopen the suit before they leave the flight
    /// pose.
    canopy_requested: bool,
    /// The sample time before which a deploy request may not follow the last one in this fall.
    retry_at_ns: Option<i64>,
    deploy_since_ns: Option<i64>,
    armed: bool,
    brake: bool,
    /// The brake may engage in this suit: a pose that releases it has been seen since the suit's
    /// entry. A fall's spread deploys with the palms turned any way, so without it, palms already
    /// rolled to the wind would brake the suit as it opens, and the brake's close act could fold it
    /// back into a fall.
    brake_ready: bool,
    neutral: Dwell,
    flight: Dwell,
    close: Dwell,
    brake_entry: Dwell,
}

impl WingsuitGestures {
    /// Cancel gestures without lowering the sample watermark. Only a newer epoch restarts time.
    pub(crate) fn reset(&mut self) {
        self.current = GestureAvailability::Unavailable;
        self.grapple_intent = false;
        self.deploy_since_ns = None;
        self.retry_at_ns = None;
        self.clear_pose();
    }

    pub(crate) fn tracking_lost(&mut self) {
        self.current = GestureAvailability::Unavailable;
        self.clear_pose();
    }

    pub(crate) fn current(&self) -> GestureAvailability {
        self.current
    }

    /// Remember a canopy request seen while the flight packet was missing, so the canopy's entry
    /// still starts unarmed.
    pub(crate) fn note_canopy_request(&mut self) {
        self.canopy_requested = true;
    }

    /// What the recognizer waits for, or holds, as of its last evaluated sample.
    pub(crate) fn phase(&self) -> GesturePhase {
        match self.current {
            GestureAvailability::Unavailable => GesturePhase::Unavailable,
            GestureAvailability::Deploy if self.ready => GesturePhase::AwaitingFlight,
            GestureAvailability::Deploy => GesturePhase::AwaitingNeutral,
            GestureAvailability::CloseAndBrake if self.brake => GesturePhase::Braking,
            GestureAvailability::CloseAndBrake if self.armed => GesturePhase::Armed,
            GestureAvailability::CloseAndBrake => GesturePhase::AwaitingFlight,
        }
    }

    /// Recheck the evaluated sample without advancing dwell or emitting another transition.
    pub(crate) fn revalidate(
        &mut self,
        input: GestureInput,
        config: &WingsuitGesturesConfig,
    ) -> GestureAvailability {
        if input.canopy_requested {
            self.note_canopy_request();
        }
        if self.last == Some((input.sample, input.stage()))
            && self.config == Some(*config)
            && !input.pose.is_some_and(FlightPose::valid)
            && matches!(input.stage(), Stage::Fall | Stage::Parachute)
            && input.blocked(config) == Some(Blocked::NoPose)
        {
            self.tracking_lost();
            return self.current;
        }
        if self.last != Some((input.sample, input.stage()))
            || self.config != Some(*config)
            || input.availability(config) != self.current
        {
            self.reset();
        }
        self.current
    }

    pub(crate) fn step(
        &mut self,
        input: GestureInput,
        config: &WingsuitGesturesConfig,
    ) -> GestureCommands {
        let previous = self.last;
        let repeated = if let Some((last, _)) = previous {
            if input.sample.epoch < last.epoch
                || (input.sample.epoch == last.epoch
                    && input.sample != last
                    && (input.sample.sequence <= last.sequence
                        || input.sample.time_ns <= last.time_ns))
            {
                self.reset();
                return GestureCommands::default();
            }
            input.sample == last
        } else {
            false
        };
        let stage = input.stage();
        if input.canopy_requested {
            self.note_canopy_request();
        }
        self.enter(stage, input.mode);
        let new_epoch = previous.is_none_or(|(last, _)| last.epoch != input.sample.epoch);
        if new_epoch {
            self.retry_at_ns = None;
        }
        let changed = new_epoch || previous.is_some_and(|(_, last)| last != stage);
        if changed || self.config != Some(*config) {
            self.deploy_since_ns = None;
            self.retry_at_ns = None;
            self.clear_pose();
        }
        self.config = Some(*config);
        if !repeated {
            self.last = Some((input.sample, stage));
        }
        self.current = input.availability(config);
        let line_released = self.grapple_intent && !input.deploy_grapple;
        if !input.deploy_grapple {
            self.grapple_intent = false;
        }
        if line_released {
            // The line's release starts a fall afresh: the suit request follows the release
            // without the hang's retry pause or intent window.
            self.deploy_since_ns = None;
            self.retry_at_ns = None;
        }
        if self
            .deploy_since_ns
            .is_some_and(|since| elapsed_s(since, input.sample.time_ns) >= DEPLOY_INTENT_S)
        {
            self.deploy_since_ns = None;
            self.retry_at_ns = None;
            self.ready = false;
            self.clear_pose();
        }
        if self.current == GestureAvailability::Unavailable {
            if input.blocked(config) != Some(Blocked::NoPose)
                || !matches!(stage, Stage::Fall | Stage::Parachute)
            {
                self.deploy_since_ns = None;
                self.retry_at_ns = None;
            }
            self.clear_pose();
            return GestureCommands::default();
        }
        if repeated {
            return GestureCommands {
                brake: self.brake,
                transition: None,
            };
        }
        if input.deploy_grapple && !self.grapple_intent && stage == Stage::Fall {
            // Grappling is a new traversal intent, not a continuation of the suit's closure.
            self.ready = true;
        }
        self.grapple_intent = input.deploy_grapple;
        let gap_s = previous
            .filter(|(last, _)| last.epoch == input.sample.epoch)
            .map_or(0.0, |(last, _)| {
                elapsed_s(last.time_ns, input.sample.time_ns)
            });
        if gap_s > f64::from(config.max_sample_gap_s) {
            self.reset();
            return GestureCommands::default();
        }
        if input
            .pose
            .is_some_and(|pose| pose.deploys_from(stage, config))
            && self.deploy_since_ns.is_some()
            && self
                .retry_at_ns
                .is_some_and(|at| input.sample.time_ns >= at)
        {
            self.retry_at_ns = Some(
                input
                    .sample
                    .time_ns
                    .saturating_add((DEPLOY_RETRY_S * 1e9) as i64),
            );
            self.clear_pose();
            return GestureCommands {
                transition: Some(SuitTransition::Deploy),
                ..Default::default()
            };
        }
        let pose = input.pose.unwrap();
        let in_suit = stage == Stage::Suit;
        if in_suit && self.update_brake(pose, input.sample.time_ns, config) {
            self.close = Dwell::default();
            self.flight = Dwell::default();
            return GestureCommands {
                transition: None,
                brake: true,
            };
        }
        let withdrawn = pose.withdrawn(config);
        let flight = pose.flight(config);
        // In the suit the flight pose arms it; out of it, the stage's deploying pose deploys it.
        let flying = if in_suit {
            flight
        } else {
            pose.deploys_from(stage, config)
        };
        // A canopy's own pose (anything but the flight pose) arms it: the player who opened the
        // canopy with the arms spread must move them before the spread opens the suit, while a
        // player already holding the canopy's toggles only has to spread them.
        let neutral_pose = if stage == Stage::Parachute {
            !flight
        } else {
            withdrawn
        };
        if !self.ready
            && self
                .neutral
                .update(neutral_pose, input.sample.time_ns, config.neutral_dwell_s)
        {
            self.ready = true;
        }
        // A fall's start is a fresh start: arms spread as the player steps off, or still spread
        // under a canopy they cut, deploy the suit. Only a fall out of the suit needs the neutral
        // first, so arms spread as the suit closes do not open it again. In the suit, the flight
        // pose alone arms: a player who opened the suit with the button often has their arms
        // spread already, and the suit's own entry is the fresh start.
        let retry = self.retry_at_ns.is_none_or(|at| input.sample.time_ns >= at);
        if self.flight.update(
            (self.ready || in_suit) && flying,
            input.sample.time_ns,
            config.flight_dwell_s,
        ) {
            if !in_suit && retry {
                self.clear_pose();
                self.deploy_since_ns.get_or_insert(input.sample.time_ns);
                self.retry_at_ns = Some(
                    input
                        .sample
                        .time_ns
                        .saturating_add((DEPLOY_RETRY_S * 1e9) as i64),
                );
                return GestureCommands {
                    transition: Some(SuitTransition::Deploy),
                    ..Default::default()
                };
            }
            if in_suit {
                self.armed = true;
            }
        }
        if self.close.update(
            in_suit && self.armed && withdrawn,
            input.sample.time_ns,
            config.close_dwell_s,
        ) {
            self.clear_pose();
            return GestureCommands {
                transition: Some(SuitTransition::Close),
                ..Default::default()
            };
        }
        GestureCommands::default()
    }

    /// Latch or release the air brake in the suit from `pose`, returning whether it is held.
    ///
    /// The brake latches once a brake pose is held for the brake's dwell, with no flight pose
    /// needed first, and holds until the pose releases it ([`FlightPose::releases_brake`]).
    fn update_brake(
        &mut self,
        pose: FlightPose,
        now_ns: i64,
        config: &WingsuitGesturesConfig,
    ) -> bool {
        let releases = pose.releases_brake(config);
        if self.brake {
            self.brake = !releases;
            return self.brake;
        }
        self.brake_ready |= releases;
        if self.brake_entry.update(
            self.brake_ready && pose.enters_brake(config),
            now_ns,
            config.brake_dwell_s,
        ) {
            self.brake = true;
            self.brake_entry = Dwell::default();
        }
        self.brake
    }

    /// Track the stage outside menus, and start each fall or canopy afresh. A menu over a fall
    /// leaves the fall's start where it was.
    ///
    /// A canopy the player requested starts unarmed, so opening it with the arms spread does not
    /// immediately reopen the suit; any pose but the flight pose arms it. A canopy the game
    /// entered on its own is a fresh start, as a fall is. The request is remembered until the
    /// canopy stage is entered, since the mode detector reports the canopy a few ticks after the
    /// request.
    fn enter(&mut self, stage: Stage, mode: Mode) {
        if mode == Mode::Ui || self.stage == Some(stage) {
            return;
        }
        self.ready = match stage {
            Stage::Fall => {
                // A request from the canopy or the suit cannot belong to this fall; one from a
                // vehicle can, since a bail can pass through a fall before its canopy shows.
                if matches!(self.stage, Some(Stage::Parachute | Stage::Suit)) {
                    self.canopy_requested = false;
                }
                self.stage != Some(Stage::Suit)
            }
            Stage::Parachute => {
                let requested = std::mem::take(&mut self.canopy_requested);
                self.stage != Some(Stage::Suit) && !requested
            }
            Stage::Suit | Stage::Other => {
                self.canopy_requested = false;
                false
            }
        };
        self.brake_ready = false;
        self.retry_at_ns = None;
        self.deploy_since_ns = None;
        self.stage = Some(stage);
    }

    fn clear_pose(&mut self) {
        self.armed = false;
        self.brake = false;
        self.neutral = Dwell::default();
        self.flight = Dwell::default();
        self.close = Dwell::default();
        self.brake_entry = Dwell::default();
    }
}

impl GestureInput {
    /// The first gate that keeps the gestures unavailable, or `None` while they are available.
    pub(crate) fn blocked(self, config: &WingsuitGesturesConfig) -> Option<Blocked> {
        if !self.gameplay {
            return Some(Blocked::Gameplay);
        }
        if self.canopy_requested {
            return Some(Blocked::CanopyRequested);
        }
        if !config.enabled {
            return Some(Blocked::Disabled);
        }
        if !config.valid() {
            return Some(Blocked::InvalidTuning);
        }
        if let Some((hand, claim)) = Hand::BOTH
            .into_iter()
            .find_map(|hand| Some((hand, self.claims[hand.index()].claim()?)))
        {
            return Some(Blocked::Claimed(hand, claim));
        }
        if !self.pose.is_some_and(FlightPose::valid) {
            return Some(Blocked::NoPose);
        }
        match self.stage() {
            Stage::Fall | Stage::Suit | Stage::Parachute => None,
            Stage::Other => Some(Blocked::Mode),
        }
    }

    /// Where the player is, as far as the gestures are concerned. An eligible line is a fall's
    /// fresh start: its release precedes the suit request, and the pose is measured as in a fall.
    pub(crate) fn stage(self) -> Stage {
        if self.deploy_grapple {
            Stage::Fall
        } else {
            Stage::of(self.mode, self.airborne)
        }
    }

    fn availability(self, config: &WingsuitGesturesConfig) -> GestureAvailability {
        if self.blocked(config).is_some() {
            return GestureAvailability::Unavailable;
        }
        match self.stage() {
            Stage::Fall | Stage::Parachute => GestureAvailability::Deploy,
            Stage::Suit => GestureAvailability::CloseAndBrake,
            Stage::Other => GestureAvailability::Unavailable,
        }
    }
}

impl HandClaims {
    /// The first interaction claiming the hand, if any.
    fn claim(self) -> Option<Claim> {
        if self.grip {
            Some(Claim::Grip)
        } else if self.slot {
            Some(Claim::Slot)
        } else if self.mechanical {
            Some(Claim::Mechanical)
        } else if self.grapple {
            Some(Claim::Grapple)
        } else {
            None
        }
    }
}

/// The way the player's body faces for the arm measures: the horizontal normal to the line from
/// the left wrist to the right while the wrists are apart and that normal is within
/// [`MAX_ARMS_FROM_HEAD_DEG`] of the head's heading, and the head's heading otherwise (wrists
/// together, crossed, or behind the player).
fn flight_heading(wrists: [Pose; 2], head: Pose) -> Quat {
    let head_heading = level(head.orientation);
    let across = (wrists[1].position - wrists[0].position).with_y(0.0);
    if across.length() < MIN_ARMS_SEPARATION_M {
        return head_heading;
    }
    let Some(forward) = Vec3::Y.cross(across).try_normalize() else {
        return head_heading;
    };
    if forward.dot(head_heading * Vec3::NEG_Z) < MAX_ARMS_FROM_HEAD_DEG.to_radians().cos() {
        return head_heading;
    }
    // About the vertical alone, so a player turned to face the cockpit's back still stands upright.
    yaw_of(forward).map_or(head_heading, Quat::from_rotation_y)
}

/// How long after a deploy request another may follow in the same fall, seconds. The suit's entry
/// reaches the mode detection within a few ticks, so a fall still running this long after a request
/// is one the game refused it in (still rising from a jump, or too near the ground), and a repeat
/// never lands in the suit, where the same action closes it.
const DEPLOY_RETRY_S: f64 = 0.5;
const DEPLOY_INTENT_S: f64 = 1.5;

/// The wrists' smallest horizontal separation that gives the body's facing, metres. Closer, the
/// line between them turns with any small movement of either hand.
const MIN_ARMS_SEPARATION_M: f32 = 0.15;

/// The largest angle between the arms' facing and the head's heading that is still read as the
/// body's facing, degrees. A head turns about as far on the shoulders; past it the arms are
/// crossed or reaching behind the player, and their line says nothing about the body.
const MAX_ARMS_FROM_HEAD_DEG: f32 = 75.0;

#[derive(Debug, Default)]
struct Dwell {
    since_ns: Option<i64>,
}

impl Dwell {
    fn update(&mut self, qualifies: bool, now_ns: i64, threshold_s: f32) -> bool {
        if !qualifies {
            self.since_ns = None;
            return false;
        }
        let since = *self.since_ns.get_or_insert(now_ns);
        elapsed_s(since, now_ns) >= f64::from(threshold_s)
    }
}

fn elapsed_s(since_ns: i64, now_ns: i64) -> f64 {
    (i128::from(now_ns) - i128::from(since_ns)) as f64 * 1e-9
}

fn valid_rotation(rotation: Quat) -> bool {
    rotation.is_finite() && (rotation.length_squared() - 1.0).abs() < 0.02
}

#[cfg(test)]
mod tests;
