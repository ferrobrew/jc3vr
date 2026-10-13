//! The torso on the animation's hips: the spine, the neck, and the clavicles posed from the
//! player's head while the animation keeps the hips (a vehicle seat, the parachute's harness), and
//! the legs hanging from them under the parachute. Pure, in Rico's model space.
//!
//! Where the animation places the hips, the clip's posture is the base the player's head moves
//! from: a bike's lean into a turn, a driver's reach for the wheel. [`solve_anchored`] keeps the
//! hips where the clip has them and the clip's shape of the spine, and changes the spine only by
//! the player's own motion:
//!
//! 1. The chest twists toward the player's torso, by the torso's turn against Rico's facing,
//!    within `chest_turn_limit_deg`, about the clip's own spine and shared out up it as on foot. A
//!    spine far from upright (lying in a swim) takes less of the twist, none lying flat, since the
//!    torso's heading is a turn about the vertical.
//! 2. The spine swings so the neck reaches the point under the player's head, shared out along the
//!    four spine bones and repeated to converge, never past `lean_limit_deg` from the clip's own
//!    spine, so a lean in the seat bends the back and the hips stay put.
//! 3. A look down pitches the chest forward by a share of it, as on foot, again by how upright the
//!    spine is, and the spine turns back onto the neck's line about its base.
//! 4. The neck takes a share of the head's turn: the head's rotation against the body is a turn on
//!    top of the clip's head, so the neck's share is a share of it.
//!
//! The legs are not touched unless the caller gives them feet to reach ([`hang_feet`]): then each
//! is solved with the two-bone IK to its foot, the knee bending toward the hips' forward.

use glam::{Quat, Vec3};

use crate::controllers::body::{
    procedural::{
        config::ProceduralBodyConfig,
        rig::{Rig, RigBone},
        solver::{
            LEGS, SolveInput, place_leg,
            pose::RigPose,
            spine::{
                NECK_SHARE, PITCH_SHARES, SPINE, SWING_ITERATIONS, SWING_SHARES, TWIST_SHARES,
                chest_pitch, swing_toward,
            },
        },
    },
    rig::legs::swing::FootPose,
};

/// Solve the torso on the animated hips of `input.animated`, toward `input.head`, and with `legs`,
/// each leg to its foot.
pub fn solve_anchored(
    rig: &Rig,
    input: &SolveInput,
    legs: Option<[FootPose; 2]>,
    config: &ProceduralBodyConfig,
) -> RigPose {
    let animated = input.animated;
    let mut pose = animated;
    let base = pose.position(RigBone::Spine);
    let Some(upright) = (pose.position(RigBone::Neck) - base).try_normalize() else {
        return animated;
    };
    let share = upright_share(upright);
    let neck_target = input.head
        + input.head_rotation
            * (animated.position(RigBone::Neck) - animated.position(RigBone::Head));

    let limit = config.chest_turn_limit_deg.max(0.0).to_radians();
    let yaw = input
        .torso_yaw
        .filter(|y| y.is_finite())
        .unwrap_or(0.0)
        .clamp(-limit, limit)
        * share;
    if yaw.abs() > 1e-5 {
        for (bone, bone_share) in SPINE.iter().zip(TWIST_SHARES) {
            let pivot = pose.position(*bone);
            pose.rotate_about(
                *bone,
                Quat::from_axis_angle(upright, yaw * bone_share),
                pivot,
            );
        }
    }

    let lean = config.lean_limit_deg.max(0.0).to_radians();
    for _ in 0..SWING_ITERATIONS {
        let Some((axis, angle)) = swing_toward(&pose, neck_target, upright, lean) else {
            break;
        };
        for (bone, bone_share) in SPINE.iter().zip(SWING_SHARES) {
            let pivot = pose.position(*bone);
            pose.rotate_about(
                *bone,
                Quat::from_axis_angle(axis, angle * bone_share),
                pivot,
            );
        }
    }

    let pitch = chest_pitch(input.head_rotation) * share;
    if pitch > 0.0
        && let Some(axis) = (pose.position(RigBone::LeftShoulder)
            - pose.position(RigBone::RightShoulder))
        .try_normalize()
    {
        // Turning about the line to the left shoulder carries the chest's up toward its front.
        for (bone, bone_share) in SPINE.iter().zip(PITCH_SHARES) {
            let pivot = pose.position(*bone);
            pose.rotate_about(
                *bone,
                Quat::from_axis_angle(axis, pitch * bone_share),
                pivot,
            );
        }
        if let Some((axis, angle)) = swing_toward(&pose, neck_target, upright, lean) {
            pose.rotate_about(RigBone::Spine, Quat::from_axis_angle(axis, angle), base);
        }
    }

    let look = Quat::IDENTITY.slerp(input.head_rotation.normalize(), NECK_SHARE);
    if look.is_finite() {
        let pivot = pose.position(RigBone::Neck);
        pose.rotate_about(RigBone::Neck, look, pivot);
    }

    if let Some(feet) = legs {
        let forward = hips_turn(&animated, rig) * Vec3::NEG_Z;
        for (side, bones) in LEGS.into_iter().enumerate() {
            let (up, leg, foot) = bones;
            place_leg(
                &mut pose,
                [up, leg, foot],
                feet[side].position,
                feet[side].rotation,
                forward,
            );
        }
    }

    if pose.is_finite() { pose } else { animated }
}

