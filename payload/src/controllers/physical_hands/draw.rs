//! The palm boxes in the world-space overlay: each acting palm's edges, coloured by whether it
//! presses into a surface or an object, and a ring at a pressing palm.

use std::time::Duration;

use crate::{
    config::Config,
    controllers::{
        overlay::{
            primitive::{Anchor, DepthMode, Facing, Primitive, Rgba, Shape, Source, SpriteSize},
            store,
        },
        physical_hands::{HandStatus, PhysicalHandsConfig, palm::EDGES},
    },
};

/// Submit the palms of the last tick, or clear them when the boxes are not shown.
pub(super) fn submit(hands: &[HandStatus; 2], config: &PhysicalHandsConfig) {
    let overlay = Config::lock_query(|c| c.controllers.overlay.enabled);
    if !(overlay && config.show_boxes) {
        clear();
        return;
    }
    let mut prims = Vec::new();
    for status in hands {
        let Some(palm) = status.palm else {
            continue;
        };
        let color = match status.press {
            None => CLEAR,
            Some((_, false)) => PRESSED,
            Some((_, true)) => PRESSED_OBJECT,
        };
        let corners = palm.corners();
        for (a, b) in EDGES {
            prims.push(Primitive::Line {
                from: Anchor::World(corners[a]),
                to: Anchor::World(corners[b]),
                width: EDGE_WIDTH_M,
                color,
                depth: DepthMode::XRay {
                    occluded_alpha: 0.35,
                },
            });
        }
        if status.press.is_some() {
            prims.push(Primitive::Sprite {
                at: Anchor::World(palm.centre),
                size: SpriteSize::World(palm.half_extents.max_element() * 1.5),
                facing: Facing::Billboard,
                shape: Shape::Ring { thickness: 0.2 },
                color,
                depth: DepthMode::Always,
            });
        }
    }
    store::submit(Source::PhysicalHands, &mut prims, TTL);
}

pub(super) fn clear() {
    store::clear(Source::PhysicalHands);
}

/// The boxes are submitted every simulation tick, so they live a little past one.
const TTL: Duration = Duration::from_millis(150);
const EDGE_WIDTH_M: f32 = 0.003;
const CLEAR: Rgba = [0.85, 0.95, 1.0, 0.8];
const PRESSED: Rgba = [1.0, 0.6, 0.15, 0.95];
const PRESSED_OBJECT: Rgba = [0.4, 1.0, 0.4, 0.95];
