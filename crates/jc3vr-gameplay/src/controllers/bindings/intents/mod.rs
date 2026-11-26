//! The semantic actions as the binding tables read them: each action's per-hand state over the
//! tick ([`Intent`]), with the inputs the mod claims masked out and the gestures folded in
//! ([`Intents`]). Pure.
//!
//! The tick starts from the [`SemanticInput`]: `global` and the active mode set, read from the
//! published [`SemanticFrame`] and its edges ([`SemanticInput::from_frame`]), each action with the
//! physical inputs it is bound to on each hand ([`BoundSet`]). [`Intents::build`] then applies,
//! in order:
//!
//! The shared input arbiter has already filtered the sampled semantic values before this module
//! applies its contextual masks. Raw stick levels never substitute for semantic action values.
//!
//! - the masks ([`Mask`]): a mechanism that claims a physical input on a hand (a grip press
//!   latched to a grab, the dual-wield split's off-hand trigger, the native UI pointer's trigger,
//!   the debug panel chord's buttons) or a whole hand (input recovery) puts every action that reads
//!   it there at rest, but the one action the mechanism exempts. An action with a source the mod
//!   does not recognise is masked only with its whole hand;
//! - the gestures, as sources of the actions they stand for: the yank presses the grapple hand's
//!   `fire_grapple` for a tick.
//!
//! The two hands' states of an action combine as OpenXR combines an action's bindings: a press is
//! held while either hand holds it, a value is the larger in magnitude, and a vector the longer.

#[cfg(test)]
pub(crate) mod simulate;
#[cfg(test)]
mod tests;

use glam::Vec2;

use crate::{
    controllers::{
        bindings::controls::{ButtonControl, radial_deadzone, scalar_deadzone},
        config::BindingsConfig,
        frame::{Hand, InteractionProfile},
    },
    vr::input::{
        semantic::{
            SemanticSet,
            bound::{BoundSet, Physical, PhysicalSet},
            defaults::{Component, default_binding},
            frame::{ActionValue, SemanticEdges, SemanticFrame, SetState},
            vocabulary::{ActionId, OnFootAction, WingsuitAction},
        },
        sources,
    },
};

/// One semantic action's state on one hand over the tick.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Intent {
    /// The press: a boolean action held, or a float action past its press thresholds.
    pub button: ButtonControl,
    /// A float action's value past its deadzone, signed.
    pub value: f32,
    /// A float action's value as reported, for consumers with their own deadzone (the body turn).
    pub raw: f32,
    /// A vector action past its radial deadzone.
    pub vector: Vec2,
    /// A vector action as reported.
    pub vector_raw: Vec2,
}

/// One action's state on both hands, with the physical inputs it is bound to on each.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ActionInput {
    /// By [`Hand::index`].
    pub hands: [Intent; 2],
    /// By [`Hand::index`].
    pub bound: [PhysicalSet; 2],
}

/// One set's actions, by action index.
#[derive(Debug, Clone, PartialEq)]
pub struct SetInput {
    pub set: SemanticSet,
    pub actions: Vec<ActionInput>,
}

impl SetInput {
    /// Every action of `set` at rest, bound as `bound` says.
    fn rest(bound: &BoundSet) -> SetInput {
        SetInput {
            set: bound.set,
            actions: bound
                .actions
                .iter()
                .map(|&bound| ActionInput {
                    hands: Default::default(),
                    bound,
                })
                .collect(),
        }
    }
}

/// The semantic actions of one tick before the bindings' masks: `global` and the active mode set.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticInput {
    pub global: SetInput,
    pub mode: SetInput,
}

impl SemanticInput {
    /// Every action of `global` and `mode` at rest, bound by the default bindings: the input of a
    /// tick with no semantic frame.
    pub fn rest(mode: SemanticSet, profiles: [InteractionProfile; 2], dominant: Hand) -> Self {
        let bound = |set| BoundSet::defaults(set, profiles, dominant, default_binding);
        Self {
            global: SetInput::rest(&bound(SemanticSet::Global)),
            mode: SetInput::rest(&bound(mode)),
        }
    }

