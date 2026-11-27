//! The pure part of a release: the rig's motion between two input ticks, and a hand's timestamped
//! history read at the released point with that motion added once.

use std::time::Duration;

use body_rig::math::rotation::{angular_velocity, shortest};
use glam::Vec3;
use jc3vr_input::{
    frame::{ControllerFrame, Hand},
    motion::{self, MotionSample, RELEASE_ESTIMATE, ReleaseShape, shaped_release},
};
use jc3vr_view::controllers::cockpit::CockpitTransform;
use tracking_format::event::WorldMotion;

use crate::controllers::{
    clock::TimeScale,
    physics::{
        rig::{RigJump, RigSample, RigWatch},
        tuning::PHYSICS,
    },
};

/// The cockpit-to-world transform sampled on an input tick, without controller translation.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ReleaseFrame {
    pub origin: Vec3,
    /// The body's facing, which rotates the cockpit into the world, and the counts of the mod's
    /// jumps of the rig.
    pub rig: RigSample,
    pub scale: f32,
    pub(crate) teleport_epoch: u64,
}

impl ReleaseFrame {
    /// Recover the transform used by hand composition without adding HMD translation twice: the
    /// origin is where the cockpit frame's origin lands with the head at `head_world`.
    pub(crate) fn with_composed_head(mut self, head_world: Vec3, head_cockpit: Vec3) -> Self {
        self.origin = CockpitTransform {
            head_world,
            head_cockpit,
            body: self.rig.rotation,
            world_scale: self.scale,
        }
        .point_to_world(Vec3::ZERO);
        self
    }

    fn valid(self) -> bool {
        self.origin.is_finite()
            && self.rig.rotation.is_finite()
            && (self.rig.rotation.length_squared() - 1.0).abs() < 1e-3
            && self.scale.is_finite()
            && self.scale > 0.0
    }
}

/// A fresh rig transform and its world-space motion, excluding hand motion.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RigMotion {
    pub frame: ReleaseFrame,
    pub motion: WorldMotion,
}

/// The bounded, per-input-tick producer of the rig's motion for releases.
#[derive(Debug, Copy, Clone, Default)]
pub struct ReleaseTransport {
    previous: Option<ReleaseFrame>,
    rig: RigWatch,
    history_after_s: Option<f64>,
}

impl ReleaseTransport {
    pub const fn new() -> Self {
        Self {
            previous: None,
            rig: RigWatch::new(),
            history_after_s: None,
        }
    }

    /// Samples at or before the latest coordinate change cannot feed a release or a difference.
    pub(crate) fn history_after_s(&self) -> Option<f64> {
        self.history_after_s
    }

    /// Take this input tick's frame, `dt` seconds after the last, with the hand history's latest
    /// time `time_s`, and return the rig's motion since the last frame.
    ///
    /// A change of the cockpit frame (a re-base, a teleport, a change of scale, an origin that
    /// leapt) or a turn the game made at once cuts the history: nothing before it feeds a release.
    /// A turn the mod made at once (a snap turn or a hard step of the view) turns only the body
    /// frame, so the history stands, but the tick's turn and the origin's swing about the head are
    /// not the player's movement: the rig is at rest on that tick and the one after it
    /// ([`RigJump::Settling`]).
    pub fn update(
        &mut self,
        frame: Option<ReleaseFrame>,
        dt: f32,
        time_s: Option<f64>,
    ) -> Option<RigMotion> {
        let previous = self.previous;
        self.previous = frame.filter(|frame| frame.valid());
        let Some(current) = self.previous else {
            self.rig = RigWatch::new();
            self.cut_history(time_s);
            return None;
        };
        let jump = self.rig.observe(current.rig);
        let Some(previous) = previous else {
            self.cut_history(time_s);
            return None;
        };
        if !(0.001..=PHYSICS.get().release.max_age_s).contains(&dt)
            || current.teleport_epoch != previous.teleport_epoch
            || (current.scale - previous.scale).abs() > 1e-4
            || current.origin.distance(previous.origin) > 5.0
            || matches!(jump, Some(RigJump::Rebase | RigJump::Unannounced))
        {
            self.cut_history(time_s);
            return None;
        }
        let angle =
            shortest(current.rig.rotation * previous.rig.rotation.inverse()).to_scaled_axis();
        if !angle.is_finite() {
            self.cut_history(time_s);
            return None;
        }
        if self.history_after_s == Some(f64::INFINITY) {
            self.cut_history(time_s);
        }
        let motion = if matches!(jump, Some(RigJump::Turn | RigJump::Settling)) {
            WorldMotion::ZERO
        } else {
            WorldMotion {
                linear: (current.origin - previous.origin) / dt,
                angular: angle / dt,
            }
        };
        Some(RigMotion {
            frame: current,
            motion,
        })
    }

