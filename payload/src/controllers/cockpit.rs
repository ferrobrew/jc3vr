//! The map between the cockpit frame and the world. Pure, but for [`CockpitTransform::current`],
//! which reads where the camera puts the eyes.
//!
//! The cockpit frame holds the player's tracked body at the player's own scale, about the HMD's
//! eye midpoint. A cockpit point lands at its offset from the cockpit head, turned by the body
//! frame and scaled by the world scale, from the eye midpoint the camera renders from
//! ([`hooks::camera::eye_point`]). That is the published head (`headpose::xr::compose`, which
//! follows Rico's head joint) with the eye rig's arm hung off it, so a controller lands where the
//! player sees their hand through the rendered eyes. [`CockpitTransform`] is that composition and
//! its inverse for points, displacements, directions, orientations, and poses, so that the hands,
//! the body, the vehicles' controls, and everything else placed from the cockpit agree with the
//! view.

use glam::{Quat, Vec3};

use crate::{controllers::frame::Pose, headpose, hooks};

/// The map between the world and the cockpit frame at one moment: the cockpit frame's
/// `head_cockpit` sits at the world's `head_world`, turned by `body` and scaled by `world_scale`.
///
/// The scale is used no smaller than [`MIN_WORLD_SCALE`] both ways, so the two directions stay each
/// other's inverse and a degenerate scale (zero, negative, or NaN) cannot divide by zero.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct CockpitTransform {
    /// The head in the world: for [`current`](Self::current), the eye midpoint the camera renders
    /// from, not the published head position it hangs from.
    pub head_world: Vec3,
    /// The head in the cockpit frame.
    pub head_cockpit: Vec3,
    /// The body frame the cockpit composes with.
    pub body: Quat,
    /// World metres per cockpit metre.
    pub world_scale: f32,
}

impl CockpitTransform {
    /// The transform the view composes now, with the head at `head_cockpit` in the cockpit frame:
    /// the rendered eye midpoint ([`hooks::camera::eye_point`]), the body frame, and
    /// `world_scale`.
    pub fn current(head_cockpit: Vec3, world_scale: f32) -> Self {
        Self {
            head_world: hooks::camera::eye_point(),
            head_cockpit,
            body: headpose::xr::body_rotation(),
            world_scale,
        }
    }

    /// The transform whose cockpit origin sits at the world's `origin`: the head pose's own
    /// composition, which places the cockpit frame from its origin rather than from the head.
    pub fn from_origin(origin: Vec3, body: Quat, world_scale: f32) -> Self {
        Self {
            head_world: origin,
            head_cockpit: Vec3::ZERO,
            body,
            world_scale,
        }
    }

    /// Whether the transform can carry a point both ways: finite, with a positive world scale.
    pub fn is_usable(&self) -> bool {
        self.head_world.is_finite()
            && self.head_cockpit.is_finite()
            && self.body.is_finite()
            && self.world_scale.is_finite()
            && self.world_scale > 0.0
    }

    /// World metres per cockpit metre, no smaller than [`MIN_WORLD_SCALE`].
    pub fn scale(&self) -> f32 {
        self.world_scale.max(MIN_WORLD_SCALE)
    }

    pub fn point_to_world(&self, cockpit: Vec3) -> Vec3 {
        self.head_world + self.offset_to_world(cockpit - self.head_cockpit)
    }

    pub fn point_to_cockpit(&self, world: Vec3) -> Vec3 {
        self.head_cockpit + self.offset_to_cockpit(world - self.head_world)
    }

    /// A cockpit-frame point as a world-space offset from the head.
    pub fn point_from_head(&self, cockpit: Vec3) -> Vec3 {
        self.offset_to_world(cockpit - self.head_cockpit)
    }

    /// A cockpit-frame displacement as a world-space one: turned by the body frame and scaled.
    pub fn offset_to_world(&self, cockpit: Vec3) -> Vec3 {
        self.body * (cockpit * self.scale())
    }

    /// A world-space displacement as a cockpit-frame one: turned into the body frame and scaled.
    pub fn offset_to_cockpit(&self, world: Vec3) -> Vec3 {
        self.body.inverse() * world / self.scale()
    }