    /// The tick's input from the latest semantic frame and the edges since the last tick. Each
    /// set's physical inputs are those the runtime last reported ([`sources::enumerated`]), with
    /// the default bindings for `profiles` and `dominant` standing in until it has.
    pub fn from_frame(
        frame: &SemanticFrame,
        edges: &SemanticEdges,
        profiles: [InteractionProfile; 2],
        dominant: Hand,
        config: &BindingsConfig,
    ) -> Self {
        let read = |state: &SetState| {
            let enumerated = sources::enumerated(state.set).unwrap_or_default();
            let bound =
                BoundSet::merged(state.set, &enumerated, profiles, dominant, default_binding);
            read_set(state, &bound, edges, config)
        };
        Self {
            global: read(&frame.global),
            mode: read(&frame.mode),
        }
    }

    /// The state of `action`, or `None` when its set is not one of the tick's.
    pub fn get(&self, action: ActionId) -> Option<&ActionInput> {
        [&self.global, &self.mode]
            .into_iter()
            .find(|s| s.set == action.set)?
            .actions
            .get(action.index())
    }

    pub(crate) fn get_mut(&mut self, action: ActionId) -> Option<&mut ActionInput> {
        [&mut self.global, &mut self.mode]
            .into_iter()
            .find(|s| s.set == action.set)?
            .actions
            .get_mut(action.index())
    }
}

/// What claims a physical input, or a whole hand, from the semantic actions.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Mask {
    pub hand: Hand,
    pub scope: MaskScope,
    /// The one action the claim leaves alone: the one the claiming mechanism reads itself.
    pub exempt: Option<ActionId>,
}

/// What a [`Mask`] covers on its hand.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MaskScope {
    /// Every action on the hand, whatever it is bound to.
    Hand,
    /// The actions bound to this input.
    Input(Physical),
}

impl Mask {
    /// Whether the mask puts `action`, bound to `bound` on the mask's hand, at rest.
    pub fn covers(&self, action: ActionId, bound: PhysicalSet) -> bool {
        if self.exempt == Some(action) {
            return false;
        }
        match self.scope {
            MaskScope::Hand => true,
            MaskScope::Input(physical) => bound.contains(physical),
        }
    }
}

/// The gestures of the tick that stand for semantic actions.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Gestures {
    /// The yank gesture fired: a grapple tap on the grapple hand.
    pub yank: bool,
}

/// The actions a gesture presses, in whichever of them is in the tick's sets.
pub const YANK_ACTIONS: [ActionId; 2] = [
    OnFootAction::FireGrapple.id(),
    WingsuitAction::FireGrapple.id(),
];

/// The semantic actions of one tick as the binding tables read them.
#[derive(Debug, Clone, PartialEq)]
pub struct Intents {
    input: SemanticInput,
}

impl Intents {
    /// Apply contextual masks and gestures to already-arbitrated semantic input.
    /// `grapple_hand` is the hand the yank presses on.
    pub fn build(
        input: &SemanticInput,
        masks: &[Mask],
        gestures: Gestures,
        grapple_hand: Hand,
    ) -> Intents {
        let mut input = input.clone();
        for set in [&mut input.global, &mut input.mode] {
            for (index, action) in set.actions.iter_mut().enumerate() {
                let id = ActionId {
                    set: set.set,
                    index: index as u8,
                };
                for hand in Hand::BOTH {
                    let i = hand.index();
                    let bound = action.bound[i];
                    if masks.iter().any(|m| m.hand == hand && m.covers(id, bound)) {
                        action.hands[i] = Intent::default();
                    }
                }
            }
        }
        if gestures.yank {
            for id in YANK_ACTIONS {
                if let Some(action) = input.get_mut(id) {
                    let button = &mut action.hands[grapple_hand.index()].button;
                    button.pressed = true;
                }
            }
        }
        Intents { input }
    }

    /// The mode set of the tick.
    pub fn mode_set(&self) -> SemanticSet {
        self.input.mode.set
    }

