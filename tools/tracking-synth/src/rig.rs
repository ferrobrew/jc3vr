//! The synthetic player's body: the script's tracks at one moment, posed as a skeleton, and the
//! head and controller poses read off it.
//!
//! The skeleton is a short chain. The body's vertical axis stands on the floor at the script's
//! position; the hips sit on it at hip height, less the squat; the torso pitches forward about the
//! hips by the lean and turns with the heading; the neck pivot is at the top of the torso, and the
//! eyes are above and ahead of it, turned by the head's yaw and pitch on the neck. The shoulders
//! ride the torso, and each arm hangs from its shoulder under gravity, swinging with the walk and
//! coming forward in a squat, so the hands follow the body as a player's do.
//!
//! On top of the chain go the motions nobody scripts: a relaxed stance's slow sway, the bob and
//! side-to-side of a walking gait (by the body's speed across the floor), and a hand's tremor.
//!
//! The poses come out in the cockpit frame the mod publishes: the standing head at the start is the
//! origin, facing -Z, so the floor is at minus the eye height.

use std::f32::consts::TAU;

use glam::{EulerRot, Quat, Vec3};
use tracking_format::model::Pose;

use crate::{script::Script, track::smootherstep};

/// The body of a 1.8 m player of standard proportions, metres, heights from the floor.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Dimensions {
    pub eye_height: f32,
    /// The hip joints' height.
    pub hip_height: f32,
    /// How far above the neck pivot the eyes are.
    pub eye_above_neck: f32,
    /// How far ahead of the neck pivot, and of the body's vertical axis, the eyes are.
    pub eye_ahead: f32,
    pub shoulder_height: f32,
    pub shoulder_width: f32,
    /// How far the hanging arm reaches from the shoulder joint to the grip (the palm's centre).
    pub arm_to_grip: f32,
    /// The distance between the eyes.
    pub ipd: f32,
}

/// The player the built-in scripts are written for.
pub const PLAYER: Dimensions = Dimensions {
    eye_height: 1.68,
    hip_height: 0.93,
    eye_above_neck: 0.12,
    eye_ahead: 0.09,
    shoulder_height: 1.48,
    shoulder_width: 0.40,
    // The upper arm (0.34), the forearm (0.27), and half the hand (0.08).
    arm_to_grip: 0.69,
    ipd: 0.063,
};

/// What the devices report at one moment.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RigPose {
    /// The centre eye in the cockpit frame.
    pub head: Pose,
    /// The OpenXR grip poses in the cockpit frame, indexed left, right.
    pub grips: [Pose; 2],
    /// The eyes' height above the floor.
    pub eye_height: f32,
}

