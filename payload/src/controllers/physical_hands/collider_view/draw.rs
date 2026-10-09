//! The collider view's primitives, pure: each body's box and shape at its frame between the last
//! two reads, the mod's joints, the contact flashes, the player's ground, and the gazed body, within
//! a primitive budget that keeps whole bodies, the nearest first.
//!
//! Every point is a world anchor at the frame's interpolation fraction between the last two reads,
//! which is where the engine draws the objects the bodies belong to (between their last two ticks'
//! matrices), so a collider stays on its object at frame rate however fast either moves.

use std::collections::HashMap;

use glam::{Affine3A, Vec3};

use crate::controllers::{
    overlay::primitive::{Anchor, DepthMode, Facing, Primitive, Rgba, Shape, SpriteSize},
    physical_hands::collider_view::{
        config::ColliderViewConfig,
        model::{Aabb, Body, BodyKind, Geometry, JointKind, Snapshot, interpolate},
    },
};

/// A contact to flash.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Flash {
    /// Where the body that met something was before the step, game space.
    pub at: Vec3,
    /// The other body, whose box flashes with it.
    pub body: Option<u32>,
    /// How long ago the contact was reported, seconds.
    pub age_s: f32,
    /// The contact's impulse, newton seconds.
    pub impulse_ns: f32,
}

/// What the view draws from.
pub struct DrawInputs<'a> {
    pub current: &'a Snapshot,
    pub previous: Option<&'a Snapshot>,
    /// The frame's interpolation fraction between the two reads.
    pub dtf: f32,
    pub config: &'a ColliderViewConfig,
    pub flashes: &'a [Flash],
    /// The body under the gaze.
    pub gaze: Option<u32>,
}

/// What a frame drew.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Drawn {
    /// The bodies drawn.
    pub bodies: u32,
    /// The bodies left out for the budget.
    pub dropped: u32,
    /// The bodies drawn of each kind, indexed by [`BodyKind::index`].
    pub kinds: [u32; BodyKind::COUNT],
}

/// How long a contact flashes, seconds.
pub const FLASH_S: f32 = 0.4;

/// The most primitives the view submits, whatever its setting: the overlay's cap covers every
/// readout together.
pub const PRIMITIVE_CEILING: usize = 2048;

