//! The grounded rig's in-world diagnostics: a sprite at the neck pivot and one at the waist pivot,
//! the spine between them, and two rings on the floor around the pelvis, the lean radius and the
//! lean reach, all coloured by the step phase (`docs/mod/body/rig.md`).
//!
//! The rig works in the room frame, which shares its axes with the cockpit frame, so each point is
//! anchored to the head by its offset from the eyes, carried into the world by the view's rotation
//! and the world scale. The overlay's producers draw it into [`Source::BodyRig`] once per rendered
//! frame ([`crate::controllers::overlay::producers`]).
//!
//! [`Source::BodyRig`]: crate::controllers::overlay::primitive::Source::BodyRig

use body_rig::StepPhase;
use glam::{Quat, Vec3};

use crate::controllers::{
    body::rig::grounded::Diagnostics,
    overlay::primitive::{Anchor, DepthMode, Facing, Primitive, Rgba, Shape, SpriteSize},
};

/// Build the rig's diagnostics for `d`, with `view` the cockpit frame's rotation into the world and
/// `scale` the world scale.
pub(crate) fn primitives(d: &Diagnostics, view: Quat, scale: f32, out: &mut Vec<Primitive>) {
    let at = |p: Vec3| Anchor::Head(view * (p - d.eyes) * scale);
    let color = phase_color(d.phase);
    let floor = Facing::Surface {
        normal: view * Vec3::Y,
    };
    for pivot in [d.neck, d.waist] {
        out.push(Primitive::Sprite {
            at: at(pivot),
            size: SpriteSize::World(PIVOT_M * scale),
            facing: Facing::Billboard,
            shape: Shape::Disc,
            color,
            depth: DEPTH,
        });
    }
    out.push(Primitive::Line {
        from: at(d.neck),
        to: at(d.waist),
        width: SPINE_WIDTH_M * scale,
        color,
        depth: DEPTH,
    });
    for (radius, alpha) in [(d.lean_radius_m, 1.0), (d.lean_reach_m, REACH_ALPHA)] {
        if radius <= 0.0 {
            continue;
        }
        let [r, g, b, a] = color;
        out.push(Primitive::Sprite {
            at: at(d.pelvis),
            size: SpriteSize::World(2.0 * radius * scale),
            facing: floor,
            shape: Shape::Ring {
                thickness: RING_THICKNESS,
            },
            color: [r, g, b, a * alpha],
            depth: DEPTH,
        });
    }
}

const IDLE: Rgba = [0.3, 0.85, 0.75, 0.85];
const SETTLING: Rgba = [0.95, 0.85, 0.3, 0.9];
const STEPPING: Rgba = [1.0, 0.45, 0.15, 0.95];

/// The reach ring's alpha relative to the radius ring's.
const REACH_ALPHA: f32 = 0.4;
const PIVOT_M: f32 = 0.05;
const SPINE_WIDTH_M: f32 = 0.01;
/// The rings' thickness as a fraction of their radius: thin, since the reach ring is wide.
const RING_THICKNESS: f32 = 0.06;
const DEPTH: DepthMode = DepthMode::XRay {
    occluded_alpha: 0.35,
};

fn phase_color(phase: StepPhase) -> Rgba {
    match phase {
        StepPhase::Idle => IDLE,
        StepPhase::Stepping => STEPPING,
        StepPhase::Settling => SETTLING,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diagnostics(phase: StepPhase) -> Diagnostics {
        Diagnostics {
            eyes: Vec3::new(0.2, 1.6, -0.1),
            neck: Vec3::new(0.2, 1.5, 0.0),
            waist: Vec3::new(0.2, 1.0, 0.05),
            pelvis: Vec3::new(0.2, 0.0, 0.05),
            lean_radius_m: 0.15,
            lean_reach_m: 0.4,
            phase,
        }
    }

    fn head_offset(anchor: Anchor) -> Vec3 {
        match anchor {
            Anchor::Head(offset) => offset,
            other => panic!("not a head anchor: {other:?}"),
        }
    }

    #[test]
    fn points_hang_from_the_head_turned_and_scaled() {
        let view = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let d = diagnostics(StepPhase::Idle);
        let mut out = Vec::new();
        primitives(&d, view, 2.0, &mut out);
        assert_eq!(out.len(), 5);
        let Primitive::Sprite { at, .. } = out[0] else {
            panic!("the neck pivot is a sprite");
        };
        let expected = view * (d.neck - d.eyes) * 2.0;
        assert!(head_offset(at).distance(expected) < 1e-5);
        let Primitive::Sprite {
            at, size, facing, ..
        } = out[3]
        else {
            panic!("the lean radius is a sprite");
        };
        assert!(head_offset(at).distance(view * (d.pelvis - d.eyes) * 2.0) < 1e-5);
        assert_eq!(size, SpriteSize::World(2.0 * 0.15 * 2.0));
        let Facing::Surface { normal } = facing else {
            panic!("the ring lies on the floor");
        };
        assert!(normal.distance(Vec3::Y) < 1e-5);
        assert_eq!(
            out[4],
            Primitive::Sprite {
                at,
                size: SpriteSize::World(2.0 * 0.4 * 2.0),
                facing,
                shape: Shape::Ring {
                    thickness: RING_THICKNESS,
                },
                color: [IDLE[0], IDLE[1], IDLE[2], IDLE[3] * REACH_ALPHA],
                depth: DEPTH,
            }
        );
    }

    #[test]
    fn the_colour_follows_the_phase() {
        let color = |phase| {
            let mut out = Vec::new();
            primitives(&diagnostics(phase), Quat::IDENTITY, 1.0, &mut out);
            match out[2] {
                Primitive::Line { color, .. } => color,
                Primitive::Sprite { .. } => panic!("the spine is a line"),
            }
        };
        assert_eq!(color(StepPhase::Idle), IDLE);
        assert_eq!(color(StepPhase::Stepping), STEPPING);
        assert_eq!(color(StepPhase::Settling), SETTLING);
    }
}
