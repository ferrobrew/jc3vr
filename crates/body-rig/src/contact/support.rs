//! What a foot stands on: a support's identity and its frame, how a support's motion carries the
//! points on it, and the frame a moving support will have a moment from now. Pure.
//!
//! A planted foot and a swing are held in their support's frame, so a support that moves carries
//! them. The body that stands on a support is carried with it too, so the rules that read the
//! body's own travel (the pelvis's velocity and the root moved by something other than the
//! player) take the carrier's motion out first ([`SupportFrame::carry`]).

use glam::{Quat, Vec3};

use crate::{
    contact::FootPose,
    math::angle::{wrap, yaw_of},
};

/// What a foot stands on: an identity the caller can find the support's frame by.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Default)]
pub struct SupportId(pub u64);

impl SupportId {
    /// The static world: the terrain and anything that never moves.
    pub const WORLD: Self = Self(0);
}

/// A support's frame: its pose in the model's frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SupportFrame {
    pub translation: Vec3,
    pub rotation: Quat,
}

impl SupportFrame {
    pub const IDENTITY: Self = Self {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
    };

    /// `pose`, held in this support's frame, in the model's frame.
    pub fn to_model(&self, pose: FootPose) -> FootPose {
        FootPose {
            ankle: self.point_to_model(pose.ankle),
            yaw: wrap(pose.yaw + yaw_turn(self.rotation)),
        }
    }

    /// `pose`, in the model's frame, held in this support's frame.
    pub fn to_local(&self, pose: FootPose) -> FootPose {
        FootPose {
            ankle: self.point_to_local(pose.ankle),
            yaw: wrap(pose.yaw - yaw_turn(self.rotation)),
        }
    }

    /// `point`, held in this support's frame, in the model's frame.
    pub fn point_to_model(&self, point: Vec3) -> Vec3 {
        self.translation + self.rotation * point
    }

    /// `point`, in the model's frame, held in this support's frame.
    pub fn point_to_local(&self, point: Vec3) -> Vec3 {
        self.rotation.inverse() * (point - self.translation)
    }

    /// How far the support's motion from `before` to this frame carried the point that stands at
    /// `point` now: `point` less where that point of the support was in `before`.
    pub fn carry(&self, before: &SupportFrame, point: Vec3) -> Vec3 {
        point - before.point_to_model(self.point_to_local(point))
    }

    /// The frame `dt` seconds on, for a support moving at `linear` and spinning at `angular`
    /// (radians per second about the world axes) about its centre of mass `centre`, all in the
    /// model's frame: the frame a support read at one moment has at a later one.
    pub fn predict(&self, motion: &SupportMotion, dt: f32) -> SupportFrame {
        if !(dt.is_finite() && dt > 0.0 && motion.is_finite()) {
            return *self;
        }
        let spin = Quat::from_scaled_axis(motion.angular * dt);
        let centre = motion.centre + motion.linear * dt;
        SupportFrame {
            translation: centre + spin * (self.translation - motion.centre),
            rotation: (spin * self.rotation).normalize(),
        }
    }

    pub(crate) fn is_finite(&self) -> bool {
        self.translation.is_finite() && self.rotation.is_finite()
    }
}

/// How a support moves: its centre of mass and its velocities, in the model's frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SupportMotion {
    pub centre: Vec3,
    /// The centre's velocity, metres per second.
    pub linear: Vec3,
    /// The spin, radians per second about the model's axes.
    pub angular: Vec3,
}

impl SupportMotion {
    /// A support at rest.
    pub const REST: Self = Self {
        centre: Vec3::ZERO,
        linear: Vec3::ZERO,
        angular: Vec3::ZERO,
    };

    fn is_finite(&self) -> bool {
        self.centre.is_finite() && self.linear.is_finite() && self.angular.is_finite()
    }
}

/// The turn about +Y a rotation makes of a heading.
fn yaw_turn(rotation: Quat) -> f32 {
    yaw_of(rotation * Vec3::NEG_Z).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deck() -> SupportFrame {
        SupportFrame {
            translation: Vec3::new(3.0, 1.0, -2.0),
            rotation: Quat::from_rotation_y(0.6),
        }
    }

    #[test]
    fn a_pose_survives_the_round_trip_through_the_frame() {
        let pose = FootPose {
            ankle: Vec3::new(0.2, 1.08, -1.5),
            yaw: 0.3,
        };
        let back = deck().to_model(deck().to_local(pose));
        assert!(back.ankle.abs_diff_eq(pose.ankle, 1e-5));
        assert!(wrap(back.yaw - pose.yaw).abs() < 1e-5);
    }

    #[test]
    fn a_translating_support_carries_every_point_by_its_travel() {
        let before = deck();
        let after = SupportFrame {
            translation: before.translation + Vec3::new(0.4, 0.0, -0.1),
            ..before
        };
        for point in [Vec3::ZERO, Vec3::new(5.0, 2.0, 1.0)] {
            assert!(
                after
                    .carry(&before, point)
                    .abs_diff_eq(Vec3::new(0.4, 0.0, -0.1), 1e-5)
            );
        }
    }

    #[test]
    fn a_turning_support_carries_a_point_round_its_axis() {
        let before = SupportFrame::IDENTITY;
        let after = SupportFrame {
            translation: Vec3::ZERO,
            rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        };
        // The point that stands at +X now stood at +Z before: a quarter turn about +Y takes +Z to
        // +X.
        let carried = after.carry(&before, Vec3::X);
        assert!(carried.abs_diff_eq(Vec3::X - Vec3::Z, 1e-5), "{carried}");
        // The axis itself does not move.
        assert!(after.carry(&before, Vec3::Y).abs_diff_eq(Vec3::ZERO, 1e-5));
    }

    #[test]
    fn the_prediction_moves_and_spins_the_frame_about_its_centre() {
        let frame = SupportFrame {
            translation: Vec3::new(1.0, 0.0, 0.0),
            rotation: Quat::IDENTITY,
        };
        let motion = SupportMotion {
            centre: Vec3::ZERO,
            linear: Vec3::new(0.0, 0.0, -10.0),
            angular: Vec3::new(0.0, std::f32::consts::PI, 0.0),
        };
        let later = frame.predict(&motion, 0.5);
        // Half a second at 10 m/s, and a quarter turn about the centre: +X goes to -Z.
        assert!(
            later
                .translation
                .abs_diff_eq(Vec3::new(0.0, 0.0, -6.0), 1e-4),
            "{later:?}"
        );
        assert!(
            (later.rotation * Vec3::X).abs_diff_eq(Vec3::NEG_Z, 1e-4),
            "{later:?}"
        );
        assert_eq!(frame.predict(&SupportMotion::REST, 0.5), frame);
        assert_eq!(frame.predict(&motion, f32::NAN), frame);
    }
}
