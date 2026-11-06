//! The palm boxes in the world-space overlay, a diagnostic: each acting palm's edges, coloured by
//! whether it presses into a surface or an object, a ring at a pressing palm, a line along the
//! shove a palm gives an object, longer the harder it pushes, and a line from the grab point of
//! what a hand holds to where the hand would have it, redder the harder the hand pulls. What a hand
//! would take, and holds, is marked by the grip model's markers
//! ([`crate::controllers::grab::markers`]).
//!
//! The palms and what they hold ride with the player, so they are anchored to the render-time head
//! at their offset from the tick's head (`docs/mod/rendering/render-time-anchoring.md`).

use std::time::Duration;

use glam::Vec3;

use crate::{
    config::Config,
    controllers::{
        overlay::{
            primitive::{Anchor, DepthMode, Facing, Primitive, Rgba, Shape, Source, SpriteSize},
            store,
        },
        physical_hands::{HandStatus, PhysicalHandsConfig, palm::EDGES},
    },
    headpose,
};

/// Submit the palms of the last tick, or clear them when they are not shown.
pub(super) fn submit(hands: &[HandStatus; 2], config: &PhysicalHandsConfig) {
    let overlay = Config::lock_query(|c| c.controllers.overlay.enabled);
    if !(overlay && config.show_boxes) {
        clear();
        return;
    }
    let rides = Rides(headpose::query().position);
    let mut prims = Vec::new();
    for status in hands {
        hold_line(status, rides, &mut prims);
    }
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
                from: rides.at(corners[a]),
                to: rides.at(corners[b]),
                width: EDGE_WIDTH_M,
                color,
                depth: DepthMode::XRay {
                    occluded_alpha: 0.35,
                },
            });
        }
        if let Some(shove) = status.shove
            && shove.load > 0.0
        {
            let direction = shove.impulse.normalize_or_zero();
            prims.push(Primitive::Line {
                from: rides.at(shove.at),
                to: rides.at(shove.at + direction * (SHOVE_LINE_M * shove.load.max(0.1))),
                width: EDGE_WIDTH_M,
                color: blend(PRESSED_OBJECT, TOO_HEAVY, shove.resistance),
                depth: DepthMode::Always,
            });
        }
        if status.press.is_some() {
            prims.push(Primitive::Sprite {
                at: rides.at(palm.centre),
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

/// A line from the grab point of what the hand holds to where the hand would have it, coloured by
/// how hard the hand pulls.
fn hold_line(status: &HandStatus, rides: Rides, prims: &mut Vec<Primitive>) {
    if let Some(held) = status.held
        && held.lag_m > MIN_LINE_M
    {
        prims.push(Primitive::Line {
            from: rides.at(held.anchor),
            to: rides.at(held.target),
            width: EDGE_WIDTH_M,
            color: blend(HELD, TOO_HEAVY, held.load),
            depth: DepthMode::Always,
        });
    }
}

/// The tick's head position, against which a point that rides with the player is anchored.
#[derive(Copy, Clone)]
struct Rides(Vec3);

impl Rides {
    /// `world`, a point the tick placed, as an offset from the head.
    fn at(self, world: Vec3) -> Anchor {
        Anchor::Head(world - self.0)
    }
}

/// The colour `share` (`0..=1`) of the way from `a` to `b`.
fn blend(a: Rgba, b: Rgba, share: f32) -> Rgba {
    let t = if share.is_finite() {
        share.clamp(0.0, 1.0)
    } else {
        0.0
    };
    [0, 1, 2, 3].map(|k| a[k] + (b[k] - a[k]) * t)
}

pub(super) fn clear() {
    store::clear(Source::PhysicalHands);
}

/// The boxes are submitted every simulation tick, so they live a little past one.
const TTL: Duration = Duration::from_millis(150);
const EDGE_WIDTH_M: f32 = 0.003;
/// The length of a full-strength shove's line, metres.
const SHOVE_LINE_M: f32 = 0.15;
/// The shortest hold line drawn, metres.
const MIN_LINE_M: f32 = 0.005;
const CLEAR: Rgba = [0.85, 0.95, 1.0, 0.8];
const PRESSED: Rgba = [1.0, 0.6, 0.15, 0.95];
const PRESSED_OBJECT: Rgba = [0.4, 1.0, 0.4, 0.95];
const TOO_HEAVY: Rgba = [1.0, 0.35, 0.3, 0.9];
const HELD: Rgba = [0.35, 0.85, 1.0, 0.6];
