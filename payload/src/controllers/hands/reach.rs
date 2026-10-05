//! Arm reach, all pure: the remap of a wrist from the player's arm onto the character's measured
//! arm, the pull ramp that lets the character's shoulder and chest follow a hand near full
//! extension, and the capped detach of the rendered hand from a wrist the solve left short.
//!
//! The pieces, each behind its toggle in [`ReachConfig`]:
//!
//! - [`ArmRemap`]: a wrist's offset from the player's shoulder, scaled by the ratio of the two arm
//!   lengths and placed at the character's shoulder. With the character's arm measured
//!   ([`ArmTracker`]), the shoulder is the character's animated shoulder joint, carried into the
//!   cockpit frame relative to his eyes, and the ratio is his measured shoulder-to-wrist length over
//!   the player's ([`player_arm_length`]), so a fully extended player arm asks for a fully extended
//!   character arm about the joint HumanIK reaches from. Without a measurement, the remap falls back
//!   to the estimated shoulders and the arm-span ratio ([`ArmRemap::estimated`]).
//! - [`pull_ramp`]: the wrist target's pull, ramping from zero as the target nears the arm's full
//!   length, so reach propagates into the girdle only when the arm alone cannot get there.
//! - [`detach_offset`]: how far the rendered hand leaves the solved wrist toward the target, past a
//!   small threshold and up to a cap.

use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::controllers::{
    body::{self, BodyDimensions},
    frame::{Hand, Pose},
};

/// The arm-reach settings, held in [`HandsConfig`](crate::controllers::config::HandsConfig). Each
/// part acts only under the controller scheme with the hand IK or the late update on, so they
/// default on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReachConfig {
    /// Remap the wrists about the character's measured shoulder joints by the ratio of his measured
    /// arm length to the player's, instead of about estimated shoulders by the arm-span ratio. Acts
    /// whether or not `body.remap_hands` is on; that setting governs only the estimated fallback.
    pub measured_remap: bool,
    /// The time constant of the smoothing on the character's measured shoulder positions, seconds.
    pub shoulder_smoothing_s: f32,
    /// Ramp the wrist targets' pull up as a hand nears full extension, so the character's shoulder
    /// and chest follow a reach.
    pub reach_pull: bool,
    /// The extension, as a fraction of the character's arm length, where the pull starts.
    pub pull_start: f32,
    /// The extension where the pull reaches [`pull_max`](Self::pull_max).
    pub pull_full: f32,
    /// The pull at and beyond full extension, `0..=1`.
    pub pull_max: f32,
    /// Move the rendered hand from a solved wrist that fell short of its target toward the target.
    pub detach: bool,
    /// The shortfall left alone, metres: below it, the rendered hand stays on the arm.
    pub detach_threshold_m: f32,
    /// The furthest the rendered hand leaves the arm, metres.
    pub detach_cap_m: f32,
    /// Stretch the rendered forearm along its axis so it stays attached to a detached hand.
    pub stretch_forearm: bool,
}

impl ReachConfig {
    pub const fn new() -> Self {
        Self {
            measured_remap: true,
            shoulder_smoothing_s: 0.1,
            reach_pull: true,
            pull_start: 0.9,
            pull_full: 1.0,
            pull_max: 0.4,
            detach: true,
            detach_threshold_m: 0.01,
            detach_cap_m: 0.1,
            stretch_forearm: true,
        }
    }
}

impl Default for ReachConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The remap of one arm's wrist: the offset from `player_shoulder`, scaled by `scale`, placed at
/// `character_shoulder`. All positions are in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ArmRemap {
    pub player_shoulder: Vec3,
    pub character_shoulder: Vec3,
    pub scale: f32,
}

impl ArmRemap {
    /// The remap from the estimated shoulders of both bodies at the same eye point, scaled by the
    /// arm-span ratio. The shoulders are estimated from the head (the eye midpoint) and the torso's
    /// heading `torso` ([`torso_heading`]), because the view puts the character's eyes where the
    /// player's are.
    pub fn estimated(
        hand: Hand,
        head: Pose,
        torso: Quat,
        player: &BodyDimensions,
        character: &BodyDimensions,
    ) -> Self {
        Self {
            player_shoulder: body::shoulder_positions(head.position, torso, player)[hand.index()],
            character_shoulder: body::shoulder_positions(head.position, torso, character)
                [hand.index()],
            scale: body::arm_span_ratio(player, character),
        }
    }

