//! What a grip press is doing, from its press to its release. Pure.
//!
//! A press takes the target the hand is in reach of. A press with nothing in reach closes the hand
//! into a fist when no game action reads that grip, and the fist keeps looking for a moment: it
//! takes the first target the hand comes to within [`FIST_WINDOW_S`] of closing, or later while
//! the hand moves slowly, so that a hand closed in the air and then laid on a handle takes it. A
//! fist that stays closed and moves fast stays a fist: a punch does not take what it strikes,
//! which the hand bodies deal as a blow instead, and a fist swung past the hip draws nothing. A
//! press with nothing in reach on a grip a game action reads (a vehicle's weapon, the wingsuit's
//! secondary weapon) drives that action for the whole press. A fist never takes a remote target:
//! only a press aimed at it does. Whatever the press took, it holds
//! until the grip opens.

use crate::controllers::{
    bindings::controls::ButtonControl,
    grab::{GrabVolume, Tier},
};

/// What a grip press is doing.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub enum GripLatch {
    /// The grip is open.
    #[default]
    Open,
    /// The grip is closed on nothing, with no game action on it, and has been for `closed_s`
    /// seconds: it takes the next target the hand comes to, within the window or while the hand
    /// moves slowly.
    Fist { closed_s: f32 },
    /// The grip took this; the tables inject nothing for the press.
    Grab(GrabVolume),
    /// The grip drives the game action bound to it for the press.
    Default,
}

/// One tick of a hand's grip, for [`GripLatch::step`].
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct LatchTick {
    pub grip: ButtonControl,
    /// What the hand would take this tick.
    pub target: Option<GrabVolume>,
    /// Whether a game action of the mode reads this hand's grip.
    pub bound: bool,
    /// Whether the hand moves slowly enough for a fist to take what it comes to.
    pub slow: bool,
    /// The tick's length, seconds.
    pub dt: f32,
}

/// How long after closing a fist still takes what the hand comes to however fast it moves,
/// seconds: a hand that closes a moment before it reaches a handle still takes the handle.
pub const FIST_WINDOW_S: f32 = 0.2;

impl GripLatch {
    /// Advance one tick. Returns the target the grip took this tick.
    pub fn step(&mut self, tick: LatchTick) -> Option<GrabVolume> {
        let LatchTick {
            grip,
            target,
            bound,
            slow,
            dt,
        } = tick;
        if !grip.held() {
            *self = GripLatch::Open;
            return None;
        }
        // A tap between two ticks is a fresh press even while the last one is latched.
        let fresh = *self == GripLatch::Open || (grip.pressed && grip.released && !grip.down);
        if fresh {
            *self = match (target, bound) {
                (Some(volume), _) => GripLatch::Grab(volume),
                (None, true) => GripLatch::Default,
                (None, false) => GripLatch::Fist { closed_s: 0.0 },
            };
            return match *self {
                GripLatch::Grab(volume) => Some(volume),
                _ => None,
            };
        }
        if let GripLatch::Fist { closed_s } = *self {
            let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
            let closed_s = closed_s + dt;
            let takes = closed_s <= FIST_WINDOW_S || slow;
            // A remote target is armed only by a press aimed at it, never by a closed hand passing
            // across it.
            *self = match target.filter(|v| takes && Tier::of(*v) != Tier::Remote) {
                Some(volume) => GripLatch::Grab(volume),
                None => GripLatch::Fist { closed_s },
            };
            if let GripLatch::Grab(volume) = *self {
                return Some(volume);
            }
        }
        None
    }

    /// Whether the press belongs to a grab, so no game action reads the grip.
    pub fn grabbing(self) -> bool {
        matches!(self, GripLatch::Grab(_))
    }

    pub const fn label(self) -> &'static str {
        match self {
            GripLatch::Open => "open",
            GripLatch::Fist { .. } => "fist",
            GripLatch::Grab(_) => "holding",
            GripLatch::Default => "game action",
        }
    }
}
