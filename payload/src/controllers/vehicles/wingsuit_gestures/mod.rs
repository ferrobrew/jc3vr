//! Bilateral wingsuit deployment, braking, and closure from tracked cockpit poses.
//!
//! The binding owner supplies input availability, transition cancellation, and transient hand
//! claims. This module returns commands without writing game actions or claiming weapon fire.
//!
//! The arms are measured against the way the player's body faces, which a player in flight shows
//! with the line between their hands rather than with their head: a flier looks around, down, and
//! along the turn while the arms stay spread across the chest. The palms are measured as each
//! wrist's roll about the fingers, the measure the arm steering's wrist tilt reads, so turning the
//! hands to face the wind reads the same whether the arms are level, raised, or swept back.

use std::{fmt, time::Instant};

use glam::{Quat, Vec3};

use crate::controllers::{
    body::{self, BodyDimensions},
    config::GripOffset,
    frame::{ControllerFrame, Hand, Pose, TrackedPose},
    hands::{pose::cockpit_hand, reach::yaw_only},
    mode::{AirborneVariant, Mode},
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
    /// Suppress pose-derived pitch during armed brake entry and its subsequent release.
    pub reserve_pitch: bool,
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
    /// Falling, waiting for both arms lowered or drawn in.
    AwaitingNeutral,
    /// Falling after the neutral, or in the suit, waiting for a stable flight pose.
    AwaitingFlight,
    /// In the suit, with closing and braking armed.
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
    /// The player is neither falling nor in the suit.
    Mode,
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
            Self::Mode => f.write_str("the player is neither falling nor in the suit"),
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
            Self::Slot => "is within reach of a slot on the body",
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
    /// The hand is within reach of a slot on the body (a socket or pouch), so an arm drawn in is a
    /// reach for it, not a closure.
    pub slot: bool,
    pub mechanical: bool,
    pub grapple: bool,
}

/// Identity and pose time of an input observation. A newer epoch permits clock/sequence reset.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct GestureSample {
    pub epoch: u64,
    pub sequence: u64,
    pub time_ns: i64,
}

/// A head pose published from the same locate and cockpit baseline as the controller observation.
///
/// The producer must preserve the original sample identity and tracking flags. Copying the current
/// controller stamp onto a separately queried global head pose does not satisfy this contract.
#[derive(Debug, Copy, Clone)]
pub(crate) struct GestureHead {
    pub sample: GestureSample,
    pub pose: TrackedPose,
    pub published_at: Instant,
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

    /// Whether both palms are turned to face the wind, within the brake's roll band, widened by the
    /// hysteresis while `braking`.
    fn braking_palms(self, config: &WingsuitGesturesConfig, braking: bool) -> bool {
        let margin = if braking {
            config.brake_hysteresis_deg
        } else {
            0.0
        };
        self.palm_roll_deg.into_iter().all(|roll| {
            roll >= config.brake_roll_deg - margin && roll <= config.brake_roll_max_deg + margin
        })
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
    pub gameplay: bool,
    /// A contextual canopy request, including a held request, cancels pending suit gestures.
    pub canopy_requested: bool,
    pub pose: Option<FlightPose>,
    pub claims: [HandClaims; 2],
}

/// Stateful recognizer owned by the binding tick, independently of physical vehicles.
#[derive(Debug, Default)]
pub(crate) struct WingsuitGestures {
    last: Option<(GestureSample, Mode)>,
    current: GestureAvailability,
    config: Option<WingsuitGesturesConfig>,
    ready: bool,
    armed: bool,
    brake: bool,
    reserve_pitch: bool,
    neutral: Dwell,
    flight: Dwell,
    close: Dwell,
    brake_entry: Dwell,
}

impl WingsuitGestures {
    /// Cancel gestures without lowering the sample watermark. Only a newer epoch restarts time.
    pub(crate) fn reset(&mut self) {
        self.current = GestureAvailability::Unavailable;
        self.clear_pose();
    }

