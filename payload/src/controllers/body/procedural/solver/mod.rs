//! The procedural body's solver: Rico's body built under the player's head. Pure, and in Rico's
//! model space (+Y up, facing -Z, the left side at -X, the root at the feet).
//!
//! The solve starts from the animated pose and changes it in order:
//!
//! 0. The clip's own shape is settled out ([`settle`]): the hips' pitch and roll, which a run
//!    swings with every step, and most of the spine's bend, which the clip's posture carries,
//!    give way to the reference pose's, so the body's shape is the player's rather than the
//!    clip's. The walk's sway comes from the hips' draw toward the planted foot instead.
//! 1. The hips ([`hips`]): their height follows the head down (the legs take a drop first,
//!    to a deep squat), and they sit over the stance the root carries ([`hips_horizontal`]), not
//!    over the feet, which leave it with every step, pulled back as the head leans forward so a
//!    lean becomes a hip hinge, and drawn a little toward the one foot on the ground while the
//!    other swings. The caller follows that place over time. They turn to the player's torso,
//!    within a limit. They drop further when a foot is out of the legs' reach ([`reach_drop`]): a
//!    stride, a slope, a stair.
//! 2. The spine ([`spine`]): the chest twists to the torso and pitches forward by a share of
//!    the head's look down (so looking at the belt shows the belt), the spine curls forward when
//!    the head is closer to the hips than the spine is long (a squat past the legs' share), and
//!    the spine swings so the neck reaches the point under the player's head, the swing shared out
//!    along the four spine bones. The neck then takes a share of the head's turn against the chest
//!    ([`spine`]), so a nod or a look aside bends the neck as well as the head.
//! 3. The legs ([`two_bone::solve`]): each foot goes back to its target, the knee bending toward
//!    the foot's forward blended with the hips', never backward, and takes the foot's own
//!    orientation.
//!
//! The clavicles then turn toward the hands ([`clavicles`]), after the spine has placed them.
//!
//! Everything is a rotation composed onto the animated bones or a translation of them, so no bone's
//! rest frame needs to be known.

pub mod air;
pub mod arms;
pub mod clavicles;
pub mod feet;
pub mod gait;
pub mod hips;
pub mod lower;
pub mod pose;
pub mod spine;
pub mod two_bone;

use glam::{Quat, Vec2, Vec3};
pub use hips::{
    DropFollower, HipsFollower, crouch_depth, hips_height, hips_horizontal, hips_room, hips_yaw,
    reach_drop,
};

use crate::controllers::body::procedural::{
    config::ProceduralBodyConfig,
    rig::{Rig, RigBone},
    solver::{
        hips::{hips_input, place_hips},
        lower::LowerTargets,
        pose::RigPose,
        spine::{ChestTurn, bend_spine, chest_pitch, turn_neck},
    },
};

/// One solve's inputs, model space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SolveInput {
    /// The animated pose.
    pub animated: RigPose,
    /// Where the head bone should be: the player's head.
    pub head: Vec3,
    /// The player's head rotation relative to the body (identity faces forward).
    pub head_rotation: Quat,
    /// The player's torso heading relative to Rico's facing, radians about +Y, when estimated.
    pub torso_yaw: Option<f32>,
    /// Where each foot (the ankle) should be, left then right.
    pub feet: [Vec3; 2],
    /// How each foot should be turned, model space, left then right.
    pub foot_rotations: [Quat; 2],
    /// The middle of the neutral stance the root carries, model space: where the hips stand over.
    pub support: Vec3,
    /// Which feet are on the ground, left then right: with one down and one swinging, the hips are
    /// drawn a little toward the one down.
    pub planted: [bool; 2],
    /// How far each heel is lifted about its toes, radians, left then right.
    pub heel_lifts: [f32; 2],
    /// Where the hips sit horizontally, model space `(x, z)`, as the caller followed
    /// [`hips_horizontal`] over time; `None` takes it as it is this tick.
    pub hips_xz: Option<Vec2>,
    /// How far the hips drop below where the head puts them so the legs reach the feet, metres:
    /// [`reach_drop`], followed over time by [`DropFollower`].
    pub hips_drop: f32,
    /// A held pose of the lower body (a kneel), mixed over the hips and the knees by its weight;
    /// [`SolveInput::hold`] mixes its feet into [`feet`](Self::feet).
    pub lower: Option<LowerTargets>,
}

