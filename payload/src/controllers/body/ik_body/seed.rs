//! The IK pose seeded into Rico's posture before the solve: the FK body carried into his model
//! space at his proportions, the clip's hips moved and turned onto the FK pelvis, the pelvis
//! lowered where the planted feet would be out of reach, and the legs' air pose. Pure.
//!
//! The hand-over works with positions, not rotations, so no bone's rest frame has to be known: the
//! clip's pelvis frame is read from its hip and spine joints' positions, the FK pelvis's from the
//! FK hips and first spine joint, and the turn between the two frames is what the clip's hips take.

use body_rig::{
    fk::FkBody,
    ik_weight::{IkWeights, blend_rotation},
};
use glam::{Mat3, Quat, Vec3};

use crate::controllers::body::procedural::{rig::RigBone, solver::pose::RigPose};

/// The FK body in Rico's model space, at his proportions.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct ModelBody {
    pub pelvis: Vec3,
    pub spine1: Vec3,
    pub hips: [Vec3; 2],
    pub knees: [Vec3; 2],
    pub ankles: [Vec3; 2],
}

/// How the player's body maps onto Rico's.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Proportions {
    /// Rico's spine over the player's, from the pelvis to the neck pivot.
    pub spine: f32,
    /// Rico's legs over the player's.
    pub legs: f32,
}

/// `fk` (relative to the neck pivot, room axes) in the model space whose neck pivot is at `neck`,
/// turned by `model_from_room` and scaled by `proportions`: the spine's points about the neck, and
/// the legs about their hips.
pub(crate) fn to_model(
    fk: &FkBody,
    neck: Vec3,
    model_from_room: Quat,
    proportions: Proportions,
) -> ModelBody {
    let spine = |p: Vec3| neck + model_from_room * (p * proportions.spine);
    let hips = fk.hips.map(spine);
    let leg = |i: usize, p: Vec3| hips[i] + model_from_room * ((p - fk.hips[i]) * proportions.legs);
    ModelBody {
        pelvis: spine(fk.pelvis),
        spine1: spine(fk.spine1),
        hips,
        knees: [leg(0, fk.knees[0]), leg(1, fk.knees[1])],
        ankles: [leg(0, fk.ankles[0]), leg(1, fk.ankles[1])],
    }
}

/// The orientation of a pelvis whose left-to-right line runs along `right` and whose up leans along
/// `up`: +X right, +Y up, +Z back. `None` for lines that do not make a frame.
pub(crate) fn pelvis_frame(right: Vec3, up: Vec3) -> Option<Quat> {
    let x = right.try_normalize()?;
    let z = x.cross(up).try_normalize()?;
    let y = z.cross(x);
    let frame = Quat::from_mat3(&Mat3::from_cols(x, y, z)).normalize();
    frame.is_finite().then_some(frame)
}

/// The clip's pelvis frame in `pose`, from its upper legs and its first spine joint.
pub(crate) fn clip_frame(pose: &RigPose) -> Option<Quat> {
    pelvis_frame(
        pose.position(RigBone::RightUpLeg) - pose.position(RigBone::LeftUpLeg),
        pose.position(RigBone::Spine1) - pose.position(RigBone::Hips),
    )
}

/// The FK pelvis's frame.
pub(crate) fn ik_frame(body: &ModelBody) -> Option<Quat> {
    pelvis_frame(body.hips[1] - body.hips[0], body.spine1 - body.pelvis)
}

/// `animated` with its hips turned toward the FK pelvis's orientation by `weights`' pelvis
/// orientation and moved toward its position by the pelvis position, carrying the whole posture
/// with them, then lowered by `drop` metres. The clip's own pose where a frame cannot be read.
pub(crate) fn seed(
    animated: &RigPose,
    body: &ModelBody,
    weights: &IkWeights,
    drop: f32,
) -> RigPose {
    let mut pose = *animated;
    let hips = animated.position(RigBone::Hips);
    if weights.pelvis_orientation > 0.0
        && let (Some(clip), Some(ik)) = (clip_frame(animated), ik_frame(body))
    {
        let turn = blend_rotation(
            Quat::IDENTITY,
            ik * clip.inverse(),
            weights.pelvis_orientation,
        );
        pose.rotate_about(RigBone::Hips, turn, hips);
    }
    let towards = (body.pelvis - hips) * weights.pelvis_position.clamp(0.0, 1.0);
    let lower = Vec3::NEG_Y * drop.max(0.0) * weights.feet.clamp(0.0, 1.0);
    let offset = towards + lower;
    if offset != Vec3::ZERO {
        pose.translate(RigBone::Hips, offset);
    }
    if pose.is_finite() { pose } else { *animated }
}

