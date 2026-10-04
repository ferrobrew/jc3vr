//! The simulator: the semantic input a runtime following a layout of default bindings would report
//! for one tick of raw controls. Pure test support, so that the tables' tests can drive them with
//! the physical presses a player makes, and so that the semantic tables can be checked against the
//! raw-input tables they replace.
//!
//! Each action reads the raw control its default binding names on each hand the binding covers,
//! with the deadzones and press thresholds the raw controls already carry. A trackpad press has no
//! position in [`HandControls`] beyond its half, so a pad action reads the hand's trackpad position
//! when one is set, and otherwise the middle of the pressed half.

use glam::Vec2;

use crate::{
    controllers::{
        bindings::{
            controls::{ButtonControl, Controls, HandControls},
            intents::{ActionInput, Intent, SemanticInput, SetInput},
        },
        frame::{Hand, InteractionProfile},
    },
    vr::input::semantic::{
        SemanticSet,
        bound::{BoundSet, Layout},
        defaults::Component,
    },
};

/// The semantic input of `global` and `mode` for one tick of `controls` under `layout`, on Index
/// controllers.
pub(crate) fn simulate(controls: &Controls, mode: SemanticSet, layout: Layout) -> SemanticInput {
    let set = |set: SemanticSet| {
        let bound = BoundSet::defaults(
            set,
            [InteractionProfile::ValveIndex; 2],
            controls.dominant,
            layout,
        );
        SetInput {
            set,
            actions: (0..set.actions().len())
                .map(|index| ActionInput {
                    hands: Hand::BOTH.map(|hand| {
                        layout(set, index)
                            .filter(|b| b.side.covers(hand, controls.dominant))
                            .map_or_else(Intent::default, |b| {
                                read(controls.hand(hand), b.component)
                            })
                    }),
                    bound: Hand::BOTH.map(|hand| bound.get(index, hand)),
                })
                .collect(),
        }
    };
    SemanticInput {
        global: set(SemanticSet::Global),
        mode: set(mode),
    }
}

/// The state an action bound to `component` reads from one hand's raw controls.
fn read(hand: &HandControls, component: Component) -> Intent {
    let press = |button: ButtonControl| Intent {
        button,
        ..Default::default()
    };
    let pulled = |button: ButtonControl| f32::from(u8::from(button.held()));
    match component {
        Component::LowerFace => press(hand.a),
        Component::UpperFace => press(hand.b),
        Component::ThumbstickClick => press(hand.stick_click),
        Component::System => press(hand.system),
        Component::TrackpadTouch => Intent::default(),
        Component::TriggerValue => Intent {
            button: hand.trigger_press,
            value: hand.trigger,
            raw: hand.trigger,
            ..Default::default()
        },
        Component::GripPress => Intent {
            button: hand.grip,
            value: pulled(hand.grip),
            raw: pulled(hand.grip),
            ..Default::default()
        },
        Component::Thumbstick => Intent {
            vector: hand.stick,
            vector_raw: hand.stick_raw,
            ..Default::default()
        },
        Component::ThumbstickX => Intent {
            value: hand.stick.x,
            raw: hand.stick_raw.x,
            ..Default::default()
        },
        Component::ThumbstickY => Intent {
            value: hand.stick.y,
            raw: hand.stick_raw.y,
            ..Default::default()
        },
        Component::Trackpad => {
            let pad = pad_position(hand);
            Intent {
                vector: pad,
                vector_raw: pad,
                ..Default::default()
            }
        }
        Component::TrackpadForce => {
            let button = pad_press(hand);
            Intent {
                button,
                value: pulled(button),
                raw: pulled(button),
                ..Default::default()
            }
        }
    }
}

/// The trackpad pressed on either half.
fn pad_press(hand: &HandControls) -> ButtonControl {
    let (l, r) = (hand.trackpad_left, hand.trackpad_right);
    ButtonControl {
        down: l.down || r.down,
        pressed: l.pressed || r.pressed,
        released: (l.released || r.released) && !(l.down || r.down),
    }
}

/// The thumb's position on the trackpad: the hand's own when set, otherwise the middle of the
/// pressed half.
fn pad_position(hand: &HandControls) -> Vec2 {
    if hand.trackpad != Vec2::ZERO {
        hand.trackpad
    } else if hand.trackpad_left.held() {
        Vec2::new(-0.6, 0.0)
    } else if hand.trackpad_right.held() {
        Vec2::new(0.6, 0.0)
    } else {
        Vec2::ZERO
    }
}
