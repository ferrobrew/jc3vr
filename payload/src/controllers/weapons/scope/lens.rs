//! The lens's geometry, all pure: where the eyepiece is, which eye sights through it, and the
//! constants of the lens draw.
//!
//! The sight line is kept in the holding hand's wrist frame between aim updates, so the lens follows
//! the controller at frame rate while the sight line itself is refreshed at the sim rate. The
//! eyepiece is on the sight line, [`ScopeConfig::eyepiece_back_m`] behind its origin, and the lens
//! is a disc there, facing back along the sight line.
//!
//! The lens magnifies the view around the sight line: a point of the lens that the eye sees at a
//! tangent `t` off the sight line shows the scene at `t / zoom` off it. The point it shows is taken
//! at the sight line's focus distance (the aim point's distance), projected into the source eye's
//! image, so the image is correct at the target and the parallax between the eyes vanishes there.

use glam::{Mat4, Vec3};

use crate::controllers::{
    aim::ray::Ray,
    frame::{Hand, Pose},
    hands::pose as hand_pose,
    weapons::config::{LensSource, ScopeConfig},
};

/// The sight line in the holding hand's wrist frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SightAnchor {
    pub hand: Hand,
    /// The sight line's origin, in the wrist frame.
    pub origin: Vec3,
    /// The sight line's direction, in the wrist frame.
    pub direction: Vec3,
    /// The distance along the sight line to the aim point, metres.
    pub focus_m: f32,
}

impl SightAnchor {
    /// The sight line `ray` of the gun in `hand`, whose wrist is at `wrist`, focused at `focus_m`.
    pub fn new(hand: Hand, ray: &Ray, wrist: Pose, focus_m: f32) -> SightAnchor {
        let to_wrist = hand_pose::inverse(wrist);
        SightAnchor {
            hand,
            origin: to_wrist.position + to_wrist.orientation * ray.origin,
            direction: to_wrist.orientation * ray.direction,
            focus_m: focus_m.max(MIN_FOCUS_M),
        }
    }

    /// The sight line in the world for the wrist at `wrist`.
    pub fn place(&self, wrist: Pose) -> Option<Ray> {
        Ray::new(
            wrist.position + wrist.orientation * self.origin,
            wrist.orientation * self.direction,
        )
    }
}

/// The scope's lens in the world for a frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Lens {
    /// The eyepiece's centre.
    pub eyepiece: Vec3,
    /// The sight line's direction, a unit vector.
    pub direction: Vec3,
    /// The distance along the sight line to the aim point, metres.
    pub focus_m: f32,
}

impl Lens {
    /// The lens of the scope on `sight`, focused at `focus_m`.
    pub fn new(sight: &Ray, focus_m: f32, config: &ScopeConfig) -> Lens {
        Lens {
            eyepiece: sight.at(-config.eyepiece_back_m),
            direction: sight.direction,
            focus_m: focus_m.max(MIN_FOCUS_M),
        }
    }
}

/// The eye nearest the eyepiece and its distance, among the eyes whose positions are known.
pub fn nearest_eye(eyepiece: Vec3, eyes: [Option<Vec3>; 2]) -> Option<(usize, f32)> {
    eyes.iter()
        .enumerate()
        .filter_map(|(i, eye)| eye.map(|p| (i, p.distance(eyepiece))))
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// The eye that sights through the lens: the nearest one, when it is within `max_distance` of the
/// eyepiece and behind it (looking along the sight line), so that a scope held backwards or beside
/// the head draws nothing.
pub fn sighting_eye(lens: &Lens, eyes: [Option<Vec3>; 2], max_distance: f32) -> Option<usize> {
    let (eye, distance) = nearest_eye(lens.eyepiece, eyes)?;
    let behind = (lens.eyepiece - eyes[eye]?).dot(lens.direction) > 0.0;
    (distance <= max_distance && behind).then_some(eye)
}

/// The eye whose image the lens reads: the other eye when `source` asks for it and its image is
/// available, the sighting eye's own otherwise.
pub fn source_eye(sighting: usize, source: LensSource, other_available: bool) -> usize {
    match source {
        LensSource::OtherEye if other_available => 1 - sighting.min(1),
        _ => sighting,
    }
}

/// Two unit vectors that span the lens's plane with `direction`, right and up as seen looking along
/// it. The lens is round and its image is mapped through the same vectors, so their roll about the
/// sight line does not change what the lens shows.
pub fn lens_basis(direction: Vec3) -> (Vec3, Vec3) {
    let right = direction
        .cross(Vec3::Y)
        .try_normalize()
        .unwrap_or_else(|| direction.cross(Vec3::Z).normalize());
    let up = right.cross(direction);
    (right, up)
}

/// The tangent off the sight line that the lens's rim shows: the rim's tangent from an eye
/// `eye_distance` from the lens, divided by the zoom.
pub fn field_tangent(radius: f32, eye_distance: f32, zoom: f32) -> f32 {
    radius / eye_distance.max(MIN_EYE_DISTANCE_M) / zoom.max(1.0)
}

/// The constants of the lens shaders' `cbuffer Lens`.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct LensConstants {
    /// The sighting eye's view-projection of a position relative to it.
    pub view_projection: [f32; 16],
    /// The source eye's view-projection of a position relative to it.
    pub source_view_projection: [f32; 16],
    pub eyepiece_from_eye: [f32; 3],
    pub radius: f32,
    pub right: [f32; 3],
    pub tangent: f32,
    pub up: [f32; 3],
    pub focus: f32,
    pub direction: [f32; 3],
    pub vignette_start: f32,
    pub eyepiece_from_source: [f32; 3],
    /// 1 to draw the reticle, 0 not to.
    pub reticle: f32,
}