/// The rig posed at `t` seconds into `script`.
pub fn solve(script: &Script, t: f32) -> RigPose {
    let body = &script.body;
    let dims = PLAYER;
    let yaw = Quat::from_rotation_y(body.yaw_deg.sample(t).to_radians());
    let torso = yaw * Quat::from_rotation_x(-body.lean_deg.sample(t).to_radians());

    let gait = gait(script, t);
    let phase = TAU * STEP_CADENCE_HZ * t;
    let bob = -GAIT_BOB_M * gait * (1.0 - phase.cos()) * 0.5;
    let side = GAIT_SIDE_M * gait * (phase * 0.5).sin();
    let sway_amount = body.sway.sample(t).max(0.0);
    let sway = sway_offset(t) * sway_amount;

    // The body's axis stands where the start puts the eyes at the origin.
    let root = Vec3::new(body.x_m.sample(t), 0.0, body.z_m.sample(t) + dims.eye_ahead);
    let hips = root
        + Vec3::Y * (dims.hip_height - body.crouch_m.sample(t) + bob)
        + yaw * (Vec3::X * side)
        + sway;

    let neck = hips + torso * (Vec3::Y * (dims.eye_height - dims.eye_above_neck - dims.hip_height));
    let head_rotation = torso
        * Quat::from_rotation_y(body.head_yaw_deg.sample(t).to_radians())
        * Quat::from_rotation_x(body.head_pitch_deg.sample(t).to_radians())
        * sway_rotation(t, sway_amount);
    let eye = neck + head_rotation * Vec3::new(0.0, dims.eye_above_neck, -dims.eye_ahead);

    let crouch_reach = (body.crouch_m.sample(t) / FULL_SQUAT_M).clamp(0.0, 1.0)
        * SQUAT_ARM_FORWARD_DEG.to_radians();
    let arm_swing = GAIT_ARM_SWING_DEG.to_radians() * gait * (phase * 0.5).sin();
    let grips = [Side::Left, Side::Right].map(|side| {
        let sign = side.sign();
        let shoulder = hips
            + torso
                * Vec3::new(
                    sign * dims.shoulder_width * 0.5,
                    dims.shoulder_height - dims.hip_height,
                    SHOULDER_BEHIND_NECK_M,
                );
        // The arms swing opposite each other, and both come forward in a squat.
        let swing = Quat::from_rotation_x(crouch_reach + sign * arm_swing);
        let hang = yaw * swing * Vec3::new(sign * HAND_OUT_M, -dims.arm_to_grip, -HAND_AHEAD_M);
        let hand = &script.hands[side.index()];
        let reach = yaw
            * Vec3::new(
                hand.reach_x_m.sample(t),
                hand.reach_y_m.sample(t),
                hand.reach_z_m.sample(t),
            );
        let tremor = tremor(t, side) * sway_amount;
        Pose {
            position: shoulder + hang + reach + tremor - Vec3::Y * dims.eye_height,
            orientation: yaw * swing * Quat::from_rotation_x(HANGING_GRIP_PITCH_DEG.to_radians()),
        }
    });

    RigPose {
        head: Pose {
            position: eye - Vec3::Y * dims.eye_height,
            orientation: head_rotation,
        },
        grips,
        eye_height: eye.y,
    }
}

/// The lean and squat that carry the standing head `forward_m` ahead and `down_m` lower by a hinge
/// at the hips, with the head held level with the torso. The lean pitches the head with the
/// torso; the squat makes up what the hinge alone does not lower it.
pub fn hip_hinge(forward_m: f32, down_m: f32) -> (f32, f32) {
    let dims = PLAYER;
    let above = dims.eye_height - dims.hip_height;
    let ahead = dims.eye_ahead;
    let radius = above.hypot(ahead);
    let start = ahead.atan2(above);
    let lean = ((forward_m + ahead) / radius).clamp(-1.0, 1.0).asin() - start;
    let hinge_drop = above * (1.0 - lean.cos()) + ahead * lean.sin();
    (lean.to_degrees(), down_m - hinge_drop)
}

/// The aim pose of an Index controller held at `grip`: a ray from the controller's tip, pitched
/// down from the handle's axis.
pub fn aim_from_grip(grip: Pose) -> Pose {
    Pose {
        position: grip.position + grip.orientation * AIM_OFFSET_M,
        orientation: grip.orientation * Quat::from_rotation_x(AIM_PITCH_DEG.to_radians()),
    }
}

