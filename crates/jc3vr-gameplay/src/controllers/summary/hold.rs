//! Accepted summary holds published by the input owner, with the default touch cancellation.

use std::time::Instant;

use parking_lot::Mutex;

use crate::{
    controllers::{
        bindings::{
            intents::{Mask, SemanticInput},
            policy_a::runtime::Tick,
        },
        frame::{Button, ControllerFrame, Hand, MAX_FRAME_AGE},
    },
    vr::input::semantic::{bound::SourceTable, defaults::Component, vocabulary::GlobalAction},
};

static HOLD: Mutex<Option<(Instant, bool, bool)>> = Mutex::new(None);

pub(crate) fn publish(held: bool, cancel: bool) {
    *HOLD.lock() = Some((Instant::now(), held, cancel));
}

pub(crate) fn accepted(
    input: &SemanticInput,
    tick: &Tick,
    masks: &[Mask],
    frame: &ControllerFrame,
    sources: &SourceTable,
) -> (bool, bool) {
    let id = GlobalAction::ShowControlsSummary.id();
    let Some(action) = input.get(id) else {
        return (false, false);
    };
    let allowed = |hand: Hand| {
        tick.permits(id, hand)
            && !masks
                .iter()
                .any(|mask| mask.hand == hand && mask.covers(id, action.bound[hand.index()]))
    };
    let held = Hand::BOTH
        .into_iter()
        .any(|hand| allowed(hand) && action.hands[hand.index()].button.down);
    // OpenXR reports aggregate action values, not the contributing binding. Cancel only an
    // unambiguous touch hold; an alternative source or another held route must remain visible.
    let cancel = held
        && Hand::BOTH.into_iter().all(|hand| {
            if !allowed(hand) || !action.hands[hand.index()].button.down {
                return true;
            }
            let Some(bound) = sources.sources(id.set, id.index()) else {
                return false;
            };
            let mut matching = bound.iter().filter(|source| source.hand == Some(hand));
            let sole_touch = matching
                .next()
                .is_some_and(|source| source.component == Some(Component::TrackpadTouch))
                && matching.next().is_none();
            let raw = frame.hand(hand);
            sole_touch
                && raw.active
                && raw.inputs.trackpad_touch
                && (raw.buttons.contains(Button::TrackpadPressLeft)
                    || raw.buttons.contains(Button::TrackpadPressRight))
        });
    (held, cancel)
}

pub(crate) fn current() -> (bool, bool) {
    HOLD.lock()
        .filter(|(at, _, _)| at.elapsed() <= MAX_FRAME_AGE)
        .map_or((false, false), |(_, held, cancel)| (held, cancel))
}
