//! The markers: one renderer for every grip target, submitted to the overlay's
//! [`Source::GrabMarkers`] slot at the input tick's rate.
//!
//! Each target's marker takes its kind's colour ([`colour`]) and its own style ([`MarkStyle`]),
//! and shows as its [`Shown`] says: faint while no hand is within reach of it, brighter once one
//! is, brightest and a little larger on the target a squeeze would take, and solid while a hand
//! holds it. A marker on a surface lies on it
//! ([`Mark::normal`](crate::controllers::grab::Mark::normal)). A marker with a label
//! ([`Mark::label`](crate::controllers::grab::Mark::label)) has it drawn above it by the HUD
//! (`hud::mark_labels`, from [`published_labels`]) while a hand is at it.
//!
//! Every marker is anchored at render time (`docs/mod/rendering/render-time-anchoring.md`): a point
//! that rides with the player to the render-time head, one on a held gun to its hand's render-time
//! grip, and only a point that lies in the world to the world.

use std::time::Duration;

use jc3vr_core::published::{Published, Stage, Stamp, Window};
use jc3vr_input::frame::Hand;
use jc3vr_view::controllers::{
    cockpit::CockpitTransform,
    overlay::{
        primitive::{Anchor, DepthMode, Facing, HandPose, Primitive, Shape, Source, SpriteSize},
        store,
    },
};

use crate::{
    controllers::{
        grab::{Category, GripTarget, MarkAt, MarkStyle, Resolved, Shown, tuning::GRAB},
        hands::CockpitHand,
    },
    tuning::FRESHNESS,
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

/// One marker's label as the markers publish it for the HUD.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct MarkLabel {
    pub text: &'static str,
    /// Where the marker is drawn.
    pub at: Anchor,
    /// How far above the marker's centre its edge is, metres: the label sits above that.
    pub clearance: f32,
}

/// Submit the markers of `targets`, resolved for the hands as `resolved` says, and their labels.
pub fn submit(targets: &[GripTarget], resolved: &[Resolved; 2], frame: &MarkerFrame) {
    let mut prims = primitives(targets, resolved, frame);
    store::submit(
        Source::GrabMarkers,
        &mut prims,
        FRESHNESS.get().hands.grab_markers_ms.duration(),
    );
    publish_labels(labels(targets, resolved, frame));
}

/// Take the markers down.
pub fn clear() {
    store::clear(Source::GrabMarkers);
    publish_labels(Vec::new());
}

/// The labels published no longer than `stale` before `now`, or none when the latest list is
/// older or none was published.
pub fn published_labels(now: Stamp, stale: Duration) -> Vec<MarkLabel> {
    LABELS.read(|latest| {
        latest
            .and_then(|latest| latest.fresh(now, Window::new(stale)))
            .cloned()
            .unwrap_or_default()
    })
}

/// The labels of `targets`' labelled markers that a hand is within reach of, takes, or holds:
/// a label names what a hand has come to, so it shows only then, even on a marker shown always.
pub(crate) fn labels(
    targets: &[GripTarget],
    resolved: &[Resolved; 2],
    frame: &MarkerFrame,
) -> Vec<MarkLabel> {
    targets
        .iter()
        .enumerate()
        .filter_map(|(index, target)| {
            let text = target.mark.label?;
            let state = state_of(index, target, resolved);
            let near = state.reacher.is_some() || state.chosen || state.held;
            if !near || alpha(target.mark.shown, state).is_none() {
                return None;
            }
            Some(MarkLabel {
                text,
                at: anchor(target, frame)?,
                clearance: target.mark.size.max(GRAB.get().markers.min_size_m)
                    * frame.xf.world_scale
                    * 0.5,
            })
        })
        .collect()
}

/// The primitives that draw `targets`' markers.
pub fn primitives(
    targets: &[GripTarget],
    resolved: &[Resolved; 2],
    frame: &MarkerFrame,
) -> Vec<Primitive> {
    let tuning = &GRAB.get().markers;
    let depth = DepthMode::XRay {
        occluded_alpha: tuning.occluded_alpha,
    };
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
            MarkStyle::NoFit | MarkStyle::NoFitRing => tuning.colours.no_fit,
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
        let scale = if state.chosen {
            tuning.chosen_scale
        } else {
            1.0
        };
        let size = target.mark.size.max(tuning.min_size_m) * frame.xf.world_scale * scale;
        let color = [r, g, b, alpha];
        let lit = state.reacher.is_some() || state.held;
        let sprite = |size: f32, shape: Shape| Primitive::Sprite {
            at,
            size: SpriteSize::World(size),
            facing,
            shape,
            color,
            depth,
        };
        match target.mark.style {
            MarkStyle::Ring | MarkStyle::NoFitRing => {
                let thickness = if state.chosen || state.held {
                    tuning.chosen_thickness
                } else {
                    tuning.ring_thickness
                };
                prims.push(sprite(size, Shape::Ring { thickness }));
            }
            MarkStyle::Handle => {
                prims.push(sprite(size * tuning.handle_dot_share, Shape::Disc));
                if lit {
                    let thickness = tuning.handle_ring_thickness;
                    prims.push(sprite(size, Shape::Ring { thickness }));
                }
            }
            MarkStyle::Button => {
                let shape = if lit && !state.held {
                    Shape::Ring {
                        thickness: tuning.button_ring_thickness,
                    }
                } else {
                    Shape::Disc
                };
                prims.push(sprite(size, shape));
            }
            MarkStyle::NoFit => prims.push(sprite(size * tuning.no_fit_share, Shape::Disc)),
        }
    }
    prims
}

/// The colour of a kind of target.
pub fn colour(category: Category) -> [f32; 3] {
    let colours = &GRAB.get().markers.colours;
    match category {
        Category::Prop => colours.prop,
        Category::Weapon => colours.weapon,
        Category::Body => colours.body,
        Category::Vehicle => colours.vehicle,
        Category::Climb => colours.climb,
        Category::Character => colours.character,
        Category::Explosive => colours.explosive,
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

/// Publish the labels to draw. Called each input tick of the markers.
fn publish_labels(labels: Vec<MarkLabel>) {
    LABELS.publish(labels);
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
    let tuning = &GRAB.get().markers;
    if state.held {
        return (shown != Shown::Never).then_some(tuning.held_alpha);
    }
    if state.chosen {
        return (shown != Shown::Never).then_some(tuning.chosen_alpha);
    }
    match (shown, state.reacher) {
        (Shown::Never, _) => None,
        (_, Some(_)) => Some(tuning.reach_alpha),
        (Shown::Always, None) => Some(tuning.idle_alpha),
        (Shown::InReach, None) => None,
    }
}

/// Where `target`'s marker is drawn.
#[inline]
pub fn anchor(target: &GripTarget, frame: &MarkerFrame) -> Option<Anchor> {
    anchor_at(target.mark.at, frame)
}

/// Where a marker at `at` is drawn.
pub fn anchor_at(at: MarkAt, frame: &MarkerFrame) -> Option<Anchor> {
    match at {
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
#[cfg(any(test, feature = "test-support"))]
pub fn test_frame() -> MarkerFrame {
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

/// The labels to draw, published at the input tick's rate with when they were published.
static LABELS: Published<Vec<MarkLabel>> = Published::new(Stage::InputGame);
