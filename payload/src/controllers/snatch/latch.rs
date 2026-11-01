//! The armed snatch's in-world marker: while the grip holds a remote target and a yank would fire,
//! a solid dot ringed at the target and a line from the gripping hand to it say that the hand has
//! latched on. The grip's own target marker goes away once the snatch arms, since the snatch
//! offers no target while it is under way, so this marker takes its place until the snatch fires or
//! is cancelled.

use std::time::Duration;

use glam::Vec3;

use crate::controllers::{
    frame::Hand,
    overlay::{
        primitive::{Anchor, DepthMode, Facing, HandPose, Primitive, Shape, Source, SpriteSize},
        store,
    },
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

    #[test]
    fn a_target_with_no_place_draws_nothing() {
        assert!(primitives(Hand::Left, Vec3::NAN).is_empty());
    }
}