    fn cut_history(&mut self, time_s: Option<f64>) {
        self.history_after_s = Some(
            time_s
                .filter(|time| time.is_finite())
                .unwrap_or(f64::INFINITY),
        );
    }
}

/// Where a release's motion came from.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ReleaseSource {
    /// The hand's timestamped history, with the rig's motion.
    TimestampedHandHistory,
    /// The rig's motion alone: the hand's history was missing, stale, or untracked.
    RigTransportOnly,
    /// Nothing: the rig's motion was not fresh.
    UnavailableAtRelease,
}

impl ReleaseSource {
    /// The source's name, as a recorded throw carries it.
    pub const fn label(self) -> &'static str {
        match self {
            Self::TimestampedHandHistory => "timestamped hand history",
            Self::RigTransportOnly => "rig transport only",
            Self::UnavailableAtRelease => "unavailable at release",
        }
    }
}

/// The motion of a released point, as everything a hand lets go of leaves with it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ReleaseMotion {
    /// Per game second: the world's.
    pub world: WorldMotion,
    /// The rig's own part of [`world`](Self::world): the player's movement at the released point,
    /// without the tracked hand's.
    pub inherited: WorldMotion,
    pub source: ReleaseSource,
    pub history_age_s: Option<f32>,
    /// The game's time scale the hand's part of [`world`](Self::world) was converted at.
    pub time_scale: TimeScale,
}

impl ReleaseMotion {
    pub const REST: Self = Self {
        world: WorldMotion::ZERO,
        inherited: WorldMotion::ZERO,
        source: ReleaseSource::UnavailableAtRelease,
        history_age_s: None,
        time_scale: TimeScale::REAL,
    };

    /// This release in a world running at `time_scale`. The hand's part, measured per real second
    /// from the controller's display times, becomes the same motion per game second, `1 /
    /// time_scale` as fast in the world's units: under slow motion the item leaves the hand as fast
    /// as the hand moved, carrying on from the hold, which kept it with the hand in real time,
    /// instead of slowing at the release. The player's own movement through the rig is already
    /// per game second and is kept.
    pub(crate) fn in_game_time(self, time_scale: TimeScale) -> Self {
        let hand = WorldMotion {
            linear: self.world.linear - self.inherited.linear,
            angular: self.world.angular - self.inherited.angular,
        };
        Self {
            world: WorldMotion {
                linear: self.inherited.linear + time_scale.to_game_rate(hand.linear),
                angular: self.inherited.angular + time_scale.to_game_rate(hand.angular),
            },
            time_scale,
            ..self
        }
    }

    /// The motion a release requests, with the hand's share held to the tuning's
    /// `release.max_hand_mps` ([`Self::capped`]).
    pub fn thrown(self) -> WorldMotion {
        self.capped(PHYSICS.get().release.max_hand_mps)
    }

    /// The motion a release requests: non-finite parts are zero, and the hand's own part of the
    /// linear motion, beyond what the player's movement carries, is held to `max_speed_mps` per
    /// real second (zero when the cap is not a finite number), so a tracking glitch is capped alike
    /// at any time scale. The carried part is bounded only by the tuning's
    /// `release.max_carried_mps`: a throw from a vehicle keeps the vehicle's speed, and a stale or
    /// corrupt reading does not fling the released object. The shaping is the one every release
    /// takes ([`shaped_release`]), with the hand's share unscaled. A cap of zero is a drop: the
    /// object leaves with the player's movement alone.
    pub fn capped(self, max_speed_mps: f32) -> WorldMotion {
        let cap = if max_speed_mps.is_finite() {
            max_speed_mps.max(0.0) / self.time_scale.get()
        } else {
            0.0
        };
        let finite = |velocity: Vec3| {
            if velocity.is_finite() {
                velocity
            } else {
                Vec3::ZERO
            }
        };
        let carried =
            finite(self.inherited.linear).clamp_length_max(PHYSICS.get().release.max_carried_mps);
        WorldMotion {
            linear: shaped_release(
                finite(self.world.linear),
                carried,
                ReleaseShape {
                    scale: 1.0,
                    max_speed_mps: cap,
                },
            ),
            angular: finite(self.world.angular),
        }
    }
}

/// The controller context a release checks the hand's history against.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ControllerContext {
    pub frame_sequence: u64,
    /// The runtime's predicted display time of the frame.
    pub display_time_ns: i64,
    /// The host's publication age at the release.
    pub publication_age: Duration,
    pub focused: bool,
    pub active: bool,
    pub position_valid: bool,
    pub orientation_valid: bool,
    pub position_tracked: bool,
    pub orientation_tracked: bool,
    pub linear_cockpit_mps: Option<Vec3>,
    pub angular_cockpit_rad_s: Option<Vec3>,
}