    /// The remap about the character's measured shoulder, by the ratio of the measured arm length to
    /// the player's. `arm` is the character's arm as measured; `world_scale` is the VR world scale
    /// the cockpit frame composes with; the player's shoulder is estimated at the torso's heading
    /// `torso` ([`torso_heading`]). `None` when either arm length is not a usable length.
    pub fn measured(
        hand: Hand,
        head: Pose,
        torso: Quat,
        player: &BodyDimensions,
        arm: &MeasuredArm,
        world_scale: f32,
    ) -> Option<Self> {
        let player_arm = player_arm_length(player);
        let character_arm = arm.length();
        let usable = |l: f32| l.is_finite() && l > MIN_ARM_M;
        if !usable(player_arm)
            || !usable(character_arm)
            || !world_scale.is_finite()
            || world_scale <= 0.0
        {
            return None;
        }
        Some(Self {
            player_shoulder: body::shoulder_positions(head.position, torso, player)[hand.index()],
            character_shoulder: head.position + arm.eye_to_shoulder / world_scale,
            scale: character_arm / player_arm / world_scale,
        })
    }

    /// The remapped wrist position.
    pub fn apply(&self, wrist: Vec3) -> Vec3 {
        self.character_shoulder + (wrist - self.player_shoulder) * self.scale
    }
}

/// The player's shoulder-to-wrist length from the body dimensions: half of the arm span less the
/// shoulder width, less the hand. This is the figure the calibration splits into the upper arm and
/// forearm; the body file's own segment lengths are not used, because a hand-written file need not
/// agree with its span.
pub fn player_arm_length(player: &BodyDimensions) -> f32 {
    (player.arm_span - player.shoulder_width) * 0.5 - player.hand
}

/// One of the character's arms as measured from his animated skeleton, smoothed.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct MeasuredArm {
    /// Shoulder joint to elbow, metres.
    pub upper_arm: f32,
    /// Elbow to wrist, metres.
    pub forearm: f32,
    /// The shoulder joint's offset from the eye midpoint, in the body frame's axes (the cockpit
    /// frame's axes), world metres.
    pub eye_to_shoulder: Vec3,
}

impl MeasuredArm {
    /// The shoulder-to-wrist length along the bones.
    pub fn length(&self) -> f32 {
        self.upper_arm + self.forearm
    }
}

/// One tick's reading of an arm: the shoulder, elbow, and wrist joints and the eye midpoint, all in
/// one space, and the rotation from that space into the body frame's axes.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ArmSample {
    pub shoulder: Vec3,
    pub elbow: Vec3,
    pub wrist: Vec3,
    pub eyes: Vec3,
    pub to_body: Quat,
}

impl ArmSample {
    /// Whether the sample can describe an arm: every segment within the plausible bounds.
    pub fn plausible(&self) -> bool {
        let segment = |a: Vec3, b: Vec3| {
            let l = a.distance(b);
            l.is_finite() && (MIN_SEGMENT_M..=MAX_SEGMENT_M).contains(&l)
        };
        segment(self.shoulder, self.elbow)
            && segment(self.elbow, self.wrist)
            && segment(self.eyes, self.shoulder)
            && self.to_body.is_finite()
    }
}

/// The smoothed measurement of one arm, updated once per tick.
///
/// The bone lengths are constant across animation (the rig carries no scale), so they settle over
/// a fixed half-second time constant and then hold. The eye-to-shoulder offset changes with the
/// animation and follows it over the configured time constant.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ArmTracker {
    arm: Option<MeasuredArm>,
    samples: u32,
    rejected: u32,
}

impl ArmTracker {
    /// No measurement yet.
    pub const EMPTY: Self = Self {
        arm: None,
        samples: 0,
        rejected: 0,
    };

    /// The measurement so far, or `None` before the first plausible sample.
    pub fn arm(&self) -> Option<MeasuredArm> {
        self.arm
    }

    /// The plausible samples taken.
    pub fn samples(&self) -> u32 {
        self.samples
    }

    /// The implausible samples dropped.
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// Fold in one tick's sample, `dt` seconds after the last, with `shoulder_smoothing_s` the
    /// time constant of the eye-to-shoulder offset. An implausible sample is dropped and counted.
    pub fn update(&mut self, sample: ArmSample, dt: f32, shoulder_smoothing_s: f32) {
        if !sample.plausible() {
            self.rejected = self.rejected.saturating_add(1);
            return;
        }
        let reading = MeasuredArm {
            upper_arm: sample.shoulder.distance(sample.elbow),
            forearm: sample.elbow.distance(sample.wrist),
            eye_to_shoulder: sample.to_body * (sample.shoulder - sample.eyes),
        };
        self.samples = self.samples.saturating_add(1);
        self.arm = Some(match self.arm {
            None => reading,
            Some(previous) => {
                let lengths = follow_factor(dt, LENGTH_SMOOTHING_S);
                let offset = follow_factor(dt, shoulder_smoothing_s);
                MeasuredArm {
                    upper_arm: lerp(previous.upper_arm, reading.upper_arm, lengths),
                    forearm: lerp(previous.forearm, reading.forearm, lengths),
                    eye_to_shoulder: previous
                        .eye_to_shoulder
                        .lerp(reading.eye_to_shoulder, offset),
                }
            }
        });
    }
}

