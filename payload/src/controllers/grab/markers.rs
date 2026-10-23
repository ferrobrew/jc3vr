//! The markers: one renderer for every grip target, submitted to the overlay's
//! [`Source::GrabMarkers`] slot at the input tick's rate.
//!
//! Each target's marker takes its kind's colour ([`colour`]) and its own style ([`MarkStyle`]),
//! and shows as its [`Shown`] says: faint while no hand is within reach of it, brighter once one
//! is, brightest and a little larger on the target a squeeze would take, and solid while a hand
//! holds it. A marker on a surface lies on it ([`Mark::normal`]).
//!
//! Every marker is anchored at render time (`docs/mod/rendering/render-time-anchoring.md`): a point
//! that rides with the player to the render-time head, one on a held gun to its hand's render-time
//! grip, and only a point that lies in the world to the world.

use std::time::Duration;

use crate::controllers::{
    frame::Hand,
    grab::{Category, GripTarget, MarkAt, MarkStyle, Resolved, Shown},
    hands::CockpitHand,
    overlay::{
        primitive::{Anchor, DepthMode, Facing, HandPose, Primitive, Shape, Source, SpriteSize},
        store,
    },
    vehicles::wheel::CockpitTransform,
};

/// Where the tick's cockpit-frame points are placed for the overlay, and the hands that reach for
/// the markers.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct MarkerFrame {
    /// The cockpit frame against the world at the tick.
    pub xf: CockpitTransform,
    /// The hands as they are drawn, by [`Hand::index`].
    pub drawn: [Option<CockpitHand>; 2],
}

/// Submit the markers of `targets`, resolved for the hands as `resolved` says.
pub fn submit(targets: &[GripTarget], resolved: &[Resolved; 2], frame: &MarkerFrame) {
    let mut prims = primitives(targets, resolved, frame);
    store::submit(Source::GrabMarkers, &mut prims, MARKER_TTL);
}

/// Take the markers down.
pub fn clear() {
    store::clear(Source::GrabMarkers);
}

/// The primitives that draw `targets`' markers.
pub fn primitives(
    targets: &[GripTarget],
    resolved: &[Resolved; 2],
    frame: &MarkerFrame,
) -> Vec<Primitive> {
    let mut prims = Vec::new();
    for (index, target) in targets.iter().enumerate() {
        let state = state_of(index, target, resolved);
        let Some(alpha) = alpha(target.mark.shown, state) else {
            continue;
        };
        let Some(at) = anchor(target, frame) else {
            continue;
        };
        let [r, g, b] = match target.mark.style {
            MarkStyle::NoFit => NO_FIT_COLOUR,
            _ => colour(target.category()),
        };
        let facing = match target.mark.normal {
            None => Facing::Billboard,
            Some(normal) => Facing::Surface {
                normal: match target.mark.at {
                    MarkAt::World(_) => normal,
                    MarkAt::Cockpit(_) | MarkAt::OnHand { .. } => {
                        frame.xf.direction_to_world(normal)
                    }
                },
            },
        };
        let scale = if state.chosen { CHOSEN_SCALE } else { 1.0 };
        let size = target.mark.size.max(MIN_SIZE_M) * frame.xf.world_scale * scale;
        let color = [r, g, b, alpha];
        let lit = state.reacher.is_some() || state.held;
        let sprite = |size: f32, shape: Shape| Primitive::Sprite {
            at,
            size: SpriteSize::World(size),
            facing,
            shape,
            color,
            depth: XRAY,
        };
        match target.mark.style {
            MarkStyle::Ring => {
                let thickness = if state.chosen || state.held {
                    CHOSEN_THICKNESS
                } else {
                    RING_THICKNESS
                };
                prims.push(sprite(size, Shape::Ring { thickness }));
            }
            MarkStyle::Handle => {
                prims.push(sprite(size * HANDLE_DOT_SHARE, Shape::Disc));
                if lit {
                    prims.push(sprite(size, Shape::Ring { thickness: 0.12 }));
                }
            }
            MarkStyle::Button => {
                let shape = if lit && !state.held {
                    Shape::Ring { thickness: 0.15 }
                } else {
                    Shape::Disc
                };
                prims.push(sprite(size, shape));
            }
            MarkStyle::NoFit => prims.push(sprite(size * NO_FIT_SHARE, Shape::Disc)),
        }
    }
    prims
}