impl SolveInput {
    /// Take the held pose `targets` by its weight: its feet replace the feet by the weight now, and
    /// the solve mixes its hips and knees by the same. A held foot does not roll onto its toes.
    pub fn hold(&mut self, targets: LowerTargets) {
        let w = targets.weight.clamp(0.0, 1.0);
        for side in 0..2 {
            self.feet[side] = self.feet[side].lerp(targets.feet[side].position, w);
            self.foot_rotations[side] = self.foot_rotations[side]
                .slerp(targets.feet[side].rotation, w)
                .normalize();
            self.heel_lifts[side] *= 1.0 - w;
        }
        self.lower = Some(targets);
    }
}

/// Solve the body.
pub fn solve(rig: &Rig, input: &SolveInput, config: &ProceduralBodyConfig) -> RigPose {
    let mut pose = input.animated;
    let torso_yaw = input.torso_yaw.filter(|y| y.is_finite()).unwrap_or(0.0);
    settle(rig, &mut pose);
    place_hips(
        rig,
        &mut pose,
        &hips_input(rig, input, config, input.hips_drop),
        config,
    );
    let neck_target = input.head
        + input.head_rotation
            * (input.animated.position(RigBone::Neck) - input.animated.position(RigBone::Head));
    let chest_limit = config.chest_turn_limit_deg.max(0.0).to_radians();
    let chest_yaw = torso_yaw.clamp(-chest_limit, chest_limit);
    let chest = ChestTurn {
        yaw: chest_yaw,
        pitch: chest_pitch(input.head_rotation),
    };
    bend_spine(&mut pose, neck_target, chest, config);
    turn_neck(rig, &mut pose, input.head_rotation);
    let hips_forward = line_yaw(
        pose.position(RigBone::LeftUpLeg),
        pose.position(RigBone::RightUpLeg),
    )
    .map_or(Vec3::NEG_Z, |yaw| Quat::from_rotation_y(yaw) * Vec3::NEG_Z);
    for (side, (up, leg, foot)) in LEGS.into_iter().enumerate() {
        let own = knee_pole(rig, foot, input.foot_rotations[side], hips_forward);
        let pole = input.lower.map_or(own, |held| {
            own.lerp(held.poles[side], held.weight.clamp(0.0, 1.0))
                .try_normalize()
                .unwrap_or(own)
        });
        let lift = heel_lift(
            rig,
            side,
            input.feet[side],
            input.foot_rotations[side],
            input.heel_lifts[side],
        );
        place_leg(&mut pose, [up, leg, foot], lift.ankle, lift.rotation, pole);
        // The toes stay flat on the ground while the heel rolls up over them.
        let toe = TOES[side];
        let pivot = pose.position(toe);
        pose.rotate_about(toe, lift.turn.inverse(), pivot);
    }
    if pose.is_finite() {
        pose
    } else {
        input.animated
    }
}

/// The yaw of the horizontal line from `left` to `right` about +Y, measured from +X: the facing of
/// a pair of bones on either side of the body.
pub fn line_yaw(left: Vec3, right: Vec3) -> Option<f32> {
    let line = Vec2::new(right.x - left.x, right.z - left.z);
    (line.length_squared() > 1e-8).then(|| (-line.y).atan2(line.x))
}

/// The legs' bones, left then right: the thigh, the shin, and the foot.
const LEGS: [(RigBone, RigBone, RigBone); 2] = [
    (RigBone::LeftUpLeg, RigBone::LeftLeg, RigBone::LeftFoot),
    (RigBone::RightUpLeg, RigBone::RightLeg, RigBone::RightFoot),
];

/// A foot rolled up onto its toes.
struct HeelLift {
    /// Where the ankle goes.
    ankle: Vec3,
    /// How the foot is turned.
    rotation: Quat,
    /// The roll, model space: the toes take it back.
    turn: Quat,
}