/// How far the hips at `hips` must come down for every planted ankle in `ankles` to stand within
/// `leg` metres of its hip, at most `max` metres.
pub(crate) fn pelvis_drop(hips: [Vec3; 2], ankles: [Option<Vec3>; 2], leg: f32, max: f32) -> f32 {
    let reach = leg * REACH_SHARE;
    let mut drop = 0.0f32;
    for (hip, ankle) in hips.iter().zip(ankles) {
        let Some(ankle) = ankle else {
            continue;
        };
        let offset = *hip - ankle;
        let across = glam::Vec2::new(offset.x, offset.z).length();
        if across >= reach || !across.is_finite() {
            drop = drop.max(max);
            continue;
        }
        let height = (reach * reach - across * across).sqrt();
        drop = drop.max(offset.y - height);
    }
    if drop.is_finite() {
        drop.clamp(0.0, max.max(0.0))
    } else {
        0.0
    }
}

/// The legs in the air: each ankle and knee of `body` drawn `relax` of the way toward a light air
/// pose under its hip in the pelvis's `frame` (the knees a little bent and forward, the feet a
/// little apart), for legs `leg` metres long.
pub(crate) fn air_legs(body: &ModelBody, frame: Quat, leg: f32, relax: f32) -> [[Vec3; 2]; 2] {
    let relax = relax.clamp(0.0, 1.0);
    let down = frame * Vec3::NEG_Y;
    let ahead = frame * Vec3::NEG_Z;
    let mut ankles = body.ankles;
    let mut knees = body.knees;
    for i in 0..2 {
        let hip = body.hips[i];
        let air_knee = hip + down * leg * AIR_KNEE_DOWN + ahead * leg * AIR_KNEE_AHEAD;
        let air_ankle = hip + down * leg * AIR_ANKLE_DOWN + ahead * leg * AIR_ANKLE_AHEAD;
        knees[i] = knees[i].lerp(air_knee, relax);
        ankles[i] = ankles[i].lerp(air_ankle, relax);
    }
    [ankles, knees]
}

/// The share of the leg's straight length the pelvis's drop keeps the planted feet within, so the
/// knee keeps a little bend to solve with.
const REACH_SHARE: f32 = 0.995;

/// The light air pose's knee and ankle under the hip, as shares of the leg's length: down along the
/// pelvis and ahead of it.
const AIR_KNEE_DOWN: f32 = 0.45;
const AIR_KNEE_AHEAD: f32 = 0.18;
const AIR_ANKLE_DOWN: f32 = 0.85;
const AIR_ANKLE_AHEAD: f32 = 0.05;

#[cfg(test)]
mod tests {
    use body_rig::{PlayerBody, fk::FkSkeleton};

    use super::*;
    use crate::controllers::body::procedural::rig::{Rig, tests::skeleton};

    fn pose() -> RigPose {
        RigPose {
            bones: Rig::from_skeleton(&skeleton()).unwrap().reference,
        }
    }

    fn body_at(pose: &RigPose) -> ModelBody {
        ModelBody {
            pelvis: pose.position(RigBone::Hips),
            spine1: pose.position(RigBone::Spine1),
            hips: [
                pose.position(RigBone::LeftUpLeg),
                pose.position(RigBone::RightUpLeg),
            ],
            knees: [
                pose.position(RigBone::LeftLeg),
                pose.position(RigBone::RightLeg),
            ],
            ankles: [
                pose.position(RigBone::LeftFoot),
                pose.position(RigBone::RightFoot),
            ],
        }
    }

    fn full() -> IkWeights {
        IkWeights {
            pelvis_position: 1.0,
            pelvis_orientation: 1.0,
            spine: 1.0,
            feet: 1.0,
            air_legs: 0.0,
        }
    }

    #[test]
    fn seeding_carries_the_clips_hips_onto_the_fk_pelvis() {
        let animated = pose();
        let mut target = animated;
        let pivot = target.position(RigBone::Hips);
        target.rotate_about(RigBone::Hips, Quat::from_rotation_y(0.5), pivot);
        target.translate(RigBone::Hips, Vec3::new(0.1, -0.05, 0.2));
        let body = body_at(&target);
        let seeded = seed(&animated, &body, &full(), 0.0);
        for bone in RigBone::ALL {
            assert!(
                seeded.position(bone).distance(target.position(bone)) < 1e-4,
                "{bone:?}"
            );
        }
        // No weight leaves the clip.
        assert_eq!(seed(&animated, &body, &IkWeights::ZERO, 0.0), animated);
    }

