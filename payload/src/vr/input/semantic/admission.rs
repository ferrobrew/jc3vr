//! Producer-side route availability retained in the ordered input history.

use tracking_format::input::{InputRoute, RouteSample};

use crate::controllers::frame::{Hand, HandFrame};

/// Retain tracking loss on each affected route, even when a later frame recovers before a tick.
/// Values remain unmasked evidence; an unavailable sample cannot establish a neutral boundary.
pub(crate) fn apply(hands: &[HandFrame; 2], samples: &mut [RouteSample]) {
    let usable = hands.each_ref().map(hand_usable);
    for sample in samples {
        let hand = match sample.route {
            InputRoute::Raw(0) => Some(Hand::Left),
            InputRoute::Raw(1 | 2) => Some(Hand::Right),
            InputRoute::Raw(_) => None,
            InputRoute::Semantic { hand, .. } => Hand::BOTH.get(hand as usize).copied(),
        };
        sample.active &= hand.is_some_and(|hand| usable[hand.index()]);
    }
}

/// The same hand admission applies to ordered route evidence and physical-control recovery.
pub(crate) fn hand_usable(hand: &HandFrame) -> bool {
    hand.active
        && hand.grip.is_valid()
        && hand.grip.pose.position.is_finite()
        && hand.grip.pose.orientation.is_finite()
}