/// The foot on `side`, its ankle at `ankle` turned `rotation`, rolled up about its toes by `lift`
/// radians: the toes stay where they were and the heel rises.
fn heel_lift(rig: &Rig, side: usize, ankle: Vec3, rotation: Quat, lift: f32) -> HeelLift {
    let flat = HeelLift {
        ankle,
        rotation,
        turn: Quat::IDENTITY,
    };
    if !(lift.is_finite() && lift > 1e-4) {
        return flat;
    }
    let (_, _, foot) = LEGS[side];
    let reference = rig.reference(foot);
    let toes = rig.reference(TOES[side]).translation - reference.translation;
    let carried = rotation * reference.rotation.inverse();
    let pivot = ankle + carried * toes;
    let axis = carried * Vec3::X;
    let turn = Quat::from_axis_angle(axis, -lift);
    let lifted = HeelLift {
        ankle: pivot + turn * (ankle - pivot),
        rotation: (turn * rotation).normalize(),
        turn,
    };
    if lifted.ankle.is_finite() && lifted.rotation.is_finite() {
        lifted
    } else {
        flat
    }
}

/// The toes of each leg, left then right.
const TOES: [RigBone; 2] = [RigBone::LeftToeBase, RigBone::RightToeBase];

/// Where a knee points: the foot's forward, from its orientation against the reference pose's
/// (which faces -Z), blended evenly with the hips' forward, on the ground plane.
fn knee_pole(rig: &Rig, foot: RigBone, foot_rotation: Quat, hips_forward: Vec3) -> Vec3 {
    let turn = foot_rotation * rig.reference(foot).rotation.inverse();
    let foot_forward = turn * Vec3::NEG_Z;
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z).normalize_or_zero();
    let pole = flat(foot_forward) + flat(hips_forward);
    if pole.length_squared() > 1e-6 && pole.is_finite() {
        pole.normalize()
    } else {
        flat(hips_forward)
    }
}

/// Bring a leg's foot to `target`, keeping the foot's model-space orientation `foot_rotation`, the
/// knee bending toward `pole`.
fn place_leg(
    pose: &mut RigPose,
    [up, leg, foot]: [RigBone; 3],
    target: Vec3,
    foot_rotation: Quat,
    pole: Vec3,
) {
    let root = pose.position(up);
    let middle = pose.position(leg);
    let end = pose.position(foot);
    let rotations = two_bone::solve(root, middle, end, target, pole);
    pose.rotate_about(up, rotations.upper, root);
    let knee = pose.position(leg);
    pose.rotate_about(leg, rotations.lower, knee);
    let mut placed = *pose.get(foot);
    placed.rotation = foot_rotation;
    pose.set(foot, placed);
}

/// Settle the clip's own shape out of `pose`: the hips' pitch and roll go entirely, keeping their
/// turn about +Y, and the spine and the neck go [`SPINE_SETTLE`] of the way to the reference pose's
/// shape on the hips, so a run's bob and the idle's slouch are not the body's.
fn settle(rig: &Rig, pose: &mut RigPose) {
    let hips = *pose.get(RigBone::Hips);
    let delta = hips.rotation * rig.reference(RigBone::Hips).rotation.inverse();
    let swing = delta * yaw_part(delta).inverse();
    if swing.is_finite() {
        pose.rotate_about(RigBone::Hips, swing.inverse(), hips.translation);
    }
    for bone in [
        RigBone::Spine,
        RigBone::Spine1,
        RigBone::Spine2,
        RigBone::Sternum,
        RigBone::Neck,
    ] {
        let Some(parent) = bone.parent() else {
            continue;
        };
        let reference_local =
            rig.reference(parent).rotation.inverse() * rig.reference(bone).rotation;
        let wanted = pose.get(parent).rotation * reference_local;
        let current = pose.get(bone).rotation;
        let full = (wanted * current.inverse()).normalize();
        let part = Quat::IDENTITY.slerp(full, SPINE_SETTLE);
        if part.is_finite() {
            let pivot = pose.position(bone);
            pose.rotate_about(bone, part, pivot);
        }
    }
}

