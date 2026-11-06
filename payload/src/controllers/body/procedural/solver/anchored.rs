//! The torso on the animation's hips: the spine, the neck, and the clavicles posed from the
//! player's head while the animation keeps the hips, the pelvis, and the legs (on foot, in a
//! vehicle seat, under the parachute's harness, in the water). Pure, in Rico's model space.
//!
//! The clip's posture is the base the player's head moves from: a walk's stride, a bike's lean
//! into a turn, a driver's reach for the wheel. [`solve_anchored`] keeps the
//! hips where the clip has them and the clip's shape of the spine, and changes the spine only by
//! the player's own motion:
//!
//! 1. The chest twists to face the player's torso: the torso's turn against Rico's facing, less the
//!    heading the clip's own shoulder line already faces, so a clip that stands with its shoulders
//!    turned (the bladed idle) is squared to the player rather than turned further. The twist is
//!    held within `chest_turn_limit_deg`, about the clip's own spine and shared out up it as on
//!    foot. A spine far from upright (lying in a swim) takes less of the twist, none lying flat,
//!    since the torso's heading is a turn about the vertical.
//! 2. The spine swings so the neck reaches the point under the player's head, shared out along the
//!    four spine bones and repeated to converge, never past `lean_limit_deg` from the clip's own
//!    spine, so a lean in the seat bends the back and the hips stay put.
//! 3. A look down pitches the chest forward by a share of it, as on foot, again by how upright the
//!    spine is, and the spine turns back onto the neck's line about its base.
//! 4. The chest is kept out of the camera's near field: a chest within `chest_clear_below_m` below
//!    the eyes is swung back about the spine's base until it is `chest_clear_back_m` behind them,
//!    along the view's level forward, so a camera that sits low, or a spine bent back, never puts
//!    the chest in front of the eyes. A look down at a chest further below is left alone. The
//!    view's level forward is the head's heading, which stays defined looking straight down, and
//!    the swing never passes `CLEAR_LIMIT`, so no pose of the head folds the spine over.
//! 5. The neck takes a share of the head's turn: the head's rotation against the body is a turn on
//!    top of the clip's head, so the neck's share is a share of it.
//!
//! The legs are not touched.

use body_rig::{
    angle::{direction, head_heading, line_yaw, wrap},
    smooth::smoothstep_between,
};
use glam::{Quat, Vec3};

use crate::controllers::body::procedural::{
    config::ProceduralBodyConfig,
    rig::RigBone,
    solver::{
        SolveInput,
        pose::RigPose,
        spine::{
            NECK_SHARE, PITCH_SHARES, SPINE, SWING_ITERATIONS, SWING_SHARES, TWIST_SHARES,
            chest_pitch, swing_toward,
        },
    },
};

/// Solve the torso on the animated hips of `input.animated`, toward `input.head`.
pub fn solve_anchored(input: &SolveInput, config: &ProceduralBodyConfig) -> RigPose {
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
    let clip_chest = line_yaw(
        animated.position(RigBone::LeftShoulder),
        animated.position(RigBone::RightShoulder),
    )
    .unwrap_or(0.0);
    let yaw = input
        .torso_yaw
        .filter(|y| y.is_finite())
        .map_or(0.0, |torso| wrap(torso - clip_chest))
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

    keep_chest_clear(&mut pose, input, base, config);

    let look = Quat::IDENTITY.slerp(input.head_rotation.normalize(), NECK_SHARE);
    if look.is_finite() {
        let pivot = pose.position(RigBone::Neck);
        pose.rotate_about(RigBone::Neck, look, pivot);
    }

    if pose.is_finite() { pose } else { animated }
}

/// Swing the spine back about its base `base` until none of the chest's points within
/// `chest_clear_below_m` below the eyes is less than `chest_clear_back_m` behind them, along the
/// view's level forward (the head's heading), swinging no further than [`CLEAR_LIMIT`] in all.
fn keep_chest_clear(
    pose: &mut RigPose,
    input: &SolveInput,
    base: Vec3,
    config: &ProceduralBodyConfig,
) {
    let Some(heading) = head_heading(input.head_rotation.normalize()) else {
        return;
    };
    let forward = direction(heading);
    let Some(axis) = Vec3::Y.cross(forward).try_normalize() else {
        return;
    };
    let (below, back) = (
        config.chest_clear_below_m.max(0.0),
        config.chest_clear_back_m.max(0.0),
    );
    let mut swung = 0.0f32;
    for _ in 0..CLEAR_ITERATIONS {
        let worst = chest_points(pose)
            .into_iter()
            .filter(|p| p.y > input.eye.y - below)
            .map(|p| ((p - input.eye).dot(forward) + back, (p - base).length()))
            .filter(|(over, _)| *over > 1e-4)
            .max_by(|a, b| a.0.total_cmp(&b.0));
        let Some((over, radius)) = worst else {
            return;
        };
        if radius < 1e-3 {
            return;
        }
        // Turning about `axis` (up crossed with forward) carries up toward forward, so a negative
        // angle carries the chest back.
        let angle = (over / radius)
            .clamp(0.0, 1.0)
            .asin()
            .min(CLEAR_LIMIT - swung);
        if angle <= 0.0 {
            return;
        }
        swung += angle;
        pose.rotate_about(RigBone::Spine, Quat::from_axis_angle(axis, -angle), base);
    }
}

