//! Kneeling on one knee: the trailing foot's knee down, or the off hand's when neither foot trails,
//! with the hips held over it at the height the head asks for, between an upright kneel and one sat
//! back toward the heel. The kneeling shin lies on the ground behind the knee with the toes tucked
//! under, and the other foot stands ahead with its knee up.

use glam::{Quat, Vec3};

use crate::controllers::body::procedural::solver::{
    arms::Side,
    feet::FootPose,
    lower::{LowerInput, LowerRig, LowerTargets, facing, flat},
};

/// How far below the squat's floor the head must ask the hips to go before the body kneels,
/// metres.
pub(super) const KNEEL_ENTER_M: f32 = 0.05;

/// How far above the squat's floor the head must ask the hips to come for the kneel to end,
/// metres.
pub(super) const KNEEL_EXIT_M: f32 = 0.03;

/// How long the head stays low before the body kneels, seconds: a dip through the floor on the
/// way somewhere else does not kneel.
pub(super) const KNEEL_AFTER_S: f32 = 0.25;

/// How high the kneeling knee's joint sits above the ground, metres: the knee's own thickness.
pub(super) const KNEE_GROUND_M: f32 = 0.06;

/// The kneeling hips' lowest, as the share of the thigh's length the hip joint stands above the
/// kneeling knee.
pub(super) const KNEEL_LOW_THIGH: f32 = 0.55;

/// A kneel: which knee is down, and where the hips are held, in the world.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct Kneel {
    pub(super) side: Side,
    /// The hips' place on the ground, world space.
    hips: Vec3,
    /// The hips' facing when the kneel began, world space, on the ground plane.
    forward: Vec3,
    pub(super) active: bool,
}

impl Kneel {
    /// Begin a kneel from where the body stands now: the trailing foot's knee goes down, or the off
    /// hand's when neither trails, and the hips move toward it.
    pub(super) fn begin(rig: &LowerRig, input: &LowerInput) -> Self {
        let forward = Quat::from_rotation_y(input.hips_yaw) * Vec3::NEG_Z;
        let right = forward.cross(Vec3::Y);
        let along = input.feet.map(|f| f.position.dot(forward));
        let side = if (along[0] - along[1]).abs() < TRAIL_M {
            input.off_side
        } else if along[0] < along[1] {
            Side::Left
        } else {
            Side::Right
        };
        let hips = Vec3::new(input.hips_xz.x, 0.0, input.hips_xz.y)
            + right * (side.sign() * rig.hip_half_width * KNEEL_SHIFT);
        Self {
            side,
            hips: input.world.transform_point3(hips),
            forward: flat(input.world.transform_vector3(forward)),
            active: true,
        }
    }

    /// The kneel's targets this tick, model space.
    pub(super) fn targets(&self, rig: &LowerRig, input: &LowerInput, weight: f32) -> LowerTargets {
        let to_model = input.world.inverse();
        let ground = to_model.transform_point3(self.hips);
        let forward = flat(to_model.transform_vector3(self.forward));
        let right = forward.cross(Vec3::Y);
        let turn = facing(forward);
        let height = rig
            .wanted_hips(input.hips_head_y)
            .clamp(rig.kneel_low(), rig.kneel_high());
        let hips = Vec3::new(ground.x, height, ground.z);
        let down = self.side;
        let up = down.other();
        let mut feet = input.stance;

        // The kneeling knee rests on the ground ahead of its hip joint by as far as the thigh
        // reaches past straight down, and the shin lies back from it to the tucked foot.
        let hip_joint =
            hips + right * (down.sign() * rig.hip_half_width) - Vec3::Y * rig.hip_joint_drop;
        let rise = (hip_joint.y - KNEE_GROUND_M).max(0.0);
        let ahead = (rig.thigh * rig.thigh - rise * rise).max(0.0).sqrt();
        let knee = Vec3::new(hip_joint.x, KNEE_GROUND_M, hip_joint.z) + forward * ahead;
        let toe_height = (rig.toe_length * KNEEL_TOE_PITCH.sin()).max(KNEE_GROUND_M);
        let lift = toe_height - KNEE_GROUND_M;
        let back = (rig.shin * rig.shin - lift * lift).max(0.0).sqrt();
        feet[down.index()] = FootPose {
            position: knee - forward * back + Vec3::Y * lift,
            rotation: (Quat::from_axis_angle(right, -KNEEL_TOE_PITCH)
                * turn
                * input.stance[down.index()].rotation)
                .normalize(),
        };

        // The other foot stands ahead of its hip joint, its knee up.
        let ankle = input.stance[up.index()].position.y;
        let standing =
            hips + right * (up.sign() * rig.hip_half_width) + forward * (FRONT_REACH * rig.thigh);
        feet[up.index()] = FootPose {
            position: Vec3::new(standing.x, ankle, standing.z),
            rotation: (turn * input.stance[up.index()].rotation).normalize(),
        };
        LowerTargets {
            weight,
            hips,
            feet,
            poles: [forward; 2],
        }
    }
}

/// How far toward the kneeling side the hips move, as a share of half the hip width: the weight
/// goes over the knee that is down.
const KNEEL_SHIFT: f32 = 0.4;

/// How far the standing foot of a kneel is ahead of the hips, as a share of the thigh.
const FRONT_REACH: f32 = 0.85;

/// How far the kneeling foot is pitched toes-down, radians: the toes tucked under.
const KNEEL_TOE_PITCH: f32 = 1.0;

/// How far one foot must be behind the other, along the hips' facing, to be the trailing one,
/// metres.
const TRAIL_M: f32 = 0.06;
