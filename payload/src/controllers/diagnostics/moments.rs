//! The hands' moments as tracking-recording events: an item leaving a hand with a requested motion
//! ([`Throw`]), and a grip letting go of what it held ([`GripRelease`]). Each producer calls in at
//! the moment it acts; nothing is noted unless a recording or the lookback collects.

use glam::Vec3;
use tracking_format::event::{GripRelease, Label, Throw, WorldMotion};

use crate::{
    controllers::frame::Hand,
    recorder::tracking::{
        events::{self, EventKind},
        format::hand_to_file,
    },
};

/// An item that left a hand with a requested motion.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Thrown {
    pub hand: Hand,
    /// What left the hand.
    pub item: &'static str,
    /// The requested motion, per game second: metres and radians.
    pub linear: Vec3,
    pub angular: Vec3,
    /// The game's time scale at the release.
    pub time_scale: f32,
    /// Where the hand's part of the estimate came from, when the release estimated one.
    pub source: Option<&'static str>,
    /// Whether the item left live, when it has such a state.
    pub armed: Option<bool>,
}

/// Note a throw.
pub(crate) fn note_throw(thrown: Thrown) {
    if !events::collecting() {
        return;
    }
    events::note(EventKind::Throw(Box::new(Throw {
        hand: hand_to_file(thrown.hand),
        item: Label::Borrowed(thrown.item),
        motion: WorldMotion {
            linear: thrown.linear,
            angular: thrown.angular,
        },
        time_scale: thrown.time_scale,
        source: thrown.source.map(Label::Borrowed),
        armed: thrown.armed,
    })));
}

/// A grip that let go of what it held.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Released {
    pub hand: Hand,
    /// What the hand held.
    pub held: &'static str,
    /// The held object's address, a correlation identity only, and its body.
    pub object: Option<usize>,
    pub body: Option<u32>,
    /// Why it let go.
    pub cause: &'static str,
    /// Whether the release threw what it held.
    pub thrown: bool,
}

/// Note a grip release.
pub(crate) fn note_release(released: Released) {
    if !events::collecting() {
        return;
    }
    events::note(EventKind::GripRelease(Box::new(GripRelease {
        hand: hand_to_file(released.hand),
        held: Label::Borrowed(released.held),
        object: released.object.map(|object| object as u64),
        body: released.body,
        cause: Label::Borrowed(released.cause),
        thrown: released.thrown,
    })));
}
