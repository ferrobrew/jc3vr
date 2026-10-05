//! How far the character's body is from the player's: the frames that relate the cockpit, the
//! world, and the character's model space, and the divergence between the torso estimate and the
//! character's skeleton. Pure.
//!
//! The cockpit frame holds the player's tracked body at the player's own scale, with the head at
//! the published head pose: a cockpit point composes into the world as the hands do, by its offset
//! from the head turned by the view frame and scaled by the world scale. The character's model
//! space faces -Z with +Y up, as the world does once the character's own world transform is undone.

use glam::{Mat4, Quat, Vec3};

use crate::controllers::{
    body::torso::{TorsoEstimate, wrap, yaw_forward, yaw_of},
    roomscale::stance::logic::lateral_twist,
};

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

    /// A cockpit heading as a heading in the character's model space, or `None` for one that
    /// turns vertical there.
    pub fn cockpit_yaw_to_model(&self, yaw: f32) -> Option<f32> {
        let (_, rotation, _) = self.character.to_scale_rotation_translation();
        yaw_of(rotation.inverse() * (self.view * yaw_forward(yaw)))
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

/// Whether landmarks in the cockpit frame describe an upright body that the holsters can be placed
/// on: the shoulders a plausible distance below the head and the hips below them, both girdles a
/// plausible width, and the chest near the head horizontally. A ragdoll, a skydive, or garbage
/// bone data fails it. The cockpit frame's +Y is up, and lengths are in its metres.
pub fn upright(landmarks: &SkeletonLandmarks) -> bool {
    if !landmarks.is_finite() {
        return false;
    }
    let up = Vec3::Y;
    let chest = landmarks.chest();
    let hips = landmarks.hip_centre();
    let head_to_chest = (landmarks.head - chest).dot(up);
    let chest_to_hips = (chest - hips).dot(up);
    let shoulder_width = landmarks.shoulders[0].distance(landmarks.shoulders[1]);
    let hip_width = landmarks.hips[0].distance(landmarks.hips[1]);
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
    (UPRIGHT_HEAD_TO_CHEST_M.0..=UPRIGHT_HEAD_TO_CHEST_M.1).contains(&head_to_chest)
        && (UPRIGHT_CHEST_TO_HIPS_M.0..=UPRIGHT_CHEST_TO_HIPS_M.1).contains(&chest_to_hips)
        && (UPRIGHT_SHOULDER_WIDTH_M.0..=UPRIGHT_SHOULDER_WIDTH_M.1).contains(&shoulder_width)
        && (UPRIGHT_HIP_WIDTH_M.0..=UPRIGHT_HIP_WIDTH_M.1).contains(&hip_width)
        && flat(landmarks.head - chest).length() <= UPRIGHT_HEAD_REACH_M
}

/// The range of the head's height above the middle of the shoulder joints for an upright body,
/// metres: from a hunch to a stretch.
const UPRIGHT_HEAD_TO_CHEST_M: (f32, f32) = (0.05, 0.45);

/// The range of the shoulders' height above the hips for an upright body, metres: from a deep bend
/// to a tall adult.
const UPRIGHT_CHEST_TO_HIPS_M: (f32, f32) = (0.2, 0.8);

/// The range of the distance between the shoulder joints, metres.
const UPRIGHT_SHOULDER_WIDTH_M: (f32, f32) = (0.2, 0.7);

/// The range of the distance between the hip joints, metres.
const UPRIGHT_HIP_WIDTH_M: (f32, f32) = (0.05, 0.5);

/// The furthest the head may be from the middle of the shoulders horizontally, metres.
const UPRIGHT_HEAD_REACH_M: f32 = 0.5;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::body::{
        BodyDimensions,
        torso::{TorsoConfig, TorsoEstimator, TorsoInputs},
    };

    fn deg(d: f32) -> f32 {
        d.to_radians()
    }

    /// A skeleton built from the dimensions the way the torso estimate hangs its joints, in a
    /// model space whose eyes are at `eyes`.
    fn skeleton(dims: &BodyDimensions, eyes: Vec3, turn: f32) -> SkeletonLandmarks {
        let mut estimator = TorsoEstimator::new();
        let inputs = TorsoInputs {
            head_position: Vec3::ZERO,
            head_orientation: Quat::IDENTITY,
            hands: [None; 2],
        };
        let mut estimate = None;
        for _ in 0..400 {
            estimate = estimator.update(&inputs, 0.01, dims, &TorsoConfig::new());
        }
        let estimate = estimate.unwrap();
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
        let mut estimator = TorsoEstimator::new();
        let inputs = TorsoInputs {
            head_position: frames.head_cockpit,
            head_orientation: Quat::IDENTITY,
            hands: [None; 2],
        };
        let mut estimate = None;
        for _ in 0..400 {
            estimate = estimator.update(&inputs, 0.01, &dims, &TorsoConfig::new());
        }
        let estimate = estimate.unwrap();
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
        let mut estimator = TorsoEstimator::new();
        let inputs = TorsoInputs {
            head_position: frames.head_cockpit,
            head_orientation: Quat::IDENTITY,
            hands: [None; 2],
        };
        let mut estimate = None;
        for _ in 0..100 {
            estimate = estimator.update(&inputs, 0.01, &dims, &TorsoConfig::new());
        }
        let eyes = frames.world_to_model(frames.head_world);
        let mut skeleton = skeleton(&dims, eyes, 0.0);
        let chest = skeleton.chest();
        let turn = Quat::from_rotation_y(deg(20.0));
        skeleton.shoulders = skeleton.shoulders.map(|s| chest + turn * (s - chest));
        let d = divergence(&estimate.unwrap(), &skeleton, &frames).unwrap();
        assert!((d.chest_turn.unwrap() + deg(20.0)).abs() < 1e-3);
        assert!(d.pelvis_turn.unwrap().abs() < 1e-3);
    }

    #[test]
    fn an_upright_skeleton_passes_and_a_fallen_one_does_not() {
        let dims = BodyDimensions::CHARACTER_ESTIMATE;
        let standing = skeleton(&dims, Vec3::ZERO, 0.4);
        assert!(upright(&standing));
        // Lying face down: the hips level with the chest.
        let lying = standing.map(|p| Quat::from_rotation_x(deg(-90.0)) * p);
        assert!(!upright(&lying));
        let broken = SkeletonLandmarks {
            pelvis: Vec3::NAN,
            ..standing
        };
        assert!(!upright(&broken));
    }

    #[test]
    fn unusable_frames_yield_nothing() {
        let dims = BodyDimensions::GENERIC_ADULT;
        let skeleton = skeleton(&dims, Vec3::ZERO, 0.0);
        let mut estimator = TorsoEstimator::new();
        let estimate = estimator
            .update(
                &TorsoInputs {
                    head_position: Vec3::ZERO,
                    head_orientation: Quat::IDENTITY,
                    hands: [None; 2],
                },
                0.01,
                &dims,
                &TorsoConfig::new(),
            )
            .unwrap();
        let bad = Frames {
            world_scale: 0.0,
            ..frames(Quat::IDENTITY, 0.0)
        };
        assert_eq!(divergence(&estimate, &skeleton, &bad), None);
    }
}