/// The share of the pull a wrist target takes at `extension`, its distance from the shoulder as a
/// fraction of the arm's length: zero up to `start`, one from `full`, and a smoothstep between. With
/// `full` at or below `start` the ramp is a step at `start`.
pub fn pull_ramp(extension: f32, start: f32, full: f32) -> f32 {
    if !extension.is_finite() {
        return 0.0;
    }
    if full <= start {
        return if extension >= start { 1.0 } else { 0.0 };
    }
    let t = ((extension - start) / (full - start)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How far the rendered hand moves from the solved wrist toward the target, given the shortfall
/// `miss` (target minus solved wrist): the shortfall beyond `threshold`, up to `cap`, along the
/// shortfall. Continuous in the shortfall, so a hand crossing the threshold does not jump.
pub fn detach_offset(miss: Vec3, threshold: f32, cap: f32) -> Vec3 {
    let length = miss.length();
    if !length.is_finite() || length <= 0.0 {
        return Vec3::ZERO;
    }
    let applied = (length - threshold.max(0.0)).clamp(0.0, cap.max(0.0));
    miss * (applied / length)
}

/// The torso's heading the player's shoulders are estimated at: the estimated torso's `torso` when
/// there is one, and the head's heading otherwise.
pub fn torso_heading(head: Pose, torso: Option<Quat>) -> Quat {
    torso.unwrap_or_else(|| yaw_only(head.orientation))
}

/// `rotation` with its pitch and roll removed, keeping the heading of its -Z axis.
pub(crate) fn yaw_only(rotation: Quat) -> Quat {
    let forward = rotation * Vec3::NEG_Z;
    match Vec3::new(forward.x, 0.0, forward.z).try_normalize() {
        Some(flat) => Quat::from_rotation_arc(Vec3::NEG_Z, flat),
        None => Quat::IDENTITY,
    }
}

/// The shortest player or character arm the measured remap accepts, metres.
const MIN_ARM_M: f32 = 0.2;
/// The bounds on one measured segment (a bone, or the eyes to a shoulder), metres.
const MIN_SEGMENT_M: f32 = 0.05;
const MAX_SEGMENT_M: f32 = 0.6;
/// The time constant the bone lengths settle over, seconds.
const LENGTH_SMOOTHING_S: f32 = 0.5;

/// The fraction of the way to a new reading to move after `dt` seconds with the time constant
/// `tau`; a non-positive `tau` follows at once.
fn follow_factor(dt: f32, tau: f32) -> f32 {
    if tau.is_nan() || tau <= 0.0 || !dt.is_finite() {
        return 1.0;
    }
    1.0 - (-dt.max(0.0) / tau).exp()
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-4;

    const PLAYER: BodyDimensions = BodyDimensions {
        height: 1.80,
        eye_height: 1.68,
        shoulder_height: 1.47,
        shoulder_width: 0.40,
        arm_span: 1.84,
        upper_arm: 0.29,
        forearm: 0.23,
        hand: 0.19,
        ..BodyDimensions::GENERIC_ADULT
    };

    fn head() -> Pose {
        Pose {
            position: Vec3::new(0.1, PLAYER.eye_height, -0.2),
            orientation: Quat::from_rotation_y(0.3),
        }
    }

    fn rico() -> MeasuredArm {
        MeasuredArm {
            upper_arm: 0.27,
            forearm: 0.25,
            eye_to_shoulder: Vec3::new(0.19, -0.22, 0.12),
        }
    }

    #[test]
    fn the_player_arm_is_half_the_span_less_the_shoulders_and_hand() {
        assert!((player_arm_length(&PLAYER) - 0.53).abs() < EPS);
    }

    #[test]
    fn full_player_extension_maps_to_full_character_extension_about_his_shoulder() {
        let head = head();
        let remap = ArmRemap::measured(
            Hand::Right,
            head,
            torso_heading(head, None),
            &PLAYER,
            &rico(),
            1.0,
        )
        .unwrap();
        for direction in [Vec3::NEG_Z, Vec3::X, Vec3::new(0.3, -0.5, -0.8).normalize()] {
            let wrist = remap.player_shoulder + direction * player_arm_length(&PLAYER);
            let remapped = remap.apply(wrist);
            let reach = remapped - remap.character_shoulder;
            assert!((reach.length() - rico().length()).abs() < EPS);
            assert!(reach.normalize().dot(direction) > 1.0 - EPS);
        }
        assert!(
            remap
                .character_shoulder
                .distance(head.position + rico().eye_to_shoulder)
                < EPS
        );
    }

    #[test]
    fn the_measured_remap_divides_out_the_world_scale() {
        let head = head();
        let scaled = ArmRemap::measured(
            Hand::Left,
            head,
            torso_heading(head, None),
            &PLAYER,
            &rico(),
            2.0,
        )
        .unwrap();
        let wrist = scaled.player_shoulder + Vec3::NEG_Z * player_arm_length(&PLAYER);
        // Composed into the world, cockpit offsets scale by the world scale; Rico's arm does not.
        let world_reach = (scaled.apply(wrist) - scaled.character_shoulder) * 2.0;
        assert!((world_reach.length() - rico().length()).abs() < EPS);
    }

    #[test]
    fn an_unusable_arm_has_no_measured_remap() {
        let short = MeasuredArm {
            upper_arm: 0.05,
            forearm: 0.05,
            ..rico()
        };
        assert!(
            ArmRemap::measured(Hand::Right, head(), Quat::IDENTITY, &PLAYER, &short, 1.0).is_none()
        );
        assert!(
            ArmRemap::measured(Hand::Right, head(), Quat::IDENTITY, &PLAYER, &rico(), 0.0)
                .is_none()
        );
    }

    #[test]
    fn the_estimated_remap_is_the_identity_for_the_same_body() {
        let wrist = Vec3::new(0.4, 1.1, -0.3);
        let remap = ArmRemap::estimated(
            Hand::Right,
            head(),
            torso_heading(head(), None),
            &PLAYER,
            &PLAYER,
        );
        assert!(remap.apply(wrist).distance(wrist) < EPS);
    }

    #[test]
    fn the_shoulders_follow_the_torso_rather_than_the_head_when_it_is_estimated() {
        let head = head();
        let torso = Quat::from_rotation_y(-0.4);
        assert_eq!(torso_heading(head, Some(torso)), torso);
        assert!(torso_heading(head, None).angle_between(head.orientation) < EPS);
        let remap = ArmRemap::estimated(Hand::Right, head, torso, &PLAYER, &PLAYER);
        let expected = body::shoulder_positions(head.position, torso, &PLAYER)[Hand::Right.index()];
        assert!(remap.player_shoulder.distance(expected) < EPS);
        let measured = ArmRemap::measured(Hand::Right, head, torso, &PLAYER, &rico(), 1.0).unwrap();
        assert!(measured.player_shoulder.distance(expected) < EPS);
    }

    #[test]
    fn the_estimated_remap_scales_reach_by_the_span_ratio() {
        let player = BodyDimensions {
            arm_span: 1.5,
            ..BodyDimensions::GENERIC_ADULT
        };
        let character = BodyDimensions::CHARACTER_ESTIMATE;
        let head = Pose {
            position: Vec3::new(0.0, player.eye_height, 0.0),
            orientation: Quat::IDENTITY,
        };
        let remap = ArmRemap::estimated(Hand::Right, head, Quat::IDENTITY, &player, &character);
        let wrist = remap.player_shoulder + Vec3::NEG_Z * 0.5;
        let reach = (remap.apply(wrist) - remap.character_shoulder).length();
        assert!((reach - 0.5 * character.arm_span / player.arm_span).abs() < EPS);
    }

    fn sample(upper: f32, forearm: f32, offset: Vec3) -> ArmSample {
        let eyes = Vec3::new(0.0, 1.7, 0.0);
        let shoulder = eyes + offset;
        let elbow = shoulder + Vec3::NEG_Y * upper;
        ArmSample {
            shoulder,
            elbow,
            wrist: elbow + Vec3::NEG_Z * forearm,
            eyes,
            to_body: Quat::IDENTITY,
        }
    }

    #[test]
    fn the_tracker_takes_the_first_sample_and_settles_on_constant_lengths() {
        let mut tracker = ArmTracker::default();
        assert!(tracker.arm().is_none());
        let offset = Vec3::new(0.2, -0.2, 0.1);
        tracker.update(sample(0.27, 0.25, offset), 0.0, 0.1);
        let arm = tracker.arm().unwrap();
        assert!((arm.upper_arm - 0.27).abs() < EPS);
        assert!((arm.forearm - 0.25).abs() < EPS);
        assert!(arm.eye_to_shoulder.distance(offset) < EPS);
        for _ in 0..200 {
            tracker.update(sample(0.27, 0.25, offset), 1.0 / 30.0, 0.1);
        }
        assert!((tracker.arm().unwrap().length() - 0.52).abs() < EPS);
        assert_eq!(tracker.samples(), 201);
    }

    #[test]
    fn the_shoulder_offset_follows_over_its_time_constant() {
        let mut tracker = ArmTracker::default();
        tracker.update(sample(0.27, 0.25, Vec3::new(0.2, -0.2, 0.0)), 0.0, 0.1);
        tracker.update(sample(0.27, 0.25, Vec3::new(0.2, -0.2, 0.1)), 0.1, 0.1);
        let z = tracker.arm().unwrap().eye_to_shoulder.z;
        assert!((z - 0.1 * (1.0 - (-1.0f32).exp())).abs() < EPS);
    }

    #[test]
    fn the_shoulder_offset_turns_into_the_body_frame() {
        let mut tracker = ArmTracker::default();
        let mut s = sample(0.27, 0.25, Vec3::new(0.2, -0.2, 0.0));
        s.to_body = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        tracker.update(s, 0.0, 0.1);
        let offset = tracker.arm().unwrap().eye_to_shoulder;
        assert!(offset.distance(Vec3::new(0.0, -0.2, -0.2)) < EPS);
    }

    #[test]
    fn an_implausible_sample_is_dropped() {
        let mut tracker = ArmTracker::default();
        tracker.update(sample(0.27, 0.25, Vec3::new(0.2, -0.2, 0.0)), 0.0, 0.1);
        let before = tracker.arm();
        tracker.update(sample(2.0, 0.25, Vec3::new(0.2, -0.2, 0.0)), 0.03, 0.1);
        tracker.update(sample(0.27, 0.0, Vec3::new(0.2, -0.2, 0.0)), 0.03, 0.1);
        let mut nan = sample(0.27, 0.25, Vec3::new(0.2, -0.2, 0.0));
        nan.wrist = Vec3::NAN;
        tracker.update(nan, 0.03, 0.1);
        assert_eq!(tracker.arm(), before);
        assert_eq!(tracker.rejected(), 3);
    }

    #[test]
    fn the_pull_ramp_is_zero_below_start_and_full_from_full() {
        assert_eq!(pull_ramp(0.5, 0.9, 1.0), 0.0);
        assert_eq!(pull_ramp(0.9, 0.9, 1.0), 0.0);
        assert!((pull_ramp(0.95, 0.9, 1.0) - 0.5).abs() < EPS);
        assert_eq!(pull_ramp(1.0, 0.9, 1.0), 1.0);
        assert_eq!(pull_ramp(1.4, 0.9, 1.0), 1.0);
        assert_eq!(pull_ramp(f32::NAN, 0.9, 1.0), 0.0);
    }

    #[test]
    fn the_pull_ramp_is_monotonic() {
        let mut previous = 0.0;
        for i in 0..=200 {
            let r = pull_ramp(0.8 + i as f32 * 0.001, 0.9, 1.0);
            assert!(r >= previous);
            previous = r;
        }
    }

    #[test]
    fn a_degenerate_pull_ramp_is_a_step() {
        assert_eq!(pull_ramp(0.89, 0.9, 0.9), 0.0);
        assert_eq!(pull_ramp(0.9, 0.9, 0.8), 1.0);
    }

    #[test]
    fn the_detach_ignores_a_small_miss_and_caps_a_large_one() {
        assert_eq!(detach_offset(Vec3::X * 0.005, 0.01, 0.1), Vec3::ZERO);
        assert!(
            detach_offset(Vec3::X * 0.05, 0.01, 0.1).distance(Vec3::X * 0.04) < EPS,
            "the miss beyond the threshold"
        );
        assert!(detach_offset(Vec3::NEG_Z * 0.5, 0.01, 0.1).distance(Vec3::NEG_Z * 0.1) < EPS);
        assert_eq!(detach_offset(Vec3::ZERO, 0.01, 0.1), Vec3::ZERO);
        assert_eq!(detach_offset(Vec3::NAN, 0.01, 0.1), Vec3::ZERO);
    }

    #[test]
    fn the_detach_is_continuous_at_the_threshold() {
        let below = detach_offset(Vec3::X * 0.0099, 0.01, 0.1);
        let above = detach_offset(Vec3::X * 0.0101, 0.01, 0.1);
        assert!(below.distance(above) < 1e-3);
    }
}
