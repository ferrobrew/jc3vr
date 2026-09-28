//! The hand rays, all pure: the gun sight line from a gun's transform and fire point, the
//! controller's aim ray, the grapple ray from the gauntlet, the aim point a gun is given, and the
//! engine-convention view matrix that stands in for the camera while the grapple is aimed.
//!
//! Matrices here are glam's column-major `Mat4`, whose columns are the rows of the engine's
//! row-major matrices (`Mat4::from(Matrix4)`): the translation is `w_axis`, and the engine's
//! forward, the negated third row, is `-z_axis`.

use glam::{EulerRot, Mat4, Quat, Vec2, Vec3};

use crate::controllers::{config::SightOverride, frame::Pose};

/// A world-space ray with a unit direction.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Ray {
    pub origin: Vec3,
    pub direction: Vec3,
}

impl Ray {
    /// A ray from `origin` along `direction`, or `None` when either is not finite or the direction
    /// has no length.
    pub fn new(origin: Vec3, direction: Vec3) -> Option<Ray> {
        let direction = direction.try_normalize()?;
        origin.is_finite().then_some(Ray { origin, direction })
    }

    /// The point `distance` along the ray.
    pub fn at(&self, distance: f32) -> Vec3 {
        self.origin + self.direction * distance
    }
}

/// The ray along a pose's -Z axis, the OpenXR aim pose's pointing direction.
pub fn pose_ray(pose: Pose) -> Option<Ray> {
    Ray::new(pose.position, pose.orientation * Vec3::NEG_Z)
}

/// The grapple ray: from the gauntlet, `origin_offset` in the wrist frame of `wrist`, along the
/// controller's aim direction.
pub fn grapple_ray(wrist: Pose, aim: Pose, origin_offset: Vec3) -> Option<Ray> {
    Ray::new(
        wrist.position + wrist.orientation * origin_offset,
        aim.orientation * Vec3::NEG_Z,
    )
}

/// The gun's world transform with a hand's correction applied: `correction × gun`. The correction
/// carries the gun from where the last simulation tick placed it to where the controller holds it
/// now, as the render-time late update does for the rendered gun.
pub fn corrected_gun(gun: Mat4, correction: Option<Pose>) -> Mat4 {
    match correction {
        Some(d) => Mat4::from_rotation_translation(d.orientation, d.position) * gun,
        None => gun,
    }
}

/// The sight line of a gun whose world transform is `gun` and whose current fire point is
/// `fire_point` (in the gun's frame): from the fire point along its barrel axis, the fire point's
/// -Z, which is the axis the game fires along when it fires along the barrel. `sight` moves the
/// origin and turns the direction in the fire point's frame.
pub fn sight_ray(gun: Mat4, fire_point: Mat4, sight: Option<&SightOverride>) -> Option<Ray> {
    let composed = gun * fire_point;
    let (offset, angles) = sight.map_or((Vec3::ZERO, Vec2::ZERO), |s| (s.offset_m, s.angles_deg));
    let local = sight_turn(angles) * Vec3::NEG_Z;
    Ray::new(
        composed.transform_point3(offset),
        composed.transform_vector3(local),
    )
}

/// The rotation from the barrel axis to the sight line: `angles.x` pitches up (about +X) and
/// `angles.y` yaws left (about +Y), degrees.
fn sight_turn(angles: Vec2) -> Quat {
    Quat::from_euler(
        EulerRot::YXZ,
        angles.y.to_radians(),
        angles.x.to_radians(),
        0.0,
    )
}

/// The point a gun aims at along `ray`: the hit at `hit_distance`, or `max_distance` along the
/// ray when nothing was hit, never nearer than `min_distance`. Returns the point and its distance.
pub fn aim_point(
    ray: &Ray,
    hit_distance: Option<f32>,
    max_distance: f32,
    min_distance: f32,
) -> (Vec3, f32) {
    let min = min_distance.max(0.0);
    let distance = hit_distance.unwrap_or(max_distance).max(min);
    (ray.at(distance), distance)
}

/// How far along its forward the player aim's raycast starts from the aim matrix's translation.
pub const DIRECT_AIM_START_OFFSET: f32 = 0.1;

