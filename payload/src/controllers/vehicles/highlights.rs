//! The vehicle handling's overlay: a ring on the control each free hand can grab, and, for tuning,
//! the wheel's rim and axes.

use std::time::Duration;

use crate::controllers::{
    bindings::grab::{Candidate, GrabVolume},
    config::ControllersConfig,
    frame::Hand,
    overlay::{
        primitive::{Anchor, DepthMode, Facing, Primitive, Rgba, Shape, Source, SpriteSize},
        store,
    },
    vehicles::wheel::{CockpitTransform, WheelFrame},
};

/// Submit a highlight on the control each free hand can grab, and the wheel's rim and axes when
/// `wheel` is given. `xf` maps the cockpit frame the candidates are in to the world.
pub(super) fn submit(
    config: &ControllersConfig,
    candidates: &[Candidate],
    volumes: [Option<GrabVolume>; 2],
    wheel: Option<WheelFrame>,
    xf: &CockpitTransform,
) {
    let mut prims = Vec::new();
    if let Some(wheel) = wheel {
        wheel_primitives(&wheel, xf, &mut prims);
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

const WHEEL: Rgba = [1.0, 0.8, 0.3, 0.8];

const XRAY: DepthMode = DepthMode::XRay {
    occluded_alpha: 0.4,
};

/// How wide the wheel's axis lines are drawn, metres.
const AXIS_WIDTH_M: f32 = 0.004;

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
