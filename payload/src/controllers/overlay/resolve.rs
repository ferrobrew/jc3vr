//! From submitted primitives to the renderer's packed form: anchors placed in the world, sizes and
//! facings settled for the frame, positions made relative to the frame's origin, and the result
//! sorted far to near.
//!
//! Everything view-dependent is settled against the head (the cyclopean eye), not per eye, so both
//! eyes draw identical world geometry and the stereo pair fuses. The overlay writes no depth, so
//! overlapping primitives are ordered by the painter's algorithm.

use glam::{Quat, Vec3};

use crate::controllers::{
    frame::Pose,
    overlay::primitive::{Anchor, Facing, HandPose, Primitive, Shape, SpriteSize},
};

/// A hand's render-time world poses.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HandPoses {
    pub wrist: Pose,
    pub grip: Pose,
    pub aim: Pose,
}

impl HandPoses {
    fn get(&self, pose: HandPose) -> Pose {
        match pose {
            HandPose::Wrist => self.wrist,
            HandPose::Grip => self.grip,
            HandPose::Aim => self.aim,
        }
    }
}

/// The frame's inputs to the resolution.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Frame {
    /// The world position the packed positions are relative to.
    pub origin: Vec3,
    /// The head's world pose: the centre between the eyes.
    pub head: Pose,
    /// Each hand's render-time poses, indexed by [`Hand::index`].
    pub hands: [Option<HandPoses>; 2],
}

/// The kind tag of a packed primitive.
pub const KIND_SPRITE: u32 = 0;
pub const KIND_LINE: u32 = 1;

/// One primitive as the vertex shader reads it from its structured buffer (`Primitive` in
/// `overlay_vs.hlsl`). Every row is a 16-byte vector.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GpuPrimitive {
    /// The sprite's centre or the line's start, relative to the origin; `w` is the sprite's radius
    /// or the line's half-width, in metres.
    pub a: [f32; 4],
    /// The line's end, relative to the origin; `w` is the ring's inner radius as a fraction of its
    /// radius (zero for a disc).
    pub b: [f32; 4],
    /// The sprite's right axis; `w` is the alpha scale of occluded pixels.
    pub u: [f32; 4],
    /// The sprite's up axis.
    pub v: [f32; 4],
    /// Straight alpha.
    pub color: [f32; 4],
    /// `x` is [`KIND_SPRITE`] or [`KIND_LINE`].
    pub kind: [u32; 4],
}

/// Resolve `primitives` for `frame` into `out` (cleared first), far to near. A primitive whose
/// hand anchor has no pose this frame, or whose geometry is not finite, is dropped.
pub fn resolve(primitives: &[Primitive], frame: &Frame, out: &mut Vec<GpuPrimitive>) {
    let mut keyed: Vec<(f32, GpuPrimitive)> = primitives
        .iter()
        .filter_map(|p| resolve_one(p, frame))
        .collect();
    keyed.sort_by(|a, b| b.0.total_cmp(&a.0));
    out.clear();
    out.extend(keyed.into_iter().map(|(_, p)| p));
}

/// The world position of an anchor, or `None` when its hand has no pose.
pub fn anchor_position(anchor: Anchor, hands: &[Option<HandPoses>; 2]) -> Option<Vec3> {
    match anchor {
        Anchor::World(p) => Some(p),
        Anchor::Hand { hand, pose, offset } => {
            let pose = hands[hand.index()]?.get(pose);
            Some(pose.position + pose.orientation * offset)
        }
    }
}

/// A sprite's world radius at `distance` metres from the head.
pub fn sprite_radius(size: SpriteSize, distance: f32) -> f32 {
    match size {
        SpriteSize::World(diameter) => diameter * 0.5,
        SpriteSize::Angular(angle) => distance * (angle * 0.5).tan(),
    }
}

/// The sort key and packed form of one primitive.
fn resolve_one(primitive: &Primitive, frame: &Frame) -> Option<(f32, GpuPrimitive)> {
    let (key, packed) = match *primitive {
        Primitive::Sprite {
            at,
            size,
            facing,
            shape,
            color,
            depth,
        } => {
            let center = anchor_position(at, &frame.hands)?;
            let distance = center.distance(frame.head.position);
            let radius = sprite_radius(size, distance);
            let (u, v) = sprite_axes(facing, frame.head.orientation);
            let inner = match shape {
                Shape::Disc => 0.0,
                Shape::Ring { thickness } => (1.0 - thickness).clamp(0.0, 1.0),
            };
            let packed = GpuPrimitive {
                a: (center - frame.origin).extend(radius).to_array(),
                b: [0.0, 0.0, 0.0, inner],
                u: u.extend(depth.occluded_alpha()).to_array(),
                v: v.extend(0.0).to_array(),
                color,
                kind: [KIND_SPRITE, 0, 0, 0],
            };
            (distance, packed)
        }
        Primitive::Line {
            from,
            to,
            width,
            color,
            depth,
        } => {
            let a = anchor_position(from, &frame.hands)?;
            let b = anchor_position(to, &frame.hands)?;
            let packed = GpuPrimitive {
                a: (a - frame.origin).extend(width * 0.5).to_array(),
                b: (b - frame.origin).extend(0.0).to_array(),
                u: Vec3::ZERO.extend(depth.occluded_alpha()).to_array(),
                v: [0.0; 4],
                color,
                kind: [KIND_LINE, 0, 0, 0],
            };
            (((a + b) * 0.5 - frame.head.position).length(), packed)
        }
    };
    let finite = key.is_finite()
        && [packed.a, packed.b, packed.u, packed.v, packed.color]
            .iter()
            .flatten()
            .all(|x| x.is_finite());
    finite.then_some((key, packed))
}

