//! The vehicle handling's overlay: a ring on the control each free hand can grab, the wingsuit's
//! arm-steering guide, and, for tuning, the wheel's rim and axes.

use std::time::Duration;

use crate::controllers::{
    bindings::grab::{Candidate, GrabVolume},
    config::ControllersConfig,
    frame::Hand,
    overlay::{
        primitive::{
            Anchor, DepthMode, Facing, HandPose, Primitive, Rgba, Shape, Source, SpriteSize,
        },
        store,
    },
    vehicles::wheel::{CockpitTransform, WheelFrame},
};

/// Submit a highlight on the control each free hand can grab, the wingsuit's guide when `wings`
/// gives whether the arms steer, and the wheel's rim and axes when `wheel` is given. `xf` maps the
/// cockpit frame the candidates are in to the world.
pub(super) fn submit(
    config: &ControllersConfig,
    candidates: &[Candidate],
    volumes: [Option<GrabVolume>; 2],
    wings: Option<bool>,
    wheel: Option<WheelFrame>,
    xf: &CockpitTransform,
) {
    let mut prims = Vec::new();
    if let Some(wheel) = wheel {
        wheel_primitives(&wheel, xf, &mut prims);
    }
    if let Some(engaged) = wings.filter(|_| config.overlay.enabled) {
        wing_primitives(engaged, &mut prims);
    }
    if config.overlay.enabled && config.overlay.grab_volumes {
        for hand in Hand::BOTH {
            let Some(volume) = volumes[hand.index()] else {
                continue;
            };
            let Some(c) = candidates
                .iter()
                .find(|c| c.volume == volume && c.hand == Some(hand))
            else {
                continue;
            };
            prims.push(Primitive::Sprite {
                at: Anchor::World(xf.point_to_world(c.centre)),
                size: SpriteSize::World(c.radius * 2.0 * xf.world_scale),
                facing: Facing::Billboard,
                shape: Shape::Ring { thickness: 0.15 },
                color: CONTROL,
                depth: XRAY,
            });
        }
    }
    if prims.is_empty() {
        store::clear(Source::VehicleControls);
    } else {
        store::submit(Source::VehicleControls, &mut prims, HIGHLIGHT_TTL);
    }
}

/// The highlights are submitted at the input tick's rate, so they live a little past one tick.
const HIGHLIGHT_TTL: Duration = Duration::from_millis(150);

const CONTROL: Rgba = [0.85, 0.55, 1.0, 0.8];

/// The wingsuit guide's rings while the arms are not spread: faint, a prompt to spread them.
const WING_IDLE: Rgba = [0.8, 0.85, 0.9, 0.35];

/// The wingsuit guide while the arms steer.
const WING_ENGAGED: Rgba = [0.35, 1.0, 0.6, 0.85];

/// The wingsuit guide's rings' diameter, metres.
const WING_RING_M: f32 = 0.09;

/// How wide the wingsuit guide's line between the hands is drawn, metres.
const WING_LINE_M: f32 = 0.006;

const WHEEL: Rgba = [1.0, 0.8, 0.3, 0.8];

const XRAY: DepthMode = DepthMode::XRay {
    occluded_alpha: 0.4,
};

/// How wide the wheel's axis lines are drawn, metres.
const AXIS_WIDTH_M: f32 = 0.004;

/// A ring on each hand, and, while the arms steer, a line between them: the wing. Both are anchored
/// to the hands at render time, since the wingsuit outruns a world anchor placed at the tick rate.
fn wing_primitives(engaged: bool, prims: &mut Vec<Primitive>) {
    let color = if engaged { WING_ENGAGED } else { WING_IDLE };
    for hand in Hand::BOTH {
        prims.push(Primitive::Sprite {
            at: Anchor::hand(hand, HandPose::Grip),
            size: SpriteSize::World(WING_RING_M),
            facing: Facing::Billboard,
            shape: Shape::Ring { thickness: 0.2 },
            color,
            depth: XRAY,
        });
    }
    if engaged {
        prims.push(Primitive::Line {
            from: Anchor::hand(Hand::Left, HandPose::Grip),
            to: Anchor::hand(Hand::Right, HandPose::Grip),
            width: WING_LINE_M,
            color,
            depth: XRAY,
        });
    }
}

/// The wheel's rim, its axis from the hub toward the driver, and a spoke to the top of its rim, in
/// the world.
fn wheel_primitives(wheel: &WheelFrame, xf: &CockpitTransform, prims: &mut Vec<Primitive>) {
    let hub = xf.point_to_world(wheel.centre);
    let normal = xf.direction_to_world(wheel.normal());
    let radius = wheel.radius * xf.world_scale;
    prims.push(Primitive::Sprite {
        at: Anchor::World(hub),
        size: SpriteSize::World(radius * 2.0),
        facing: Facing::Surface { normal },
        shape: Shape::Ring { thickness: 0.06 },
        color: WHEEL,
        depth: XRAY,
    });
    for to in [
        hub + normal * radius,
        xf.point_to_world(wheel.rim_point(0.0)),
    ] {
        prims.push(Primitive::Line {
            from: Anchor::World(hub),
            to: Anchor::World(to),
            width: AXIS_WIDTH_M,
            color: WHEEL,
            depth: XRAY,
        });
    }
}