/// The turn of `rotation` about +Y alone: the twist part of its swing-twist split.
fn yaw_part(rotation: Quat) -> Quat {
    let length = (rotation.y * rotation.y + rotation.w * rotation.w).sqrt();
    if length > 1e-6 && length.is_finite() {
        Quat::from_xyzw(0.0, rotation.y / length, 0.0, rotation.w / length)
    } else {
        Quat::IDENTITY
    }
}

/// How much of the reference pose's spine shape the clip's gives way to.
const SPINE_SETTLE: f32 = 0.8;

fn wrap(angle: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    (angle + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::body::procedural::{
        rig::tests::skeleton,
        solver::hips::{MAX_LEAN_M, SQUAT_BACK},
    };

    fn setup() -> (Rig, SolveInput, ProceduralBodyConfig) {
        let rig = Rig::from_skeleton(&skeleton()).unwrap();
        let animated = RigPose {
            bones: rig.reference,
        };
        let input = SolveInput {
            animated,
            head: animated.position(RigBone::Head),
            head_rotation: Quat::IDENTITY,
            torso_yaw: Some(0.0),
            feet: [
                animated.position(RigBone::LeftFoot),
                animated.position(RigBone::RightFoot),
            ],
            foot_rotations: [
                animated.get(RigBone::LeftFoot).rotation,
                animated.get(RigBone::RightFoot).rotation,
            ],
            support: (animated.position(RigBone::LeftFoot) + animated.position(RigBone::RightFoot))
                * 0.5,
            planted: [true; 2],
            heel_lifts: [0.0; 2],
            hips_xz: None,
            hips_drop: 0.0,
            lower: None,
        };
        (rig, input, ProceduralBodyConfig::new())
    }

    #[test]
    fn the_neutral_head_leaves_the_reference_pose_standing() {
        let (rig, input, config) = setup();
        let pose = solve(&rig, &input, &config);
        for bone in RigBone::ALL {
            assert!(
                pose.position(bone)
                    .abs_diff_eq(input.animated.position(bone), 1e-3),
                "{bone:?} moved to {:?}",
                pose.position(bone)
            );
        }
    }

    #[test]
    fn a_lowered_head_bends_the_knees_and_keeps_the_feet() {
        let (rig, mut input, config) = setup();
        input.head.y -= 0.3;
        let pose = solve(&rig, &input, &config);
        assert!((pose.position(RigBone::Hips).y - 0.7).abs() < 1e-3);
        for (side, foot) in [RigBone::LeftFoot, RigBone::RightFoot]
            .into_iter()
            .enumerate()
        {
            assert!(pose.position(foot).abs_diff_eq(input.feet[side], 2e-3));
        }
        // The test legs bend forward at the knee, as the shipped ones do.
        assert!(pose.position(RigBone::LeftLeg).z < input.animated.position(RigBone::LeftLeg).z);
        assert!(
            pose.position(RigBone::Neck)
                .distance(input.head - Vec3::Y * 0.1)
                < 0.02
        );
    }

    #[test]
    fn a_forward_lean_hinges_the_hips_back() {
        let (rig, mut input, config) = setup();
        input.head += Vec3::new(0.0, -0.1, -0.3);
        let pose = solve(&rig, &input, &config);
        assert!(
            pose.position(RigBone::Hips).z > 0.0,
            "the hips did not move back"
        );
        // The target is a little beyond the spine's length, so the neck points at it.
        let neck_target = input.head - Vec3::Y * 0.1;
        let base = pose.position(RigBone::Spine);
        let aim = (pose.position(RigBone::Neck) - base).angle_between(neck_target - base);
        assert!(
            aim.to_degrees() < 3.0,
            "the neck is {} degrees off",
            aim.to_degrees()
        );
    }

    #[test]
    fn the_torso_turn_twists_the_hips_within_their_limit() {
        let (rig, mut input, config) = setup();
        input.torso_yaw = Some(1.2);
        let pose = solve(&rig, &input, &config);
        let hips = line_yaw(
            pose.position(RigBone::LeftUpLeg),
            pose.position(RigBone::RightUpLeg),
        )
        .unwrap();
        assert!((hips - config.pelvis_turn_limit_deg.to_radians()).abs() < 1e-3);
        let chest = line_yaw(
            pose.position(RigBone::LeftShoulder),
            pose.position(RigBone::RightShoulder),
        )
        .unwrap();
        assert!(chest > hips);
    }

    #[test]
    fn a_foot_out_of_reach_drops_the_hips_until_it_reaches() {
        let (rig, mut input, config) = setup();
        assert!(reach_drop(&rig, &input, &config) < 1e-4);
        // A long stride: the front foot ahead and the back foot behind.
        input.feet[0].z -= 0.45;
        input.feet[1].z += 0.45;
        let drop = reach_drop(&rig, &input, &config);
        assert!(drop > 0.02, "{drop}");
        input.hips_drop = drop;
        let pose = solve(&rig, &input, &config);
        for (side, foot) in [RigBone::LeftFoot, RigBone::RightFoot]
            .into_iter()
            .enumerate()
        {
            assert!(
                pose.position(foot).distance(input.feet[side]) < 2e-3,
                "{foot:?} at {:?}",
                pose.position(foot)
            );
        }
    }

    #[test]
    fn a_head_below_the_lowest_hips_curls_the_spine() {
        let (rig, mut input, config) = setup();
        // The hips stop at half their height; the head goes a little further down.
        input.head.y = 1.65 - 0.5 - 0.03;
        let pose = solve(&rig, &input, &config);
        assert!((pose.position(RigBone::Hips).y - 0.5).abs() < 1e-3);
        let neck_target = input.head - Vec3::Y * 0.1;
        let base = pose.position(RigBone::Spine);
        let reach = (pose.position(RigBone::Neck) - base).length();
        assert!(
            (reach - (neck_target - base).length()).abs() < 0.02,
            "the neck is at {:?}",
            pose.position(RigBone::Neck)
        );
        assert!((pose.position(RigBone::Neck) - base).angle_between(neck_target - base) < 0.05);
        // Far lower, the spine curls only as far as it bends.
        input.head.y = 0.5;
        let pose = solve(&rig, &input, &config);
        assert!(pose.is_finite() && pose != input.animated);
        assert!(pose.position(RigBone::Neck).y > input.head.y);
    }

    #[test]
    fn a_step_does_not_move_the_hips() {
        let (rig, mut input, config) = setup();
        let before = hips_horizontal(&rig, &input, &config);
        // One foot lifts and swings ahead: the stance the root carries has not moved.
        input.feet[0].z -= 0.4;
        input.feet[0].y += 0.1;
        let during = hips_horizontal(&rig, &input, &config);
        assert!(during.abs_diff_eq(before, 1e-6), "{before} -> {during}");
        // With the other foot down alone, the hips lean over it, and only a little.
        input.planted = [false, true];
        let swaying = hips_horizontal(&rig, &input, &config);
        assert!(swaying.x > before.x && swaying.x - before.x < 0.03);
        assert!((swaying.y - before.y).abs() < 1e-6);
    }

    #[test]
    fn a_far_reach_counts_only_as_a_capped_lean() {
        let (rig, mut input, config) = setup();
        input.head.z -= 3.0;
        let place = hips_horizontal(&rig, &input, &config);
        assert!(
            (place.y - MAX_LEAN_M * config.counterbalance).abs() < 1e-4,
            "{place}"
        );
    }

    #[test]
    fn a_crouch_leaves_the_hips_less_room_to_drop() {
        let (rig, mut input, config) = setup();
        let standing = hips_room(&rig, input.head.y, &config);
        assert!((standing - 0.5).abs() < 1e-4, "{standing}");
        input.head.y -= 0.3;
        assert!((hips_room(&rig, input.head.y, &config) - 0.2).abs() < 1e-4);
        input.head.y -= 1.0;
        assert_eq!(hips_room(&rig, input.head.y, &config), 0.0);
        // A long stride in the crouch drops the hips no lower than their floor.
        input.head.y += 1.0;
        input.feet[0].z -= 0.6;
        input.feet[1].z += 0.6;
        input.hips_drop = reach_drop(&rig, &input, &config);
        assert!(input.hips_drop <= 0.2 + 1e-4);
        let pose = solve(&rig, &input, &config);
        assert!(pose.position(RigBone::Hips).y >= 0.5 - 1e-3);
    }

    #[test]
    fn looking_down_pitches_the_chest_and_the_neck_takes_a_share() {
        let (rig, input, config) = setup();
        let level = solve(&rig, &input, &config);
        let mut down = input;
        down.head_rotation = Quat::from_rotation_x(-0.8);
        let looking = solve(&rig, &down, &config);
        // The chest bends forward against the lower back.
        let bend = |pose: &RigPose| {
            let chest = (pose.position(RigBone::Neck) - pose.position(RigBone::Spine2)).normalize();
            let lower =
                (pose.position(RigBone::Spine1) - pose.position(RigBone::Spine)).normalize();
            chest.z - lower.z
        };
        assert!(
            bend(&looking) < bend(&level) - 0.05,
            "the chest did not pitch forward: {} vs {}",
            bend(&looking),
            bend(&level)
        );
        let neck_turn = looking
            .get(RigBone::Neck)
            .rotation
            .angle_between(level.get(RigBone::Neck).rotation);
        assert!(neck_turn > 0.1, "{neck_turn}");
        // Looking up leaves the chest.
        assert_eq!(chest_pitch(Quat::from_rotation_x(0.6)), 0.0);
    }

    #[test]
    fn a_lifted_heel_rolls_the_foot_over_flat_toes() {
        let (rig, mut input, config) = setup();
        input.heel_lifts = [0.4, 0.0];
        // A long stride back, so the leg reaches.
        input.feet[0].z += 0.2;
        input.hips_drop = reach_drop(&rig, &input, &config);
        let flat = {
            let mut i = input;
            i.heel_lifts = [0.0; 2];
            solve(&rig, &i, &config)
        };
        let pose = solve(&rig, &input, &config);
        let toes = |p: &RigPose| p.position(RigBone::LeftToeBase);
        assert!(toes(&pose).distance(toes(&flat)) < 2e-3, "the toes moved");
        assert!(pose.position(RigBone::LeftFoot).y > flat.position(RigBone::LeftFoot).y + 0.03);
        let toe_turn = pose
            .get(RigBone::LeftToeBase)
            .rotation
            .angle_between(flat.get(RigBone::LeftToeBase).rotation);
        assert!(toe_turn < 1e-3, "the toes did not stay flat: {toe_turn}");
    }

    #[test]
    fn a_squat_sends_the_hips_back() {
        let (rig, mut input, config) = setup();
        let standing = hips_horizontal(&rig, &input, &config);
        input.head.y -= 0.4;
        let squatting = hips_horizontal(&rig, &input, &config);
        assert!(
            (squatting.y - standing.y - 0.4 * SQUAT_BACK).abs() < 1e-4,
            "{squatting}"
        );
    }

    #[test]
    fn the_clips_hip_pitch_and_spine_bend_are_settled_out() {
        let (rig, mut input, config) = setup();
        let hips = input.animated.position(RigBone::Hips);
        input
            .animated
            .rotate_about(RigBone::Hips, Quat::from_rotation_x(0.3), hips);
        let spine = input.animated.position(RigBone::Spine1);
        input
            .animated
            .rotate_about(RigBone::Spine1, Quat::from_rotation_x(0.4), spine);
        let mut pose = input.animated;
        settle(&rig, &mut pose);
        let hips_tilt = pose
            .get(RigBone::Hips)
            .rotation
            .angle_between(rig.reference(RigBone::Hips).rotation);
        assert!(hips_tilt < 1e-4, "{hips_tilt}");
        let bend = pose
            .get(RigBone::Spine1)
            .rotation
            .angle_between(pose.get(RigBone::Spine).rotation);
        assert!(bend < 0.4 * (1.0 - SPINE_SETTLE) + 1e-3, "{bend}");
        // The solve from it still stands the reference pose up.
        let solved = solve(&rig, &input, &config);
        assert!(solved.is_finite());
    }

    #[test]
    fn non_finite_input_returns_the_animation() {
        let (rig, mut input, config) = setup();
        input.head = Vec3::NAN;
        assert_eq!(solve(&rig, &input, &config), input.animated);
    }
}