/// The points of the chest that must stay out of the near field: the upper spine, the sternum, and
/// the shoulders.
fn chest_points(pose: &RigPose) -> [Vec3; 4] {
    [
        pose.position(RigBone::Spine2),
        pose.position(RigBone::Sternum),
        pose.position(RigBone::LeftShoulder),
        pose.position(RigBone::RightShoulder),
    ]
}

/// How many times the chest clearance swings: the radius changes with the swing, so a second pass
/// takes up what the first left.
const CLEAR_ITERATIONS: usize = 3;

/// The furthest the chest clearance swings the spine back in all, radians: forty-five degrees. A
/// camera far inside the chest would otherwise fold the spine over backwards to clear it.
const CLEAR_LIMIT: f32 = std::f32::consts::FRAC_PI_4;

/// How much of the torso's turn and of the look down a spine pointing along `upright` takes: all
/// of it within [`UPRIGHT_FULL`] of the vertical, none past [`UPRIGHT_NONE`], eased between.
pub fn upright_share(upright: Vec3) -> f32 {
    let cos = upright.y;
    let (none, full) = (UPRIGHT_NONE.cos(), UPRIGHT_FULL.cos());
    smoothstep_between(none, full, cos)
}

/// The spine's tilt from the vertical within which it takes the whole of the torso's turn,
/// radians: about thirty degrees.
const UPRIGHT_FULL: f32 = std::f32::consts::FRAC_PI_6;

/// The spine's tilt from the vertical past which it takes none of it, radians: about seventy
/// degrees.
const UPRIGHT_NONE: f32 = 1.22;

#[cfg(test)]
mod tests {
    use body_rig::angle::line_yaw;

    use super::*;
    use crate::controllers::body::procedural::rig::{Rig, tests::skeleton};

    fn setup() -> (SolveInput, ProceduralBodyConfig) {
        let rig = Rig::from_skeleton(&skeleton()).unwrap();
        let animated = RigPose {
            bones: rig.reference,
        };
        let input = SolveInput {
            animated,
            head: animated.position(RigBone::Head),
            eye: animated.position(RigBone::Head) + Vec3::new(0.0, 0.08, -0.1),
            head_rotation: Quat::IDENTITY,
            torso_yaw: Some(0.0),
        };
        (input, ProceduralBodyConfig::new())
    }

    /// The clip leaning the whole body by `angle` about +Z (a bike in a turn), from the hips.
    fn leaning(input: &mut SolveInput, angle: f32) {
        let hips = input.animated.position(RigBone::Hips);
        input
            .animated
            .rotate_about(RigBone::Hips, Quat::from_rotation_z(angle), hips);
        input.head = input.animated.position(RigBone::Head);
        input.eye = input.head + Vec3::new(0.0, 0.08, -0.1);
    }

    #[test]
    fn a_low_camera_keeps_the_chest_behind_the_eyes() {
        let (mut input, config) = setup();
        let shoulders = (input.animated.position(RigBone::LeftShoulder)
            + input.animated.position(RigBone::RightShoulder))
            * 0.5;
        // The eyes straight over the shoulders and only a little above them.
        input.eye = shoulders + Vec3::new(0.0, 0.1, 0.0);
        let pose = solve_anchored(&input, &config);
        for p in chest_points(&pose) {
            if p.y > input.eye.y - config.chest_clear_below_m {
                assert!(
                    (p - input.eye).dot(Vec3::NEG_Z) <= -config.chest_clear_back_m + 2e-3,
                    "{p:?} in front of {:?}",
                    input.eye
                );
            }
        }
        // The hips stay the clip's.
        assert!(
            pose.position(RigBone::Hips)
                .abs_diff_eq(input.animated.position(RigBone::Hips), 1e-5)
        );
    }

    /// The spine's swing back from the clip's, radians.
    fn spine_swing(pose: &RigPose, input: &SolveInput) -> f32 {
        let base = input.animated.position(RigBone::Spine);
        (pose.position(RigBone::Neck) - base)
            .angle_between(input.animated.position(RigBone::Neck) - base)
    }

    #[test]
    fn looking_straight_down_keeps_the_clearance_steady() {
        let (mut input, config) = setup();
        let shoulders = (input.animated.position(RigBone::LeftShoulder)
            + input.animated.position(RigBone::RightShoulder))
            * 0.5;
        input.eye = shoulders + Vec3::new(0.0, 0.1, 0.0);
        // Just short of and just past straight down: the forward axis's level part flips, the
        // heading does not.
        let swings: Vec<f32> = [-1.55f32, -std::f32::consts::FRAC_PI_2, -1.59]
            .into_iter()
            .map(|pitch| {
                input.head_rotation = Quat::from_rotation_x(pitch);
                spine_swing(&solve_anchored(&input, &config), &input)
            })
            .collect();
        for swing in &swings {
            assert!(swing.is_finite() && *swing < 1.2, "{swings:?}");
        }
        assert!(
            (swings[0] - swings[2]).abs() < 0.2,
            "the clearance flipped: {swings:?}"
        );
    }

