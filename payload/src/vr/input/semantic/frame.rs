//! The published per-frame semantic state ([`SemanticFrame`]): each action's per-hand state, for
//! `global` and for the mode set that was active when the frame was sampled.
//!
//! The VR frame loop publishes one beside every
//! [`ControllerFrame`](crate::controllers::frame::ControllerFrame), read in the same sync. Nothing
//! consumes it but the debug UI yet; the binding tables still read the raw inputs.

use std::time::Instant;

use glam::Vec2;
use parking_lot::Mutex;

use crate::{
    controllers::frame::{ActionSetKind, MAX_FRAME_AGE},
    vr::input::semantic::{ActionKind, SemanticSet},
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

/// A copy of the latest published frame, or `None` when there is none or it is older than
/// [`MAX_FRAME_AGE`].
pub(crate) fn latest() -> Option<SemanticFrame> {
    let now = Instant::now();
    SLOT.lock()
        .as_ref()
        .filter(|f| now.saturating_duration_since(f.published_at) <= MAX_FRAME_AGE)
        .cloned()
}

/// Publish a frame.
pub(crate) fn publish(frame: SemanticFrame) {
    *SLOT.lock() = Some(frame);
}

/// Drop the latest frame, alongside [`crate::controllers::frame::clear`].
pub(crate) fn clear() {
    *SLOT.lock() = None;
}

static SLOT: Mutex<Option<SemanticFrame>> = Mutex::new(None);
