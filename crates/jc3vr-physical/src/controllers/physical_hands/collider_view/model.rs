//! The collider view's snapshot of the physics world, pure: what a body is, what its shape looks
//! like in its own frame, and the geometry that picks the bodies near the player and the one under
//! the gaze. The engine side that fills it is [`crate::controllers::physical_hands::collider_view::scan`].

use glam::{Affine3A, Quat, Vec3, Vec3A};

/// What a body is, for its colour and the readout.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum BodyKind {
    /// A static body: the terrain, buildings, and fixed props.
    Static,
    /// A dynamic body no other kind claims: a loose prop or debris.
    Dynamic,
    /// A keyframed body: moved along a set path that collisions do not push.
    Keyframed,
    /// One of the mod's hand bodies.
    HandBody,
    /// One of the mod's held guns' bodies.
    GunBody,
    /// The player's own character proxy.
    PlayerCapsule,
    /// Another character's proxy.
    Character,
    /// A ragdoll's part, or a ragdoll's proxy.
    Ragdoll,
    /// A vehicle's body or wheel collision.
    Vehicle,
}

impl BodyKind {
    pub const COUNT: usize = 9;

    pub const ALL: [BodyKind; BodyKind::COUNT] = [
        BodyKind::Static,
        BodyKind::Dynamic,
        BodyKind::Keyframed,
        BodyKind::HandBody,
        BodyKind::GunBody,
        BodyKind::PlayerCapsule,
        BodyKind::Character,
        BodyKind::Ragdoll,
        BodyKind::Vehicle,
    ];

    pub const fn index(self) -> usize {
        match self {
            BodyKind::Static => 0,
            BodyKind::Dynamic => 1,
            BodyKind::Keyframed => 2,
            BodyKind::HandBody => 3,
            BodyKind::GunBody => 4,
            BodyKind::PlayerCapsule => 5,
            BodyKind::Character => 6,
            BodyKind::Ragdoll => 7,
            BodyKind::Vehicle => 8,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            BodyKind::Static => "static",
            BodyKind::Dynamic => "dynamic",
            BodyKind::Keyframed => "keyframed",
            BodyKind::HandBody => "hand body",
            BodyKind::GunBody => "gun body",
            BodyKind::PlayerCapsule => "player capsule",
            BodyKind::Character => "character",
            BodyKind::Ragdoll => "ragdoll part",
            BodyKind::Vehicle => "vehicle",
        }
    }
}

/// Which of the mod's own bodies a body is.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ModBody {
    Hand,
    Gun,
}

/// What a body's kind is decided from.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct KindInputs {
    /// Whether the body is static, dynamic, or keyframed.
    pub motion: MotionKind,
    /// The collision layer: the low field of the collision-filter word.
    pub layer: Layer,
    /// Whether the body is one of the mod's own.
    pub mod_body: Option<ModBody>,
    /// Whether the body is the player's character proxy.
    pub player: bool,
}

/// A body's motion, from its flags.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MotionKind {
    Static,
    Dynamic,
    Keyframed,
}

impl MotionKind {
    pub const fn label(self) -> &'static str {
        match self {
            MotionKind::Static => "static",
            MotionKind::Dynamic => "dynamic",
            MotionKind::Keyframed => "keyframed",
        }
    }
}

/// The layers the kinds are told apart by, from the engine's own layer list.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Layer {
    /// A character proxy's layer, in or out of ghost mode.
    Character,
    /// A ragdoll's part, its keyframed part, or its proxy.
    Ragdoll,
    /// A vehicle's collision or wheel collision.
    Vehicle,
    /// Any other layer, by its value.
    Other(u32),
}

/// The kind of a body. The mod's own bodies and the player's proxy are named by id and win over
/// the layer; a character, ragdoll, or vehicle layer wins over the motion.
pub fn classify(inputs: KindInputs) -> BodyKind {
    match (inputs.mod_body, inputs.player, inputs.layer, inputs.motion) {
        (Some(ModBody::Hand), ..) => BodyKind::HandBody,
        (Some(ModBody::Gun), ..) => BodyKind::GunBody,
        (None, true, ..) => BodyKind::PlayerCapsule,
        (None, false, Layer::Character, _) => BodyKind::Character,
        (None, false, Layer::Ragdoll, _) => BodyKind::Ragdoll,
        (None, false, Layer::Vehicle, _) => BodyKind::Vehicle,
        (None, false, Layer::Other(_), MotionKind::Static) => BodyKind::Static,
        (None, false, Layer::Other(_), MotionKind::Keyframed) => BodyKind::Keyframed,
        (None, false, Layer::Other(_), MotionKind::Dynamic) => BodyKind::Dynamic,
    }
}

