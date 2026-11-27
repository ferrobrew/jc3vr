//! The torso on the animation's hips: the spine, the neck, and the clavicles posed from the
//! player's head while the animation keeps the hips, the pelvis, and the legs (on foot, in a
//! vehicle seat, under the parachute's harness, in the water). Pure, in Rico's model space.
//!
//! The clip's posture is the base the player's head moves from: a walk's stride, a bike's lean
//! into a turn, a driver's reach for the wheel. [`solve_anchored`] keeps the hips where the clip has
//! them and the clip's shape of the spine, and changes the spine only by the player's own motion:
//!
//! 1. The chest twists to face the player's torso: the torso's turn against Rico's facing, less the
//!    heading the clip's own shoulder line already faces, so a clip that stands with its shoulders
//!    turned (the bladed idle) is squared to the player rather than turned further. The twist is
//!    held within `torso.chest_turn_limit_deg`, about the clip's own spine and shared out up it as
//!    on foot. A spine far from upright (lying in a swim) takes less of the twist, none lying flat,
//!    since the torso's heading is a turn about the vertical.
//! 2. The spine swings so the neck reaches the point under the player's head, shared out along the
//!    four spine bones and repeated to converge, never past `torso.lean_limit_deg` from the clip's
//!    own spine, so a lean in the seat bends the back and the hips stay put.
//! 3. A look down pitches the chest forward by a share of it, as on foot, again by how upright the
//!    spine is, and the spine turns back onto the neck's line about its base.
//! 4. The chest is kept out of the camera's near field: a chest within `chest_clearance.below_m`
//!    below the eyes is swung back about the spine's base until it is `chest_clearance.back_m`
//!    behind them, along the view's level forward, so a camera that sits low, or a spine bent back,
//!    never puts the chest in front of the eyes. A look down at a chest further below is left
//!    alone. The view's level forward is the head's heading, which stays defined looking straight
//!    down, and the swing never passes `chest_clearance.limit_rad`, so no pose of the head folds
//!    the spine over.
//! 5. The neck takes a share of the head's turn: the head's rotation against the body is a turn on
//!    top of the clip's head, so the neck's share is a share of it.
//!
//! The legs are not touched.

use body_rig::math::{
    angle::{direction, head_heading, line_yaw, wrap},
    smooth::smoothstep_between,
};
use glam::{Quat, Vec3};

use crate::controllers::body::procedural::{
    rig::RigBone,
    solver::{
        SolveInput,
        pose::RigPose,
        spine::{SPINE, SWING_ITERATIONS, chest_pitch, swing_toward},
    },
    tuning::{PROCEDURAL, ProceduralTuning, TorsoTuning},
};

/// Solve the torso on the animated hips of `input.animated`, toward `input.head`.
pub(crate) fn solve_anchored(input: &SolveInput, config: &ProceduralTuning) -> RigPose {
    let animated = input.animated;
    let mut pose = animated;
    let base = pose.position(RigBone::Spine);
    let Some(clip_upright) = (pose.position(RigBone::Neck) - base).try_normalize() else {
        return animated;
    };
    // The clip's lean is taken out about the spine's base first, by the input's share, so the
    // player's motion bends the spine from a torso square over the pelvis, since a clip's forward
    // lean would otherwise kink the chest back toward the eyes and fill the view looking down.
    let delean = if input.delean.is_finite() {
        input.delean.clamp(0.0, 1.0)
    } else {
        0.0
    };
    if delean > 0.0 && clip_upright.y > 0.0 {
        let straighten =
            Quat::IDENTITY.slerp(Quat::from_rotation_arc(clip_upright, Vec3::Y), delean);
        pose.rotate_about(RigBone::Spine, straighten, base);
    }
    let Some(upright) = (pose.position(RigBone::Neck) - base).try_normalize() else {
        return animated;
    };
    let torso = &config.torso;
    let share = upright_share_with(upright, torso);
    let neck_target = input.head
        + input.head_rotation
            * (animated.position(RigBone::Neck) - animated.position(RigBone::Head));

    let limit = torso.chest_turn_limit_deg.max(0.0).to_radians();
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
        for (bone, bone_share) in SPINE.iter().zip(torso.twist_shares) {
            let pivot = pose.position(*bone);
            pose.rotate_about(
                *bone,
                Quat::from_axis_angle(upright, yaw * bone_share),
                pivot,
            );
        }
    }

    let lean = torso.lean_limit_deg.max(0.0).to_radians();
    for _ in 0..SWING_ITERATIONS {
        let Some((axis, angle)) = swing_toward(&pose, neck_target, upright, lean) else {
            break;
        };
        for (bone, bone_share) in SPINE.iter().zip(torso.swing_shares) {
            let pivot = pose.position(*bone);
            pose.rotate_about(
                *bone,
                Quat::from_axis_angle(axis, angle * bone_share),
                pivot,
            );
        }
    }

    let pitch = chest_pitch(input.head_rotation, torso) * share;
    if pitch > 0.0
        && let Some(axis) = (pose.position(RigBone::LeftShoulder)
            - pose.position(RigBone::RightShoulder))
        .try_normalize()
    {
        // Turning about the line to the left shoulder carries the chest's up toward its front.
        for (bone, bone_share) in SPINE.iter().zip(torso.pitch_shares) {
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

    let look = Quat::IDENTITY.slerp(input.head_rotation.normalize(), torso.neck_share);
    if look.is_finite() {
        let pivot = pose.position(RigBone::Neck);
        pose.rotate_about(RigBone::Neck, look, pivot);
    }

    if pose.is_finite() { pose } else { animated }
}

/// Swing the spine back about its base `base` until none of the chest's points within
/// `chest_clearance.below_m` below the eyes is less than `chest_clearance.back_m` behind them,
/// along the view's level forward (the head's heading), swinging no further than
/// `chest_clearance.limit_rad` in all.
fn keep_chest_clear(pose: &mut RigPose, input: &SolveInput, base: Vec3, config: &ProceduralTuning) {
    let Some(heading) = head_heading(input.head_rotation.normalize()) else {
        return;
    };
    let forward = direction(heading);
    let Some(axis) = Vec3::Y.cross(forward).try_normalize() else {
        return;
    };
    let clearance = &config.chest_clearance;
    let (below, back) = (clearance.below_m.max(0.0), clearance.back_m.max(0.0));
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
            .min(clearance.limit_rad - swung);
        if angle <= 0.0 {
            return;
        }
        swung += angle;
        pose.rotate_about(RigBone::Spine, Quat::from_axis_angle(axis, -angle), base);
    }
}