    #[test]
    fn the_clearance_swings_no_further_than_its_limit() {
        let (mut input, config) = setup();
        let mut no_look = config;
        no_look.lean_limit_deg = 0.0;
        // The eyes well behind the chest, a little below the sternum.
        let sternum = input.animated.position(RigBone::Sternum);
        input.eye = sternum + Vec3::new(0.0, -0.05, 0.25);
        input.head = input.animated.position(RigBone::Head);
        let pose = solve_anchored(&input, &no_look);
        let swing = spine_swing(&pose, &input);
        assert!(swing <= CLEAR_LIMIT + 1e-3, "{}", swing.to_degrees());
        assert!(swing > CLEAR_LIMIT * 0.5, "{}", swing.to_degrees());
    }

    #[test]
    fn the_head_where_the_clip_has_it_leaves_the_clip_alone() {
        let (mut input, config) = setup();
        leaning(&mut input, 0.4);
        let pose = solve_anchored(&input, &config);
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
        let (mut input, config) = setup();
        leaning(&mut input, 0.3);
        input.head += Vec3::new(0.0, -0.05, -0.2);
        let pose = solve_anchored(&input, &config);
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
        let (mut input, config) = setup();
        leaning(&mut input, 0.5);
        let base = input.animated.position(RigBone::Spine);
        let clip = (input.animated.position(RigBone::Neck) - base).normalize();
        // Far to the other side of the clip's spine.
        input.head = base + Quat::from_rotation_z(-2.0) * clip * 0.6;
        let pose = solve_anchored(&input, &config);
        let lean = (pose.position(RigBone::Neck) - base).angle_between(clip);
        assert!(
            (lean - config.lean_limit_deg.to_radians()).abs() < 0.05,
            "{} degrees",
            lean.to_degrees()
        );
    }

    #[test]
    fn the_torso_turn_twists_an_upright_chest_and_not_a_lying_one() {
        let (mut input, config) = setup();
        input.torso_yaw = Some(0.5);
        let pose = solve_anchored(&input, &config);
        let chest = line_yaw(
            pose.position(RigBone::LeftShoulder),
            pose.position(RigBone::RightShoulder),
        )
        .unwrap();
        assert!((chest - 0.5).abs() < 0.02, "{chest}");
        assert_eq!(pose.get(RigBone::Hips), input.animated.get(RigBone::Hips));
        // Lying face down, the torso's heading is not a twist of the spine.
        let (mut lying, _) = setup();
        let hips = lying.animated.position(RigBone::Hips);
        lying
            .animated
            .rotate_about(RigBone::Hips, Quat::from_rotation_x(-1.5), hips);
        lying.head = lying.animated.position(RigBone::Head);
        lying.torso_yaw = Some(0.5);
        let pose = solve_anchored(&lying, &config);
        assert!(
            pose.position(RigBone::LeftShoulder)
                .abs_diff_eq(lying.animated.position(RigBone::LeftShoulder), 1e-4)
        );
    }

    #[test]
    fn a_bladed_clip_chest_is_squared_to_the_torso() {
        let (mut input, config) = setup();
        // The clip turns the spine a fifth of a radian to the right from the hips up.
        let spine = input.animated.position(RigBone::Spine);
        input
            .animated
            .rotate_about(RigBone::Spine, Quat::from_rotation_y(-0.2), spine);
        input.head = input.animated.position(RigBone::Head);
        input.eye = input.head + Vec3::new(0.0, 0.08, -0.1);
        let chest = |pose: &RigPose| {
            line_yaw(
                pose.position(RigBone::LeftShoulder),
                pose.position(RigBone::RightShoulder),
            )
            .unwrap()
        };
        assert!((chest(&input.animated) + 0.2).abs() < 0.02);
        for torso in [0.0, 0.3] {
            input.torso_yaw = Some(torso);
            let pose = solve_anchored(&input, &config);
            assert!(
                (chest(&pose) - torso).abs() < 0.03,
                "{torso}: {}",
                chest(&pose)
            );
        }
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
        let (input, config) = setup();
        let mut down = input;
        down.head_rotation = Quat::from_rotation_x(-0.6);
        let level = solve_anchored(&input, &config);
        let looking = solve_anchored(&down, &config);
        let turn = looking
            .get(RigBone::Neck)
            .rotation
            .angle_between(level.get(RigBone::Neck).rotation);
        assert!(turn > 0.05, "{turn}");
        assert_eq!(looking.get(RigBone::Hips), level.get(RigBone::Hips));
    }

    #[test]
    fn non_finite_input_returns_the_animation() {
        let (mut input, config) = setup();
        input.head = Vec3::NAN;
        assert_eq!(solve_anchored(&input, &config), input.animated);
    }
}