/// A box with its least and greatest corners.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn centre(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn translated(&self, by: Vec3) -> Aabb {
        Aabb {
            min: self.min + by,
            max: self.max + by,
        }
    }

    /// How far `point` is from the box: zero inside it.
    pub fn distance(&self, point: Vec3) -> f32 {
        (self.min - point)
            .max(point - self.max)
            .max(Vec3::ZERO)
            .length()
    }

    /// The box's volume, cubic metres.
    pub fn volume(&self) -> f32 {
        (self.max - self.min).max(Vec3::ZERO).element_product()
    }

    /// Where the ray from `origin` along the unit `direction` enters the box, metres along it,
    /// within `max_m`: zero when it starts inside, `None` when it misses.
    pub fn ray_entry(&self, origin: Vec3, direction: Vec3, max_m: f32) -> Option<f32> {
        let mut near = 0.0f32;
        let mut far = max_m;
        for axis in 0..3 {
            let (o, d) = (origin[axis], direction[axis]);
            let (lo, hi) = (self.min[axis], self.max[axis]);
            if d.abs() < 1e-9 {
                if o < lo || o > hi {
                    return None;
                }
                continue;
            }
            let (a, b) = ((lo - o) / d, (hi - o) / d);
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if near > far {
                return None;
            }
        }
        Some(near)
    }

    /// The box's twelve edges.
    pub fn edges(&self) -> [(Vec3, Vec3); 12] {
        let corner = |i: usize| {
            Vec3::new(
                if i & 1 == 0 { self.min.x } else { self.max.x },
                if i & 2 == 0 { self.min.y } else { self.max.y },
                if i & 4 == 0 { self.min.z } else { self.max.z },
            )
        };
        BOX_EDGES.map(|(a, b)| (corner(a), corner(b)))
    }
}

/// A box quantized into the world's integer space, restored as the engine restores it
/// ([`hknpWorld::getBodyAabb`]): each corner times the inverse scale, less the low offset.
///
/// [`hknpWorld::getBodyAabb`]: jc3gi::physics::hknpWorld::getBodyAabb
pub fn restore_aabb(min: [u16; 3], max: [u16; 3], scale_inv: Vec3, offset_low: Vec3) -> Aabb {
    let restore = |q: [u16; 3]| Vec3::from(q.map(f32::from)) * scale_inv - offset_low;
    Aabb {
        min: restore(min),
        max: restore(max),
    }
}

/// A body's shape in its own frame, as far as the view draws it.
#[derive(Debug, Clone, PartialEq)]
pub enum Geometry {
    /// A sphere.
    Sphere { centre: Vec3, radius: f32 },
    /// A capsule: the points within `radius` of the segment from `a` to `b`.
    Capsule { a: Vec3, b: Vec3, radius: f32 },
    /// A convex hull with its edges, each a pair of indices into `vertices`.
    Hull {
        vertices: Vec<Vec3>,
        edges: Vec<(u8, u8)>,
    },
    /// A convex hull known by its vertices alone.
    Points(Vec<Vec3>),
    /// A shape the view draws by its box alone: a compound, a mesh, or a height field. The
    /// engine's kind value is kept for the readout.
    Other(u32),
}

impl Geometry {
    pub fn label(&self) -> &'static str {
        match self {
            Geometry::Sphere { .. } => "sphere",
            Geometry::Capsule { .. } => "capsule",
            Geometry::Hull { .. } => "convex hull",
            Geometry::Points(_) => "convex vertices",
            Geometry::Other(_) => "box only",
        }
    }
}

/// A capsule's radius from its convex radius: the core box reaches `radius_in_core` of the radius
/// past the segment, and the convex radius is the rest
/// ([`hknpCapsuleShape::RADIUS_IN_CORE`](jc3gi::havok::hknpCapsuleShape::RADIUS_IN_CORE)).
pub fn capsule_radius(convex_radius: f32, radius_in_core: f32) -> f32 {
    let rest = 1.0 - radius_in_core;
    if rest > 0.0 {
        convex_radius / rest
    } else {
        convex_radius
    }
}

