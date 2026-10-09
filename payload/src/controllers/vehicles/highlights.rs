//! The vehicle handling's overlay: a ring on the control each free hand can grab, the parachute's
//! handles, the wingsuit's arm-steering guide, and, for tuning, the wheel's rim and axes.
//!
//! Every mark is placed in the cockpit frame, which rides with the player, so each is anchored to
//! the render-time head (or a hand), never to the world: a world point placed at the tick rate
//! trails a moving vehicle by up to a tick (`docs/mod/rendering/render-time-anchoring.md`).

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
    vehicles::{
        parachute::{Mark, MarkState},
        physical::buttons::{Button, ButtonPlacement, ButtonState, PushButton, PushPhase},
        wheel::{CockpitTransform, WheelFrame},
    },
};

/// What the overlay shows this tick.
pub(super) struct Highlights<'a> {
    /// The placed grab volumes, in the cockpit frame.
    pub candidates: &'a [Candidate],
    /// The volume each hand is in, indexed by [`Hand::index`].
    pub volumes: [Option<GrabVolume>; 2],
    /// Whether the wingsuit's arms steer, while its guide shows.
    pub wings: Option<bool>,
    /// The parachute handles' marks, while they show.
    pub handles: Option<[Mark; 2]>,
    /// The wheel's rim and axes, while they show.
    pub wheel: Option<WheelFrame>,
    /// The cockpit's push buttons and how near a hand is to pressing each.
    pub buttons: &'a [ButtonMark],
}

/// A push button's mark: the button and the nearest any hand is to pressing it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct ButtonMark {
    pub button: Button,
    pub at: PushButton,
    pub phase: PushPhase,
}

/// The marks of the push buttons `placement` places, by how near each hand is to pressing them.
pub(super) fn button_marks(placement: &ButtonPlacement, state: &ButtonState) -> Vec<ButtonMark> {
    Button::ALL
        .into_iter()
        .filter_map(|button| {
            let at = placement.button(button)?;
            let phases = Hand::BOTH.map(|hand| state.phase(button, hand));
            let phase = if phases.contains(&PushPhase::Pressed) {
                PushPhase::Pressed
            } else if phases.contains(&PushPhase::Armed) {
                PushPhase::Armed
            } else {
                PushPhase::Out
            };
            Some(ButtonMark { button, at, phase })
        })
        .collect()
}

