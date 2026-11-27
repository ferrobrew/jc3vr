//! The character's motion a carried object rides along with: the hands ride on the character, so
//! a carried object the hands can lift takes on a share of the character's own motion between
//! two ticks, turn included, rather than chasing it through the hands' strength.

use body_rig::math::rotation::angular_velocity;
use glam::{Quat, Vec3};
use jc3vr_body::controllers::physics::rigid::Velocities;
use jc3vr_input::frame::Pose;

use crate::controllers::physical_hands::pickup::tuning::TransportTuning;

#[derive(Debug, Copy, Clone, Default)]
pub(super) struct Transport {
    previous: Velocities,
    current: Velocities,
    rotation: Quat,
    origin: Vec3,
    previous_origin: Vec3,
    share: f32,
}

impl Transport {
    pub fn between(
        previous: Option<(Pose, Velocities)>,
        current: Option<(Pose, Vec3)>,
        dt: f32,
        share: f32,
        limits: &TransportTuning,
    ) -> Self {
        let Some((now, linear)) = current.filter(|(pose, v)| finite_pose(*pose) && v.is_finite())
        else {
            return Self::default();
        };
        let Some((before, previous)) = previous else {
            return Self {
                current: Velocities {
                    linear,
                    angular: Vec3::ZERO,
                },
                origin: now.position,
                ..Self::default()
            };
        };
        if !(dt.is_finite() && dt > 0.0 && finite_pose(before) && finite_pose(now))
            || !linear.is_finite()
            || !previous.linear.is_finite()
            || !previous.angular.is_finite()
        {
            return Self::default();
        }
        let rotation =
            (now.orientation.normalize() * before.orientation.normalize().inverse()).normalize();
        let angular = angular_velocity(before.orientation, now.orientation, dt)
            .unwrap_or(Vec3::ZERO)
            .clamp_length_max(limits.max_angular_speed_rps);
        Self {
            previous,
            current: Velocities {
                linear: ((now.position - before.position) / dt)
                    .clamp_length_max(limits.max_linear_speed_mps),
                angular,
            },
            rotation,
            origin: now.position,
            previous_origin: before.position,
            share: if share.is_finite() {
                share.clamp(0.0, 1.0)
            } else {
                0.0
            },
        }
    }

    pub fn linear(&self) -> Vec3 {
        self.current.linear
    }

    pub fn angular(&self) -> Vec3 {
        self.current.angular
    }

    pub fn frame(&self, centre: Vec3) -> Velocities {
        Velocities {
            linear: self.current.linear + self.current.angular.cross(centre - self.origin),
            angular: self.current.angular,
        }
    }

    pub fn carry(&self, centre: Vec3, velocity: Velocities) -> Velocities {
        let current = self.frame(centre);
        let previous_offset = self.rotation.inverse() * (centre - self.origin);
        let previous_centre = self.previous_origin + previous_offset;
        let old_linear = self.previous.linear
            + self
                .previous
                .angular
                .cross(previous_centre - self.previous_origin);
        let linear = current.linear + self.rotation * (velocity.linear - old_linear);
        let angular = current.angular + self.rotation * (velocity.angular - self.previous.angular);
        Velocities {
            linear: velocity.linear + (linear - velocity.linear) * self.share,
            angular: velocity.angular + (angular - velocity.angular) * self.share,
        }
    }
}

fn finite_pose(pose: Pose) -> bool {
    pose.position.is_finite()
        && pose.orientation.is_finite()
        && pose.orientation.length_squared() > 1e-8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::physical_hands::pickup::PICKUP;

    fn limits() -> &'static TransportTuning {
        &PICKUP.get().transport
    }

    fn pose(position: Vec3, orientation: Quat) -> Pose {
        Pose {
            position,
            orientation,
        }
    }

    #[test]
    fn translation_and_turn_carry_the_same_rigid_frame() {
        let before = pose(Vec3::ZERO, Quat::IDENTITY);
        let now = pose(Vec3::X, Quat::from_rotation_y(0.1));
        let transport = Transport::between(
            Some((before, Velocities::default())),
            Some((now, Vec3::X * 10.0)),
            0.1,
            1.0,
            limits(),
        );
        let centre = now.position + now.orientation * Vec3::Z;
        let carried = transport.carry(centre, Velocities::default());
        let frame = transport.frame(centre);
        assert!(carried.linear.abs_diff_eq(frame.linear, 1e-4));
        assert!(carried.angular.abs_diff_eq(Vec3::Y, 1e-4));
    }

    #[test]
    fn steady_transport_preserves_relative_motion_and_collision_losses() {
        let velocity = Velocities {
            linear: Vec3::X * 8.0,
            angular: Vec3::Y,
        };
        let frame = Velocities {
            linear: Vec3::X * 10.0,
            angular: Vec3::ZERO,
        };
        let transport = Transport::between(
            Some((pose(Vec3::ZERO, Quat::IDENTITY), frame)),
            Some((pose(Vec3::X, Quat::IDENTITY), frame.linear)),
            0.1,
            1.0,
            limits(),
        );
        assert_eq!(transport.carry(Vec3::Z, velocity), velocity);
    }

    #[test]
    fn first_sample_and_disabled_transport_do_not_add_momentum() {
        let now = pose(Vec3::ZERO, Quat::IDENTITY);
        let velocity = Velocities::default();
        for transport in [
            Transport::between(None, Some((now, Vec3::X)), 0.1, 1.0, limits()),
            Transport::between(
                Some((now, velocity)),
                Some((now, Vec3::X)),
                0.1,
                0.0,
                limits(),
            ),
            Transport::between(
                Some((now, velocity)),
                Some((now, Vec3::X)),
                f32::NAN,
                1.0,
                limits(),
            ),
        ] {
            assert_eq!(transport.carry(Vec3::ZERO, velocity), velocity);
        }
    }
}