/// The edges of a polytope from its faces, each face a run `(first, count)` of `indices` going
/// round it: every pair of consecutive indices and the last and first, each edge once, smaller
/// index first. A face that runs past the indices, or an index past `vertex_count`, is skipped.
pub fn hull_edges(faces: &[(u16, u8)], indices: &[u8], vertex_count: usize) -> Vec<(u8, u8)> {
    let mut edges: Vec<(u8, u8)> = Vec::new();
    for &(first, count) in faces {
        let (first, count) = (usize::from(first), usize::from(count));
        let Some(run) = indices.get(first..first + count) else {
            continue;
        };
        if count < 2 || run.iter().any(|&i| usize::from(i) >= vertex_count) {
            continue;
        }
        for k in 0..count {
            let (a, b) = (run[k], run[(k + 1) % count]);
            if a == b {
                continue;
            }
            let edge = (a.min(b), a.max(b));
            if !edges.contains(&edge) {
                edges.push(edge);
            }
        }
    }
    edges
}

/// One body as the snapshot holds it, game space.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub id: u32,
    /// The shape's address, which tells a reused body id from the body it was.
    pub shape_key: usize,
    pub kind: BodyKind,
    pub motion: MotionKind,
    /// The collision-filter word.
    pub filter_info: u32,
    /// The body's frame, game space.
    pub transform: Affine3A,
    pub aabb: Aabb,
    /// The mass of a dynamic body's motion, kilograms; `None` for an infinite mass.
    pub mass_kg: Option<f32>,
    pub geometry: Geometry,
    /// How far the body's box is from the player, metres.
    pub distance_m: f32,
}

/// A joint of the mod's, between two bodies.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Joint {
    pub kind: JointKind,
    pub body_a: u32,
    pub body_b: u32,
}

/// Which of the mod's joints a joint is.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum JointKind {
    /// A hand body joined to the loose object its grip holds.
    Grip,
    /// A hand body joined to the gun it holds.
    GunGrip,
    /// The other hand's body joined to the gun it braces.
    GunBrace,
}

/// What the player's proxy stands on, as its last support check found it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Ground {
    pub body: u32,
    /// The surface's normal.
    pub normal: Vec3,
}

/// One read of the world near the player.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Snapshot {
    /// The bodies kept, nearest first.
    pub bodies: Vec<Body>,
    pub joints: Vec<Joint>,
    pub ground: Option<Ground>,
    /// The body slots read.
    pub scanned: u32,
    /// The bodies within the radius, kept or not.
    pub in_range: u32,
}

impl Snapshot {
    pub fn body(&self, id: u32) -> Option<&Body> {
        self.bodies.iter().find(|b| b.id == id)
    }
}

/// The ids of the `max` nearest candidates `(distance, id)` within `radius`, nearest first, and how
/// many were within it.
pub fn nearest(mut candidates: Vec<(f32, u32)>, radius: f32, max: usize) -> (Vec<(f32, u32)>, u32) {
    candidates.retain(|(d, _)| d.is_finite() && *d <= radius);
    let in_range = u32::try_from(candidates.len()).unwrap_or(u32::MAX);
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    candidates.truncate(max);
    (candidates, in_range)
}

/// A body's frame between the last two reads, `t` of the way from `previous` to `current`.
pub fn interpolate(previous: &Affine3A, current: &Affine3A, t: f32) -> Affine3A {
    let t = if t.is_finite() {
        t.clamp(0.0, 1.0)
    } else {
        1.0
    };
    let (_, r0, p0) = previous.to_scale_rotation_translation();
    let (_, r1, p1) = current.to_scale_rotation_translation();
    let rotation = if r0.is_finite() && r1.is_finite() {
        r0.slerp(r1, t)
    } else {
        Quat::IDENTITY
    };
    Affine3A {
        matrix3: glam::Mat3A::from_quat(rotation),
        translation: Vec3A::from(p0.lerp(p1, t)),
    }
}

/// The body whose box the gaze ray from `origin` along the unit `forward` enters within `max_m`:
/// the smallest such box, so that a prop on the ground wins over the ground around it.
pub fn gazed<'a>(
    bodies: impl IntoIterator<Item = &'a Body>,
    origin: Vec3,
    forward: Vec3,
    max_m: f32,
) -> Option<&'a Body> {
    bodies
        .into_iter()
        .filter(|b| b.aabb.distance(origin) > 0.0)
        .filter(|b| b.aabb.ray_entry(origin, forward, max_m).is_some())
        .min_by(|a, b| a.aabb.volume().total_cmp(&b.aabb.volume()))
}