/// `offset`, a translation of a clavicle whose root and shoulder joint are at `points`, less as
/// much of its forward part as would carry either point into the near field [`keep_chest_clear`]
/// keeps the chest out of: within `chest_clearance.below_m` below the eyes and less than
/// `chest_clearance.back_m` behind them along the view's level forward. Only the offset's own
/// forward part is taken back, so a point the solve left in the near field is not pushed further
/// back than it stood, and a translation back or to the side is untouched.
pub(crate) fn clear_clavicle_offset(
    points: [Vec3; 2],
    offset: Vec3,
    input: &SolveInput,
    config: &ProceduralTuning,
) -> Vec3 {
    let Some(heading) = head_heading(input.head_rotation.normalize()) else {
        return offset;
    };
    let forward = direction(heading);
    let ahead = offset.dot(forward);
    if ahead <= 0.0 {
        return offset;
    }
    let clearance = &config.chest_clearance;
    let (below, back) = (clearance.below_m.max(0.0), clearance.back_m.max(0.0));
    let over = points
        .into_iter()
        .map(|p| p + offset)
        .filter(|p| p.y > input.eye.y - below)
        .map(|p| (p - input.eye).dot(forward) + back)
        .fold(0.0f32, f32::max);
    offset - forward * over.min(ahead)
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

/// How much of the torso's turn and of the look down a spine pointing along `upright` takes: all
/// of it within the tuning's `torso.upright_full_rad` of the vertical, none past
/// `torso.upright_none_rad`, eased between.
pub(crate) fn upright_share(upright: Vec3) -> f32 {
    upright_share_with(upright, &PROCEDURAL.get().torso)
}

/// [`upright_share`] with the torso's tuning `torso`.
fn upright_share_with(upright: Vec3, torso: &TorsoTuning) -> f32 {
    let cos = upright.y;
    let (none, full) = (torso.upright_none_rad.cos(), torso.upright_full_rad.cos());
    smoothstep_between(none, full, cos)
}

#[cfg(test)]
mod tests {
    use body_rig::math::angle::line_yaw;

    use super::*;
    use crate::controllers::body::procedural::{
        rig::{Rig, test_support::skeleton},
        tuning::PROCEDURAL,
    };

    fn setup() -> (SolveInput, ProceduralTuning) {
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
            delean: 0.0,
        };
        (input, *PROCEDURAL.get())
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
            if p.y > input.eye.y - config.chest_clearance.below_m {
                assert!(
                    (p - input.eye).dot(Vec3::NEG_Z) <= -config.chest_clearance.back_m + 2e-3,
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
    fn a_clavicle_translated_toward_the_camera_stops_at_the_clearance() {
        let (input, config) = setup();
        let back = config.chest_clearance.back_m;
        // A root and joint level with the eyes, a clearance and a half behind them.
        let root = input.eye + Vec3::new(0.0, -0.05, back * 1.5);
        let joint = root + Vec3::new(0.18, -0.02, 0.02);
        let pulled =
            clear_clavicle_offset([root, joint], Vec3::new(0.01, 0.0, -back), &input, &config);
        assert!((pulled.x - 0.01).abs() < 1e-5, "the sideways part is kept");
        let cleared = root + pulled;
        assert!(
            ((cleared - input.eye).dot(Vec3::NEG_Z) + back).abs() < 1e-4,
            "{pulled}"
        );
        // Within the clearance, or back, or below the near field, the offset is untouched.
        for offset in [Vec3::new(0.0, 0.0, -back * 0.4), Vec3::new(0.0, 0.0, 0.1)] {
            assert_eq!(
                clear_clavicle_offset([root, joint], offset, &input, &config),
                offset
            );
        }
        let low = root - Vec3::Y * (config.chest_clearance.below_m + 0.1);
        let ahead = Vec3::new(0.0, 0.0, -0.3);
        assert_eq!(
            clear_clavicle_offset([low, low], ahead, &input, &config),
            ahead
        );
        // A root the solve already left inside is not pushed back past where it stood.
        let inside = input.eye + Vec3::new(0.0, -0.05, back * 0.5);
        let pulled = clear_clavicle_offset([inside, inside], Vec3::NEG_Z * 0.05, &input, &config);
        assert!(pulled.length() < 1e-5, "{pulled}");
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
        no_look.torso.lean_limit_deg = 0.0;
        // The eyes well behind the chest, a little below the sternum.
        let sternum = input.animated.position(RigBone::Sternum);
        input.eye = sternum + Vec3::new(0.0, -0.05, 0.25);
        input.head = input.animated.position(RigBone::Head);
        let pose = solve_anchored(&input, &no_look);
        let swing = spine_swing(&pose, &input);
        assert!(
            swing <= config.chest_clearance.limit_rad + 1e-3,
            "{}",
            swing.to_degrees()
        );
        assert!(
            swing > config.chest_clearance.limit_rad * 0.5,
            "{}",
            swing.to_degrees()
        );
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
            (lean - config.torso.lean_limit_deg.to_radians()).abs() < 0.05,
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
    fn parachute_yaw_and_bend_leave_the_harness_and_legs_animated() {
        let (mut input, config) = setup();
        let hips = input.animated.position(RigBone::Hips);
        input
            .animated
            .rotate_about(RigBone::Hips, Quat::from_rotation_x(0.2), hips);
        input.head = input.animated.position(RigBone::Head);
        input.eye = input.head + Vec3::new(0.0, 0.08, -0.1);
        let neutral = solve_anchored(&input, &config);
        input.torso_yaw = Some(0.55);
        let turned = solve_anchored(&input, &config);
        input.head_rotation = Quat::from_euler(glam::EulerRot::YXZ, 0.55, -0.7, 0.0);
        let solved = solve_anchored(&input, &config);
        let chest_yaw = |pose: &RigPose| {
            line_yaw(
                pose.position(RigBone::LeftShoulder),
                pose.position(RigBone::RightShoulder),
            )
            .unwrap()
        };
        assert!(chest_yaw(&solved) - chest_yaw(&neutral) > 0.4);
        assert!(
            solved
                .get(RigBone::Sternum)
                .rotation
                .angle_between(turned.get(RigBone::Sternum).rotation)
                > 0.05
        );
        for bone in RigBone::ALL {
            if bone == RigBone::Hips || bone.descends_from(RigBone::Pelvis) {
                assert_eq!(solved.get(bone), input.animated.get(bone));
            }
            if let Some(parent) = bone.parent() {
                let before = input
                    .animated
                    .position(bone)
                    .distance(input.animated.position(parent));
                let after = solved.position(bone).distance(solved.position(parent));
                assert!((before - after).abs() < 1e-5, "{bone:?}: {before} {after}");
            }
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

    #[test]
    fn a_gait_clips_forward_lean_is_taken_out_of_the_lower_spine_not_bent_back_above_it() {
        let (mut input, config) = setup();
        // The walk clip leans the torso forward from the spine's base.
        let base = input.animated.position(RigBone::Spine);
        input
            .animated
            .rotate_about(RigBone::Spine, Quat::from_rotation_x(-0.4), base);
        input.head = input.animated.position(RigBone::Head);
        let lower_lean = |pose: &RigPose| {
            let segment = pose.position(RigBone::Spine1) - pose.position(RigBone::Spine);
            segment.angle_between(Vec3::Y)
        };
        let kept = solve_anchored(&input, &config);
        input.delean = 0.8;
        let straightened = solve_anchored(&input, &config);
        assert!(
            lower_lean(&straightened) < lower_lean(&kept) - 0.1,
            "{} against {}",
            lower_lean(&straightened),
            lower_lean(&kept)
        );
    }
}
