//! How far the character's body is from the player's: the frames that relate the cockpit, the
//! world, and the character's model space, and the divergence between the torso estimate and the
//! character's skeleton. Pure.
//!
//! The cockpit frame holds the player's tracked body at the player's own scale, with the head at
//! the published head pose: a cockpit point composes into the world as the hands do, by its offset
//! from the head turned by the view frame and scaled by the world scale. The character's model
//! space faces -Z with +Y up, as the world does once the character's own world transform is undone.

use std::fmt;

use body_rig::angle::{direction, line_yaw, wrap, yaw_of};
use glam::{Mat4, Quat, Vec3};

use crate::controllers::body::rig::torso::TorsoEstimate;

/// The character's skeleton landmarks in its model space, from the bones HumanIK drives.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SkeletonLandmarks {
    /// The head bone.
    pub head: Vec3,
    /// The hips effector's bone, the pelvis.
    pub pelvis: Vec3,
    /// The hip joints, left then right.
    pub hips: [Vec3; 2],
    /// The shoulder joints, left then right.
    pub shoulders: [Vec3; 2],
}

impl SkeletonLandmarks {
    /// Whether every landmark is a finite position.
    pub fn is_finite(&self) -> bool {
        self.head.is_finite()
            && self.pelvis.is_finite()
            && self.hips.iter().all(|p| p.is_finite())
            && self.shoulders.iter().all(|p| p.is_finite())
    }

    /// The middle of the shoulder joints.
    pub fn chest(&self) -> Vec3 {
        (self.shoulders[0] + self.shoulders[1]) * 0.5
    }

    /// The middle of the hip joints.
    pub fn hip_centre(&self) -> Vec3 {
        (self.hips[0] + self.hips[1]) * 0.5
    }

    /// The landmarks carried through `f`.
    pub fn map(&self, f: impl Fn(Vec3) -> Vec3) -> Self {
        Self {
            head: f(self.head),
            pelvis: f(self.pelvis),
            hips: self.hips.map(&f),
            shoulders: self.shoulders.map(&f),
        }
    }
}

/// The transforms between the cockpit frame, the world, and the character's model space at one
/// moment.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Frames {
    /// The head in the world: the published head pose's position.
    pub head_world: Vec3,
    /// The head in the cockpit frame.
    pub head_cockpit: Vec3,
    /// The view frame the cockpit composes with.
    pub view: Quat,
    /// World metres per cockpit metre.
    pub world_scale: f32,
    /// The character's model-to-world transform.
    pub character: Mat4,
}

impl Frames {
    /// Whether the frames can carry a point both ways.
    pub fn is_usable(&self) -> bool {
        self.head_world.is_finite()
            && self.head_cockpit.is_finite()
            && self.view.is_finite()
            && self.world_scale.is_finite()
            && self.world_scale > 0.0
            && self.character.is_finite()
            && self.character.determinant().abs() > 1.0e-6
    }

    pub fn cockpit_to_world(&self, point: Vec3) -> Vec3 {
        self.head_world + self.view * ((point - self.head_cockpit) * self.world_scale)
    }

    pub fn world_to_cockpit(&self, point: Vec3) -> Vec3 {
        self.head_cockpit + self.view.inverse() * (point - self.head_world) / self.world_scale
    }

    pub fn model_to_world(&self, point: Vec3) -> Vec3 {
        self.character.transform_point3(point)
    }

    pub fn world_to_model(&self, point: Vec3) -> Vec3 {
        self.character.inverse().transform_point3(point)
    }

    pub fn cockpit_to_model(&self, point: Vec3) -> Vec3 {
        self.world_to_model(self.cockpit_to_world(point))
    }

    pub fn model_to_cockpit(&self, point: Vec3) -> Vec3 {
        self.world_to_cockpit(self.model_to_world(point))
    }

    /// An orientation in the cockpit frame as an orientation in the character's model space.
    pub fn cockpit_rotation_to_model(&self, rotation: Quat) -> Quat {
        let (_, character, _) = self.character.to_scale_rotation_translation();
        (character.inverse() * self.view * rotation).normalize()
    }