    #[test]
    fn the_walk_turns_the_hips_part_way_and_leaves_them_where_they_stand() {
        let animated = pose();
        let mut target = animated;
        let pivot = target.position(RigBone::Hips);
        target.rotate_about(RigBone::Hips, Quat::from_rotation_y(0.5), pivot);
        target.translate(RigBone::Hips, Vec3::new(0.3, 0.0, 0.0));
        let walk = IkWeights {
            pelvis_orientation: 0.2,
            spine: 0.2,
            ..IkWeights::ZERO
        };
        let seeded = seed(&animated, &body_at(&target), &walk, 0.0);
        assert_eq!(seeded.position(RigBone::Hips), pivot);
        let turned = clip_frame(&seeded).unwrap() * clip_frame(&animated).unwrap().inverse();
        assert!((turned.angle_between(Quat::IDENTITY) - 0.1).abs() < 1e-3);
    }

    #[test]
    fn the_pelvis_comes_down_only_as_far_as_the_feet_need() {
        let hips = [Vec3::new(-0.1, 0.95, 0.0), Vec3::new(0.1, 0.95, 0.0)];
        let leg = 0.85;
        // Straight under the hips at full length: nothing but the bend's margin.
        let under = [
            Some(Vec3::new(-0.1, 0.1, 0.0)),
            Some(Vec3::new(0.1, 0.1, 0.0)),
        ];
        let small = pelvis_drop(hips, under, leg, 0.2);
        assert!(small > 0.0 && small < 0.01, "{small}");
        // A foot 30 cm aside: the hips come down so the leg reaches it.
        let aside = [Some(Vec3::new(-0.4, 0.1, 0.0)), None];
        let drop = pelvis_drop(hips, aside, leg, 0.2);
        let hip = hips[0] - Vec3::Y * drop;
        assert!((hip.distance(aside[0].unwrap()) - leg * REACH_SHARE).abs() < 1e-4);
        // Out of reach altogether: the most.
        let far = [Some(Vec3::new(-1.5, 0.1, 0.0)), None];
        assert_eq!(pelvis_drop(hips, far, leg, 0.2), 0.2);
        assert_eq!(pelvis_drop(hips, [None, None], leg, 0.2), 0.0);
    }

    #[test]
    fn the_fk_body_carries_into_model_space_at_ricos_proportions() {
        let body = PlayerBody::from_heights(1.8, 1.68);
        let skeleton = FkSkeleton::for_body(&body, body.standing_neck());
        let fk = body_rig::fk::fallback(Vec3::new(0.0, -0.6, 0.0), 0.0, &skeleton);
        let neck = Vec3::new(0.0, 1.5, 0.0);
        let half = to_model(
            &fk,
            neck,
            Quat::IDENTITY,
            Proportions {
                spine: 0.5,
                legs: 2.0,
            },
        );
        assert!((half.pelvis - Vec3::new(0.0, 1.2, 0.0)).length() < 1e-5);
        for i in 0..2 {
            let player = fk.hips[i].distance(fk.ankles[i]);
            assert!((half.hips[i].distance(half.ankles[i]) - player * 2.0).abs() < 1e-4);
        }
        let turned = to_model(
            &fk,
            neck,
            Quat::from_rotation_y(std::f32::consts::PI),
            Proportions {
                spine: 1.0,
                legs: 1.0,
            },
        );
        // A half turn swaps the hips' sides.
        assert!((turned.hips[0].x + fk.hips[0].x).abs() < 1e-5);
    }

    #[test]
    fn the_air_pose_hangs_the_legs_under_the_hips() {
        let body = body_at(&pose());
        let frame = ik_frame(&body).unwrap();
        let [ankles, knees] = air_legs(&body, frame, 0.85, 1.0);
        for i in 0..2 {
            assert!(ankles[i].y < body.hips[i].y - 0.6);
            assert!(knees[i].y > ankles[i].y);
        }
        let [kept, _] = air_legs(&body, frame, 0.85, 0.0);
        assert_eq!(kept, body.ankles);
    }

    #[test]
    fn a_degenerate_frame_is_none() {
        assert_eq!(pelvis_frame(Vec3::ZERO, Vec3::Y), None);
        assert_eq!(pelvis_frame(Vec3::X, Vec3::X), None);
        let frame = pelvis_frame(Vec3::X, Vec3::Y).unwrap();
        assert!(frame.angle_between(Quat::IDENTITY) < 1e-5);
    }
}