    /// A cockpit-frame direction (or angular velocity) in the world: turned, not scaled.
    pub fn direction_to_world(&self, cockpit: Vec3) -> Vec3 {
        self.body * cockpit
    }

    /// A world-space direction (or angular velocity) in the cockpit frame: turned, not scaled.
    pub fn direction_to_cockpit(&self, world: Vec3) -> Vec3 {
        self.body.inverse() * world
    }

    pub fn rotation_to_world(&self, cockpit: Quat) -> Quat {
        (self.body * cockpit).normalize()
    }

    pub fn pose_to_world(&self, cockpit: Pose) -> Pose {
        Pose {
            position: self.point_to_world(cockpit.position),
            orientation: self.rotation_to_world(cockpit.orientation),
        }
    }
}

/// The smallest world scale the transform divides by.
pub const MIN_WORLD_SCALE: f32 = 1e-3;

#[cfg(test)]
mod tests {
    use test_case::test_case;

    use super::*;

    fn transform() -> CockpitTransform {
        CockpitTransform {
            head_world: Vec3::new(100.0, 20.0, -50.0),
            head_cockpit: Vec3::new(0.0, 1.6, 0.1),
            body: Quat::from_rotation_y(0.7),
            world_scale: 1.25,
        }
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        a.abs_diff_eq(b, 1e-4)
    }

    #[test]
    fn the_head_lands_on_the_head() {
        let xf = transform();
        assert!(close(xf.point_to_world(xf.head_cockpit), xf.head_world));
        assert!(close(xf.point_to_cockpit(xf.head_world), xf.head_cockpit));
    }

    #[test]
    fn a_point_turns_and_scales_about_the_head() {
        let xf = transform();
        let p = xf.head_cockpit + Vec3::NEG_Z * 0.4;
        let expected = xf.head_world + Quat::from_rotation_y(0.7) * Vec3::NEG_Z * 0.5;
        assert!(close(xf.point_to_world(p), expected));
        assert!(close(xf.point_from_head(p), expected - xf.head_world));
    }

    #[test]
    fn every_kind_round_trips() {
        let xf = transform();
        let p = Vec3::new(0.1, -0.4, -0.3);
        assert!(close(xf.point_to_cockpit(xf.point_to_world(p)), p));
        assert!(close(xf.offset_to_cockpit(xf.offset_to_world(p)), p));
        assert!(close(xf.direction_to_cockpit(xf.direction_to_world(p)), p));
        let pose = Pose {
            position: p,
            orientation: Quat::from_rotation_x(0.3),
        };
        let world = xf.pose_to_world(pose);
        assert!(close(world.position, xf.point_to_world(p)));
        let expected = Quat::from_rotation_y(0.7) * pose.orientation;
        assert!(world.orientation.angle_between(expected) < 1e-4);
    }

    #[test]
    fn a_direction_is_not_scaled() {
        let xf = transform();
        assert!((xf.direction_to_world(Vec3::X).length() - 1.0).abs() < 1e-6);
        assert!((xf.offset_to_world(Vec3::X).length() - 1.25).abs() < 1e-6);
    }

    #[test]
    fn the_origin_form_composes_from_the_cockpit_origin() {
        let xf = CockpitTransform::from_origin(Vec3::new(5.0, 0.0, 0.0), Quat::IDENTITY, 2.0);
        assert!(close(
            xf.point_to_world(Vec3::new(0.0, 1.5, 0.0)),
            Vec3::new(5.0, 3.0, 0.0)
        ));
    }

    #[test_case(0.0; "zero")]
    #[test_case(-1.0; "negative")]
    #[test_case(f32::NAN; "nan")]
    fn a_degenerate_scale_is_floored_and_stays_invertible(world_scale: f32) {
        let xf = CockpitTransform {
            world_scale,
            ..transform()
        };
        assert_eq!(xf.scale(), MIN_WORLD_SCALE);
        assert!(!xf.is_usable());
        let p = Vec3::new(0.1, -0.4, -0.3);
        assert!(xf.point_to_world(p).is_finite());
        assert!(close(xf.offset_to_cockpit(xf.offset_to_world(p)), p));
    }
}
