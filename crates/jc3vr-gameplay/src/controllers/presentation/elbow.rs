//! The elbow's pole for the render-rate arm solve, pure: which way an arm's elbow points about the
//! line from its shoulder to its wrist, from where the hand is and how the wrist is turned, rather
//! than from where HumanIK left the elbow.
//!
//! Everything is in Rico's model space (+X his right, +Y up, -Z forward), and so are the inputs
//! ([`PoleInput`]): the shoulders as the torso's edit carries them, the wrist as the late update
//! draws it, the player's up (gravity as the cockpit has it, which tilts against the model only
//! where the body frame does), and the hand bone's orientation.
//!
//! The pole ([`pole`]) points down, out to the arm's side, and a little back. A hand raised toward
//! or above the shoulder, or reaching across the body, lifts it further out, as the HumanIK elbow
//! hint does ([`crate::controllers::body::procedural::solver::arms::elbow_hint`]). On top of that
//! it leans toward the little-finger side of the wrist ([`little_finger`]): with the palm down
//! that adds out, with the thumb up down, and with the palm up it tucks the elbow in, as a
//! forearm's pronation drags a real elbow. Where the reach runs along the pole (a hand down, out,
//! and back), the pole's part across the reach vanishes, and a pole straight back fades in as it
//! shrinks. The direction is then smoothed ([`smooth`]) with a time constant of a few frames.
//!
//! The rule and its weights come from an offline study of the 6 November 2026 session, where it
//! halved the elbow's flips about the shoulder-wrist axis and the elbow's jumps the wrist did not
//! share, against the elbow HumanIK drew; `docs/mod/rendering/presentation-layer.md` has the
//! numbers.

use body_rig::smooth::follow_factor;
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::controllers::body::procedural::solver::arms::Side;

/// The rule's settings.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ElbowPoleConfig {
    /// Bend each controller-driven arm toward the rule's pole. Off, the arm bends toward where
    /// HumanIK left the elbow.
    pub enabled: bool,
    /// How hard the little-finger side of the wrist pulls the pole, against the body pole's
    /// downward pull of one.
    pub palm_weight: f32,
    /// The smoothing's time constant, seconds.
    pub smoothing_s: f32,
}

impl ElbowPoleConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            palm_weight: DEFAULT_PALM_WEIGHT,
            smoothing_s: DEFAULT_SMOOTHING_S,
        }
    }
}

impl Default for ElbowPoleConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The default pull of the little-finger side of the wrist.
pub const DEFAULT_PALM_WEIGHT: f32 = 0.7;

/// The default smoothing time constant, seconds.
pub const DEFAULT_SMOOTHING_S: f32 = 0.06;

/// The longest step the smoothing carries its direction across, seconds; past it (the layer was
/// idle, a hitch) the smoothing starts again from the rule's pole.
pub const MAX_SMOOTHING_GAP_S: f32 = 0.15;

/// One arm's inputs, model space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PoleInput {
    /// The arm's shoulder joint.
    pub shoulder: Vec3,
    /// The other arm's shoulder joint, which fixes which way is out to this arm's side.
    pub other_shoulder: Vec3,
    /// The arm's wrist.
    pub wrist: Vec3,
    /// The player's up, a unit vector.
    pub up: Vec3,
    /// The little-finger side of the wrist, a unit vector ([`little_finger`]).
    pub little_finger: Vec3,
}

