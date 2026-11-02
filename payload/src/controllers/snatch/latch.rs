//! The armed snatch's in-world marker: while the grip holds a remote target and a yank would fire,
//! a solid dot ringed at the target and a line from the gripping hand to it say that the hand has
//! latched on. The grip's own target marker goes away once the snatch arms, since the snatch
//! offers no target while it is under way, so this marker takes its place until the snatch fires or
//! is cancelled.
//!
//! While the held line swings its target, the same slot draws the line: from the gripping hand to
//! the target's centre, its colour running from the latch's cyan when slack to red at the line's
//! strength and its width growing with the tension, a ring on it where the held length ends (the
//! target beyond the ring is stretching the line), and a dot between the hands while the other hand
//! braces.

use std::time::Duration;

use glam::Vec3;

use crate::controllers::{
    frame::Hand,
    overlay::{
        primitive::{Anchor, DepthMode, Facing, HandPose, Primitive, Shape, Source, SpriteSize},
        store,
    },
    snatch::SwingReadout,
};

/// Draw the latch of `hand` on the target at the world point `point`.
pub(super) fn submit(hand: Hand, point: Vec3) {
    let mut prims = primitives(hand, point);
    if prims.is_empty() {
        clear();
    } else {
        store::submit(Source::SnatchLatch, &mut prims, LATCH_TTL);
    }
}

/// Take the latch marker down.
pub(super) fn clear() {
    store::clear(Source::SnatchLatch);
}

/// Draw the held line of `hand` as `swing` reads it.
pub(super) fn submit_line(hand: Hand, swing: &SwingReadout) {
    let mut prims = line_primitives(hand, swing);
    if prims.is_empty() {
        clear();
    } else {
        store::submit(Source::SnatchLatch, &mut prims, LATCH_TTL);
    }
}