    /// A cockpit heading as a heading in the character's model space, or `None` for one that
    /// turns vertical there.
    pub fn cockpit_yaw_to_model(&self, yaw: f32) -> Option<f32> {
        let (_, rotation, _) = self.character.to_scale_rotation_translation();
        yaw_of(rotation.inverse() * (self.view * direction(yaw)))
    }
}

/// How far the character's body is from the player's estimated torso, in the character's model
/// space. Turns are radians, positive turning -Z toward -X; offsets are the player's landmark less
/// the character's, metres.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Divergence {
    /// The torso's turn from the character's facing.
    pub root_turn: f32,
    /// The torso's turn from the character's chest (the line through his shoulder joints).
    pub chest_turn: Option<f32>,
    /// The torso's turn from the character's pelvis (the line through his hip joints).
    pub pelvis_turn: Option<f32>,
    pub chest: Vec3,
    pub pelvis: Vec3,
    pub shoulders: [Vec3; 2],
    pub hips: [Vec3; 2],
}

/// The divergence between the player's estimated torso and the character's skeleton, or `None`
/// when the frames cannot relate them.
pub fn divergence(
    estimate: &TorsoEstimate,
    skeleton: &SkeletonLandmarks,
    frames: &Frames,
) -> Option<Divergence> {
    if !frames.is_usable() || !skeleton.is_finite() {
        return None;
    }
    let root_turn = frames.cockpit_yaw_to_model(estimate.yaw)?;
    let turn_from = |girdle: Option<f32>| girdle.map(|g| wrap(root_turn - g));
    let to_model = |p: Vec3| frames.cockpit_to_model(p);
    Some(Divergence {
        root_turn,
        chest_turn: turn_from(lateral_twist(skeleton.shoulders[0], skeleton.shoulders[1])),
        pelvis_turn: turn_from(lateral_twist(skeleton.hips[0], skeleton.hips[1])),
        chest: to_model(estimate.chest) - skeleton.chest(),
        pelvis: to_model(estimate.pelvis) - skeleton.pelvis,
        shoulders: [0, 1].map(|i| to_model(estimate.shoulders[i]) - skeleton.shoulders[i]),
        hips: [0, 1].map(|i| to_model(estimate.hips[i]) - skeleton.hips[i]),
    })
}

/// Whether landmarks describe a body that the volumes can be placed on, in whatever orientation:
/// [`implausibility`] finds nothing wrong with them.
pub fn plausible(landmarks: &SkeletonLandmarks) -> bool {
    implausibility(landmarks).is_none()
}

/// What makes landmarks implausible as a body, the first failing test in the order listed by
/// [`Implausible`], or `None` for a body that the volumes can be placed on: the shoulders a plausible
/// distance from the hips, both girdles a plausible width, and the head within a neck's reach of
/// the shoulders. Garbage bone data fails it; a crouch, a lean, a fall, a skydive, or a look
/// straight down does not. Lengths are in the landmarks' metres.
///
/// The head is held loosely, anywhere within reach of the chest, below the shoulders included: the
/// body IK keeps the head bone on the player's neck pivot, but a clip, a fold over the hips, or a
/// head the solve cannot reach can still hang it forward of and below the shoulder joints.
pub fn implausibility(landmarks: &SkeletonLandmarks) -> Option<Implausible> {
    if !landmarks.is_finite() {
        return Some(Implausible::NotFinite);
    }
    let chest = landmarks.chest();
    let spine = chest - landmarks.hip_centre();
    if !(CHEST_TO_HIPS_M.0..=CHEST_TO_HIPS_M.1).contains(&spine.length()) {
        return Some(Implausible::Spine);
    }
    let shoulder_width = landmarks.shoulders[0].distance(landmarks.shoulders[1]);
    if !(SHOULDER_WIDTH_M.0..=SHOULDER_WIDTH_M.1).contains(&shoulder_width) {
        return Some(Implausible::ShoulderWidth);
    }
    let hip_width = landmarks.hips[0].distance(landmarks.hips[1]);
    if !(HIP_WIDTH_M.0..=HIP_WIDTH_M.1).contains(&hip_width) {
        return Some(Implausible::HipWidth);
    }
    (landmarks.head.distance(chest) > HEAD_REACH_M).then_some(Implausible::Head)
}

