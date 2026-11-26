//! The controls' vocabulary every subsystem speaks: the hold requests the subsystems hand the
//! bindings, the per-hand controls they read, what the bindings hand the second half of each
//! subsystem's input tick ([`finish`]), the yank gesture, the grapple button's context, and the
//! rows the controls summary describes. It also holds the bindings' settings, the game's localised
//! strings, the reads of each hand's grip outside the bindings' tick, and whether the bindings'
//! game stage may spawn ([`stage`]).

pub mod config;
pub mod controls;
pub mod finish;
pub mod grapple_button;
pub mod hold;
pub mod localise;
pub mod rows;
pub mod stage;
pub mod yank;

use std::sync::atomic::{AtomicBool, Ordering};

use jc3vr_input::frame::{self, Button, GripPress, Hand};
use jc3vr_view::{tuning::VIEW, vr::input::config::grip_press};

use crate::controllers::intent::controls::holds_unseen;

/// Whether `hand` was still closed on what its grip took as of the last input tick
/// ([`ClosedHand`](crate::controllers::intent::controls::ClosedHand)), for the holds checked
/// outside the tick.
#[inline]
pub fn hand_closed(hand: Hand) -> bool {
    HANDS_CLOSED[hand.index()].load(Ordering::Relaxed)
}

/// Record whether `hand` is still closed on what its grip took, as the bindings' tick found it.
#[inline]
pub fn record_hand_closed(hand: Hand, closed: bool) {
    HANDS_CLOSED[hand.index()].store(closed, Ordering::Relaxed);
}

/// Forget every hand's closed state, for the bindings standing down.
#[inline]
pub fn clear_hands_closed() {
    for closed in &HANDS_CLOSED {
        closed.store(false, Ordering::Relaxed);
    }
}

/// Whether `hand`'s grip holds what the hand has on the latest controller frame, read as the
/// controller is, unmasked by the wait for a returning hand's controls to rest
/// ([`holds_unseen`]), for a hold resumed with no tick's history of the hand. `None`
/// while the frame is unfocused or the hand is not tracked, when the grip is not known.
pub fn grip_holds_now(hand: Hand) -> Option<bool> {
    frame::with_latest(|f| {
        let source = f.hand(hand);
        (f.focused && source.active).then(|| {
            let force = matches!(grip_press(source.profile, hand), GripPress::Force(_));
            holds_unseen(
                source.buttons.contains(Button::SqueezePress),
                force.then_some(source.inputs.squeeze),
                VIEW.get().press.squeeze.release,
            )
        })
    })
    .flatten()
}

/// Whether `hand`'s grip is pressed on the latest controller frame: the derived press, with its
/// hysteresis, which is what opens and closes a hold. Unlike [`grip_holds_now`], fingers resting
/// round a force-sensing grip's handle after the press has let go do not count: the press is read
/// with its history, so a hold that has just opened reads open. `None` while the frame is
/// unfocused or the hand is not tracked.
pub fn grip_pressed_now(hand: Hand) -> Option<bool> {
    frame::with_latest(|f| {
        let source = f.hand(hand);
        (f.focused && source.active).then(|| source.buttons.contains(Button::SqueezePress))
    })
    .flatten()
}

/// [`hand_closed`]'s copy of each hand's closed state, by [`Hand::index`].
static HANDS_CLOSED: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
