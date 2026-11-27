//! The published per-frame semantic state ([`SemanticFrame`]): each action's per-hand state, for
//! `global` and for the mode set that was active when the frame was sampled.
//!
//! The VR frame loop publishes one with every [`ControllerFrame`], read in the same sync, through
//! [`publish`], which also publishes the controller frame. This module holds the input's one edge
//! latch: the press and release edges of the raw buttons and of the semantic actions, accumulated
//! across publishes under one lock until the input tick takes both together ([`take`]). Render
//! frames outpace the sim tick, and a tap between two ticks must still reach the bindings. One
//! [`clear`] drops both frames and both sets of edges, so the raw and semantic streams can never
//! disagree about what was pressed since the last take.
//!
//! Each action also carries a digital state ([`ActionState::down`]): a boolean action's value, or
//! a float action's value past its action-owned trigger, grip, or trackpad thresholds, with
//! hysteresis against the previous frame. Suggested bindings do not define press behavior. The
//! edges are those of the digital state.

use std::time::Instant;

use glam::Vec2;
use jc3vr_input::{
    frame::{
        self as controller_frame, ActionSetKind, ButtonSet, ControllerFrame, Edges, GripPress,
        Hand, MAX_FRAME_AGE, PressRules,
    },
    semantic::{
        ActionKind, SemanticSet,
        vocabulary::{ActionId, FloatPress},
    },
};
use parking_lot::Mutex;

/// One action's value on one hand.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ActionValue {
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
pub struct ActionState {
    pub value: ActionValue,
    /// Whether the runtime bound the action to an active source on this hand this sync.
    pub active: bool,
    /// The digital state: a boolean action's value, or a float action past its press thresholds.
    /// Always false for a vector action, and for a float action with no press semantics.
    pub down: bool,
}

impl ActionState {
    /// The rest state of an action of `kind`.
    pub const fn rest(kind: ActionKind) -> ActionState {
        ActionState {
            value: ActionValue::rest(kind),
            active: false,
            down: false,
        }
    }
}

/// Every action of one set, each per hand, indexed like [`SemanticSet::actions`] and then by
/// [`Hand::index`](jc3vr_input::frame::Hand::index).
#[derive(Debug, Clone, PartialEq)]
pub struct SetState {
    pub set: SemanticSet,
    pub actions: Vec<[ActionState; 2]>,
}

/// The semantic state of one frame.
#[derive(Debug, Clone)]
pub struct SemanticFrame {
    /// When the frame was published, on the host clock.
    pub published_at: Instant,
    /// The mode set that was active.
    pub action_set: ActionSetKind,
    pub global: SetState,
    /// The active mode set's actions.
    pub mode: SetState,
}

impl SemanticFrame {
    /// The state of `set`, when it was active for this frame.
    pub fn set(&self, set: SemanticSet) -> Option<&SetState> {
        [&self.global, &self.mode]
            .into_iter()
            .find(|state| state.set == set)
    }
}

/// The digital state of action `index` of `set` after observing `value` on a hand read by `rules`,
/// given whether it was down before. A float action uses its action-owned press classification,
/// wherever the player binds it.
pub fn digital(
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
            let thresholds = match (ActionId {
                set,
                index: index as u8,
            })
            .float_press()
            {
                Some(FloatPress::Trigger) => rules.trigger,
                Some(FloatPress::Grip) => match rules.grip {
                    GripPress::Value(t) | GripPress::Force(t) => t,
                },
                Some(FloatPress::Trackpad) => rules.trackpad,
                None => return false,
            };
            f.is_finite() && thresholds.apply(was_down, f)
        }
    }
}

/// The press and release edges of every action's digital state since the last [`take`], per set
/// and hand: bit `i` is the action of index `i`. An action pressed and released between two takes
/// appears in both.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct SemanticEdges {
    /// By [`SemanticSet::index`], then [`Hand::index`].
    pressed: [[u32; 2]; SET_COUNT],
    released: [[u32; 2]; SET_COUNT],
}

impl SemanticEdges {
    /// Whether `action` was pressed on `hand` since the last take.
    pub fn pressed(&self, action: ActionId, hand: Hand) -> bool {
        self.pressed[action.set.index()][hand.index()] & bit(action.index()) != 0
    }