/// The primitives that draw the held line of `hand`: none for a line whose ends are not finite.
fn line_primitives(hand: Hand, swing: &SwingReadout) -> Vec<Primitive> {
    let offset = swing.centre - swing.anchor;
    let Some(direction) = offset.try_normalize().filter(|_| swing.centre.is_finite()) else {
        return Vec::new();
    };
    let share = if swing.max_tension_n > 0.0 {
        (swing.tension_n / swing.max_tension_n).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let mix = |a: f32, b: f32| a + (b - a) * share;
    let color = [
        mix(LATCH_COLOUR[0], STRAINED_COLOUR[0]),
        mix(LATCH_COLOUR[1], STRAINED_COLOUR[1]),
        mix(LATCH_COLOUR[2], STRAINED_COLOUR[2]),
        1.0,
    ];
    let held = swing.anchor + direction * swing.length_m.max(0.0);
    let mut prims = vec![
        Primitive::Line {
            from: Anchor::Hand {
                hand,
                pose: HandPose::Grip,
                offset: Vec3::ZERO,
            },
            to: Anchor::World(swing.centre),
            width: LINE_WIDTH_M + share * STRAINED_WIDTH_M,
            color,
            depth: DEPTH,
        },
        Primitive::Sprite {
            at: Anchor::World(held),
            size: SpriteSize::Angular(RING_RAD * 0.5),
            facing: Facing::Billboard,
            shape: Shape::Ring { thickness: 0.3 },
            color,
            depth: DEPTH,
        },
    ];
    if swing.braced {
        prims.push(Primitive::Sprite {
            at: Anchor::World(swing.anchor),
            size: SpriteSize::Angular(DOT_RAD),
            facing: Facing::Billboard,
            shape: Shape::Disc,
            color,
            depth: DEPTH,
        });
    }
    prims
}

/// The primitives that draw the latch of `hand` on the target at `point`: none for a point that is
/// not finite.
fn primitives(hand: Hand, point: Vec3) -> Vec<Primitive> {
    if !point.is_finite() {
        return Vec::new();
    }
    let at = Anchor::World(point);
    let sprite = |size: f32, shape: Shape| Primitive::Sprite {
        at,
        size: SpriteSize::Angular(size),
        facing: Facing::Billboard,
        shape,
        color: LATCH_COLOUR,
        depth: DEPTH,
    };
    vec![
        sprite(DOT_RAD, Shape::Disc),
        sprite(RING_RAD, Shape::Ring { thickness: 0.3 }),
        Primitive::Line {
            from: Anchor::Hand {
                hand,
                pose: HandPose::Grip,
                offset: Vec3::ZERO,
            },
            to: at,
            width: LINE_WIDTH_M,
            color: [
                LATCH_COLOUR[0],
                LATCH_COLOUR[1],
                LATCH_COLOUR[2],
                LINE_ALPHA,
            ],
            depth: DEPTH,
        },
    ]
}

/// How long the marker lasts without a fresh submit: a little over a few input ticks, so that a
/// snatch that stops ticking does not leave it behind.
const LATCH_TTL: Duration = Duration::from_millis(150);

/// The latch's colour: a cyan no grip target uses, so that it reads as held rather than offered.
const LATCH_COLOUR: [f32; 4] = [0.3, 0.95, 1.0, 1.0];

/// The held line's colour at its strength.
const STRAINED_COLOUR: [f32; 4] = [1.0, 0.25, 0.15, 1.0];

/// How much wider the held line is at its strength, metres.
const STRAINED_WIDTH_M: f32 = 0.008;

/// The line's opacity, fainter than the dot so that it does not hide the target.
const LINE_ALPHA: f32 = 0.6;

/// The dot's and the ring's apparent diameters, radians.
const DOT_RAD: f32 = 0.012;
const RING_RAD: f32 = 0.04;

/// The line's width, metres.
const LINE_WIDTH_M: f32 = 0.004;

/// Seen through the scene, dimmed, as the grip markers are.
const DEPTH: DepthMode = DepthMode::XRay {
    occluded_alpha: 0.5,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_latch_rings_the_target_and_joins_it_to_the_gripping_hand() {
        let point = Vec3::new(1.0, 0.0, -3.0);
        let prims = primitives(Hand::Left, point);
        let at_target = prims
            .iter()
            .filter(|p| matches!(p, Primitive::Sprite { at, .. } if *at == Anchor::World(point)))
            .count();
        assert_eq!(at_target, 2);
        assert!(prims.iter().any(|p| matches!(
            p,
            Primitive::Line {
                from: Anchor::Hand {
                    hand: Hand::Left,
                    pose: HandPose::Grip,
                    ..
                },
                to,
                ..
            } if *to == Anchor::World(point)
        )));
    }

    fn swing(tension_n: f32, braced: bool) -> SwingReadout {
        SwingReadout {
            length_m: 3.0,
            tension_n,
            max_tension_n: 1000.0,
            mass_kg: 100.0,
            braced,
            tethered: true,
            anchor: Vec3::ZERO,
            centre: Vec3::new(0.0, 0.0, -3.5),
        }
    }

    #[test]
    fn the_held_line_rings_its_held_length_and_reddens_with_its_tension() {
        let slack = line_primitives(Hand::Left, &swing(0.0, false));
        assert_eq!(slack.len(), 2);
        assert!(slack.iter().any(|p| matches!(
            p,
            Primitive::Sprite { at: Anchor::World(at), .. } if (*at - Vec3::new(0.0, 0.0, -3.0)).length() < 1e-5
        )));
        let line_of = |prims: &[Primitive]| {
            prims.iter().find_map(|p| match p {
                Primitive::Line { color, width, .. } => Some((*color, *width)),
                _ => None,
            })
        };
        let (slack_colour, slack_width) = line_of(&slack).unwrap();
        let (taut_colour, taut_width) =
            line_of(&line_primitives(Hand::Left, &swing(1000.0, false))).unwrap();
        let near = |a: [f32; 4], b: [f32; 4]| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-5);
        assert!(near(slack_colour, LATCH_COLOUR));
        assert!(near(taut_colour, STRAINED_COLOUR));
        assert!(taut_width > slack_width);
        assert_eq!(line_primitives(Hand::Left, &swing(0.0, true)).len(), 3);
    }

    #[test]
    fn a_line_with_no_length_draws_nothing() {
        let mut coincident = swing(0.0, false);
        coincident.centre = coincident.anchor;
        assert!(line_primitives(Hand::Left, &coincident).is_empty());
    }

    #[test]
    fn a_target_with_no_place_draws_nothing() {
        assert!(primitives(Hand::Left, Vec3::NAN).is_empty());
    }
}