/// Why landmarks are not a plausible body, in the order [`implausibility`] tests.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Implausible {
    /// A landmark is not a finite position.
    NotFinite,
    /// The shoulders are too near the hips or too far from them.
    Spine,
    /// The shoulder joints are too close together or too far apart.
    ShoulderWidth,
    /// The hip joints are too close together or too far apart.
    HipWidth,
    /// The head is beyond a neck's reach of the shoulders.
    Head,
}

impl fmt::Display for Implausible {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Implausible::NotFinite => "a landmark is not finite",
            Implausible::Spine => "the spine's length is out of range",
            Implausible::ShoulderWidth => "the shoulders' width is out of range",
            Implausible::HipWidth => "the hips' width is out of range",
            Implausible::Head => "the head is beyond the neck's reach",
        })
    }
}

/// The turn of the line from `left` to `right` away from the model's lateral axis, radians, in
/// `-π/2..=π/2`; `None` when the points coincide horizontally. Wrapping to a half turn makes the
/// measure independent of which side of the model the rig puts its left.
pub fn lateral_twist(left: Vec3, right: Vec3) -> Option<f32> {
    let angle = line_yaw(left, right)?;
    let half = std::f32::consts::FRAC_PI_2;
    Some(if angle > half {
        angle - std::f32::consts::PI
    } else if angle <= -half {
        angle + std::f32::consts::PI
    } else {
        angle
    })
}

/// The range of the shoulders' distance from the hips, metres: from a deep bend to a tall adult.
const CHEST_TO_HIPS_M: (f32, f32) = (0.2, 0.8);

/// The range of the distance between the shoulder joints, metres.
const SHOULDER_WIDTH_M: (f32, f32) = (0.2, 0.7);

/// The range of the distance between the hip joints, metres.
const HIP_WIDTH_M: (f32, f32) = (0.05, 0.5);

/// The furthest the head bone may be from the middle of the shoulder joints, metres: well past a
/// stretched neck, short of a head torn off the body.
const HEAD_REACH_M: f32 = 0.5;

#[cfg(test)]
mod tests {
    use body_rig::body::EYES_TO_NECK;

    use super::*;
    use crate::controllers::body::{
        BodyDimensions, procedural::head::head_bone_target, rig::torso::HIP_SPACING_FRACTION,
    };

    fn deg(d: f32) -> f32 {
        d.to_radians()
    }

    /// An upright torso facing -Z under eyes at `head`, its joints hung from the dimensions as the
    /// grounded rig hangs them.
    fn upright(head: Vec3, dims: &BodyDimensions) -> TorsoEstimate {
        let neck = head + EYES_TO_NECK;
        let chest = neck - Vec3::Y * (dims.eye_height + EYES_TO_NECK.y - dims.shoulder_height);
        let pelvis = chest - Vec3::Y * 0.5;
        let half = Vec3::X * dims.shoulder_width * 0.5;
        let hip = half * HIP_SPACING_FRACTION;
        TorsoEstimate {
            yaw: 0.0,
            head_yaw: 0.0,
            neck_twist: 0.0,
            hands_yaw: None,
            hands_weight: 0.0,
            neck,
            shoulders: [chest - half, chest + half],
            chest,
            pelvis,
            hips: [pelvis - hip, pelvis + hip],
        }
    }

    /// A skeleton built from the dimensions the way the torso is hung, in a model space whose eyes
    /// are at `eyes`.
    fn skeleton(dims: &BodyDimensions, eyes: Vec3, turn: f32) -> SkeletonLandmarks {
        let estimate = upright(Vec3::ZERO, dims);
        let rotation = Quat::from_rotation_y(turn);
        let place = |p: Vec3| eyes + rotation * p;
        SkeletonLandmarks {
            head: place(Vec3::ZERO),
            pelvis: place(estimate.pelvis),
            hips: estimate.hips.map(place),
            shoulders: estimate.shoulders.map(place),
        }
    }

    fn frames(view: Quat, character_yaw: f32) -> Frames {
        Frames {
            head_world: Vec3::new(10.0, 3.0, -4.0),
            head_cockpit: Vec3::new(0.1, -0.05, 0.2),
            view,
            world_scale: 1.0,
            character: Mat4::from_rotation_translation(
                Quat::from_rotation_y(character_yaw),
                Vec3::new(10.0, 1.3, -4.0),
            ),
        }
    }