/// Build the frame's primitives into `out` (cleared first).
pub fn build(inputs: &DrawInputs<'_>, out: &mut Vec<Primitive>) -> Drawn {
    out.clear();
    let budget = (inputs.config.max_primitives as usize).min(PRIMITIVE_CEILING);
    let frames = Frames::new(inputs);
    let mut drawn = Drawn::default();
    let mut scratch = Vec::new();

    if inputs.config.joints {
        for joint in &inputs.current.joints {
            let (Some(a), Some(b)) = (frames.origin(joint.body_a), frames.origin(joint.body_b))
            else {
                continue;
            };
            let color = match joint.kind {
                JointKind::Grip => GRIP_JOINT,
                JointKind::GunGrip => GUN_GRIP_JOINT,
                JointKind::GunBrace => GUN_BRACE_JOINT,
            };
            scratch.clear();
            scratch.push(line(a, b, JOINT_WIDTH_M, color, DepthMode::Always));
            for end in [a, b] {
                scratch.push(disc(end, JOINT_END_M, color));
            }
            commit(out, &mut scratch, budget);
        }
    }

    if inputs.config.contacts {
        for flash in inputs.flashes {
            let Some(fade) = flash_fade(flash.age_s) else {
                continue;
            };
            scratch.clear();
            let size = FLASH_RING_M + FLASH_RING_PER_NS_M * flash.impulse_ns.clamp(0.0, 20.0);
            scratch.push(Primitive::Sprite {
                at: Anchor::World(flash.at),
                size: SpriteSize::World(size * (1.0 + flash.age_s / FLASH_S)),
                facing: Facing::Billboard,
                shape: Shape::Ring { thickness: 0.25 },
                color: with_alpha(CONTACT, fade),
                depth: DepthMode::Always,
            });
            if let Some(aabb) = flash.body.and_then(|id| frames.aabb(id)) {
                box_lines(
                    &aabb,
                    BOX_WIDTH_M * 2.0,
                    with_alpha(CONTACT, fade),
                    &mut scratch,
                );
            }
            commit(out, &mut scratch, budget);
        }
        if let Some(ground) = inputs.current.ground {
            scratch.clear();
            if let Some(aabb) = frames.aabb(ground.body) {
                box_lines(&aabb, BOX_WIDTH_M * 1.5, GROUND, &mut scratch);
            }
            let player = inputs
                .current
                .bodies
                .iter()
                .find(|b| b.kind == BodyKind::PlayerCapsule)
                .and_then(|b| frames.aabb(b.id));
            if let Some(player) = player {
                let centre = player.centre();
                let foot = Vec3::new(centre.x, player.min.y, centre.z);
                scratch.push(line(
                    foot,
                    foot + ground.normal.normalize_or_zero() * NORMAL_M,
                    JOINT_WIDTH_M,
                    GROUND,
                    DepthMode::Always,
                ));
            }
            commit(out, &mut scratch, budget);
        }
    }

    if let Some(aabb) = inputs.gaze.and_then(|id| frames.aabb(id)) {
        scratch.clear();
        box_lines(&aabb, BOX_WIDTH_M * 2.0, GAZE, &mut scratch);
        commit(out, &mut scratch, budget);
    }

    for body in &inputs.current.bodies {
        scratch.clear();
        let transform = frames.transform(body);
        let color = kind_color(body.kind);
        if inputs.config.boxes {
            let aabb = body.aabb.translated(Vec3::from(
                transform.translation - body.transform.translation,
            ));
            box_lines(
                &aabb,
                BOX_WIDTH_M,
                with_alpha(color, BOX_ALPHA),
                &mut scratch,
            );
        }
        if inputs.config.shapes {
            shape(&body.geometry, &transform, color, &mut scratch);
        }
        if scratch.is_empty() {
            continue;
        }
        if commit(out, &mut scratch, budget) {
            drawn.bodies += 1;
            drawn.kinds[body.kind.index()] += 1;
        } else {
            drawn.dropped += 1;
        }
    }
    drawn
}

/// The colour a body of `kind` is drawn in.
pub const fn kind_color(kind: BodyKind) -> Rgba {
    match kind {
        BodyKind::Static => [0.6, 0.65, 0.75, 0.7],
        BodyKind::Dynamic => [0.3, 0.85, 1.0, 0.9],
        BodyKind::Keyframed => [0.75, 0.5, 1.0, 0.9],
        BodyKind::HandBody => [1.0, 0.6, 0.15, 0.95],
        BodyKind::GunBody => [1.0, 0.9, 0.25, 0.95],
        BodyKind::PlayerCapsule => [0.35, 1.0, 0.45, 0.9],
        BodyKind::Character => [1.0, 0.4, 0.8, 0.9],
        BodyKind::Ragdoll => [1.0, 0.45, 0.45, 0.9],
        BodyKind::Vehicle => [0.4, 0.55, 1.0, 0.9],
    }
}

/// Each body's frame at the frame's interpolation fraction.
struct Frames<'a> {
    current: &'a Snapshot,
    previous: HashMap<u32, &'a Body>,
    dtf: f32,
}

impl<'a> Frames<'a> {
    fn new(inputs: &DrawInputs<'a>) -> Self {
        Self {
            current: inputs.current,
            previous: inputs
                .previous
                .map(|p| p.bodies.iter().map(|b| (b.id, b)).collect())
                .unwrap_or_default(),
            dtf: inputs.dtf,
        }
    }

    /// The body's frame between its last two reads; the last read's alone when the earlier read
    /// did not have it, or had another body under its id.
    fn transform(&self, body: &Body) -> Affine3A {
        match self.previous.get(&body.id) {
            Some(previous) if previous.shape_key == body.shape_key => {
                interpolate(&previous.transform, &body.transform, self.dtf)
            }
            _ => body.transform,
        }
    }