    pub(crate) fn current(&self) -> GestureAvailability {
        self.current
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
        if self.last != Some((input.sample, input.mode))
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
        let changed = previous
            .is_none_or(|(last, mode)| last.epoch != input.sample.epoch || mode != input.mode);
        if changed || self.config != Some(*config) {
            self.clear_pose();
        }
        self.config = Some(*config);
        if !repeated {
            self.last = Some((input.sample, input.mode));
        }
        self.current = input.availability(config);
        if self.current == GestureAvailability::Unavailable {
            self.clear_pose();
            return GestureCommands::default();
        }
        if repeated {
            return GestureCommands {
                brake: self.brake,
                reserve_pitch: self.reserve_pitch,
                transition: None,
            };
        }
        let gap_s = previous
            .filter(|(last, _)| last.epoch == input.sample.epoch)
            .map_or(0.0, |(last, _)| {
                elapsed_s(last.time_ns, input.sample.time_ns)
            });
        if gap_s > f64::from(config.max_sample_gap_s) {
            self.reset();
            return GestureCommands::default();
        }
        let pose = input.pose.unwrap();
        let in_suit = input.mode == Mode::Airborne(AirborneVariant::Wingsuit);
        let wide = pose.lateral.into_iter().all(|v| {
            v >= config.flight_lateral_span
                - if self.brake {
                    config.pose_hysteresis_span
                } else {
                    0.0
                }
        });
        let brake_pose = in_suit && self.armed && wide && pose.braking_palms(config, self.brake);
        let brake_was = self.brake;
        self.brake = brake_pose
            && (self.brake
                || self
                    .brake_entry
                    .update(brake_pose, input.sample.time_ns, config.brake_dwell_s));
        if !brake_pose {
            self.brake_entry = Dwell::default();
        }
        let palms_returned = pose.flat_palms(config.pitch_release_roll_deg);
        self.reserve_pitch = brake_pose || brake_was || (self.reserve_pitch && !palms_returned);
        if self.reserve_pitch {
            self.close = Dwell::default();
            self.flight = Dwell::default();
            return GestureCommands {
                transition: None,
                brake: self.brake,
                reserve_pitch: self.reserve_pitch,
            };
        }
        let withdrawn = pose.withdrawn(config);
        let flight = pose
            .lateral
            .into_iter()
            .all(|v| v >= config.flight_lateral_span)
            && pose
                .angle_deg
                .into_iter()
                .all(|v| v <= config.flight_angle_deg)
            && pose.flat_palms(config.flight_roll_deg);
        if !self.ready
            && self
                .neutral
                .update(withdrawn, input.sample.time_ns, config.neutral_dwell_s)
        {
            self.ready = true;
        }
        // Deploying needs the neutral first, so a T-pose held through a fall does not open the suit
        // by itself. In the suit, the flight pose alone arms: a player who opened the suit with the
        // button often has their arms spread already, and the suit's own entry is the fresh start.
        if self.flight.update(
            (self.ready || in_suit) && flight,
            input.sample.time_ns,
            config.flight_dwell_s,
        ) {
            if !in_suit {
                self.clear_pose();
                return GestureCommands {
                    transition: Some(SuitTransition::Deploy),
                    ..Default::default()
                };
            }
            self.armed = true;
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

    fn clear_pose(&mut self) {
        self.ready = false;
        self.armed = false;
        self.brake = false;
        self.reserve_pitch = false;
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
        match self.mode {
            Mode::Airborne(AirborneVariant::Falling | AirborneVariant::Wingsuit) => None,
            _ => Some(Blocked::Mode),
        }
    }

    fn availability(self, config: &WingsuitGesturesConfig) -> GestureAvailability {
        if self.blocked(config).is_some() {
            return GestureAvailability::Unavailable;
        }
        match self.mode {
            Mode::Airborne(AirborneVariant::Falling) => GestureAvailability::Deploy,
            Mode::Airborne(AirborneVariant::Wingsuit) => GestureAvailability::CloseAndBrake,
            _ => GestureAvailability::Unavailable,
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
    let head_heading = yaw_only(head.orientation);
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
    Quat::from_rotation_y((-forward.x).atan2(-forward.z))
}

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
