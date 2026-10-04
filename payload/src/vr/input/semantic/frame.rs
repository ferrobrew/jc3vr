//! The published per-frame semantic state ([`SemanticFrame`]): each action's per-hand state, for
//! `global` and for the mode set that was active when the frame was sampled.
//!
//! The VR frame loop publishes one beside every
//! [`ControllerFrame`](crate::controllers::frame::ControllerFrame), read in the same sync. The
//! input tick takes the latest with the press and release edges accumulated since its last take
//! ([`take`]), as it takes the raw frame's: render frames outpace the sim tick, and a tap between
//! two ticks must still reach the bindings.
//!
//! Each action also carries a digital state ([`ActionState::down`]): a boolean action's value, or
//! a float action's value past the press thresholds of the input it reads by default (a trigger's,
//! a grip's, or a trackpad's), with hysteresis against the previous frame. The edges are those of
//! the digital state.

use std::time::Instant;

use glam::Vec2;
use parking_lot::Mutex;

use crate::{
    controllers::frame::{ActionSetKind, GripPress, Hand, MAX_FRAME_AGE, PressRules},
    vr::input::semantic::{
        ActionKind, SemanticSet,
        defaults::{Component, default_binding},
        vocabulary::ActionId,
    },
};

/// One action's value on one hand.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) enum ActionValue {
    Bool(bool),
    Float(f32),
    Vec2(Vec2),
}

impl ActionValue {
    /// The rest value of an action of `kind`.
    pub(crate) const fn rest(kind: ActionKind) -> ActionValue {
        match kind {
            ActionKind::Bool => ActionValue::Bool(false),
            ActionKind::Float => ActionValue::Float(0.0),
            ActionKind::Vec2 => ActionValue::Vec2(Vec2::ZERO),
        }
    }

    /// Whether the value reads as engaged: a boolean held, a scalar past half either way (an axis
    /// bound to a scalar is signed), or a vector pushed past half its range. For diagnostics, not
    /// the bindings, which apply their own thresholds.
    pub(crate) fn engaged(self) -> bool {
        match self {
            ActionValue::Bool(b) => b,
            ActionValue::Float(f) => f.abs() > 0.5,
            ActionValue::Vec2(v) => v.length() > 0.5,
        }
    }
}

/// One action's state on one hand.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct ActionState {
    pub(crate) value: ActionValue,
    /// Whether the runtime bound the action to an active source on this hand this sync.
    pub(crate) active: bool,
    /// The digital state: a boolean action's value, or a float action past its press thresholds.
    /// Always false for a vector action, and for a float action whose default input has no press.
    pub(crate) down: bool,
}

impl ActionState {
    /// The rest state of an action of `kind`.
    pub(crate) const fn rest(kind: ActionKind) -> ActionState {
        ActionState {
            value: ActionValue::rest(kind),
            active: false,
            down: false,
        }
    }
}

/// Every action of one set, each per hand, indexed like [`SemanticSet::actions`] and then by
/// [`Hand::index`](crate::controllers::frame::Hand::index).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SetState {
    pub(crate) set: SemanticSet,
    pub(crate) actions: Vec<[ActionState; 2]>,
}

/// The semantic state of one frame.
#[derive(Debug, Clone)]
pub(crate) struct SemanticFrame {
    /// When the frame was published, on the host clock.
    pub(crate) published_at: Instant,
    /// The mode set that was active.
    pub(crate) action_set: ActionSetKind,
    pub(crate) global: SetState,
    /// The active mode set's actions.
    pub(crate) mode: SetState,
}

impl SemanticFrame {
    /// The state of `set`, when it was active for this frame.
    pub(crate) fn set(&self, set: SemanticSet) -> Option<&SetState> {
        [&self.global, &self.mode]
            .into_iter()
            .find(|state| state.set == set)
    }
}

/// The digital state of action `index` of `set` after observing `value` on a hand read by `rules`,
/// given whether it was down before. A float action is thresholded by the press thresholds of the
/// input it reads by default, wherever the player bound it, since the thresholds belong to the
/// action's meaning (a trigger pull, a grip squeeze).
pub(crate) fn digital(
    set: SemanticSet,
    index: usize,
    value: ActionValue,
    rules: &PressRules,
    was_down: bool,
) -> bool {
    match value {
        ActionValue::Bool(b) => b,
        ActionValue::Vec2(_) => false,
        ActionValue::Float(f) => {
            let thresholds = match default_binding(set, index).map(|b| b.component) {
                Some(Component::TriggerValue) => rules.trigger,
                Some(Component::GripPress) => match rules.grip {
                    GripPress::Value(t) | GripPress::Force(t) => t,
                },
                Some(Component::TrackpadForce) => rules.trackpad,
                _ => return false,
            };
            f.is_finite() && thresholds.apply(was_down, f)
        }
    }
}