    fn origin(&self, id: u32) -> Option<Vec3> {
        let body = self.current.body(id)?;
        Some(Vec3::from(self.transform(body).translation))
    }

    /// The body's box carried with its interpolated frame.
    fn aabb(&self, id: u32) -> Option<Aabb> {
        let body = self.current.body(id)?;
        let moved = self.transform(body).translation - body.transform.translation;
        Some(body.aabb.translated(Vec3::from(moved)))
    }
}

/// Append `scratch` to `out` when all of it fits within `budget`; it is cleared either way.
fn commit(out: &mut Vec<Primitive>, scratch: &mut Vec<Primitive>, budget: usize) -> bool {
    let fits = out.len() + scratch.len() <= budget;
    if fits {
        out.append(scratch);
    }
    scratch.clear();
    fits
}

/// How visible a flash `age_s` old is, or `None` once it is over.
fn flash_fade(age_s: f32) -> Option<f32> {
    (age_s.is_finite() && (0.0..FLASH_S).contains(&age_s)).then(|| 1.0 - age_s / FLASH_S)
}

fn box_lines(aabb: &Aabb, width: f32, color: Rgba, out: &mut Vec<Primitive>) {
    out.extend(
        aabb.edges()
            .into_iter()
            .map(|(a, b)| line(a, b, width, color, XRAY)),
    );
}

/// A body's shape at `transform`.
fn shape(geometry: &Geometry, transform: &Affine3A, color: Rgba, out: &mut Vec<Primitive>) {
    let at = |p: Vec3| transform.transform_point3(p);
    match geometry {
        Geometry::Sphere { centre, radius } => out.push(ring(at(*centre), *radius, color)),
        Geometry::Capsule { a, b, radius } => {
            let (a, b) = (at(*a), at(*b));
            let axis = (b - a).normalize_or(Vec3::Y);
            let (u, v) = axis.any_orthonormal_pair();
            for side in [u, -u, v, -v] {
                let offset = side * *radius;
                out.push(line(a + offset, b + offset, SHAPE_WIDTH_M, color, XRAY));
            }
            out.push(ring(a, *radius, color));
            out.push(ring(b, *radius, color));
        }
        Geometry::Hull { vertices, edges } => {
            for &(i, j) in edges.iter().take(MAX_HULL_EDGES) {
                let (Some(&p), Some(&q)) =
                    (vertices.get(usize::from(i)), vertices.get(usize::from(j)))
                else {
                    continue;
                };
                out.push(line(at(p), at(q), SHAPE_WIDTH_M, color, XRAY));
            }
        }
        Geometry::Points(vertices) => {
            for &p in vertices.iter().take(MAX_POINTS) {
                out.push(disc(at(p), POINT_M, color));
            }
        }
        Geometry::Other(_) => {}
    }
}

fn line(a: Vec3, b: Vec3, width: f32, color: Rgba, depth: DepthMode) -> Primitive {
    Primitive::Line {
        from: Anchor::World(a),
        to: Anchor::World(b),
        width,
        color,
        depth,
    }
}

fn ring(centre: Vec3, radius: f32, color: Rgba) -> Primitive {
    Primitive::Sprite {
        at: Anchor::World(centre),
        size: SpriteSize::World(radius * 2.0),
        facing: Facing::Billboard,
        shape: Shape::Ring { thickness: 0.08 },
        color,
        depth: XRAY,
    }
}

fn disc(centre: Vec3, diameter: f32, color: Rgba) -> Primitive {
    Primitive::Sprite {
        at: Anchor::World(centre),
        size: SpriteSize::World(diameter),
        facing: Facing::Billboard,
        shape: Shape::Disc,
        color,
        depth: DepthMode::Always,
    }
}

fn with_alpha(color: Rgba, alpha: f32) -> Rgba {
    [color[0], color[1], color[2], color[3] * alpha]
}