/// A sprite's right and up axes: the head's for a billboard, or any orthonormal pair across a
/// surface normal.
fn sprite_axes(facing: Facing, head: Quat) -> (Vec3, Vec3) {
    let billboard = (head * Vec3::X, head * Vec3::Y);
    match facing {
        Facing::Billboard => billboard,
        Facing::Surface { normal } => normal
            .try_normalize()
            .map_or(billboard, |n| n.any_orthonormal_pair()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::{frame::Hand, overlay::primitive::DepthMode};

    fn frame() -> Frame {
        let right = HandPoses {
            wrist: Pose {
                position: Vec3::new(10.3, 1.2, -20.4),
                orientation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            },
            grip: Pose::IDENTITY,
            aim: Pose::IDENTITY,
        };
        Frame {
            origin: Vec3::new(10.0, 1.6, -20.0),
            head: Pose {
                position: Vec3::new(10.0, 1.6, -20.0),
                orientation: Quat::IDENTITY,
            },
            hands: [None, Some(right)],
        }
    }

    fn dot(at: Anchor, size: SpriteSize) -> Primitive {
        Primitive::Sprite {
            at,
            size,
            facing: Facing::Billboard,
            shape: Shape::Disc,
            color: [1.0; 4],
            depth: DepthMode::Tested,
        }
    }

    #[test]
    fn a_hand_anchor_follows_its_pose_and_offset() {
        let f = frame();
        let anchor = Anchor::Hand {
            hand: Hand::Right,
            pose: HandPose::Wrist,
            offset: Vec3::new(0.0, 0.0, -1.0),
        };
        // A quarter turn about +Y carries -Z to -X.
        let p = anchor_position(anchor, &f.hands).unwrap();
        assert!(p.distance(Vec3::new(9.3, 1.2, -20.4)) < 1e-5);
        assert!(anchor_position(Anchor::hand(Hand::Left, HandPose::Grip), &f.hands).is_none());
    }

    #[test]
    fn an_angular_size_keeps_its_apparent_size() {
        let angle = 0.02;
        let near = sprite_radius(SpriteSize::Angular(angle), 2.0);
        let far = sprite_radius(SpriteSize::Angular(angle), 20.0);
        assert!((far / near - 10.0).abs() < 1e-4);
        assert!((near - 2.0 * (angle * 0.5).tan()).abs() < 1e-7);
        assert_eq!(sprite_radius(SpriteSize::World(0.1), 50.0), 0.05);
    }

    #[test]
    fn primitives_are_sorted_far_to_near_and_unresolved_ones_dropped() {
        let f = frame();
        let prims = [
            dot(
                Anchor::World(Vec3::new(10.0, 1.6, -25.0)),
                SpriteSize::World(0.1),
            ),
            dot(
                Anchor::hand(Hand::Left, HandPose::Aim),
                SpriteSize::World(0.1),
            ),
            dot(
                Anchor::World(Vec3::new(10.0, 1.6, -60.0)),
                SpriteSize::World(0.1),
            ),
            Primitive::Line {
                from: Anchor::World(Vec3::new(10.0, 1.6, -20.0)),
                to: Anchor::World(Vec3::new(10.0, 1.6, -40.0)),
                width: 0.02,
                color: [1.0; 4],
                depth: DepthMode::Always,
            },
        ];
        let mut out = Vec::new();
        resolve(&prims, &f, &mut out);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].a[2], -40.0);
        assert_eq!(out[1].kind[0], KIND_LINE);
        assert_eq!(out[1].u[3], 1.0);
        assert_eq!(out[2].a[2], -5.0);
    }

    #[test]
    fn packed_positions_are_relative_to_the_origin() {
        let mut f = frame();
        f.origin = Vec3::new(4096.0, 200.0, -8192.0);
        f.head.position = f.origin;
        let world = f.origin + Vec3::new(0.25, -0.1, -0.5);
        let mut out = Vec::new();
        resolve(
            &[dot(Anchor::World(world), SpriteSize::Angular(0.01))],
            &f,
            &mut out,
        );
        let a = Vec3::new(out[0].a[0], out[0].a[1], out[0].a[2]);
        assert!(a.distance(Vec3::new(0.25, -0.1, -0.5)) < 1e-3);
    }

    #[test]
    fn a_surface_sprite_lies_across_its_normal() {
        let (u, v) = sprite_axes(
            Facing::Surface {
                normal: Vec3::new(0.0, 2.0, 0.0),
            },
            Quat::IDENTITY,
        );
        assert!(u.dot(Vec3::Y).abs() < 1e-6 && v.dot(Vec3::Y).abs() < 1e-6);
        assert!(u.dot(v).abs() < 1e-6);
        let (u, _) = sprite_axes(Facing::Surface { normal: Vec3::ZERO }, Quat::IDENTITY);
        assert_eq!(u, Vec3::X);
    }

    #[test]
    fn a_ring_packs_its_inner_radius() {
        let f = frame();
        let mut out = Vec::new();
        let ring = Primitive::Sprite {
            at: Anchor::World(Vec3::new(10.0, 1.6, -21.0)),
            size: SpriteSize::World(0.2),
            facing: Facing::Billboard,
            shape: Shape::Ring { thickness: 0.25 },
            color: [1.0; 4],
            depth: DepthMode::XRay {
                occluded_alpha: 0.3,
            },
        };
        resolve(&[ring], &f, &mut out);
        assert_eq!(out[0].b[3], 0.75);
        assert!((out[0].a[3] - 0.1).abs() < 1e-6);
        assert!((out[0].u[3] - 0.3).abs() < 1e-6);
    }
}
