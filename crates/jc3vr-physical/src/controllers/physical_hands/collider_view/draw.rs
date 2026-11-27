//! The collider view's primitives, pure: each body's box and shape at its collision pose, the
//! mod's joints, the contact flashes, the player's ground, and the gazed body, within a primitive
//! budget that keeps whole bodies, the nearest first.
//!
//! Every point is a world anchor. The collision geometry is not displaced to match an interpolated
//! mesh.

use glam::{Affine3A, Vec3};
use jc3vr_body::controllers::physics::scan::Aabb;
use jc3vr_view::controllers::overlay::primitive::{
    Anchor, DepthMode, Facing, Primitive, Rgba, Shape, SpriteSize, scale_alpha,
};

use crate::controllers::physical_hands::{
    collider_view::model::{BodyKind, Geometry, JointKind, Snapshot},
    views::ColliderDrawTuning,
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
    pub(crate) impulse_ns: f32,
}

/// What the view draws from.
pub struct DrawInputs<'a> {
    pub current: &'a Snapshot,
    /// The most primitives to submit, at most [`PRIMITIVE_CEILING`].
    pub budget: usize,
    pub flashes: &'a [Flash],
    /// The body under the gaze.
    pub gaze: Option<u32>,
    pub tuning: &'a ColliderDrawTuning,
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

/// The most primitives the view submits, whatever its budget, and the bound the views' tuning
/// checks its budgets against: the overlay's cap covers every readout together.
pub(crate) const PRIMITIVE_CEILING: usize = 2048;