/// An eye's camera as the lens needs it: its position and the view-projection of a position
/// relative to it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct EyeCamera {
    pub position: Vec3,
    pub offset_view_projection: Mat4,
}

/// The lens draw's constants for the sighting eye `eye`, reading the image of `source`.
pub fn constants(
    lens: &Lens,
    eye: &EyeCamera,
    source: &EyeCamera,
    config: &ScopeConfig,
) -> LensConstants {
    let (right, up) = lens_basis(lens.direction);
    let eyepiece_from_eye = lens.eyepiece - eye.position;
    LensConstants {
        view_projection: eye.offset_view_projection.to_cols_array(),
        source_view_projection: source.offset_view_projection.to_cols_array(),
        eyepiece_from_eye: eyepiece_from_eye.to_array(),
        radius: config.lens_radius_m,
        right: right.to_array(),
        tangent: field_tangent(
            config.lens_radius_m,
            eyepiece_from_eye.length(),
            config.zoom,
        ),
        up: up.to_array(),
        focus: lens.focus_m,
        direction: lens.direction.to_array(),
        vignette_start: config.vignette_start.clamp(0.0, 0.99),
        eyepiece_from_source: (lens.eyepiece - source.position).to_array(),
        reticle: if config.reticle { 1.0 } else { 0.0 },
    }
}

/// The nearest the aim point is taken to be: a closer point would make the other eye's image
/// swing with the tiny parallax between the eyes.
const MIN_FOCUS_M: f32 = 1.0;

/// The nearest an eye is taken to be to the lens, so the field stays finite.
const MIN_EYE_DISTANCE_M: f32 = 0.01;

#[cfg(test)]
mod tests {
    use glam::{Quat, Vec2, Vec4};

    use super::*;

    /// The scene point the lens shows at `q` (the lens's unit disc, +x right and +y up), relative to
    /// the source eye: `eyepiece_from_source` is the eyepiece relative to that eye. The lens pixel shader
    /// computes the same.
    fn sample_point(lens: &Lens, eyepiece_from_source: Vec3, tangent: f32, q: glam::Vec2) -> Vec3 {
        let (right, up) = lens_basis(lens.direction);
        let direction = (lens.direction + (right * q.x + up * q.y) * tangent).normalize();
        eyepiece_from_source + direction * lens.focus_m
    }

    fn config() -> ScopeConfig {
        ScopeConfig::new()
    }

    #[test]
    fn the_anchor_round_trips_through_the_wrist() {
        let wrist = Pose {
            position: Vec3::new(10.0, 2.0, -3.0),
            orientation: Quat::from_rotation_y(0.8) * Quat::from_rotation_x(-0.3),
        };
        let ray = Ray::new(Vec3::new(10.2, 2.1, -3.5), Vec3::new(0.1, 0.0, -1.0)).unwrap();
        let anchor = SightAnchor::new(Hand::Right, &ray, wrist, 120.0);
        let placed = anchor.place(wrist).unwrap();
        assert!(placed.origin.distance(ray.origin) < 1e-4);
        assert!(placed.direction.distance(ray.direction) < 1e-5);

        let moved = Pose {
            position: wrist.position + Vec3::X,
            orientation: wrist.orientation,
        };
        let placed = anchor.place(moved).unwrap();
        assert!(placed.origin.distance(ray.origin + Vec3::X) < 1e-4);
    }

    #[test]
    fn the_eyepiece_is_behind_the_sight_origin() {
        let sight = Ray::new(Vec3::ZERO, Vec3::NEG_Z).unwrap();
        let lens = Lens::new(&sight, 50.0, &config());
        assert!((lens.eyepiece - Vec3::Z * config().eyepiece_back_m).length() < 1e-6);
    }