/// The two eyes of a head at `head`, indexed left, right.
pub fn eyes(head: Pose) -> [Pose; 2] {
    let half = PLAYER.ipd * 0.5;
    [-half, half].map(|x| Pose {
        position: head.position + head.orientation * Vec3::new(x, 0.0, 0.0),
        orientation: head.orientation,
    })
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Side {
    Left,
    Right,
}

impl Side {
    const fn sign(self) -> f32 {
        match self {
            Side::Left => -1.0,
            Side::Right => 1.0,
        }
    }

    const fn index(self) -> usize {
        match self {
            Side::Left => 0,
            Side::Right => 1,
        }
    }
}

/// How fully the body walks, `0..=1`, by its speed across the floor. It eases into the full gait
/// rather than being cut off at it, so the bob and the arm swing have no kink in their rates.
fn gait(script: &Script, t: f32) -> f32 {
    let at = |t: f32| Vec3::new(script.body.x_m.sample(t), 0.0, script.body.z_m.sample(t));
    let speed = (at(t + SPEED_H_S) - at(t - SPEED_H_S)).length() / (2.0 * SPEED_H_S);
    smootherstep(speed / FULL_GAIT_SPEED_MPS)
}

/// A relaxed stance's drift of the hips, a sum of slow incommensurate waves.
fn sway_offset(t: f32) -> Vec3 {
    let wave = |amplitude: f32, hz: f32, phase: f32| amplitude * (TAU * hz * t + phase).sin();
    Vec3::new(
        wave(0.006, 0.23, 0.0) + wave(0.003, 0.61, 1.3),
        wave(0.002, 0.31, 0.4),
        wave(0.008, 0.17, 0.7) + wave(0.003, 0.53, 2.1),
    )
}

/// The head's small wander on the neck that goes with the sway.
fn sway_rotation(t: f32, amount: f32) -> Quat {
    let wave = |degrees: f32, hz: f32, phase: f32| {
        (degrees * amount).to_radians() * (TAU * hz * t + phase).sin()
    };
    Quat::from_euler(
        EulerRot::YXZ,
        wave(0.8, 0.13, 0.2),
        wave(0.6, 0.19, 1.1),
        wave(0.3, 0.29, 2.5),
    )
}

/// A hand's tremor, different on each side.
fn tremor(t: f32, side: Side) -> Vec3 {
    let phase = side.index() as f32 * 1.7;
    let wave = |hz: f32, offset: f32| 0.002 * (TAU * hz * t + phase + offset).sin();
    Vec3::new(wave(1.3, 0.0), wave(1.7, 0.9), wave(1.1, 2.3))
}

/// The steps per second of a walking gait.
const STEP_CADENCE_HZ: f32 = 1.8;
/// How far the head drops between steps at a full gait.
const GAIT_BOB_M: f32 = 0.03;
/// How far the hips shift side to side over a stride at a full gait.
const GAIT_SIDE_M: f32 = 0.015;
/// How far an arm swings each way about its shoulder at a full gait.
const GAIT_ARM_SWING_DEG: f32 = 15.0;
/// The speed across the floor at which the gait is full.
const FULL_GAIT_SPEED_MPS: f32 = 1.0;
/// The interval the body's speed is measured over, each side of the moment.
const SPEED_H_S: f32 = 0.01;
/// The squat at which the arms have come fully forward.
const FULL_SQUAT_M: f32 = 0.45;
/// How far forward the arms come in a full squat, for balance.
const SQUAT_ARM_FORWARD_DEG: f32 = 50.0;
/// How far behind the neck pivot the shoulder joints are.
const SHOULDER_BEHIND_NECK_M: f32 = 0.02;
/// How far out from the shoulder joint a hanging hand is.
const HAND_OUT_M: f32 = 0.03;
/// How far ahead of the shoulder joint a hanging hand is.
const HAND_AHEAD_M: f32 = 0.05;
/// The pitch of a hanging hand's grip: the fist's axis tips a little down.
const HANGING_GRIP_PITCH_DEG: f32 = -10.0;
/// Where the aim pose's origin is in the grip's frame.
const AIM_OFFSET_M: Vec3 = Vec3::new(0.0, -0.01, -0.06);
/// The aim pose's pitch from the grip's.
const AIM_PITCH_DEG: f32 = -40.0;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{script::BodyTracks, track::Track};

    fn still() -> Script {
        Script {
            name: "test",
            summary: "",
            body: BodyTracks {
                sway: Track::start(0.0),
                ..BodyTracks::default()
            },
            hands: Default::default(),
        }
    }

    fn close(a: Vec3, b: Vec3, tolerance: f32) -> bool {
        (a - b).length() <= tolerance
    }

    #[test]
    fn the_standing_head_is_the_origin_at_eye_height() {
        let pose = solve(&still(), 0.0);
        assert!(close(pose.head.position, Vec3::ZERO, 1e-5), "{pose:?}");
        assert!(pose.head.orientation.angle_between(Quat::IDENTITY) < 1e-5);
        assert!((pose.eye_height - PLAYER.eye_height).abs() < 1e-5);
    }

    #[test]
    fn the_hanging_hands_are_at_the_sides_below_the_hips() {
        let pose = solve(&still(), 0.0);
        let [left, right] = pose.grips;
        assert!(left.position.x < -0.2 && right.position.x > 0.2, "{pose:?}");
        // The palms of hanging arms are a little below the hip joints.
        let palm_height = left.position.y + PLAYER.eye_height;
        assert!((0.75..0.85).contains(&palm_height), "{palm_height}");
        assert_eq!(left.position.y, right.position.y);
        // Both handles point ahead, tipped a little down.
        let forward = left.orientation * Vec3::NEG_Z;
        assert!(forward.z < -0.95 && forward.y < 0.0, "{forward}");
    }

    #[test]
    fn a_hip_hinge_carries_the_head_where_it_was_asked() {
        let (lean, crouch) = hip_hinge(0.35, 0.20);
        assert!(crouch >= 0.0, "{crouch}");
        let mut script = still();
        script.body.lean_deg = Track::start(lean);
        script.body.crouch_m = Track::start(crouch);
        let head = solve(&script, 0.0).head.position;
        assert!(close(head, Vec3::new(0.0, -0.20, -0.35), 1e-4), "{head}");
    }

    #[test]
    fn a_turn_swings_the_head_round_the_body_and_the_hands_with_it() {
        let mut script = still();
        script.body.yaw_deg = Track::start(90.0);
        let pose = solve(&script, 0.0);
        // Facing -X after a left turn: the eyes are ahead of the body's axis, which stands where
        // it did behind the start's head.
        let axis = Vec3::new(0.0, 0.0, PLAYER.eye_ahead);
        let expected = axis + Vec3::new(-PLAYER.eye_ahead, 0.0, 0.0);
        assert!(close(pose.head.position, expected, 1e-4), "{pose:?}");
        assert!((pose.head.orientation * Vec3::NEG_Z).x < -0.99);
        // The left hand is now on the +Z side, the right on the -Z side.
        assert!(pose.grips[0].position.z > pose.grips[1].position.z + 0.3);
    }

    #[test]
    fn looking_down_pitches_the_head_and_not_the_hands() {
        let mut script = still();
        script.body.head_pitch_deg = Track::start(-60.0);
        let pose = solve(&script, 0.0);
        let gaze = pose.head.orientation * Vec3::NEG_Z;
        assert!(
            (gaze.y - (-60f32).to_radians().sin()).abs() < 1e-4,
            "{gaze}"
        );
        assert_eq!(pose.grips, solve(&still(), 0.0).grips);
    }

    #[test]
    fn walking_bobs_the_head_and_standing_does_not() {
        let mut script = still();
        script.body.z_m = Track::start(0.0).ease(10.0, -10.0);
        let heights: Vec<f32> = (0..100)
            .map(|i| solve(&script, 4.0 + i as f32 * 0.02).eye_height)
            .collect();
        let range = heights.iter().copied().fold(f32::MIN, f32::max)
            - heights.iter().copied().fold(f32::MAX, f32::min);
        assert!(range > 0.02, "{range}");
        let standing = solve(&still(), 4.3).eye_height;
        assert!((standing - PLAYER.eye_height).abs() < 1e-5);
    }

    #[test]
    fn the_eyes_straddle_the_head() {
        let head = Pose {
            position: Vec3::new(1.0, 2.0, 3.0),
            orientation: Quat::from_rotation_y(90f32.to_radians()),
        };
        let [left, right] = eyes(head);
        assert!((left.position.distance(right.position) - PLAYER.ipd).abs() < 1e-5);
        assert!(close(
            (left.position + right.position) * 0.5,
            head.position,
            1e-5
        ));
        // Facing -X, the left eye is on the +Z side.
        assert!(left.position.z > right.position.z);
    }
}
