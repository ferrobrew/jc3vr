//! What a grip press is doing, from its press to its release. Pure.
//!
//! A press takes the target the hand is in reach of. A press with nothing in reach closes the hand
//! into a fist when no game action reads that grip, and the fist keeps looking for a moment: it
//! takes the first target the hand comes to within the grab tuning's `reach.fist_window_s` of
//! closing, or later while the hand moves slowly, so that a hand closed in the air and then laid on
//! a handle takes it. A fist that stays closed and moves fast stays a fist: a punch does not take
//! what it strikes, which the hand bodies deal as a blow instead, and a fist swung past the hip
//! draws nothing. A press with nothing in reach on a grip a game action reads (a vehicle's weapon,
//! the wingsuit's secondary weapon) drives that action for the whole press. A fist never takes a
//! remote target: only a press aimed at it does. Past its window a fist takes nothing on the body
//! either: the hand is slow against the room while the body swings under it (a grapple swing, a
//! landing), and a fist closed on the grapple line drew a holstered gun twice on the 14 November
//! take, then threw it when the hand let go of the line. Whatever the press took, it holds until
//! the hand opens ([`ClosedHand`](crate::controllers::intent::controls::ClosedHand)), which on a
//! force-sensing grip can be after the press itself ends. A fist or a game action ends with the
//! press.

use crate::controllers::{
    grab::{GrabVolume, Tier, tuning::GRAB},
    intent::controls::ButtonControl,
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
    /// Whether the hand is still closed since the press
    /// ([`HandControls::hold`](crate::controllers::intent::controls::HandControls::hold)).
    pub closed: bool,
    /// What the hand would take this tick.
    pub target: Option<GrabVolume>,
    /// Whether a game action of the mode reads this hand's grip.
    pub bound: bool,
    /// Whether the hand moves slowly enough for a fist to take what it comes to.
    pub slow: bool,
    /// Whether the hand is the grapple's while its line is out or reeling: its fist holds the
    /// line, and a slow fist past its window takes nothing on the body.
    pub holds_line: bool,
    /// The tick's length, seconds.
    pub dt: f32,
}

impl GripLatch {
    /// Advance one tick. Returns the target the grip took this tick.
    pub fn step(&mut self, tick: LatchTick) -> Option<GrabVolume> {
        let LatchTick {
            grip,
            closed,
            target,
            bound,
            slow,
            holds_line,
            dt,
        } = tick;
        if !grip.held() {
            if !(closed && self.grabbing()) {
                *self = GripLatch::Open;
            }
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
            let window = closed_s <= GRAB.get().reach.fist_window_s;
            let takes = |v: GrabVolume| {
                (window || (slow && !(holds_line && v.on_body()))) && Tier::of(v) != Tier::Remote
            };
            // A remote target is armed only by a press aimed at it, never by a closed hand passing
            // across it.
            *self = match target.filter(|v| takes(*v)) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_holding_slow_fist_past_its_window_takes_nothing_on_the_body() {
        let press = ButtonControl {
            down: true,
            pressed: true,
            released: false,
        };
        let held = ButtonControl {
            down: true,
            pressed: false,
            released: false,
        };
        let tick = |grip, target, dt| LatchTick {
            grip,
            closed: true,
            target,
            bound: false,
            slow: true,
            holds_line: true,
            dt,
        };
        let mut latch = GripLatch::Open;
        assert_eq!(latch.step(tick(press, None, 0.02)), None);
        assert_eq!(latch.step(tick(held, None, 0.5)), None);
        assert_eq!(
            latch.step(tick(held, Some(GrabVolume::GrenadePouch), 0.02)),
            None
        );
        assert_eq!(
            latch.step(tick(held, Some(GrabVolume::Climb), 0.02)),
            Some(GrabVolume::Climb)
        );
        // The other hand's slow fist still takes the pouch.
        let free = |grip, target, dt| LatchTick {
            holds_line: false,
            ..tick(grip, target, dt)
        };
        let mut latch = GripLatch::Open;
        latch.step(free(press, None, 0.02));
        latch.step(free(held, None, 0.5));
        assert_eq!(
            latch.step(free(held, Some(GrabVolume::GrenadePouch), 0.02)),
            Some(GrabVolume::GrenadePouch)
        );
    }
}
