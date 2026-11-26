//! The lens's geometry, all pure: where the eyepiece is, which eye sights through it, and the lens
//! draw it hands the pass ([`jc3vr_render::controllers::weapons::scope::pass`]), which holds the draw's
//! constants.
//!
//! The sight line is kept in the holding hand's wrist frame between aim updates, so the lens follows
//! the controller at frame rate while the sight line itself is refreshed at the sim rate. The
//! eyepiece is on the sight line, [`LensTuning::eyepiece_back_m`] behind its origin, and the lens
//! is a disc there, facing back along the sight line.
//!
//! The lens magnifies the view around the sight line: a point of the lens that the eye sees at a
//! tangent `t` off the sight line shows the scene at `t / zoom` off it. The point it shows is taken
//! at the sight line's focus distance (the aim point's distance), projected into the source eye's
//! image, so the image is correct at the target and the parallax between the eyes vanishes there.

use glam::Vec3;
use jc3vr_input::frame::{Hand, Pose};
use jc3vr_render::controllers::weapons::scope::pass::LensDraw;

use crate::controllers::{aim::ray::Ray, hands::pose as hand_pose};

/// The lens's fixed tuning.
pub const LENS: LensTuning = LensTuning {
    zoom: 4.0,
    lens_radius_m: 0.02,
    eyepiece_back_m: 0.8,
    max_eye_distance_m: 0.15,
    vignette_start: 0.7,
};

/// The lens's geometry and look, fixed at [`LENS`].
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct LensTuning {
    /// The lens's magnification.
    pub zoom: f32,
    /// The lens's radius, metres.
    pub lens_radius_m: f32,
    /// How far behind the sight line's origin the eyepiece sits along the sight line, metres. The
    /// sight line's origin is above the muzzle (with the weapon's sight-table entry), so this is
    /// roughly the distance from the muzzle back to the scope's rear lens.
    pub eyepiece_back_m: f32,
    /// How close an eye must be to the eyepiece for the lens to draw, metres.
    pub max_eye_distance_m: f32,
    /// Where the dark rim starts, as a fraction of the lens radius.
    pub vignette_start: f32,
}

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
    pub fn new(sight: &Ray, focus_m: f32, config: &LensTuning) -> Lens {
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

/// The lens's draw: its geometry with the tuning's look.
pub fn draw_of(lens: &Lens, config: &LensTuning) -> LensDraw {
    LensDraw {
        eyepiece: lens.eyepiece,
        direction: lens.direction,
        focus_m: lens.focus_m,
        radius_m: config.lens_radius_m,
        zoom: config.zoom,
        vignette_start: config.vignette_start,
    }
}

/// The nearest the aim point is taken to be: a closer point would make the other eye's image
/// swing with the tiny parallax between the eyes.
const MIN_FOCUS_M: f32 = 1.0;

#[cfg(test)]
mod tests {
    use glam::{Mat4, Quat, Vec2, Vec4};
    use jc3vr_render::controllers::weapons::scope::pass::{
        EyeCamera, LensConstants, field_tangent, lens_basis,
    };

    use super::*;

    /// The scene point the lens shows at `q` (the lens's unit disc, +x right and +y up), relative to
    /// the source eye: `eyepiece_from_source` is the eyepiece relative to that eye. The lens pixel shader
    /// computes the same.
    fn sample_point(lens: &Lens, eyepiece_from_source: Vec3, tangent: f32, q: glam::Vec2) -> Vec3 {
        let (right, up) = lens_basis(lens.direction);
        let direction = (lens.direction + (right * q.x + up * q.y) * tangent).normalize();
        eyepiece_from_source + direction * lens.focus_m
    }

    fn config() -> LensTuning {
        LENS
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
            let c = draw_of(&lens, &config()).constants(&camera, &camera);
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