const XRAY: DepthMode = DepthMode::XRay {
    occluded_alpha: 0.3,
};
const BOX_WIDTH_M: f32 = 0.003;
const SHAPE_WIDTH_M: f32 = 0.005;
const JOINT_WIDTH_M: f32 = 0.008;
const JOINT_END_M: f32 = 0.025;
const POINT_M: f32 = 0.02;
const NORMAL_M: f32 = 0.3;
const FLASH_RING_M: f32 = 0.06;
const FLASH_RING_PER_NS_M: f32 = 0.01;
/// A box is fainter than its shape, so the shape reads through it.
const BOX_ALPHA: f32 = 0.4;
/// The most edges of one hull drawn.
const MAX_HULL_EDGES: usize = 96;
/// The most vertices of one hull without faces drawn.
const MAX_POINTS: usize = 32;
const GRIP_JOINT: Rgba = [0.5, 1.0, 0.3, 1.0];
const GUN_GRIP_JOINT: Rgba = [1.0, 0.85, 0.2, 1.0];
const GUN_BRACE_JOINT: Rgba = [1.0, 0.55, 0.2, 1.0];
const CONTACT: Rgba = [1.0, 1.0, 0.85, 1.0];
const GROUND: Rgba = [0.35, 1.0, 0.45, 0.8];
const GAZE: Rgba = [1.0, 1.0, 1.0, 0.9];

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::controllers::physical_hands::collider_view::model::{Ground, Joint, MotionKind};

    fn body(id: u32, kind: BodyKind, at: Vec3, geometry: Geometry) -> Body {
        Body {
            id,
            shape_key: 1,
            kind,
            motion: MotionKind::Dynamic,
            filter_info: 0,
            transform: Affine3A::from_translation(at),
            aabb: Aabb {
                min: at - Vec3::splat(0.5),
                max: at + Vec3::splat(0.5),
            },
            mass_kg: Some(1.0),
            geometry,
            distance_m: at.length(),
        }
    }

    fn inputs<'a>(
        current: &'a Snapshot,
        previous: Option<&'a Snapshot>,
        config: &'a ColliderViewConfig,
    ) -> DrawInputs<'a> {
        DrawInputs {
            current,
            previous,
            dtf: 0.5,
            config,
            flashes: &[],
            gaze: None,
        }
    }

    fn sphere() -> Geometry {
        Geometry::Sphere {
            centre: Vec3::ZERO,
            radius: 0.5,
        }
    }

    #[test]
    fn a_body_is_drawn_between_its_last_two_reads() {
        let previous = Snapshot {
            bodies: vec![body(1, BodyKind::Dynamic, Vec3::ZERO, sphere())],
            ..Snapshot::default()
        };
        let current = Snapshot {
            bodies: vec![body(
                1,
                BodyKind::Dynamic,
                Vec3::new(2.0, 0.0, 0.0),
                sphere(),
            )],
            ..Snapshot::default()
        };
        let config = ColliderViewConfig {
            boxes: false,
            ..ColliderViewConfig::new()
        };
        let mut out = Vec::new();
        build(&inputs(&current, Some(&previous), &config), &mut out);
        let [Primitive::Sprite { at, .. }] = out.as_slice() else {
            panic!("{out:?}");
        };
        assert_eq!(*at, Anchor::World(Vec3::X));
    }

    #[test]
    fn a_reused_body_id_is_not_interpolated_from_the_body_it_was() {
        let mut was = body(1, BodyKind::Dynamic, Vec3::ZERO, sphere());
        was.shape_key = 7;
        let previous = Snapshot {
            bodies: vec![was],
            ..Snapshot::default()
        };
        let current = Snapshot {
            bodies: vec![body(
                1,
                BodyKind::Dynamic,
                Vec3::new(2.0, 0.0, 0.0),
                sphere(),
            )],
            ..Snapshot::default()
        };
        let config = ColliderViewConfig {
            boxes: false,
            ..ColliderViewConfig::new()
        };
        let mut out = Vec::new();
        build(&inputs(&current, Some(&previous), &config), &mut out);
        let [Primitive::Sprite { at, .. }] = out.as_slice() else {
            panic!("{out:?}");
        };
        assert_eq!(*at, Anchor::World(Vec3::new(2.0, 0.0, 0.0)));
    }

    #[test]
    fn the_budget_keeps_whole_bodies_nearest_first() {
        // Each body is a box of twelve lines and one ring.
        let current = Snapshot {
            bodies: (0..5)
                .map(|i| body(i, BodyKind::Static, Vec3::splat(i as f32), sphere()))
                .collect(),
            ..Snapshot::default()
        };
        let config = ColliderViewConfig {
            max_primitives: 30,
            ..ColliderViewConfig::new()
        };
        let mut out = Vec::new();
        let drawn = build(&inputs(&current, None, &config), &mut out);
        assert_eq!(out.len(), 26);
        assert_eq!(drawn.bodies, 2);
        assert_eq!(drawn.dropped, 3);
        assert_eq!(drawn.kinds[BodyKind::Static.index()], 2);
    }

    #[test]
    fn the_budget_never_passes_the_ceiling() {
        let current = Snapshot {
            bodies: (0..400)
                .map(|i| body(i, BodyKind::Dynamic, Vec3::ZERO, sphere()))
                .collect(),
            ..Snapshot::default()
        };
        let config = ColliderViewConfig {
            max_primitives: u32::MAX,
            ..ColliderViewConfig::new()
        };
        let mut out = Vec::new();
        build(&inputs(&current, None, &config), &mut out);
        assert!(out.len() <= PRIMITIVE_CEILING);
    }

    #[test]
    fn a_capsule_is_four_sides_and_two_rings() {
        let capsule = Geometry::Capsule {
            a: Vec3::ZERO,
            b: Vec3::Y,
            radius: 0.3,
        };
        let mut out = Vec::new();
        shape(&capsule, &Affine3A::IDENTITY, [1.0; 4], &mut out);
        assert_eq!(out.len(), 6);
        for p in &out[..4] {
            let Primitive::Line {
                from: Anchor::World(a),
                to: Anchor::World(b),
                ..
            } = p
            else {
                panic!("{p:?}");
            };
            assert!((a.distance(Vec3::ZERO) - 0.3).abs() < 1e-5);
            assert!((b.distance(Vec3::Y) - 0.3).abs() < 1e-5);
        }
    }

    #[test]
    fn joints_flashes_and_the_ground_come_before_the_bodies() {
        let current = Snapshot {
            bodies: vec![
                body(1, BodyKind::HandBody, Vec3::ZERO, sphere()),
                body(2, BodyKind::Dynamic, Vec3::X, sphere()),
                body(3, BodyKind::PlayerCapsule, Vec3::Y, sphere()),
            ],
            joints: vec![Joint {
                kind: JointKind::Grip,
                body_a: 1,
                body_b: 2,
            }],
            ground: Some(Ground {
                body: 2,
                normal: Vec3::Y,
            }),
            ..Snapshot::default()
        };
        let flashes = [
            Flash {
                at: Vec3::ZERO,
                body: Some(2),
                age_s: 0.1,
                impulse_ns: 1.0,
            },
            Flash {
                at: Vec3::ZERO,
                body: None,
                age_s: FLASH_S,
                impulse_ns: 1.0,
            },
        ];
        let config = ColliderViewConfig {
            max_primitives: 3 + 13 + 13,
            ..ColliderViewConfig::new()
        };
        let mut out = Vec::new();
        let drawn = build(
            &DrawInputs {
                flashes: &flashes,
                ..inputs(&current, None, &config)
            },
            &mut out,
        );
        // The joint's line and ends, the live flash's ring and box, and the ground's box and
        // normal; the spent flash draws nothing and no body fits after them.
        assert_eq!(out.len(), 3 + 13 + 13);
        assert_eq!(drawn.bodies, 0);
        assert!(matches!(out[0], Primitive::Line { .. }));
    }
}