    /// Whether `action` was released on `hand` since the last take.
    pub fn released(&self, action: ActionId, hand: Hand) -> bool {
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

/// What the input tick takes: the latest semantic frame, or `None` when there is none or it is
/// older than [`MAX_FRAME_AGE`], and every raw and semantic edge since the last take.
#[derive(Debug, Clone)]
pub struct Taken {
    pub semantic: Option<SemanticFrame>,
    pub edges: Edges,
    pub semantic_edges: SemanticEdges,
}

/// Take the latest semantic frame and both sets of edges, resetting the latch. The input tick is
/// the one taker: a second would steal edges from it.
pub fn take() -> Taken {
    let now = Instant::now();
    let mut slot = SLOT.lock();
    let semantic = slot
        .frame
        .as_ref()
        .filter(|f| now.saturating_duration_since(f.published_at) <= MAX_FRAME_AGE)
        .cloned();
    Taken {
        semantic,
        edges: std::mem::take(&mut slot.edges),
        semantic_edges: std::mem::take(&mut slot.semantic_edges),
    }
}

/// A copy of the latest published frame, or `None` when there is none or it is older than
/// [`MAX_FRAME_AGE`]. Leaves the edges to [`take`].
pub fn latest() -> Option<SemanticFrame> {
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

/// Publish a sync's controller frame and semantic frame together: publish `raw` (which stamps its
/// sequence and time, returned here), stamp `semantic` with the same time, fold both frames'
/// transitions from the previous ones into the latch, and make `semantic` the latest. There are no
/// raw transitions without a previous raw frame (the first publish, or the first after a
/// [`clear`]), and none for a set the previous semantic frame did not read (the mode set just
/// changed), so an input held across either is not pressed anew. A change of mode set does not
/// interrupt the raw edges: the raw inputs are read from the always-active `hands` set.
pub(crate) fn publish(raw: ControllerFrame, semantic: SemanticFrame) -> (u64, Instant) {
    let mut slot = SLOT.lock();
    let buttons = [raw.hands[0].buttons, raw.hands[1].buttons];
    let (sequence, published_at) = controller_frame::publish(raw);
    slot.fold(buttons, published_at, semantic);
    (sequence, published_at)
}

/// Drop the latest controller and semantic frames and any untaken edges: the controllers are
/// disabled, the session is gone, or the frame loop stopped sampling. Buttons held at the time
/// produce no release edge; consumers treat a missing frame as everything released.
pub(crate) fn clear() {
    let mut slot = SLOT.lock();
    controller_frame::clear();
    *slot = Slot::EMPTY;
}

const SET_COUNT: usize = SemanticSet::ALL.len();

const fn bit(index: usize) -> u32 {
    1 << index
}

/// The latest semantic frame and the edge latch. The controller frame is published under this lock
/// too, so a publish and a take never interleave.
static SLOT: Mutex<Slot> = Mutex::new(Slot::EMPTY);

struct Slot {
    frame: Option<SemanticFrame>,
    /// The previous controller frame's buttons, by [`Hand::index`], or `None` when there is no
    /// previous frame to compare against.
    buttons: Option<[ButtonSet; 2]>,
    edges: Edges,
    semantic_edges: SemanticEdges,
}

impl Slot {
    /// Fold a publish into the latch: the controller frame's `buttons`, and `semantic`, stamped
    /// `published_at` (see [`publish`]).
    fn fold(
        &mut self,
        buttons: [ButtonSet; 2],
        published_at: Instant,
        mut semantic: SemanticFrame,
    ) {
        if let Some(previous) = self.buttons {
            self.edges.accumulate(previous, buttons);
        }
        self.buttons = Some(buttons);
        semantic.published_at = published_at;
        if let Some(previous) = &self.frame {
            for current in [&semantic.global, &semantic.mode] {
                if let Some(before) = previous.set(current.set) {
                    self.semantic_edges.accumulate(before, current);
                }
            }
        }
        self.frame = Some(semantic);
    }

    const EMPTY: Slot = Slot {
        frame: None,
        buttons: None,
        edges: Edges {
            pressed: [ButtonSet::EMPTY; 2],
            released: [ButtonSet::EMPTY; 2],
        },
        semantic_edges: SemanticEdges {
            pressed: [[0; 2]; SET_COUNT],
            released: [[0; 2]; SET_COUNT],
        },
    };
}

#[cfg(test)]
mod tests {
    use jc3vr_input::{frame::Button, semantic::vocabulary::ActionId};

    use super::*;

    fn buttons(right: &[Button]) -> [ButtonSet; 2] {
        let mut set = ButtonSet::EMPTY;
        for &button in right {
            set.set(button, true);
        }
        [ButtonSet::EMPTY, set]
    }

    /// A semantic frame whose mode set is `mode`, with its first action down on the right hand
    /// when `down`.
    fn semantic(mode: SemanticSet, down: bool) -> SemanticFrame {
        let state = |set: SemanticSet, down: bool| SetState {
            set,
            actions: set
                .actions()
                .iter()
                .enumerate()
                .map(|(index, action)| {
                    let mut right = ActionState::rest(action.kind);
                    right.down = down && index == 0;
                    [ActionState::rest(action.kind), right]
                })
                .collect(),
        };
        SemanticFrame {
            published_at: Instant::now(),
            action_set: ActionSetKind::OnFoot,
            global: state(SemanticSet::Global, false),
            mode: state(mode, down),
        }
    }

    fn first(set: SemanticSet) -> ActionId {
        ActionId { set, index: 0 }
    }

    fn take(slot: &mut Slot) -> (Edges, SemanticEdges) {
        (
            std::mem::take(&mut slot.edges),
            std::mem::take(&mut slot.semantic_edges),
        )
    }

    #[test]
    fn a_tap_between_takes_reaches_both_streams() {
        let mut slot = Slot::EMPTY;
        let now = Instant::now();
        let on_foot = SemanticSet::OnFoot;
        slot.fold(buttons(&[]), now, semantic(on_foot, false));
        slot.fold(buttons(&[Button::AClick]), now, semantic(on_foot, true));
        slot.fold(buttons(&[]), now, semantic(on_foot, false));
        let (edges, semantic_edges) = take(&mut slot);
        assert!(edges.pressed(Hand::Right, Button::AClick));
        assert!(edges.released(Hand::Right, Button::AClick));
        assert!(semantic_edges.pressed(first(on_foot), Hand::Right));
        assert!(semantic_edges.released(first(on_foot), Hand::Right));
        assert_eq!(
            take(&mut slot),
            (Edges::default(), SemanticEdges::default())
        );
    }

    #[test]
    fn a_mode_set_change_keeps_the_raw_edges_and_presses_no_action_anew() {
        let mut slot = Slot::EMPTY;
        let now = Instant::now();
        let held = buttons(&[Button::TriggerPress]);
        slot.fold(held, now, semantic(SemanticSet::OnFoot, true));
        slot.fold(held, now, semantic(SemanticSet::LandVehicle, true));
        assert_eq!(
            take(&mut slot),
            (Edges::default(), SemanticEdges::default())
        );
        slot.fold(buttons(&[]), now, semantic(SemanticSet::LandVehicle, false));
        let (edges, semantic_edges) = take(&mut slot);
        assert!(edges.released(Hand::Right, Button::TriggerPress));
        assert!(semantic_edges.released(first(SemanticSet::LandVehicle), Hand::Right));
    }

    #[test]
    fn an_input_held_across_a_clear_is_not_pressed_again() {
        let mut slot = Slot::EMPTY;
        let now = Instant::now();
        let held = buttons(&[Button::TriggerPress]);
        slot.fold(buttons(&[]), now, semantic(SemanticSet::OnFoot, false));
        slot.fold(held, now, semantic(SemanticSet::OnFoot, true));
        slot = Slot::EMPTY;
        slot.fold(held, now, semantic(SemanticSet::OnFoot, true));
        assert_eq!(
            take(&mut slot),
            (Edges::default(), SemanticEdges::default())
        );
        slot.fold(buttons(&[]), now, semantic(SemanticSet::OnFoot, false));
        assert!(
            take(&mut slot)
                .0
                .released(Hand::Right, Button::TriggerPress)
        );
    }

    #[test]
    fn a_publish_stamps_the_semantic_frame_with_the_controller_frames_time() {
        let mut slot = Slot::EMPTY;
        let at = Instant::now() + std::time::Duration::from_millis(3);
        slot.fold(buttons(&[]), at, semantic(SemanticSet::OnFoot, false));
        assert_eq!(slot.frame.map(|f| f.published_at), Some(at));
    }
}