/// The engine-convention camera matrix whose aim ray is `ray`: forward (`-z_axis`) along the ray,
/// up as close to world up as the ray allows, and the translation set back so that the player
/// aim's raycast, which starts [`DIRECT_AIM_START_OFFSET`] along the forward, starts at the ray's
/// origin.
pub fn view_matrix(ray: &Ray) -> Mat4 {
    let back = -ray.direction;
    let right = Vec3::Y
        .cross(back)
        .try_normalize()
        .unwrap_or_else(|| Vec3::Z.cross(back).normalize());
    let up = back.cross(right);
    let translation = ray.origin - ray.direction * DIRECT_AIM_START_OFFSET;
    Mat4::from_cols(
        right.extend(0.0),
        up.extend(0.0),
        back.extend(0.0),
        translation.extend(1.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-4;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < EPS
    }

    #[test]
    fn a_degenerate_ray_is_refused() {
        assert!(Ray::new(Vec3::ZERO, Vec3::ZERO).is_none());
        assert!(Ray::new(Vec3::NAN, Vec3::X).is_none());
        let ray = Ray::new(Vec3::ONE, Vec3::new(0.0, 0.0, -3.0)).unwrap();
        assert!(close(ray.direction, Vec3::NEG_Z));
    }

    #[test]
    fn the_pose_ray_points_along_negative_z() {
        let pose = Pose {
            position: Vec3::new(1.0, 2.0, 3.0),
            orientation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        };
        let ray = pose_ray(pose).unwrap();
        assert!(close(ray.origin, pose.position));
        assert!(close(ray.direction, Vec3::NEG_X));
    }

    #[test]
    fn the_grapple_ray_starts_at_the_offset_wrist_and_follows_the_aim() {
        let wrist = Pose {
            position: Vec3::new(0.0, 1.0, 0.0),
            orientation: Quat::from_rotation_y(std::f32::consts::PI),
        };
        let aim = Pose {
            position: Vec3::new(5.0, 5.0, 5.0),
            orientation: Quat::IDENTITY,
        };
        let ray = grapple_ray(wrist, aim, Vec3::new(0.0, 0.0, 0.1)).unwrap();
        assert!(close(ray.origin, Vec3::new(0.0, 1.0, -0.1)));
        assert!(close(ray.direction, Vec3::NEG_Z));
    }

    #[test]
    fn the_sight_follows_the_fire_point_barrel() {
        let gun = Mat4::from_rotation_translation(
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            Vec3::new(10.0, 0.0, 0.0),
        );
        let fire_point = Mat4::from_translation(Vec3::new(0.0, 0.05, -0.3));
        let ray = sight_ray(gun, fire_point, None).unwrap();
        assert!(close(ray.origin, Vec3::new(10.0 - 0.3, 0.05, 0.0)));
        assert!(close(ray.direction, Vec3::NEG_X));
    }

    #[test]
    fn a_sight_override_moves_and_turns_the_line() {
        let sight = SightOverride {
            weapon_hash: 1,
            offset_m: Vec3::new(0.0, 0.04, 0.0),
            angles_deg: Vec2::new(90.0, 0.0),
        };
        let ray = sight_ray(Mat4::IDENTITY, Mat4::IDENTITY, Some(&sight)).unwrap();
        assert!(close(ray.origin, Vec3::new(0.0, 0.04, 0.0)));
        assert!(close(ray.direction, Vec3::Y));

        let yawed = SightOverride {
            angles_deg: Vec2::new(0.0, 90.0),
            ..sight
        };
        let ray = sight_ray(Mat4::IDENTITY, Mat4::IDENTITY, Some(&yawed)).unwrap();
        assert!(close(ray.direction, Vec3::NEG_X));
    }

    #[test]
    fn the_correction_premultiplies_the_gun() {
        let gun = Mat4::from_translation(Vec3::new(1.0, 0.0, 0.0));
        let d = Pose {
            position: Vec3::new(0.0, 1.0, 0.0),
            orientation: Quat::IDENTITY,
        };
        let corrected = corrected_gun(gun, Some(d));
        assert!(close(corrected.w_axis.truncate(), Vec3::new(1.0, 1.0, 0.0)));
        assert_eq!(corrected_gun(gun, None), gun);
    }

    #[test]
    fn the_aim_point_is_the_hit_or_the_far_point_but_never_near() {
        let ray = Ray::new(Vec3::ZERO, Vec3::NEG_Z).unwrap();
        assert_eq!(aim_point(&ray, Some(12.0), 500.0, 2.0).1, 12.0);
        assert_eq!(aim_point(&ray, None, 500.0, 2.0).1, 500.0);
        let (point, distance) = aim_point(&ray, Some(0.2), 500.0, 2.0);
        assert_eq!(distance, 2.0);
        assert!(close(point, Vec3::new(0.0, 0.0, -2.0)));
    }

    #[test]
    fn the_view_matrix_casts_the_ray() {
        for direction in [Vec3::NEG_Z, Vec3::new(1.0, 0.3, 0.2), Vec3::Y, Vec3::NEG_Y] {
            let ray = Ray::new(Vec3::new(3.0, 1.0, -2.0), direction).unwrap();
            let m = view_matrix(&ray);
            let forward = -m.z_axis.truncate();
            assert!(close(forward, ray.direction));
            let start = m.w_axis.truncate() + forward * DIRECT_AIM_START_OFFSET;
            assert!(close(start, ray.origin));
            let rotation = glam::Mat3::from_mat4(m);
            assert!((rotation.determinant() - 1.0).abs() < EPS);
            assert!(close(
                m.x_axis.truncate().cross(m.y_axis.truncate()),
                m.z_axis.truncate()
            ));
        }
    }

    #[test]
    fn the_view_matrix_keeps_up_near_world_up() {
        let ray = Ray::new(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)).unwrap();
        let m = view_matrix(&ray);
        assert!(close(m.y_axis.truncate(), Vec3::Y));
    }
}