impl ControllerContext {
    pub fn from_frame(frame: &ControllerFrame, hand: Hand) -> Self {
        let controller = frame.hand(hand);
        let grip = controller.grip;
        Self {
            frame_sequence: frame.sequence,
            display_time_ns: frame.display_time_ns,
            publication_age: frame.age(),
            focused: frame.focused,
            active: controller.active,
            position_valid: grip.position_valid,
            orientation_valid: grip.orientation_valid,
            position_tracked: grip.position_tracked,
            orientation_tracked: grip.orientation_tracked,
            linear_cockpit_mps: grip.linear_velocity,
            angular_cockpit_rad_s: grip.angular_velocity,
        }
    }
}

/// What one release reads.
pub struct ReleaseInput<'a> {
    pub rig: Option<RigMotion>,
    pub controller: Option<&'a ControllerContext>,
    pub samples: &'a [MotionSample],
    /// The display time the hand took what it lets go of: earlier samples belong to whatever the
    /// hand did before.
    pub since_s: f64,
    pub(crate) history_after_s: Option<f64>,
    pub point_world: Vec3,
}

impl ReleaseInput<'_> {
    fn accepts_time(&self, time_s: f64, release_s: f64) -> bool {
        time_s >= self.since_s
            && time_s <= release_s
            && self
                .history_after_s
                .is_none_or(|boundary| time_s > boundary)
    }
}

/// The release of the point `input.point_world`, in real time: the hand's history in the cockpit
/// frame, read at its fastest moment ([`motion::release_velocity`]) at the point carried rigidly
/// with the grip, so the hand's turn adds its lever; turned and scaled into the world; with the
/// rig's motion at the point added once. The spin is the hand's.
pub fn estimate(input: ReleaseInput<'_>) -> ReleaseMotion {
    let max_age_s = PHYSICS.get().release.max_age_s;
    let Some(rig) = input.rig.filter(|rig| {
        rig.frame.valid()
            && rig.motion.linear.is_finite()
            && rig.motion.angular.is_finite()
            && input.point_world.is_finite()
    }) else {
        return ReleaseMotion::REST;
    };
    let inherited = WorldMotion {
        linear: rig.motion.linear
            + rig
                .motion
                .angular
                .cross(input.point_world - rig.frame.origin),
        angular: rig.motion.angular,
    };
    let fallback = ReleaseMotion {
        world: inherited,
        inherited,
        source: ReleaseSource::RigTransportOnly,
        history_age_s: None,
        time_scale: TimeScale::REAL,
    };
    let Some(controller) = input.controller else {
        return fallback;
    };
    if !(controller.focused
        && controller.active
        && controller.position_valid
        && controller.orientation_valid
        && controller.position_tracked
        && controller.orientation_tracked)
        || controller.publication_age.as_secs_f32() > max_age_s
    {
        return fallback;
    }
    let release_s = controller.display_time_ns as f64 * 1e-9;
    let Some(latest) = input
        .samples
        .iter()
        .rev()
        .find(|sample| input.accepts_time(sample.time_s, release_s))
    else {
        return fallback;
    };
    let age_s = (release_s - latest.time_s) as f32;
    if !(0.0..=max_age_s).contains(&age_s) {
        return fallback;
    }
    let mut samples = input
        .samples
        .iter()
        .copied()
        .filter(|sample| input.accepts_time(sample.time_s, release_s))
        .collect::<Vec<_>>();
    for sample in &mut samples {
        sample.linear_velocity = sample
            .linear_velocity
            .filter(|velocity| velocity.is_finite());
        sample.angular_velocity = sample
            .angular_velocity
            .filter(|velocity| velocity.is_finite());
    }
    for index in 1..samples.len() {
        let previous = samples[index - 1];
        let sample = &mut samples[index];
        let dt = (sample.time_s - previous.time_s) as f32;
        if sample.angular_velocity.is_none() {
            sample.angular_velocity =
                angular_velocity(previous.orientation, sample.orientation, dt);
        }
    }
    let point_cockpit =
        rig.frame.rig.rotation.inverse() * (input.point_world - rig.frame.origin) / rig.frame.scale;
    let offset = latest.orientation.inverse() * (point_cockpit - latest.position);
    let Some(linear) = motion::release_velocity(&samples, release_s, offset, RELEASE_ESTIMATE)
    else {
        return fallback;
    };
    let angular = motion::release_angular_velocity(&samples, release_s, RELEASE_ESTIMATE)
        .unwrap_or(Vec3::ZERO);
    let world = WorldMotion {
        linear: rig.frame.rig.rotation * linear * rig.frame.scale + inherited.linear,
        angular: rig.frame.rig.rotation * angular + inherited.angular,
    };
    if !(world.linear.is_finite() && world.angular.is_finite()) {
        return fallback;
    }
    ReleaseMotion {
        world,
        inherited,
        source: ReleaseSource::TimestampedHandHistory,
        history_age_s: Some(age_s),
        time_scale: TimeScale::REAL,
    }
}