    /// `action`'s state on `hand`, at rest when its set is not one of the tick's.
    pub fn hand(&self, action: ActionId, hand: Hand) -> Intent {
        self.input
            .get(action)
            .map_or_else(Intent::default, |a| a.hands[hand.index()])
    }

    /// `action`'s press on either hand.
    pub fn button(&self, action: ActionId) -> ButtonControl {
        let [l, r] = Hand::BOTH.map(|h| self.hand(action, h).button);
        ButtonControl {
            down: l.down || r.down,
            pressed: l.pressed || r.pressed,
            released: (l.released || r.released) && !(l.down || r.down),
        }
    }

    /// `action`'s value past its deadzone: the larger in magnitude of the two hands'.
    pub fn value(&self, action: ActionId) -> f32 {
        self.larger(action, |i| i.value)
    }

    /// `action`'s value as reported: the larger in magnitude of the two hands'.
    pub fn raw(&self, action: ActionId) -> f32 {
        self.larger(action, |i| i.raw)
    }

    /// `action`'s vector past its deadzone: the longer of the two hands'.
    pub fn vector(&self, action: ActionId) -> Vec2 {
        self.longer(action, |i| i.vector)
    }

    fn larger(&self, action: ActionId, f: impl Fn(&Intent) -> f32) -> f32 {
        let [l, r] = Hand::BOTH.map(|h| f(&self.hand(action, h)));
        if r.abs() > l.abs() { r } else { l }
    }

    fn longer(&self, action: ActionId, f: impl Fn(&Intent) -> Vec2) -> Vec2 {
        let [l, r] = Hand::BOTH.map(|h| f(&self.hand(action, h)));
        if r.length_squared() > l.length_squared() {
            r
        } else {
            l
        }
    }
}

/// Read one set's frame state into its tick input.
fn read_set(
    state: &SetState,
    bound: &BoundSet,
    edges: &SemanticEdges,
    config: &BindingsConfig,
) -> SetInput {
    let actions = state
        .actions
        .iter()
        .enumerate()
        .map(|(index, hands)| {
            let id = ActionId {
                set: state.set,
                index: index as u8,
            };
            let component = default_binding(state.set, index).map(|b| b.component);
            ActionInput {
                hands: Hand::BOTH.map(|hand| {
                    let s = hands[hand.index()];
                    let button = ButtonControl {
                        down: s.down,
                        pressed: edges.pressed(id, hand),
                        released: edges.released(id, hand),
                    };
                    shape(s.value, button, component, config)
                }),
                bound: Hand::BOTH.map(|hand| bound.get(index, hand)),
            }
        })
        .collect();
    SetInput {
        set: state.set,
        actions,
    }
}

/// One hand's [`Intent`] from its value and press, with the deadzone of the input the action reads
/// by default: the trigger deadzone for a trigger pull, the move deadzone for a stick or a stick
/// axis, and none for the rest (a trackpad position, a grip).
pub(crate) fn shape(
    value: ActionValue,
    button: ButtonControl,
    component: Option<Component>,
    config: &BindingsConfig,
) -> Intent {
    let mut intent = Intent {
        button,
        ..Default::default()
    };
    match value {
        ActionValue::Bool(_) => {}
        ActionValue::Float(raw) => {
            intent.raw = raw;
            intent.value = match component {
                Some(Component::TriggerValue) => scalar_deadzone(raw, config.trigger_deadzone),
                Some(Component::ThumbstickX | Component::ThumbstickY) => {
                    signed_deadzone(raw, config.move_deadzone)
                }
                _ if raw.is_finite() => raw,
                _ => 0.0,
            };
        }
        ActionValue::Vec2(raw) => {
            intent.vector_raw = raw;
            intent.vector = match component {
                Some(Component::Thumbstick) => radial_deadzone(raw, config.move_deadzone),
                _ if raw.is_finite() => raw,
                _ => Vec2::ZERO,
            };
        }
    }
    intent
}

/// A signed axis with a deadzone about zero, rescaled so that it still reaches one either way.
fn signed_deadzone(value: f32, deadzone: f32) -> f32 {
    scalar_deadzone(value.abs(), deadzone).copysign(value)
}