    #[test]
    fn the_sighting_eye_is_the_nearest_behind_the_lens() {
        let lens = Lens {
            eyepiece: Vec3::new(-0.03, 0.0, 0.0),
            direction: Vec3::NEG_Z,
            focus_m: 100.0,
        };
        let cases: [([Option<Vec3>; 2], Option<usize>); 4] = [
            // The left eye is nearer.
            (
                [
                    Some(Vec3::new(-0.032, 0.0, 0.1)),
                    Some(Vec3::new(0.032, 0.0, 0.1)),
                ],
                Some(0),
            ),
            // Both eyes are too far.
            (
                [
                    Some(Vec3::new(-0.032, 0.0, 0.4)),
                    Some(Vec3::new(0.032, 0.0, 0.4)),
                ],
                None,
            ),
            // The eye is in front of the lens.
            ([Some(Vec3::new(-0.032, 0.0, -0.1)), None], None),
            // Only the right eye is known.
            ([None, Some(Vec3::new(0.0, 0.0, 0.05))], Some(1)),
        ];
        for (eyes, expected) in cases {
            assert_eq!(sighting_eye(&lens, eyes, 0.15), expected, "{eyes:?}");
        }
    }

    #[test]
    fn the_source_is_the_other_eye_only_when_available() {
        assert_eq!(source_eye(0, LensSource::OtherEye, true), 1);
        assert_eq!(source_eye(1, LensSource::OtherEye, true), 0);
        assert_eq!(source_eye(1, LensSource::OtherEye, false), 1);
        assert_eq!(source_eye(0, LensSource::OwnEye, true), 0);
    }

    #[test]
    fn the_basis_is_orthonormal_even_looking_straight_up() {
        for direction in [Vec3::NEG_Z, Vec3::Y, Vec3::new(0.3, -0.2, -0.9).normalize()] {
            let (right, up) = lens_basis(direction);
            assert!((right.length() - 1.0).abs() < 1e-5);
            assert!((up.length() - 1.0).abs() < 1e-5);
            assert!(right.dot(up).abs() < 1e-5);
            assert!(right.dot(direction).abs() < 1e-5);
            assert!(up.dot(direction).abs() < 1e-5);
        }
        let (right, up) = lens_basis(Vec3::NEG_Z);
        assert!(right.distance(Vec3::X) < 1e-5);
        assert!(up.distance(Vec3::Y) < 1e-5);
    }

    #[test]
    fn the_lens_magnifies_by_the_zoom() {
        let eye_distance = 0.08;
        let lens = Lens {
            eyepiece: Vec3::new(0.0, 0.0, -eye_distance),
            direction: Vec3::NEG_Z,
            focus_m: 200.0,
        };
        let c = config();
        let tangent = field_tangent(c.lens_radius_m, eye_distance, c.zoom);
        // The rim, seen at radius / distance off the sight line, shows the scene at a quarter of that.
        let point = sample_point(&lens, lens.eyepiece, tangent, Vec2::X);
        let shown = point.x / -point.z;
        let seen = c.lens_radius_m / eye_distance;
        assert!(
            (shown * c.zoom - seen).abs() < 1e-3 * seen,
            "{shown} vs {seen}"
        );
        // The centre shows the aim point, and right on the lens is right in the scene.
        let centre = sample_point(&lens, lens.eyepiece, tangent, Vec2::ZERO);
        assert!(centre.distance(Vec3::new(0.0, 0.0, -eye_distance - 200.0)) < 1e-2);
        assert!(point.x > 0.0);
    }

    #[test]
    fn the_centre_projects_to_the_aim_point_in_either_eye() {
        let projection = Mat4::perspective_rh(1.2, 1.0, 0.1, 1000.0);
        let lens = Lens {
            eyepiece: Vec3::new(-0.03, 0.0, -0.08),
            direction: Vec3::NEG_Z,
            focus_m: 300.0,
        };
        let aim_point = lens.eyepiece + lens.direction * lens.focus_m;
        for eye in [Vec3::new(-0.032, 0.0, 0.0), Vec3::new(0.032, 0.0, 0.0)] {
            let camera = EyeCamera {
                position: eye,
                offset_view_projection: projection,
            };
            let c = constants(&lens, &camera, &camera, &config());
            let from_source = Vec3::from_array(c.eyepiece_from_source);
            let p = sample_point(&lens, from_source, c.tangent, Vec2::ZERO);
            let clip: Vec4 = projection * p.extend(1.0);
            let expected: Vec4 = projection * (aim_point - eye).extend(1.0);
            assert!((clip.truncate() / clip.w - expected.truncate() / expected.w).length() < 1e-4);
        }
    }

    #[test]
    fn the_constants_fill_whole_registers() {
        assert_eq!(size_of::<LensConstants>() % 16, 0);
        assert_eq!(size_of::<LensConstants>(), 208);
    }
}