    #[test]
    fn the_frames_carry_points_both_ways() {
        let frames = Frames {
            world_scale: 1.3,
            ..frames(Quat::from_rotation_y(0.4), -1.1)
        };
        let p = Vec3::new(0.3, -0.6, 0.1);
        assert!(
            frames
                .world_to_cockpit(frames.cockpit_to_world(p))
                .distance(p)
                < 1e-4
        );
        assert!(
            frames
                .model_to_cockpit(frames.cockpit_to_model(p))
                .distance(p)
                < 1e-4
        );
        assert!(
            frames
                .cockpit_to_world(frames.head_cockpit)
                .distance(frames.head_world)
                < 1e-5
        );
    }

    #[test]
    fn a_cockpit_heading_turns_into_model_space_by_the_body_relative_to_the_view() {
        // The body is turned 30 degrees left of the view; a torso straight ahead of the view is 30
        // degrees right of the body.
        let frames = frames(Quat::from_rotation_y(0.2), 0.2 + deg(30.0));
        assert!((frames.cockpit_yaw_to_model(0.0).unwrap() + deg(30.0)).abs() < 1e-4);
    }

    #[test]
    fn a_body_that_matches_the_estimate_has_no_divergence() {
        let dims = BodyDimensions::GENERIC_ADULT;
        let view = Quat::from_rotation_y(0.7);
        let frames = frames(view, 0.7);
        let estimate = upright(frames.head_cockpit, &dims);
        let eyes = frames.world_to_model(frames.head_world);
        let skeleton = skeleton(&dims, eyes, 0.0);
        let d = divergence(&estimate, &skeleton, &frames).unwrap();
        assert!(d.root_turn.abs() < 1e-4);
        assert!(d.chest_turn.unwrap().abs() < 1e-4);
        assert!(d.pelvis_turn.unwrap().abs() < 1e-4);
        for offset in [
            d.chest,
            d.pelvis,
            d.shoulders[0],
            d.shoulders[1],
            d.hips[0],
            d.hips[1],
        ] {
            assert!(offset.length() < 1e-3, "{offset}");
        }
    }

    #[test]
    fn a_turned_chest_reads_as_a_chest_turn() {
        let dims = BodyDimensions::GENERIC_ADULT;
        let frames = frames(Quat::IDENTITY, 0.0);
        let estimate = upright(frames.head_cockpit, &dims);
        let eyes = frames.world_to_model(frames.head_world);
        let mut skeleton = skeleton(&dims, eyes, 0.0);
        let chest = skeleton.chest();
        let turn = Quat::from_rotation_y(deg(20.0));
        skeleton.shoulders = skeleton.shoulders.map(|s| chest + turn * (s - chest));
        let d = divergence(&estimate, &skeleton, &frames).unwrap();
        assert!((d.chest_turn.unwrap() + deg(20.0)).abs() < 1e-3);
        assert!(d.pelvis_turn.unwrap().abs() < 1e-3);
    }

    #[test]
    fn a_body_in_any_orientation_passes_and_a_broken_one_does_not() {
        let dims = BodyDimensions::CHARACTER_ESTIMATE;
        let standing = skeleton(&dims, Vec3::ZERO, 0.4);
        assert!(plausible(&standing));
        // Lying face down, as in a skydive.
        let lying = standing.map(|p| Quat::from_rotation_x(deg(-90.0)) * p);
        assert!(plausible(&lying));
        let broken = SkeletonLandmarks {
            pelvis: Vec3::NAN,
            ..standing
        };
        assert!(!plausible(&broken));
        // The hips folded onto the shoulders.
        let collapsed = SkeletonLandmarks {
            hips: standing.shoulders,
            ..standing
        };
        assert_eq!(implausibility(&collapsed), Some(Implausible::Spine));
        // The head torn off the neck.
        let torn = SkeletonLandmarks {
            head: standing.head + Vec3::new(1.0, 0.0, 0.0),
            ..standing
        };
        assert_eq!(implausibility(&torn), Some(Implausible::Head));
    }