/// Build the frame's primitives into `out` (cleared first).
pub fn build(inputs: &DrawInputs<'_>, out: &mut Vec<Primitive>) -> Drawn {
    out.clear();
    let tuning = inputs.tuning;
    let budget = inputs.budget.min(PRIMITIVE_CEILING);
    let mut drawn = Drawn::default();
    let mut scratch = Vec::new();

    for joint in &inputs.current.joints {
        let (Some(a), Some(b)) = (
            origin(inputs.current, joint.body_a),
            origin(inputs.current, joint.body_b),
        ) else {
            continue;
        };
        let color = match joint.kind {
            JointKind::Grip => GRIP_JOINT,
            JointKind::GunGrip => GUN_GRIP_JOINT,
            JointKind::GunBrace => GUN_BRACE_JOINT,
        };
        scratch.clear();
        scratch.push(line(a, b, tuning.joint_width_m, color, DepthMode::Always));
        for end in [a, b] {
            scratch.push(disc(end, tuning.joint_end_m, color));
        }
        commit(out, &mut scratch, budget);
    }

    for flash in inputs.flashes {
        let Some(fade) = flash_fade(flash.age_s, tuning.flash_s) else {
            continue;
        };
        scratch.clear();
        let size = tuning.flash_ring_m
            + tuning.flash_ring_per_ns_m * flash.impulse_ns.clamp(0.0, tuning.flash_max_impulse_ns);
        scratch.push(Primitive::Sprite {
            at: Anchor::World(flash.at),
            size: SpriteSize::World(size * (1.0 + flash.age_s / tuning.flash_s)),
            facing: Facing::Billboard,
            shape: Shape::Ring {
                thickness: tuning.flash_ring_thickness,
            },
            color: scale_alpha(CONTACT, fade),
            depth: DepthMode::Always,
        });
        if let Some(aabb) = flash.body.and_then(|id| aabb(inputs.current, id)) {
            box_lines(
                &aabb,
                tuning.box_width_m * tuning.highlight_box_width,
                scale_alpha(CONTACT, fade),
                &mut scratch,
            );
        }
        commit(out, &mut scratch, budget);
    }
    if let Some(ground) = inputs.current.ground {
        scratch.clear();
        if let Some(aabb) = aabb(inputs.current, ground.body) {
            box_lines(
                &aabb,
                tuning.box_width_m * tuning.ground_box_width,
                GROUND,
                &mut scratch,
            );
        }
        let player = inputs
            .current
            .bodies
            .iter()
            .find(|b| b.kind == BodyKind::PlayerCapsule)
            .and_then(|b| aabb(inputs.current, b.id));
        if let Some(player) = player {
            let centre = player.centre();
            let foot = Vec3::new(centre.x, player.min.y, centre.z);
            scratch.push(line(
                foot,
                foot + ground.normal.normalize_or_zero() * tuning.normal_m,
                tuning.joint_width_m,
                GROUND,
                DepthMode::Always,
            ));
        }
        commit(out, &mut scratch, budget);
    }

    if let Some(aabb) = inputs.gaze.and_then(|id| aabb(inputs.current, id)) {
        scratch.clear();
        box_lines(
            &aabb,
            tuning.box_width_m * tuning.highlight_box_width,
            GAZE,
            &mut scratch,
        );
        commit(out, &mut scratch, budget);
    }

    for body in &inputs.current.bodies {
        scratch.clear();
        let transform = body.transform;
        let color = kind_color(body.kind);
        let aabb = body.aabb;
        box_lines(
            &aabb,
            tuning.box_width_m,
            scale_alpha(color, BOX_ALPHA),
            &mut scratch,
        );
        shape(&body.geometry, &transform, color, tuning, &mut scratch);
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

fn origin(snapshot: &Snapshot, id: u32) -> Option<Vec3> {
    Some(Vec3::from(snapshot.body(id)?.transform.translation))
}

fn aabb(snapshot: &Snapshot, id: u32) -> Option<Aabb> {
    Some(snapshot.body(id)?.aabb)
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

/// How visible a flash `age_s` old is, of one lasting `flash_s`, or `None` once it is over.
fn flash_fade(age_s: f32, flash_s: f32) -> Option<f32> {
    (age_s.is_finite() && (0.0..flash_s).contains(&age_s)).then(|| 1.0 - age_s / flash_s)
}

fn box_lines(aabb: &Aabb, width: f32, color: Rgba, out: &mut Vec<Primitive>) {
    out.extend(
        aabb.edges()
            .into_iter()
            .map(|(a, b)| line(a, b, width, color, XRAY)),
    );
}

/// A body's shape at `transform`.
fn shape(
    geometry: &Geometry,
    transform: &Affine3A,
    color: Rgba,
    tuning: &ColliderDrawTuning,
    out: &mut Vec<Primitive>,
) {
    let at = |p: Vec3| transform.transform_point3(p);
    let ring = |centre: Vec3, radius: f32| ring(centre, radius, tuning.shape_ring_thickness, color);
    let width = tuning.shape_width_m;
    match geometry {
        Geometry::Sphere { centre, radius } => out.push(ring(at(*centre), *radius)),
        Geometry::Capsule { a, b, radius } => {
            let (a, b) = (at(*a), at(*b));
            let axis = (b - a).normalize_or(Vec3::Y);
            let (u, v) = axis.any_orthonormal_pair();
            for side in [u, -u, v, -v] {
                let offset = side * *radius;
                out.push(line(a + offset, b + offset, width, color, XRAY));
            }
            out.push(ring(a, *radius));
            out.push(ring(b, *radius));
        }
        Geometry::Hull { vertices, edges } => {
            for &(i, j) in edges.iter().take(tuning.max_hull_edges) {
                let (Some(&p), Some(&q)) =
                    (vertices.get(usize::from(i)), vertices.get(usize::from(j)))
                else {
                    continue;
                };
                out.push(line(at(p), at(q), width, color, XRAY));
            }
        }
        Geometry::Points(vertices) => {
            for &p in vertices.iter().take(tuning.max_points) {
                out.push(disc(at(p), tuning.point_m, color));
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

fn ring(centre: Vec3, radius: f32, thickness: f32, color: Rgba) -> Primitive {
    Primitive::Sprite {
        at: Anchor::World(centre),
        size: SpriteSize::World(radius * 2.0),
        facing: Facing::Billboard,
        shape: Shape::Ring { thickness },
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

const XRAY: DepthMode = DepthMode::XRay {
    occluded_alpha: 0.3,
};
/// A box is fainter than its shape, so the shape reads through it.
const BOX_ALPHA: f32 = 0.4;
const GRIP_JOINT: Rgba = [0.5, 1.0, 0.3, 1.0];
const GUN_GRIP_JOINT: Rgba = [1.0, 0.85, 0.2, 1.0];
const GUN_BRACE_JOINT: Rgba = [1.0, 0.55, 0.2, 1.0];
const CONTACT: Rgba = [1.0, 1.0, 0.85, 1.0];
const GROUND: Rgba = [0.35, 1.0, 0.45, 0.8];
const GAZE: Rgba = [1.0, 1.0, 1.0, 0.9];

#[cfg(test)]
mod tests {
    use jc3vr_body::controllers::physics::scan::MotionKind;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::controllers::physical_hands::{
        collider_view::model::{Body, Ground, Joint},
        views::VIEWS,
    };

    fn config() -> &'static ColliderDrawTuning {
        &VIEWS.get().collider_view.draw
    }

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

    fn inputs(current: &Snapshot) -> DrawInputs<'_> {
        DrawInputs {
            current,
            budget: PRIMITIVE_CEILING,
            flashes: &[],
            gaze: None,
            tuning: config(),
        }
    }

    fn sphere() -> Geometry {
        Geometry::Sphere {
            centre: Vec3::ZERO,
            radius: 0.5,
        }
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
        let mut out = Vec::new();
        let drawn = build(
            &DrawInputs {
                budget: 30,
                ..inputs(&current)
            },
            &mut out,
        );
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
        let mut out = Vec::new();
        build(
            &DrawInputs {
                budget: usize::MAX,
                ..inputs(&current)
            },
            &mut out,
        );
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
        shape(&capsule, &Affine3A::IDENTITY, [1.0; 4], config(), &mut out);
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
                age_s: config().flash_s,
                impulse_ns: 1.0,
            },
        ];
        let mut out = Vec::new();
        let drawn = build(
            &DrawInputs {
                budget: 3 + 13 + 13,
                flashes: &flashes,
                ..inputs(&current)
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