/// The direction the elbow of the arm on `side` points from the line from its shoulder to its
/// wrist: a unit vector square to that line. `palm_weight` is the little finger's pull
/// ([`ElbowPoleConfig::palm_weight`]). `None` for a degenerate arm: the wrist on the shoulder, the
/// shoulders on one vertical line, or non-finite inputs.
pub fn pole(input: &PoleInput, side: Side, palm_weight: f32) -> Option<Vec3> {
    let up = input.up.try_normalize()?;
    let out = across(input.shoulder - input.other_shoulder, up).try_normalize()?;
    // Out and up are square, so their cross is a unit vector; the order follows the side so that
    // it points back on both.
    let back = match side {
        Side::Left => up.cross(out),
        Side::Right => out.cross(up),
    };
    let reach = input.wrist - input.shoulder;
    let axis = reach.try_normalize()?;
    let length = reach.length();
    let raise = ((reach.dot(up) - RAISE_FROM_M) / RAISE_SPAN_M).clamp(0.0, 1.0);
    let across_body = (-reach.dot(out) / length.max(ACROSS_MIN_REACH_M)).clamp(0.0, 1.0);
    let palm = if palm_weight.is_finite() {
        palm_weight
    } else {
        0.0
    };
    let little_finger = if input.little_finger.is_finite() {
        input.little_finger
    } else {
        Vec3::ZERO
    };
    let pole = -up * POLE_DOWN
        + out * (POLE_OUT + POLE_RAISE_OUT * raise + POLE_ACROSS_OUT * across_body)
        + back * POLE_BACK
        + little_finger * palm;
    let primary = across(pole, axis);
    let fallback = (1.0 - primary.length() / (POLE_FALLBACK_BELOW * pole.length())).clamp(0.0, 1.0);
    let side_of_reach = across(primary + across(back, axis) * fallback, axis);
    side_of_reach.try_normalize().filter(|p| p.is_finite())
}

/// The little-finger side of the hand on `side` whose bone is turned `hand` in model space: the
/// direction across the palm from the index finger's knuckle toward the little finger's.
pub fn little_finger(hand: Quat, side: Side) -> Vec3 {
    let local = match side {
        Side::Left => LEFT_LITTLE_FINGER,
        // The right hand's bone frame is the left's mirrored with every axis reversed, so the
        // same direction on the body is the left's axis negated in it.
        Side::Right => -LEFT_LITTLE_FINGER,
    };
    hand.normalize() * local.normalize()
}

/// One step of the smoothing over `dt` seconds with the time constant `smoothing_s`: `previous`,
/// laid square to the arm's `axis` (from the shoulder to the wrist), turned about the axis toward
/// the rule's `target` (square to it already) by the share of the angle between them an
/// exponential follow closes in the step. Turning by a share of the angle, rather than blending
/// the vectors, keeps the follow the same whatever the frame rate, however far apart the two are.
/// A gap longer than [`MAX_SMOOTHING_GAP_S`], or a `previous` along the axis, starts again from
/// `target`.
pub fn smooth(previous: Vec3, target: Vec3, axis: Vec3, dt: f32, smoothing_s: f32) -> Vec3 {
    if !(dt <= MAX_SMOOTHING_GAP_S && previous.is_finite() && target.is_finite()) {
        return target;
    }
    let (Some(axis), Some(from)) = (
        axis.try_normalize(),
        across(previous, axis.normalize_or_zero()).try_normalize(),
    ) else {
        return target;
    };
    let angle = axis.dot(from.cross(target)).atan2(from.dot(target));
    Quat::from_axis_angle(axis, angle * follow_factor(dt, smoothing_s)) * from
}

/// The part of `v` square to the unit `axis`.
fn across(v: Vec3, axis: Vec3) -> Vec3 {
    v - axis * v.dot(axis)
}

/// The body pole: its pull down, out to the arm's side, and back.
const POLE_DOWN: f32 = 1.0;
const POLE_OUT: f32 = 0.4;
const POLE_BACK: f32 = 0.3;

/// How much further out a raised hand, and a hand reaching across the body, lift the pole.
const POLE_RAISE_OUT: f32 = 1.0;
const POLE_ACROSS_OUT: f32 = 0.6;

/// The raise ramps from none with the hand this far above the shoulder (below it, negative) to
/// full over this span, metres.
const RAISE_FROM_M: f32 = -0.10;
const RAISE_SPAN_M: f32 = 0.30;

/// The reach across the body is measured against the arm's reach, or this, whichever is longer,
/// metres, so a hand near the shoulder does not swing the pole by a few centimetres.
const ACROSS_MIN_REACH_M: f32 = 0.2;

/// The share of the pole's length its part across the reach falls below for the pole straight
/// back to fade in, fully at none.
const POLE_FALLBACK_BELOW: f32 = 0.5;