    /// The local player's skeleton in its reference pose, model space, from the skeleton the
    /// procedural body logs: the head bone, the hips effector's bone, the hip joints, and the
    /// shoulder joints.
    fn reference_skeleton() -> SkeletonLandmarks {
        SkeletonLandmarks {
            head: Vec3::new(0.0, 1.6348, 0.0098),
            pelvis: Vec3::new(0.0, 1.0352, 0.0),
            hips: [
                Vec3::new(-0.0913, 0.9789, -0.0263),
                Vec3::new(0.0913, 0.9789, -0.0263),
            ],
            shoulders: [
                Vec3::new(-0.188, 1.4455, 0.0417),
                Vec3::new(0.188, 1.4455, 0.0417),
            ],
        }
    }

    /// The reference skeleton looking down by `look` radians as the posing leaves it: the upper
    /// back pitched forward by the procedural torso's share of the look, and the head bone placed
    /// on the body IK's head target, for an eye point the player's look down swings down and
    /// forward about the neck pivot.
    fn looking_down(look: f32) -> SkeletonLandmarks {
        let reference = reference_skeleton();
        let upper_back = Vec3::new(0.0, 1.3096, 0.0082);
        let pitch =
            |angle: f32, pivot: Vec3, p: Vec3| pivot + Quat::from_rotation_x(-angle) * (p - pivot);
        let chest_pitch = (look * 0.35).min(0.44);
        let head_rotation = Quat::from_rotation_x(-look);
        let eyes = -EYES_TO_NECK;
        let eye = reference.head + head_rotation * eyes - eyes;
        SkeletonLandmarks {
            head: head_bone_target(eye, head_rotation, 1.0),
            shoulders: reference
                .shoulders
                .map(|s| pitch(chest_pitch, upper_back, s)),
            ..reference
        }
    }

    /// How far the head is up the spine from the middle of the shoulder joints.
    fn head_up_the_spine(landmarks: &SkeletonLandmarks) -> f32 {
        let up = (landmarks.chest() - landmarks.hip_centre()).normalize();
        (landmarks.head - landmarks.chest()).dot(up)
    }

    #[test]
    fn a_look_down_keeps_the_head_above_the_shoulders() {
        assert!(plausible(&reference_skeleton()));
        // The head target stays on the neck pivot, so the head bone holds its height while the
        // eyes swing down and forward: more than a hunch's 5 cm up the spine from the shoulders
        // at every pitch, where aiming it at the eye point sank it below them past 75 degrees.
        let reference = reference_skeleton();
        for look in [0.0, 30.0, 60.0, 75.0, 90.0] {
            let landmarks = looking_down(deg(look));
            assert!(
                landmarks.head.distance(reference.head) < 1e-4,
                "looking {look} degrees down: {}",
                landmarks.head
            );
            let up = head_up_the_spine(&landmarks);
            assert!(up > 0.05, "looking {look} degrees down: {up}");
            assert_eq!(
                implausibility(&landmarks),
                None,
                "looking {look} degrees down"
            );
        }
    }

    #[test]
    fn a_deep_crouch_bent_over_the_floor_stays_plausible() {
        // The hips lowered half a metre and the whole torso folded 70 degrees forward over them,
        // looking straight down.
        let down = looking_down(deg(90.0));
        let hips = down.hip_centre();
        let fold = |p: Vec3| hips + Quat::from_rotation_x(deg(-70.0)) * (p - hips);
        let crouched = SkeletonLandmarks {
            head: fold(down.head),
            pelvis: down.pelvis,
            hips: down.hips,
            shoulders: down.shoulders.map(fold),
        }
        .map(|p| p - Vec3::Y * 0.5);
        assert_eq!(implausibility(&crouched), None);
    }

    #[test]
    fn unusable_frames_yield_nothing() {
        let dims = BodyDimensions::GENERIC_ADULT;
        let skeleton = skeleton(&dims, Vec3::ZERO, 0.0);
        let estimate = upright(Vec3::ZERO, &dims);
        let bad = Frames {
            world_scale: 0.0,
            ..frames(Quat::IDENTITY, 0.0)
        };
        assert_eq!(divergence(&estimate, &skeleton, &bad), None);
    }
}
