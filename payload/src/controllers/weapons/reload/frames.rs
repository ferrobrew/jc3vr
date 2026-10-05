//! The gun's model frame carried into the cockpit frame, where the hands are. Pure.
//!
//! The gun's last placement followed its hand's wrist target, so the gun's world matrix relative to
//! that target is the gun relative to the wrist ([`gun_in_wrist`]), the same measurement the
//! foregrip makes. Composed with the hand's cockpit wrist, it gives the gun's model frame in the
//! cockpit frame ([`GunFrame`]), in which the wells table's points and axes are placed. The
//! world's lengths are the VR world scale times the player's, so model-frame lengths are divided
//! by it.

use glam::{Mat4, Vec3};

use crate::controllers::{
    frame::{Hand, Pose},
    hands::pose as hand_pose,
    weapons::reload::plan::WellFrame,
};

/// A gun's model frame in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GunFrame {
    pub pose: Pose,
    /// The VR world scale.
    pub world_scale: f32,
}

impl GunFrame {
    /// The gun's frame held by a wrist at `wrist` (cockpit frame) with the gun at `in_wrist`
    /// relative to it.
    pub fn of(wrist: Pose, in_wrist: Pose, world_scale: f32) -> Self {
        Self {
            pose: hand_pose::mul(wrist, in_wrist),
            world_scale,
        }
    }

    /// A model-frame point in the cockpit frame.
    pub fn point(&self, local: Vec3) -> Vec3 {
        self.pose.position + self.pose.orientation * (local / self.world_scale)
    }

    /// A model-frame direction in the cockpit frame.
    pub fn direction(&self, local: Vec3) -> Vec3 {
        self.pose.orientation * local
    }

    /// The gun's rearward axis: the model's +Z, from the muzzle toward the stock.
    pub fn rearward(&self) -> Vec3 {
        self.direction(Vec3::Z)
    }
}

/// A gun's world matrix relative to the world-frame wrist target its placement followed, with its
/// translation in the player's metres. `None` for a degenerate matrix or scale.
pub fn gun_in_wrist(gun_world: Mat4, wrist_world: Pose, world_scale: f32) -> Option<Pose> {
    if !(world_scale.is_finite() && world_scale > 0.0) {
        return None;
    }
    let (scale, rotation, translation) = gun_world.to_scale_rotation_translation();
    if !(scale.is_finite() && rotation.is_finite() && translation.is_finite()) || scale.x <= 0.0 {
        return None;
    }
    let to_wrist = hand_pose::inverse(wrist_world);
    let local = hand_pose::mul(
        to_wrist,
        Pose {
            position: translation,
            orientation: rotation.normalize(),
        },
    );
    Some(Pose {
        position: local.position / world_scale,
        orientation: local.orientation,
    })
}

/// A well's mouth and insertion axis in the cockpit frame: on the gun for a table entry, or at the
/// configured offset from the holding wrist (mirrored for a left hand), its axis toward the wrist.
pub fn well(
    frame: WellFrame,
    gun: Option<&GunFrame>,
    wrist: Pose,
    holder: Hand,
) -> Option<(Vec3, Vec3)> {
    match frame {
        WellFrame::Gun { mouth, axis } => {
            let gun = gun?;
            Some((gun.point(mouth), gun.direction(axis).try_normalize()?))
        }
        WellFrame::Wrist { offset } => {
            let local = match holder {
                Hand::Left => Vec3::new(-offset.x, offset.y, offset.z),
                Hand::Right => offset,
            };
            let axis = wrist.orientation * (-local).try_normalize()?;
            Some((wrist.position + wrist.orientation * local, axis))
        }
    }
}

/// The direction a closed hand at `grip` points: the grip pose's -Z.
pub fn pointing(grip: Pose) -> Vec3 {
    grip.orientation * Vec3::NEG_Z
}

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    #[test]
    fn the_gun_relative_to_the_wrist_round_trips_through_the_cockpit() {
        let wrist_world = Pose {
            position: Vec3::new(10.0, 2.0, -3.0),
            orientation: Quat::from_rotation_y(0.7),
        };
        let gun_local = Pose {
            position: Vec3::new(0.0, -0.02, -0.05),
            orientation: Quat::from_rotation_x(0.2),
        };
        let scale = 1.0;
        let gun_world = hand_pose::mul(wrist_world, gun_local);
        let measured = gun_in_wrist(
            Mat4::from_rotation_translation(gun_world.orientation, gun_world.position),
            wrist_world,
            scale,
        )
        .unwrap();
        assert!((measured.position - gun_local.position).length() < 1e-4);
        assert!(measured.orientation.angle_between(gun_local.orientation) < 1e-4);

        let cockpit_wrist = Pose {
            position: Vec3::new(0.3, -0.4, -0.3),
            orientation: Quat::IDENTITY,
        };
        let frame = GunFrame::of(cockpit_wrist, measured, scale);
        let mouth = frame.point(Vec3::new(0.0, -0.1, 0.0));
        let expected = cockpit_wrist.position
            + gun_local.position
            + gun_local.orientation * Vec3::new(0.0, -0.1, 0.0);
        assert!((mouth - expected).length() < 1e-4);
    }

    #[test]
    fn model_lengths_shrink_with_the_world_scale() {
        let frame = GunFrame {
            pose: Pose::IDENTITY,
            world_scale: 2.0,
        };
        assert_eq!(
            frame.point(Vec3::new(0.0, 0.2, 0.0)),
            Vec3::new(0.0, 0.1, 0.0)
        );
        assert!(gun_in_wrist(Mat4::IDENTITY, Pose::IDENTITY, 0.0).is_none());
    }

    #[test]
    fn the_wrist_well_mirrors_and_points_at_the_wrist() {
        let offset = Vec3::new(0.03, -0.08, -0.06);
        let (right, axis) = well(
            WellFrame::Wrist { offset },
            None,
            Pose::IDENTITY,
            Hand::Right,
        )
        .unwrap();
        assert_eq!(right, offset);
        assert!((axis + offset.normalize()).length() < 1e-6);
        let (left, _) = well(
            WellFrame::Wrist { offset },
            None,
            Pose::IDENTITY,
            Hand::Left,
        )
        .unwrap();
        assert_eq!(left, Vec3::new(-0.03, -0.08, -0.06));
        assert!(
            well(
                WellFrame::Gun {
                    mouth: Vec3::ZERO,
                    axis: Vec3::Y
                },
                None,
                Pose::IDENTITY,
                Hand::Right
            )
            .is_none()
        );
    }
}
