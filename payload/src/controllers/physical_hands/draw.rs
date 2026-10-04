//! The palm boxes in the world-space overlay: each acting palm's edges, coloured by whether it
//! presses into a surface or an object, a ring at a pressing palm, and a line along the shove a
//! palm gives an object, longer the harder it pushes. With the pickup targets shown, a ring marks
//! what each hand would take with its grip, a disc the grab point of what it holds, and a line
//! from the grab point to where the hand would have it, redder the harder the hand pulls.

use std::time::Duration;

use crate::{
    config::Config,
    controllers::{
        overlay::{
            primitive::{Anchor, DepthMode, Facing, Primitive, Rgba, Shape, Source, SpriteSize},
            store,
        },
        physical_hands::{
            HandStatus, PhysicalHandsConfig,
            grasp::{self, TargetKind},
            palm::EDGES,
        },
    },
};

/// Submit the palms and the pickup targets of the last tick, or clear them when neither is shown.
pub(super) fn submit(hands: &[HandStatus; 2], config: &PhysicalHandsConfig) {
    let overlay = Config::lock_query(|c| c.controllers.overlay.enabled);
    let targets = config.pickup.enabled && config.pickup.show_targets;
    if !(overlay && (config.show_boxes || targets)) {
        clear();
        return;
    }
    let mut prims = Vec::new();
    if targets {
        for status in hands {
            pickup_markers(status, config, &mut prims);
        }
    }
    if !config.show_boxes {
        store::submit(Source::PhysicalHands, &mut prims, TTL);
        return;
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
                from: Anchor::World(corners[a]),
                to: Anchor::World(corners[b]),
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
                from: Anchor::World(shove.at),
                to: Anchor::World(shove.at + direction * (SHOVE_LINE_M * shove.load.max(0.1))),
                width: EDGE_WIDTH_M,
                color: blend(PRESSED_OBJECT, TOO_HEAVY, shove.resistance),
                depth: DepthMode::Always,
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

/// A ring at what the hand would take, coloured by what it is and whether one hand lifts it; a
/// disc at the grab point of what it holds, coloured by whether the hands holding it lift it; and a
/// line from the grab point to where the hand would have it, coloured by how hard the hand pulls.
fn pickup_markers(status: &HandStatus, config: &PhysicalHandsConfig, prims: &mut Vec<Primitive>) {
    let size = |status: &HandStatus| {
        status
            .palm
            .map_or(0.04, |palm| palm.half_extents.max_element() * 2.0)
    };
    if let Some(held) = status.held {
        prims.push(Primitive::Sprite {
            at: Anchor::World(held.anchor),
            size: SpriteSize::World(size(status) * 0.5),
            facing: Facing::Billboard,
            shape: Shape::Disc,
            color: if held.liftable { HELD } else { TOO_HEAVY },
            depth: DepthMode::XRay {
                occluded_alpha: 0.5,
            },
        });
        if held.lag_m > MIN_LINE_M {
            prims.push(Primitive::Line {
                from: Anchor::World(held.anchor),
                to: Anchor::World(held.target),
                width: EDGE_WIDTH_M,
                color: blend(HELD, TOO_HEAVY, held.load),
                depth: DepthMode::Always,
            });
        }
    } else if let Some(reach) = status.reach {
        let color = match reach.kind {
            TargetKind::Weapon => WEAPON,
            TargetKind::Prop { mass_kg } if grasp::can_lift(mass_kg, 1, &config.pickup) => PROP,
            TargetKind::Prop { .. } => TOO_HEAVY,
        };
        prims.push(Primitive::Sprite {
            at: Anchor::World(reach.point),
            size: SpriteSize::World(size(status)),
            facing: Facing::Billboard,
            shape: Shape::Ring { thickness: 0.25 },
            color,
            depth: DepthMode::Always,
        });
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
const PROP: Rgba = [0.35, 0.85, 1.0, 0.9];
const WEAPON: Rgba = [1.0, 0.85, 0.3, 0.95];
const TOO_HEAVY: Rgba = [1.0, 0.35, 0.3, 0.9];
const HELD: Rgba = [0.35, 0.85, 1.0, 0.6];