/// The little-finger side of the left hand in its bone's frame, fitted to the tracked wrist
/// joint's across the 6 November 2026 session with a median residual of about a degree.
const LEFT_LITTLE_FINGER: Vec3 = Vec3::new(0.265_185_6, 0.319_041_06, -0.909_884_3);

#[cfg(test)]
mod tests {
    use std::f32::consts::{FRAC_PI_2, TAU};

    use super::*;
    use crate::controllers::frame::Hand;

    const SHOULDER_HEIGHT: f32 = 1.45;

    fn shoulder(side: Side) -> Vec3 {
        let x = match side {
            Side::Left => -0.18,
            Side::Right => 0.18,
        };
        Vec3::new(x, SHOULDER_HEIGHT, 0.0)
    }

    fn other(side: Side) -> Side {
        match side {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }

    /// The right arm's inputs with the wrist `offset` from the right shoulder.
    fn right(offset: Vec3, little_finger: Vec3) -> PoleInput {
        PoleInput {
            shoulder: shoulder(Side::Right),
            other_shoulder: shoulder(Side::Left),
            wrist: shoulder(Side::Right) + offset,
            up: Vec3::Y,
            little_finger,
        }
    }

    /// The model-space X mirror, as a hand's frame mirrors between the hands.
    fn mirror(v: Vec3) -> Vec3 {
        Hand::Right.mirror_from_left(v)
    }

    fn degrees(a: Vec3, b: Vec3) -> f32 {
        a.angle_between(b).to_degrees()
    }

    fn assert_square_unit(pole: Vec3, input: &PoleInput) {
        let axis = (input.wrist - input.shoulder).normalize();
        assert!((pole.length() - 1.0).abs() < 1e-5, "{pole:?}");
        assert!(pole.dot(axis).abs() < 1e-5, "{pole:?} against {axis:?}");
    }

    #[test]
    fn a_hand_low_in_front_hangs_the_elbow_down_out_and_back() {
        // Thumb up: the little finger points down.
        let input = right(Vec3::new(0.05, -0.35, -0.3), Vec3::NEG_Y);
        let p = pole(&input, Side::Right, DEFAULT_PALM_WEIGHT).unwrap();
        assert_square_unit(p, &input);
        assert!(p.y < -0.5, "down: {p:?}");
        assert!(p.x > 0.15, "out: {p:?}");
        assert!(p.z > 0.0, "back: {p:?}");
    }

    #[test]
    fn a_raised_hand_lifts_the_elbow_out_to_the_side() {
        // Palm forward overhead: the little finger points out.
        let low = pole(
            &right(Vec3::new(0.05, -0.35, -0.3), Vec3::X),
            Side::Right,
            DEFAULT_PALM_WEIGHT,
        )
        .unwrap();
        let input = right(Vec3::new(0.05, 0.3, -0.35), Vec3::X);
        let raised = pole(&input, Side::Right, DEFAULT_PALM_WEIGHT).unwrap();
        assert_square_unit(raised, &input);
        assert!(raised.x > 0.8, "out: {raised:?}");
        assert!(raised.x > low.x);
    }

    #[test]
    fn a_hand_reaching_across_the_body_lifts_the_elbow_out() {
        // Palm down across the chest: the little finger points forward and out.
        let little = Vec3::new(0.3, 0.0, -1.0).normalize();
        let level = pole(
            &right(Vec3::new(0.05, -0.2, -0.4), little),
            Side::Right,
            DEFAULT_PALM_WEIGHT,
        )
        .unwrap();
        let input = right(Vec3::new(-0.4, -0.2, -0.3), little);
        let across = pole(&input, Side::Right, DEFAULT_PALM_WEIGHT).unwrap();
        assert_square_unit(across, &input);
        // The elbow lifts further out than with the hand in front of its shoulder.
        assert!(across.x > 0.5, "out: {across:?}");
        assert!(across.x > level.x, "{across:?} against {level:?}");
    }

    #[test]
    fn a_hand_behind_the_hip_points_the_elbow_back() {
        let input = right(Vec3::new(0.08, -0.5, 0.2), Vec3::Z);
        let p = pole(&input, Side::Right, DEFAULT_PALM_WEIGHT).unwrap();
        assert_square_unit(p, &input);
        assert!(p.z > 0.5, "back: {p:?}");
    }

    #[test]
    fn a_reach_along_the_pole_falls_back_to_a_pole_straight_back() {
        // The reach exactly along the body pole without the palm: the body pole has no part across
        // it, and the pole straight back decides.
        let along = (-Vec3::Y + Vec3::X * POLE_OUT + Vec3::Z * POLE_BACK).normalize();
        let input = right(along * 0.45, Vec3::ZERO);
        let p = pole(&input, Side::Right, 0.0).unwrap();
        assert_square_unit(p, &input);
        let back = across(Vec3::Z, along).normalize();
        assert!(degrees(p, back) < 1e-2, "{p:?} against {back:?}");
    }

    #[test]
    fn the_left_arm_mirrors_the_right() {
        let cases = [
            (Vec3::new(0.05, -0.35, -0.3), Vec3::NEG_Y),
            (Vec3::new(0.05, 0.3, -0.35), Vec3::X),
            (
                Vec3::new(-0.4, -0.2, -0.3),
                Vec3::new(0.3, 0.0, -1.0).normalize(),
            ),
            (Vec3::new(0.08, -0.5, 0.2), Vec3::Z),
            (Vec3::new(0.2, -0.1, -0.5), Vec3::new(0.0, 0.6, 0.8)),
        ];
        for (offset, little) in cases {
            let r = right(offset, little);
            let l = PoleInput {
                shoulder: mirror(r.shoulder),
                other_shoulder: mirror(r.other_shoulder),
                wrist: mirror(r.wrist),
                up: r.up,
                little_finger: mirror(r.little_finger),
            };
            assert_eq!(l.shoulder, shoulder(Side::Left));
            let pr = pole(&r, Side::Right, DEFAULT_PALM_WEIGHT).unwrap();
            let pl = pole(&l, Side::Left, DEFAULT_PALM_WEIGHT).unwrap();
            assert!(pl.abs_diff_eq(mirror(pr), 1e-5), "{pl:?} against {pr:?}");
        }
    }

    #[test]
    fn the_hands_little_fingers_mirror_with_their_bone_frames() {
        // The right hand's bone frame is the left's mirrored with its axes reversed (the fitted
        // axes are each other's negation): a left hand turned `q` has the right twin `-M R(q)`,
        // which is a rotation, and their little fingers mirror.
        let mirror_matrix = glam::Mat3::from_diagonal(Vec3::new(-1.0, 1.0, 1.0));
        for q in [
            Quat::IDENTITY,
            Quat::from_rotation_y(0.7),
            Quat::from_euler(glam::EulerRot::XYZ, 0.3, -1.1, 2.0),
        ] {
            let right_turn = Quat::from_mat3(&(-(mirror_matrix * glam::Mat3::from_quat(q))));
            let left = little_finger(q, Side::Left);
            let right = little_finger(right_turn, Side::Right);
            assert!((left.length() - 1.0).abs() < 1e-5);
            assert!(right.abs_diff_eq(mirror(left), 1e-5), "{right:?} {left:?}");
        }
    }

    #[test]
    fn turning_the_wrist_through_the_little_finger_pull_swings_the_pole_without_a_flip() {
        // The forearm points forward and down from the elbow; the wrist rolls a full turn about it.
        let offset = Vec3::new(0.05, -0.3, -0.35);
        let axis = offset.normalize();
        let start = across(Vec3::X, axis).normalize();
        let steps = 360;
        let mut previous_raw: Option<Vec3> = None;
        let mut smoothed: Option<Vec3> = None;
        let mut history: Vec<Vec3> = vec![];
        // A full turn in a second, at 90 Hz: about four degrees a frame.
        let dt = 1.0 / 90.0;
        let per_frame = 4;
        for i in 0..=steps {
            let roll = TAU * i as f32 / steps as f32;
            let little = Quat::from_axis_angle(axis, roll) * start;
            let input = right(offset, little);
            let raw = pole(&input, Side::Right, DEFAULT_PALM_WEIGHT).unwrap();
            if let Some(before) = previous_raw {
                assert!(
                    degrees(before, raw) < 5.0,
                    "roll {i}: {before:?} to {raw:?}"
                );
            }
            previous_raw = Some(raw);
            if i % per_frame == 0 {
                let next = smoothed.map_or(raw, |s| smooth(s, raw, axis, dt, DEFAULT_SMOOTHING_S));
                smoothed = Some(next);
                history.push(next);
            }
        }
        // No swing of more than 60 degrees within 100 ms, the flip the study counted.
        let window = (0.1 / dt).round() as usize;
        for (i, a) in history.iter().enumerate() {
            for b in history.iter().skip(i + 1).take(window) {
                assert!(degrees(*a, *b) < 60.0, "{a:?} to {b:?}");
            }
        }
    }

    #[test]
    fn the_smoothing_closes_on_its_target_alike_at_any_frame_rate() {
        let axis = Vec3::new(0.1, -0.8, -0.5).normalize();
        let from = across(Vec3::X, axis).normalize();
        let to = Quat::from_axis_angle(axis, FRAC_PI_2) * from;
        let run = |hz: f32| {
            let dt = 1.0 / hz;
            let mut v = from;
            for _ in 0..(0.1 * hz).round() as usize {
                v = smooth(v, to, axis, dt, DEFAULT_SMOOTHING_S);
            }
            v
        };
        let (slow, fast) = (run(30.0), run(120.0));
        assert!(degrees(slow, fast) < 0.05, "{slow:?} against {fast:?}");
        // The follow closes the angle exponentially.
        let expected = 90.0 * (-0.1 / DEFAULT_SMOOTHING_S).exp();
        assert!((degrees(fast, to) - expected).abs() < 0.05, "{fast:?}");
        // After the time constant's worth and more, most of the way is closed.
        assert!(degrees(fast, to) < 25.0, "{fast:?}");
        assert!(degrees(fast, from) > 50.0, "{fast:?}");
        assert!(fast.dot(axis).abs() < 1e-5);
    }

    #[test]
    fn the_smoothing_starts_again_after_a_gap_and_holds_on_a_zero_step() {
        let axis = Vec3::NEG_Y;
        let (a, b) = (Vec3::X, Vec3::Z);
        assert_eq!(smooth(a, b, axis, MAX_SMOOTHING_GAP_S * 2.0, 0.06), b);
        assert_eq!(smooth(a, b, axis, f32::NAN, 0.06), b);
        assert!(smooth(a, b, axis, 0.0, 0.06).abs_diff_eq(a, 1e-6));
        // No smoothing time lands on the target at once.
        assert!(smooth(a, b, axis, 0.01, 0.0).abs_diff_eq(b, 1e-6));
        // From straight opposite it turns about the axis rather than through it.
        let turned = smooth(Vec3::NEG_X, Vec3::X, axis, 0.01, 0.06);
        assert!((turned.length() - 1.0).abs() < 1e-5 && turned.y.abs() < 1e-5);
        assert!(degrees(turned, Vec3::NEG_X) > 1.0);
        // A previous pole along the axis has no direction to turn from.
        assert_eq!(smooth(Vec3::NEG_Y, b, axis, 0.01, 0.06), b);
    }

    #[test]
    fn a_degenerate_arm_has_no_pole() {
        let mut input = right(Vec3::ZERO, Vec3::X);
        assert!(pole(&input, Side::Right, DEFAULT_PALM_WEIGHT).is_none());
        input.wrist = input.shoulder + Vec3::NEG_Y * 0.4;
        input.other_shoulder = input.shoulder + Vec3::Y;
        assert!(pole(&input, Side::Right, DEFAULT_PALM_WEIGHT).is_none());
        input.other_shoulder = shoulder(other(Side::Right));
        input.little_finger = Vec3::NAN;
        assert!(pole(&input, Side::Right, DEFAULT_PALM_WEIGHT).is_some());
    }
}