/// The corners each of a box's edges joins, the corner index's bits picking the greatest x, y,
/// and z.
const BOX_EDGES: [(usize, usize); 12] = [
    (0, 1),
    (2, 3),
    (4, 5),
    (6, 7),
    (0, 2),
    (1, 3),
    (4, 6),
    (5, 7),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(motion: MotionKind, layer: Layer) -> KindInputs {
        KindInputs {
            motion,
            layer,
            mod_body: None,
            player: false,
        }
    }

    #[test]
    fn a_body_is_classified() {
        let player = KindInputs {
            player: true,
            ..inputs(MotionKind::Dynamic, Layer::Character)
        };
        let hand = KindInputs {
            mod_body: Some(ModBody::Hand),
            ..inputs(MotionKind::Dynamic, Layer::Other(15))
        };
        // A mod body named as the player's too is still the mod's.
        let gun = KindInputs {
            mod_body: Some(ModBody::Gun),
            player: true,
            ..inputs(MotionKind::Dynamic, Layer::Other(15))
        };
        let cases = [
            (
                inputs(MotionKind::Static, Layer::Other(2)),
                BodyKind::Static,
            ),
            (
                inputs(MotionKind::Dynamic, Layer::Other(4)),
                BodyKind::Dynamic,
            ),
            (
                inputs(MotionKind::Keyframed, Layer::Other(4)),
                BodyKind::Keyframed,
            ),
            (
                inputs(MotionKind::Dynamic, Layer::Character),
                BodyKind::Character,
            ),
            (
                inputs(MotionKind::Keyframed, Layer::Ragdoll),
                BodyKind::Ragdoll,
            ),
            (
                inputs(MotionKind::Dynamic, Layer::Vehicle),
                BodyKind::Vehicle,
            ),
            (player, BodyKind::PlayerCapsule),
            (hand, BodyKind::HandBody),
            (gun, BodyKind::GunBody),
        ];
        for (inputs, kind) in cases {
            assert_eq!(classify(inputs), kind, "{inputs:?}");
        }
    }

    #[test]
    fn the_kind_indices_match_their_order() {
        for (i, kind) in BodyKind::ALL.into_iter().enumerate() {
            assert_eq!(kind.index(), i);
        }
    }

    #[test]
    fn a_quantized_box_is_restored_by_the_inverse_scale_less_the_low_offset() {
        let aabb = restore_aabb(
            [100, 200, 300],
            [110, 220, 330],
            Vec3::splat(0.5),
            Vec3::new(10.0, 20.0, 30.0),
        );
        assert_eq!(aabb.min, Vec3::new(40.0, 80.0, 120.0));
        assert_eq!(aabb.max, Vec3::new(45.0, 90.0, 135.0));
    }

    #[test]
    fn a_point_is_as_far_from_a_box_as_from_its_nearest_point() {
        let aabb = Aabb {
            min: Vec3::ZERO,
            max: Vec3::ONE,
        };
        assert_eq!(aabb.distance(Vec3::splat(0.5)), 0.0);
        assert_eq!(aabb.distance(Vec3::new(3.0, 0.5, 0.5)), 2.0);
        assert!((aabb.distance(Vec3::new(2.0, 2.0, 0.5)) - 2f32.sqrt()).abs() < 1e-6);
    }

    #[test]
    fn a_ray_enters_a_box_where_it_crosses_its_nearest_face() {
        let aabb = Aabb {
            min: Vec3::new(-1.0, -1.0, 4.0),
            max: Vec3::new(1.0, 1.0, 6.0),
        };
        assert_eq!(aabb.ray_entry(Vec3::ZERO, Vec3::Z, 10.0), Some(4.0));
        assert_eq!(aabb.ray_entry(Vec3::ZERO, Vec3::Z, 3.0), None);
        assert_eq!(aabb.ray_entry(Vec3::ZERO, Vec3::NEG_Z, 10.0), None);
        assert_eq!(
            aabb.ray_entry(Vec3::new(2.0, 0.0, 0.0), Vec3::Z, 10.0),
            None
        );
        assert_eq!(
            aabb.ray_entry(Vec3::new(0.0, 0.0, 5.0), Vec3::X, 10.0),
            Some(0.0)
        );
    }

    #[test]
    fn a_box_has_twelve_distinct_edges_along_the_axes() {
        let aabb = Aabb {
            min: Vec3::ZERO,
            max: Vec3::new(1.0, 2.0, 3.0),
        };
        let edges = aabb.edges();
        for (a, b) in edges {
            let d = (b - a).abs();
            assert_eq!(
                [d.x, d.y, d.z].iter().filter(|&&c| c > 0.0).count(),
                1,
                "{a} {b}"
            );
        }
        let total: f32 = edges.iter().map(|(a, b)| a.distance(*b)).sum();
        assert_eq!(total, 4.0 * (1.0 + 2.0 + 3.0));
    }

    #[test]
    fn a_box_polytope_has_twelve_edges() {
        // A cube's six quads over eight vertices.
        let indices = [
            0, 1, 3, 2, 4, 5, 7, 6, 0, 1, 5, 4, 2, 3, 7, 6, 0, 2, 6, 4, 1, 3, 7, 5,
        ];
        let faces: Vec<(u16, u8)> = (0..6).map(|f| (f * 4, 4)).collect();
        let edges = hull_edges(&faces, &indices, 8);
        assert_eq!(edges.len(), 12);
        assert!(edges.iter().all(|(a, b)| a < b));
    }

    #[test]
    fn a_face_past_the_indices_or_the_vertices_is_skipped() {
        let indices = [0, 1, 2, 0, 1, 9];
        assert_eq!(hull_edges(&[(4, 4)], &indices, 3), vec![]);
        assert_eq!(hull_edges(&[(3, 3)], &indices, 3), vec![]);
        assert_eq!(
            hull_edges(&[(0, 3)], &indices, 3),
            vec![(0, 1), (1, 2), (0, 2)]
        );
    }

    #[test]
    fn a_capsule_radius_is_its_convex_radius_and_its_core_share() {
        assert!((capsule_radius(0.99, 0.01) - 1.0).abs() < 1e-6);
        assert_eq!(capsule_radius(0.5, 1.0), 0.5);
    }

    #[test]
    fn the_nearest_candidates_within_the_radius_are_kept() {
        let (kept, in_range) = nearest(
            vec![(5.0, 1), (1.0, 2), (7.0, 3), (f32::NAN, 4), (2.0, 5)],
            6.0,
            2,
        );
        assert_eq!(kept, vec![(1.0, 2), (2.0, 5)]);
        assert_eq!(in_range, 3);
    }

    #[test]
    fn a_frame_is_interpolated_between_the_reads() {
        let a = Affine3A::from_translation(Vec3::ZERO);
        let b = Affine3A::from_rotation_translation(
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            Vec3::new(2.0, 0.0, 0.0),
        );
        let mid = interpolate(&a, &b, 0.5);
        assert!((Vec3::from(mid.translation) - Vec3::X).length() < 1e-5);
        let (_, rotation, _) = mid.to_scale_rotation_translation();
        assert!(rotation.angle_between(Quat::from_rotation_y(std::f32::consts::FRAC_PI_4)) < 1e-4);
        let end = interpolate(&a, &b, f32::NAN);
        assert!((Vec3::from(end.translation) - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-5);
    }

    fn body(id: u32, min: Vec3, max: Vec3) -> Body {
        Body {
            id,
            shape_key: 0,
            kind: BodyKind::Static,
            motion: MotionKind::Static,
            filter_info: 0,
            transform: Affine3A::IDENTITY,
            aabb: Aabb { min, max },
            mass_kg: None,
            geometry: Geometry::Other(0),
            distance_m: 0.0,
        }
    }

    #[test]
    fn the_gaze_picks_the_smallest_box_it_enters_and_skips_boxes_around_the_head() {
        let ground = body(1, Vec3::new(-50.0, -1.0, -50.0), Vec3::new(50.0, 0.0, 50.0));
        let crate_ = body(2, Vec3::new(-0.5, -1.0, -5.5), Vec3::new(0.5, 0.0, -4.5));
        let around = body(3, Vec3::splat(-2.0), Vec3::splat(2.0));
        let bodies = [ground, crate_, around];
        let origin = Vec3::new(0.0, 1.6, 0.0);
        let forward = (Vec3::new(0.0, -0.5, -5.0) - origin).normalize();
        assert_eq!(gazed(&bodies, origin, forward, 20.0).map(|b| b.id), Some(2));
        assert_eq!(gazed(&bodies, origin, Vec3::Y, 20.0).map(|b| b.id), None);
    }
}
