//! The wingsuit's arm-steering guide on the overlay: a ring on each hand, and, while the arms
//! steer, a line between them, the wing. Both are anchored to the hands at render time, since the
//! wingsuit outruns a world anchor placed at the tick rate
//! (`docs/mod/rendering/render-time-anchoring.md`).

use std::time::Duration;

use crate::controllers::{
    frame::Hand,
    overlay::{
        primitive::{
            Anchor, DepthMode, Facing, HandPose, Primitive, Rgba, Shape, Source, SpriteSize,
        },
        store,
    },
};

/// Submit the guide while `shown`, lit while the arms steer (`engaged`), or clear it.
pub(super) fn submit(shown: bool, engaged: bool) {
    if !shown {
        store::clear(Source::WingGuide);
        return;
    }
    let mut prims = primitives(engaged);
    store::submit(Source::WingGuide, &mut prims, GUIDE_TTL);
}

fn primitives(engaged: bool) -> Vec<Primitive> {
    let color = if engaged { WING_ENGAGED } else { WING_IDLE };
    let mut prims: Vec<Primitive> = Hand::BOTH
        .into_iter()
        .map(|hand| Primitive::Sprite {
            at: Anchor::hand(hand, HandPose::Grip),
            size: SpriteSize::World(WING_RING_M),
            facing: Facing::Billboard,
            shape: Shape::Ring { thickness: 0.2 },
            color,
            depth: XRAY,
        })
        .collect();
    if engaged {
        prims.push(Primitive::Line {
            from: Anchor::hand(Hand::Left, HandPose::Grip),
            to: Anchor::hand(Hand::Right, HandPose::Grip),
            width: WING_LINE_M,
            color,
            depth: XRAY,
        });
    }
    prims
}

/// The guide is submitted at the input tick's rate, so it lives a little past one tick.
const GUIDE_TTL: Duration = Duration::from_millis(150);

const XRAY: DepthMode = DepthMode::XRay {
    occluded_alpha: 0.4,
};

/// The guide's rings while the arms are not spread: faint, a prompt to spread them.
const WING_IDLE: Rgba = [0.8, 0.85, 0.9, 0.35];

/// The guide while the arms steer.
const WING_ENGAGED: Rgba = [0.35, 1.0, 0.6, 0.85];

/// The guide's rings' diameter, metres.
const WING_RING_M: f32 = 0.09;

/// How wide the guide's line between the hands is drawn, metres.
const WING_LINE_M: f32 = 0.006;