/// Submit a highlight on the control each free hand can grab, the parachute handles' marks, the
/// wingsuit's guide, and the wheel's rim and axes, as `highlights` gives them. `xf` maps the
/// cockpit frame the candidates are in to the world.
pub(super) fn submit(config: &ControllersConfig, highlights: &Highlights, xf: &CockpitTransform) {
    let Highlights {
        candidates,
        volumes,
        wings,
        handles,
        wheel,
        buttons,
    } = *highlights;
    let mut prims = Vec::new();
    if config.overlay.enabled {
        for mark in buttons {
            button_primitives(mark, xf, &mut prims);
        }
    }
    if let Some(wheel) = wheel {
        wheel_primitives(&wheel, xf, &mut prims);
    }
    if let Some(engaged) = wings.filter(|_| config.overlay.enabled) {
        wing_primitives(engaged, &mut prims);
    }
    if let Some(marks) = handles.filter(|_| config.overlay.enabled) {
        for mark in marks {
            handle_primitives(&mark, xf, &mut prims);
        }
    }
    if config.overlay.enabled && config.overlay.grab_volumes {
        for hand in Hand::BOTH {
            let Some(volume) = volumes[hand.index()] else {
                continue;
            };
            if handles.is_some() && matches!(volume, GrabVolume::ParachuteHandle(_)) {
                // The handle's own mark already shows it in reach.
                continue;
            }
            let Some(c) = candidates
                .iter()
                .find(|c| c.volume == volume && c.hand == Some(hand))
            else {
                continue;
            };
            prims.push(Primitive::Sprite {
                at: Anchor::Head(xf.point_from_head(c.centre)),
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

/// A parachute handle out of its hand's reach: faint, so it can be found without cluttering the
/// view.
const HANDLE_IDLE: Rgba = [0.85, 0.75, 0.55, 0.3];

/// A parachute handle in its hand's reach: a grip press takes it.
const HANDLE_IN_REACH: Rgba = [1.0, 0.85, 0.45, 0.8];

/// A held parachute handle.
const HANDLE_HELD: Rgba = [0.35, 1.0, 0.6, 0.9];

/// The share of a handle's grab radius its centre dot spans.
const HANDLE_DOT_SHARE: f32 = 0.3;

/// The wingsuit guide's rings while the arms are not spread: faint, a prompt to spread them.
const WING_IDLE: Rgba = [0.8, 0.85, 0.9, 0.35];

/// The wingsuit guide while the arms steer.
const WING_ENGAGED: Rgba = [0.35, 1.0, 0.6, 0.85];

/// The wingsuit guide's rings' diameter, metres.
const WING_RING_M: f32 = 0.09;

/// How wide the wingsuit guide's line between the hands is drawn, metres.
const WING_LINE_M: f32 = 0.006;

const WHEEL: Rgba = [1.0, 0.8, 0.3, 0.8];

/// A push button no hand is at: shown faintly for the nitrous button, which the vehicle's model
/// does not draw, and not at all for the horn, which is the drawn hub.
const BUTTON_IDLE: Rgba = [1.0, 0.45, 0.3, 0.35];

/// A push button a hand is in front of: a push from here presses it.
const BUTTON_ARMED: Rgba = [1.0, 0.6, 0.35, 0.8];

/// A pressed push button.
const BUTTON_PRESSED: Rgba = [0.35, 1.0, 0.6, 0.9];

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

/// A parachute handle: a ring the size of its grab volume around a dot, a toggle to reach for. Out of
/// reach only the dot shows, faintly.
fn handle_primitives(mark: &Mark, xf: &CockpitTransform, prims: &mut Vec<Primitive>) {
    // Anchored to the head, so the mark rides with the falling player between ticks.
    let at = Anchor::Head(xf.point_from_head(mark.centre));
    let diameter = mark.radius * 2.0 * xf.world_scale;
    let color = match mark.state {
        MarkState::Idle => HANDLE_IDLE,
        MarkState::InReach => HANDLE_IN_REACH,
        MarkState::Held => HANDLE_HELD,
    };
    prims.push(Primitive::Sprite {
        at,
        size: SpriteSize::World(diameter * HANDLE_DOT_SHARE),
        facing: Facing::Billboard,
        shape: Shape::Disc,
        color,
        depth: XRAY,
    });
    if mark.state != MarkState::Idle {
        prims.push(Primitive::Sprite {
            at,
            size: SpriteSize::World(diameter),
            facing: Facing::Billboard,
            shape: Shape::Ring { thickness: 0.12 },
            color,
            depth: XRAY,
        });
    }
}

/// A push button: a disc on its face, ringed while a hand is in front of it or pressing it.
fn button_primitives(mark: &ButtonMark, xf: &CockpitTransform, prims: &mut Vec<Primitive>) {
    let color = match mark.phase {
        PushPhase::Out if mark.button == Button::Horn => return,
        PushPhase::Out => BUTTON_IDLE,
        PushPhase::Armed => BUTTON_ARMED,
        PushPhase::Pressed => BUTTON_PRESSED,
    };
    let at = Anchor::Head(xf.point_from_head(mark.at.centre));
    let normal = xf.direction_to_world(mark.at.normal);
    let diameter = mark.at.radius * 2.0 * xf.world_scale;
    let shape = if mark.phase == PushPhase::Out {
        Shape::Disc
    } else {
        Shape::Ring { thickness: 0.15 }
    };
    prims.push(Primitive::Sprite {
        at,
        size: SpriteSize::World(diameter),
        facing: Facing::Surface { normal },
        shape,
        color,
        depth: XRAY,
    });
}

/// The wheel's rim, its axis from the hub toward the driver, and a spoke to the top of its rim,
/// anchored to the head.
fn wheel_primitives(wheel: &WheelFrame, xf: &CockpitTransform, prims: &mut Vec<Primitive>) {
    let hub = xf.point_from_head(wheel.centre);
    let normal = xf.direction_to_world(wheel.normal());
    let radius = wheel.radius * xf.world_scale;
    prims.push(Primitive::Sprite {
        at: Anchor::Head(hub),
        size: SpriteSize::World(radius * 2.0),
        facing: Facing::Surface { normal },
        shape: Shape::Ring { thickness: 0.06 },
        color: WHEEL,
        depth: XRAY,
    });
    for to in [
        hub + normal * radius,
        xf.point_from_head(wheel.rim_point(0.0)),
    ] {
        prims.push(Primitive::Line {
            from: Anchor::Head(hub),
            to: Anchor::Head(to),
            width: AXIS_WIDTH_M,
            color: WHEEL,
            depth: XRAY,
        });
    }
}