/// The press and release edges of every action's digital state since the last [`take`], per set
/// and hand: bit `i` is the action of index `i`. An action pressed and released between two takes
/// appears in both.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(crate) struct SemanticEdges {
    /// By [`SemanticSet::index`], then [`Hand::index`].
    pressed: [[u32; 2]; SET_COUNT],
    released: [[u32; 2]; SET_COUNT],
}

impl SemanticEdges {
    /// Whether `action` was pressed on `hand` since the last take.
    pub(crate) fn pressed(&self, action: ActionId, hand: Hand) -> bool {
        self.pressed[action.set.index()][hand.index()] & bit(action.index()) != 0
    }

    /// Whether `action` was released on `hand` since the last take.
    pub(crate) fn released(&self, action: ActionId, hand: Hand) -> bool {
        self.released[action.set.index()][hand.index()] & bit(action.index()) != 0
    }

    /// Fold the transition of one set from `previous` to `current` into the edges.
    fn accumulate(&mut self, previous: &SetState, current: &SetState) {
        debug_assert_eq!(previous.set, current.set);
        let set = current.set.index();
        for (index, (before, after)) in previous.actions.iter().zip(&current.actions).enumerate() {
            for hand in 0..2 {
                match (before[hand].down, after[hand].down) {
                    (false, true) => self.pressed[set][hand] |= bit(index),
                    (true, false) => self.released[set][hand] |= bit(index),
                    _ => {}
                }
            }
        }
    }
}

/// The latest published frame, or `None` when there is none or it is older than
/// [`MAX_FRAME_AGE`], with the edges accumulated since the last take; the edges are reset. The
/// input tick is the one taker.
pub(crate) fn take() -> (Option<SemanticFrame>, SemanticEdges) {
    let now = Instant::now();
    let mut slot = SLOT.lock();
    let frame = slot
        .frame
        .as_ref()
        .filter(|f| now.saturating_duration_since(f.published_at) <= MAX_FRAME_AGE)
        .cloned();
    (frame, std::mem::take(&mut slot.edges))
}

/// A copy of the latest published frame, or `None` when there is none or it is older than
/// [`MAX_FRAME_AGE`]. Leaves the edges to [`take`].
pub(crate) fn latest() -> Option<SemanticFrame> {
    let now = Instant::now();
    SLOT.lock()
        .frame
        .as_ref()
        .filter(|f| now.saturating_duration_since(f.published_at) <= MAX_FRAME_AGE)
        .cloned()
}

/// The previous frame's digital state of `set`, by action index and then [`Hand::index`], for
/// the hysteresis of the next frame's, or `None` when the previous frame did not read `set`.
pub(crate) fn previous_down(set: SemanticSet) -> Option<Vec<[bool; 2]>> {
    let slot = SLOT.lock();
    let state = slot.frame.as_ref()?.set(set)?;
    Some(
        state
            .actions
            .iter()
            .map(|hands| hands.map(|h| h.down))
            .collect(),
    )
}

/// Publish a frame: fold its transitions from the previous frame into the edges and make it the
/// latest. A set the previous frame did not read (the mode set just changed) has no transition to
/// fold, so an input held across the change is not pressed anew.
pub(crate) fn publish(frame: SemanticFrame) {
    let mut slot = SLOT.lock();
    let Slot {
        frame: previous,
        edges,
    } = &mut *slot;
    if let Some(previous) = previous {
        for current in [&frame.global, &frame.mode] {
            if let Some(before) = previous.set(current.set) {
                edges.accumulate(before, current);
            }
        }
    }
    *previous = Some(frame);
}

/// Drop the latest frame and any untaken edges, alongside [`crate::controllers::frame::clear`].
pub(crate) fn clear() {
    *SLOT.lock() = Slot {
        frame: None,
        edges: SemanticEdges {
            pressed: [[0; 2]; SET_COUNT],
            released: [[0; 2]; SET_COUNT],
        },
    };
}

const SET_COUNT: usize = SemanticSet::ALL.len();

const fn bit(index: usize) -> u32 {
    1 << index
}

static SLOT: Mutex<Slot> = Mutex::new(Slot {
    frame: None,
    edges: SemanticEdges {
        pressed: [[0; 2]; SET_COUNT],
        released: [[0; 2]; SET_COUNT],
    },
});

struct Slot {
    frame: Option<SemanticFrame>,
    edges: SemanticEdges,
}
