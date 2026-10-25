//! Bilateral wingsuit deployment, braking, and closure from tracked cockpit poses.
//!
//! The binding owner supplies input availability, transition cancellation, and transient hand
//! claims. This module returns commands without writing game actions or claiming weapon fire.

use std::time::Instant;

use glam::{Quat, Vec3};

use crate::controllers::{
    body::{self, BodyDimensions},
    config::GripOffset,
    frame::{ControllerFrame, Hand, Pose, TrackedPose},
    hands::{pose::cockpit_hand, reach::torso_heading},
    mode::{AirborneVariant, Mode},
    vehicles::config::WingsuitGesturesConfig,
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

/// Current physical input claims, indexed by `Hand::index`.
///
/// These describe actual interactions, not the suit's own weapon triggers. A support or reload
/// hand reports only the inputs its interaction owns. Grip release remains the owner's concern.
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub(crate) struct HandClaims {
    pub grip: bool,
    pub trigger: bool,
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
    /// Each palm normal's cosine against the torso's horizontal forward direction.
    pub palm_forward: [f32; 2],
}

impl FlightPose {
    /// Measure raw wrists against calibrated shoulders from explicitly matched head/hand samples.
    /// The configured grip-to-wrist offset supplies palm -Y; the aim pose is not a palm basis.
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
        let heading = torso_heading(head.pose.pose, *torso);
        let shoulders = body::shoulder_positions(head.pose.pose.position, heading, dimensions);
        let wrists = Hand::BOTH.map(|hand| {
            let tracked = controllers.hand(hand);
            if !tracked.active || !tracked.grip.is_tracked() {
                return None;
            }
            Some(cockpit_hand(hand, tracked.grip.pose, tracked.aim.pose, grip_offset, None).wrist)
        });
        Self::measure(
            [wrists[0]?, wrists[1]?],
            shoulders,
            heading,
            dimensions.arm_span,
        )
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
        let forward = heading * Vec3::NEG_Z;
        let mut pose = Self {
            lateral: [0.0; 2],
            reach: [0.0; 2],
            height: [0.0; 2],
            angle_deg: [0.0; 2],
            palm_forward: [0.0; 2],
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
            pose.palm_forward[i] = (wrists[i].orientation * Vec3::NEG_Y).dot(forward);
        }
        pose.valid().then_some(pose)
    }

    fn valid(self) -> bool {
        self.lateral
            .into_iter()
            .chain(self.reach)
            .chain(self.height)
            .chain(self.angle_deg)
            .chain(self.palm_forward)
            .all(f32::is_finite)
            && self.reach.into_iter().all(|v| (0.0..=0.8).contains(&v))
            && self
                .angle_deg
                .into_iter()
                .all(|v| (0.0..=180.0).contains(&v))
            && self
                .palm_forward
                .into_iter()
                .all(|v| (-1.001..=1.001).contains(&v))
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
    pub occupied: [bool; 2],
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
        let palms = pose.palm_forward.into_iter().all(|v| {
            v >= (config.brake_angle_deg
                + if self.brake {
                    config.brake_hysteresis_deg
                } else {
                    0.0
                })
            .to_radians()
            .cos()
        });
        let brake_pose = in_suit && self.armed && wide && palms;
        let brake_was = self.brake;
        self.brake = brake_pose
            && (self.brake
                || self
                    .brake_entry
                    .update(brake_pose, input.sample.time_ns, config.brake_dwell_s));
        if !brake_pose {
            self.brake_entry = Dwell::default();
        }
        let palms_returned = pose
            .palm_forward
            .into_iter()
            .all(|v| v <= config.pitch_release_alignment);
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
            && palms_returned;
        if !self.ready
            && self
                .neutral
                .update(withdrawn, input.sample.time_ns, config.neutral_dwell_s)
        {
            self.ready = true;
        }
        if self.flight.update(
            self.ready && flight,
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
    fn availability(self, config: &WingsuitGesturesConfig) -> GestureAvailability {
        if !self.gameplay
            || self.canopy_requested
            || !config.enabled
            || !config.valid()
            || self.occupied.into_iter().any(|occupied| occupied)
            || self.claims.into_iter().any(HandClaims::interacting)
            || !self.pose.is_some_and(FlightPose::valid)
        {
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
    fn interacting(self) -> bool {
        self.grip || self.trigger || self.mechanical || self.grapple
    }
}

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