/// The colour of a kind of target.
pub const fn colour(category: Category) -> [f32; 3] {
    match category {
        Category::Prop => [1.0, 1.0, 1.0],
        Category::Weapon => [1.0, 0.8, 0.25],
        Category::Body => [0.35, 0.6, 1.0],
        Category::Vehicle => [0.4, 1.0, 0.45],
        Category::Climb => [1.0, 0.55, 0.15],
        Category::Character => [0.75, 0.45, 1.0],
        Category::Explosive => [1.0, 0.3, 0.25],
    }
}

/// How the hands stand to one target this tick.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
struct MarkState {
    /// The hand nearest the target among those within reach of it.
    reacher: Option<Hand>,
    /// Whether a squeeze of some hand would take it.
    chosen: bool,
    held: bool,
}

fn state_of(index: usize, target: &GripTarget, resolved: &[Resolved; 2]) -> MarkState {
    let mut state = MarkState {
        held: target.held_by.is_some(),
        ..MarkState::default()
    };
    let mut nearest = f32::INFINITY;
    for hand in Hand::BOTH {
        let resolved = &resolved[hand.index()];
        if let Some(reach) = resolved.reaches.iter().find(|r| r.index == index)
            && reach.distance < nearest
        {
            nearest = reach.distance;
            state.reacher = Some(hand);
        }
        state.chosen |= resolved.chosen.is_some_and(|c| c.index == index);
    }
    state
}

/// The marker's opacity in `state`, or `None` when it is not drawn.
fn alpha(shown: Shown, state: MarkState) -> Option<f32> {
    if state.held {
        return (shown != Shown::Never).then_some(HELD_ALPHA);
    }
    if state.chosen {
        return (shown != Shown::Never).then_some(CHOSEN_ALPHA);
    }
    match (shown, state.reacher) {
        (Shown::Never, _) => None,
        (_, Some(_)) => Some(REACH_ALPHA),
        (Shown::Always, None) => Some(IDLE_ALPHA),
        (Shown::InReach, None) => None,
    }
}

/// Where `target`'s marker is drawn.
pub(crate) fn anchor(target: &GripTarget, frame: &MarkerFrame) -> Option<Anchor> {
    match target.mark.at {
        MarkAt::World(point) => point.is_finite().then_some(Anchor::World(point)),
        MarkAt::Cockpit(point) => point
            .is_finite()
            .then(|| Anchor::Head(frame.xf.point_from_head(point))),
        MarkAt::OnHand { hand, point } => {
            let grip = frame.drawn[hand.index()]?.grip;
            let grip_world = frame.xf.point_to_world(grip.position);
            let orientation = (frame.xf.body * grip.orientation).normalize();
            let offset = orientation.inverse() * (frame.xf.point_to_world(point) - grip_world);
            offset.is_finite().then_some(Anchor::Hand {
                hand,
                pose: HandPose::Grip,
                offset,
            })
        }
    }
}

/// A frame with no turn, for the tests.
#[cfg(test)]
pub(crate) fn test_frame() -> MarkerFrame {
    use glam::{Quat, Vec3};

    MarkerFrame {
        xf: CockpitTransform {
            head_world: Vec3::new(10.0, 2.0, -5.0),
            head_cockpit: Vec3::new(0.0, 1.7, 0.0),
            body: Quat::IDENTITY,
            world_scale: 1.0,
        },
        drawn: [None; 2],
    }
}

/// The markers are submitted at the input tick's rate, so they live a little past one tick.
const MARKER_TTL: Duration = Duration::from_millis(150);

const XRAY: DepthMode = DepthMode::XRay {
    occluded_alpha: 0.4,
};

/// The opacity of a marker no hand is within reach of: faint, so that it shows where the target is
/// without drawing the eye.
const IDLE_ALPHA: f32 = 0.22;
/// The opacity of a marker a hand is within reach of.
const REACH_ALPHA: f32 = 0.55;
/// The opacity of the marker a squeeze would take.
const CHOSEN_ALPHA: f32 = 0.95;
/// The opacity of a marker a hand holds.
const HELD_ALPHA: f32 = 0.8;
/// How much larger the marker a squeeze would take is drawn.
const CHOSEN_SCALE: f32 = 1.15;
const RING_THICKNESS: f32 = 0.15;
const CHOSEN_THICKNESS: f32 = 0.3;
/// The share of a handle's marker its centre dot spans.
const HANDLE_DOT_SHARE: f32 = 0.3;
/// The colour of a place that does not take what the hand holds.
const NO_FIT_COLOUR: [f32; 3] = [1.0, 0.2, 0.2];
/// The share of its size a no-fit marker's disc spans.
const NO_FIT_SHARE: f32 = 0.4;
/// The smallest a marker is drawn, metres.
const MIN_SIZE_M: f32 = 0.02;