/// How the legs hang from a harness.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HangTuning {
    /// The hip-to-ankle length, as a share of the standing leg's.
    pub share: f32,
    /// How far the legs swing forward at a full brake, radians: the flare before a landing.
    pub flare: f32,
    /// How far the toes point down while the legs hang, radians.
    pub toe_drop: f32,
}

/// Where the feet hang from the hips the animation keeps in `animated`, for `rig`: each ankle
/// below its hip joint, along the hips' own down, by `tuning.share` of the standing leg, the toes
/// pointing down by the toe drop. A brake (`brake`, `0..=1`) swings the legs forward about the
/// hips' sideways axis by up to the flare, lifts the knees a little, and brings the toes up, as a
/// canopy pilot's legs come forward for a flare.
pub fn hang_feet(rig: &Rig, animated: &RigPose, brake: f32, tuning: &HangTuning) -> [FootPose; 2] {
    let brake = if brake.is_finite() {
        brake.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let turn = hips_turn(animated, rig);
    let sideways = turn * Vec3::X;
    let swing = Quat::from_axis_angle(sideways, tuning.flare * brake);
    let share = (tuning.share - FLARE_KNEE_LIFT * brake).clamp(0.3, 1.0);
    let toes = Quat::from_axis_angle(sideways, -tuning.toe_drop * (1.0 - brake));
    LEGS.map(|(up, _, foot)| {
        let leg = rig.reference(up).translation.y - rig.reference(foot).translation.y;
        let hip = animated.position(up);
        FootPose {
            position: hip + swing * (turn * Vec3::NEG_Y) * (leg * share),
            rotation: (swing * toes * turn * rig.reference(foot).rotation).normalize(),
        }
    })
}

/// The hips' turn in `pose` against the reference pose's: what carries the reference pose's axes
/// into the hips' frame.
fn hips_turn(pose: &RigPose, rig: &Rig) -> Quat {
    let turn = pose.get(RigBone::Hips).rotation * rig.reference(RigBone::Hips).rotation.inverse();
    if turn.is_finite() {
        turn.normalize()
    } else {
        Quat::IDENTITY
    }
}

/// How much shorter the hanging leg is at a full brake, as a share of the standing leg: the knees
/// come up as the legs swing forward.
const FLARE_KNEE_LIFT: f32 = 0.12;

/// How much of the torso's turn and of the look down a spine pointing along `upright` takes: all
/// of it within [`UPRIGHT_FULL`] of the vertical, none past [`UPRIGHT_NONE`], eased between.
pub fn upright_share(upright: Vec3) -> f32 {
    let cos = upright.y;
    let (none, full) = (UPRIGHT_NONE.cos(), UPRIGHT_FULL.cos());
    let t = ((cos - none) / (full - none)).clamp(0.0, 1.0);
    if t.is_finite() {
        t * t * (3.0 - 2.0 * t)
    } else {
        0.0
    }
}

/// The spine's tilt from the vertical within which it takes the whole of the torso's turn,
/// radians: about thirty degrees.
const UPRIGHT_FULL: f32 = std::f32::consts::FRAC_PI_6;

/// The spine's tilt from the vertical past which it takes none of it, radians: about seventy
/// degrees.
const UPRIGHT_NONE: f32 = 1.22;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::body::procedural::{
        rig::{Rig, tests::skeleton},
        solver::line_yaw,
    };

    fn setup() -> (Rig, SolveInput, ProceduralBodyConfig) {
        let rig = Rig::from_skeleton(&skeleton()).unwrap();
        let animated = RigPose {
            bones: rig.reference,
        };
        let input = SolveInput {
            animated,
            head: animated.position(RigBone::Head),
            hips_head_y: None,
            head_rotation: Quat::IDENTITY,
            torso_yaw: Some(0.0),
            feet: [
                animated.position(RigBone::LeftFoot),
                animated.position(RigBone::RightFoot),
            ],
            foot_rotations: [Quat::IDENTITY; 2],
            support: Vec3::ZERO,
            planted: [true; 2],
            heel_lifts: [0.0; 2],
            hips_xz: None,
            hips_drop: 0.0,
            lower: None,
        };
        (rig, input, ProceduralBodyConfig::new())
    }

    /// The clip leaning the whole body by `angle` about +Z (a bike in a turn), from the hips.
    fn leaning(input: &mut SolveInput, angle: f32) {
        let hips = input.animated.position(RigBone::Hips);
        input
            .animated
            .rotate_about(RigBone::Hips, Quat::from_rotation_z(angle), hips);
        input.head = input.animated.position(RigBone::Head);
    }

    #[test]
    fn the_head_where_the_clip_has_it_leaves_the_clip_alone() {
        let (rig, mut input, config) = setup();
        leaning(&mut input, 0.4);
        let pose = solve_anchored(&rig, &input, None, &config);
        for bone in RigBone::ALL {
            assert!(
                pose.position(bone)
                    .abs_diff_eq(input.animated.position(bone), 1e-4),
                "{bone:?} moved to {:?}",
                pose.position(bone)
            );
        }
    }

    #[test]
    fn a_lean_in_the_seat_bends_the_back_and_keeps_the_hips_and_legs() {
        let (rig, mut input, config) = setup();
        leaning(&mut input, 0.3);
        input.head += Vec3::new(0.0, -0.05, -0.2);
        let pose = solve_anchored(&rig, &input, None, &config);
        for bone in [
            RigBone::Hips,
            RigBone::Pelvis,
            RigBone::LeftUpLeg,
            RigBone::LeftFoot,
            RigBone::RightFoot,
        ] {
            assert_eq!(pose.get(bone), input.animated.get(bone), "{bone:?} moved");
        }
        let neck_target = input.head
            + (input.animated.position(RigBone::Neck) - input.animated.position(RigBone::Head));
        let base = pose.position(RigBone::Spine);
        let aim = (pose.position(RigBone::Neck) - base).angle_between(neck_target - base);
        assert!(
            aim.to_degrees() < 2.0,
            "the neck is {} off",
            aim.to_degrees()
        );
    }

    #[test]
    fn a_lean_past_the_limit_stops_at_it_from_the_clips_spine() {
        let (rig, mut input, config) = setup();
        leaning(&mut input, 0.5);
        let base = input.animated.position(RigBone::Spine);
        let clip = (input.animated.position(RigBone::Neck) - base).normalize();
        // Far to the other side of the clip's spine.
        input.head = base + Quat::from_rotation_z(-2.0) * clip * 0.6;
        let pose = solve_anchored(&rig, &input, None, &config);
        let lean = (pose.position(RigBone::Neck) - base).angle_between(clip);
        assert!(
            (lean - config.lean_limit_deg.to_radians()).abs() < 0.05,
            "{} degrees",
            lean.to_degrees()
        );
    }

    #[test]
    fn the_torso_turn_twists_an_upright_chest_and_not_a_lying_one() {
        let (rig, mut input, config) = setup();
        input.torso_yaw = Some(0.5);
        let pose = solve_anchored(&rig, &input, None, &config);
        let chest = line_yaw(
            pose.position(RigBone::LeftShoulder),
            pose.position(RigBone::RightShoulder),
        )
        .unwrap();
        assert!((chest - 0.5).abs() < 0.02, "{chest}");
        assert_eq!(pose.get(RigBone::Hips), input.animated.get(RigBone::Hips));
        // Lying face down, the torso's heading is not a twist of the spine.
        let (_, mut lying, _) = setup();
        let hips = lying.animated.position(RigBone::Hips);
        lying
            .animated
            .rotate_about(RigBone::Hips, Quat::from_rotation_x(-1.5), hips);
        lying.head = lying.animated.position(RigBone::Head);
        lying.torso_yaw = Some(0.5);
        let pose = solve_anchored(&rig, &lying, None, &config);
        assert!(
            pose.position(RigBone::LeftShoulder)
                .abs_diff_eq(lying.animated.position(RigBone::LeftShoulder), 1e-4)
        );
    }

    #[test]
    fn the_share_eases_from_upright_to_lying() {
        assert_eq!(upright_share(Vec3::Y), 1.0);
        assert_eq!(upright_share(Vec3::NEG_Z), 0.0);
        let half = upright_share(Quat::from_rotation_x(0.87) * Vec3::Y);
        assert!(half > 0.1 && half < 0.9, "{half}");
    }

    #[test]
    fn looking_down_bends_the_neck_and_the_chest() {
        let (rig, input, config) = setup();
        let mut down = input;
        down.head_rotation = Quat::from_rotation_x(-0.6);
        let level = solve_anchored(&rig, &input, None, &config);
        let looking = solve_anchored(&rig, &down, None, &config);
        let turn = looking
            .get(RigBone::Neck)
            .rotation
            .angle_between(level.get(RigBone::Neck).rotation);
        assert!(turn > 0.05, "{turn}");
        assert_eq!(looking.get(RigBone::Hips), level.get(RigBone::Hips));
    }

    const HANG: HangTuning = HangTuning {
        share: 0.94,
        flare: 0.7,
        toe_drop: 0.35,
    };

    #[test]
    fn the_legs_hang_under_the_hips_and_reach_their_feet() {
        let (rig, input, config) = setup();
        let feet = hang_feet(&rig, &input.animated, 0.0, &HANG);
        for (side, up) in [RigBone::LeftUpLeg, RigBone::RightUpLeg]
            .into_iter()
            .enumerate()
        {
            let hip = input.animated.position(up);
            let drop = hip - feet[side].position;
            assert!(drop.x.abs() < 1e-5 && drop.z.abs() < 1e-5, "{drop}");
            assert!((drop.y - 0.84 * 0.94).abs() < 1e-4, "{drop}");
        }
        // The toes point down.
        let toes = feet[0].rotation * Vec3::NEG_Z;
        assert!(toes.y < -0.3, "{toes}");
        let pose = solve_anchored(&rig, &input, Some(feet), &config);
        for (side, foot) in [RigBone::LeftFoot, RigBone::RightFoot]
            .into_iter()
            .enumerate()
        {
            assert!(pose.position(foot).distance(feet[side].position) < 2e-3);
        }
        // The knees bend forward.
        assert!(pose.position(RigBone::LeftLeg).z < pose.position(RigBone::LeftUpLeg).z);
        assert_eq!(pose.get(RigBone::Hips), input.animated.get(RigBone::Hips));
    }

    #[test]
    fn a_brake_swings_the_legs_forward_and_the_toes_up() {
        let (rig, input, _) = setup();
        let hanging = hang_feet(&rig, &input.animated, 0.0, &HANG);
        let flared = hang_feet(&rig, &input.animated, 1.0, &HANG);
        assert!(flared[0].position.z < hanging[0].position.z - 0.3);
        assert!(flared[0].position.y > hanging[0].position.y);
        let toes = |f: &FootPose| (f.rotation * Vec3::NEG_Z).y;
        assert!(toes(&flared[0]) > toes(&hanging[0]) + 0.3);
        // Half a brake is between the two.
        let half = hang_feet(&rig, &input.animated, 0.5, &HANG);
        assert!(half[0].position.z < hanging[0].position.z);
        assert!(half[0].position.z > flared[0].position.z);
    }

    #[test]
    fn the_hang_turns_with_the_hips() {
        let (rig, mut input, _) = setup();
        let hips = input.animated.position(RigBone::Hips);
        input
            .animated
            .rotate_about(RigBone::Hips, Quat::from_rotation_y(1.0), hips);
        let flared = hang_feet(&rig, &input.animated, 1.0, &HANG);
        let hip = input.animated.position(RigBone::LeftUpLeg);
        let ahead = (flared[0].position - hip).with_y(0.0).normalize();
        let forward = Quat::from_rotation_y(1.0) * Vec3::NEG_Z;
        assert!(ahead.dot(forward) > 0.99, "{ahead}");
    }

    #[test]
    fn non_finite_input_returns_the_animation() {
        let (rig, mut input, config) = setup();
        input.head = Vec3::NAN;
        assert_eq!(solve_anchored(&rig, &input, None, &config), input.animated);
    }
}
