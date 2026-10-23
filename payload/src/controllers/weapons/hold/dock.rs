//! Docking a held weapon on the body and taking it off, as one object: the short snap between the
//! socket and the hand, and the watch on the grip that lets go of it. Pure.
//!
//! A draw or a dock moves the weapon's guns between a socket's bone and a hand attach bone at a
//! stroke. [`Snap`] hides the switch: for its duration the rigid grip places the gun's hand attach
//! bone part of the way between the socket's pose and the hand's grip ([`Snap::toward_hand`]),
//! easing in and out, so the gun slides off the socket into the hand, or out of the hand onto the
//! socket, and the bone table switches where the two poses meet. A draw switches at its start and
//! slides toward the hand; a dock slides toward the socket and switches at its end.
//!
//! A gun taken from the world (off the ground, out of a character's hands, or off a rack) slides
//! the same way from where it was in the world ([`SnapFrom::World`]), so it is seen to leave its
//! place rather than appear in the hand.
//!
//! With the grip held to hold a gun, [`ReleaseWatch`] reports the holding hand's grip opening
//! after it took the gun by a grab; what the release does is the table's decision
//! ([`crate::controllers::loadout::grab::release`]).

use crate::controllers::frame::{Hand, Pose};

/// Which way a snap moves the gun.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SnapKind {
    /// From the socket into the hand.
    Draw,
    /// From the hand onto the socket.
    Holster,
}

impl SnapKind {
    pub const fn label(self) -> &'static str {
        match self {
            SnapKind::Draw => "draw",
            SnapKind::Holster => "dock",
        }
    }
}

/// Where the far end of a snap is: the place the gun slides from (a draw) or to (a holster).
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum SnapFrom {
    /// A dock bone on the body.
    Dock {
        /// The dock bone (an `NBone::ESafeBoneIndex`) the gun sits on while docked.
        bone: u32,
        /// The gun's pose relative to the dock bone while docked, in the skeleton's model space.
        offset: Pose,
    },
    /// A fixed pose in the world, where the gun was before a hand took it.
    World(Pose),
}

/// A gun moving between a dock and a hand.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Snap {
    pub kind: SnapKind,
    /// The weapon's address.
    pub weapon: usize,
    pub hand: Hand,
    pub from: SnapFrom,
    elapsed_s: f32,
    duration_s: f32,
}

impl Snap {
    pub fn new(kind: SnapKind, weapon: usize, hand: Hand, from: SnapFrom, duration_s: f32) -> Self {
        Self {
            kind,
            weapon,
            hand,
            from,
            elapsed_s: 0.0,
            duration_s: if duration_s.is_finite() {
                duration_s.max(0.0)
            } else {
                0.0
            },
        }
    }

    /// Advance the snap by `dt` seconds.
    pub fn advance(&mut self, dt: f32) {
        if dt.is_finite() && dt > 0.0 {
            let elapsed = self.elapsed_s + dt;
            // A run of ticks summing to the duration may fall short of it by rounding.
            self.elapsed_s = if elapsed >= self.duration_s - ROUNDING_S {
                self.duration_s
            } else {
                elapsed
            };
        }
    }

    /// Whether the gun has arrived.
    pub fn done(&self) -> bool {
        self.elapsed_s >= self.duration_s
    }

    /// How far the gun is from the dock toward the hand, `0..=1`, eased in and out.
    pub fn toward_hand(&self) -> f32 {
        let progress = if self.duration_s > 0.0 {
            (self.elapsed_s / self.duration_s).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let eased = progress * progress * (3.0 - 2.0 * progress);
        match self.kind {
            SnapKind::Draw => eased,
            SnapKind::Holster => 1.0 - eased,
        }
    }
}

/// How close to its duration a snap counts as arrived, seconds.
const ROUNDING_S: f32 = 1e-5;

/// Watches the holding hand's grip while the grip holds the gun.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct ReleaseWatch {
    /// The hand whose grip holds the gun.
    armed: Option<Hand>,
    /// Each hand's grip on the last tick, by [`Hand::index`], for [`Self::latch_press`].
    before: [bool; 2],
}

impl ReleaseWatch {
    pub const fn new() -> Self {
        Self {
            armed: None,
            before: [false; 2],
        }
    }

    /// `hand` took the gun by a grab: its grip holds it from now on.
    pub fn arm(&mut self, hand: Hand) {
        self.armed = Some(hand);
    }

    /// Stop watching a gun that left the hand for another, keeping the grip's last state, so that
    /// a grip already held when the new gun arrived is not taken for a press.
    pub fn unarm(&mut self) {
        self.armed = None;
    }

    /// Stop watching. The gun stays in the hand, and is the grip's again once the grip closes on
    /// it ([`Self::latch_press`]): after a stand-down (a tracking loss, a lost focus, the debug
    /// panel) a gun held through it is kept rather than dropped by the masked grip.
    pub fn disarm(&mut self) {
        self.armed = None;
        self.before = [false; 2];
    }

    /// Fold in one input tick before [`Self::observe`]: a gun in `holder`'s hand that no grab put
    /// there (a draw the game made, the weapon wheel's, or one kept across a stand-down) becomes the
    /// grip's on the tick the grip closes on it. Only a press counts, not a grip already held when
    /// the gun arrived, so a hand that held a vehicle's control while the game drew the gun does
    /// not drop it on letting go. Returns the hand when it latched.
    pub fn latch_press(&mut self, holder: Option<Hand>, grips: [bool; 2]) -> Option<Hand> {
        let before = std::mem::replace(&mut self.before, grips);
        let hand = holder.filter(|h| self.armed.is_none() && grips[h.index()])?;
        if before[hand.index()] {
            return None;
        }
        self.armed = Some(hand);
        Some(hand)
    }

    /// Fold in one input tick: `holder` is the hand holding the gun, and `grips` whether each
    /// hand's grip is held, by [`Hand::index`]. Returns the holding hand when its grip opened. A
    /// gun that left the armed hand disarms the watch.
    pub fn observe(&mut self, holder: Option<Hand>, grips: [bool; 2]) -> Option<Hand> {
        let hand = self.armed?;
        if holder != Some(hand) {
            self.armed = None;
            return None;
        }
        if grips[hand.index()] {
            return None;
        }
        self.armed = None;
        Some(hand)
    }

    pub fn armed(&self) -> Option<Hand> {
        self.armed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(kind: SnapKind, duration_s: f32) -> Snap {
        Snap::new(
            kind,
            0x1000,
            Hand::Left,
            SnapFrom::World(Pose::IDENTITY),
            duration_s,
        )
    }

    #[test]
    fn a_draw_slides_from_the_dock_to_the_hand() {
        let mut s = snap(SnapKind::Draw, 0.12);
        assert_eq!(s.toward_hand(), 0.0);
        let mut last = 0.0;
        for _ in 0..12 {
            s.advance(0.01);
            let t = s.toward_hand();
            assert!(t >= last, "{t} after {last}");
            last = t;
        }
        assert!(s.done());
        assert!((s.toward_hand() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_holster_slides_from_the_hand_to_the_dock() {
        let mut s = snap(SnapKind::Holster, 0.12);
        assert_eq!(s.toward_hand(), 1.0);
        s.advance(0.06);
        assert!((s.toward_hand() - 0.5).abs() < 1e-6, "halfway at half time");
        s.advance(1.0);
        assert!(s.done());
        assert_eq!(s.toward_hand(), 0.0);
    }

    #[test]
    fn a_snap_ignores_bad_time() {
        let s = snap(SnapKind::Draw, f32::NAN);
        assert!(s.done(), "a snap without a duration has arrived");
        assert_eq!(s.toward_hand(), 1.0);
        let mut s = snap(SnapKind::Draw, 0.1);
        s.advance(f32::NAN);
        s.advance(-1.0);
        assert_eq!(s.toward_hand(), 0.0);
    }

    #[test]
    fn the_holding_hand_s_release_is_reported_once() {
        let mut w = ReleaseWatch::new();
        assert_eq!(w.observe(Some(Hand::Left), [false; 2]), None, "unarmed");
        w.arm(Hand::Left);
        assert_eq!(w.observe(Some(Hand::Left), [true, false]), None);
        // The other hand's grip does not count.
        assert_eq!(w.observe(Some(Hand::Left), [true, true]), None);
        assert_eq!(w.observe(Some(Hand::Left), [false, true]), Some(Hand::Left));
        assert_eq!(w.observe(Some(Hand::Left), [false, true]), None);
    }

    #[test]
    fn a_gun_no_grab_put_in_the_hand_is_the_grips_once_it_closes() {
        let mut w = ReleaseWatch::new();
        // The wheel put the gun in the right hand with its grip open.
        assert_eq!(w.latch_press(Some(Hand::Right), [false; 2]), None);
        assert_eq!(
            w.observe(Some(Hand::Right), [false; 2]),
            None,
            "not yet held"
        );
        // The grip closes on it: from now on it holds the gun.
        assert_eq!(
            w.latch_press(Some(Hand::Right), [false, true]),
            Some(Hand::Right)
        );
        assert_eq!(w.observe(Some(Hand::Right), [false, true]), None);
        assert_eq!(w.latch_press(Some(Hand::Right), [false; 2]), None);
        assert_eq!(w.observe(Some(Hand::Right), [false; 2]), Some(Hand::Right));
    }

    #[test]
    fn a_grip_already_held_when_the_gun_arrived_does_not_latch() {
        let mut w = ReleaseWatch::new();
        // The hand held a vehicle's control as the game drew the gun into it.
        assert_eq!(w.latch_press(None, [true, false]), None);
        assert_eq!(w.latch_press(Some(Hand::Left), [true, false]), None);
        assert_eq!(w.observe(Some(Hand::Left), [false; 2]), None, "kept");
        // A fresh press latches.
        assert_eq!(w.latch_press(Some(Hand::Left), [false; 2]), None);
        assert_eq!(
            w.latch_press(Some(Hand::Left), [true, false]),
            Some(Hand::Left)
        );
    }

    #[test]
    fn a_stand_down_keeps_the_gun_and_relatches_on_the_next_press() {
        let mut w = ReleaseWatch::new();
        w.arm(Hand::Right);
        w.latch_press(Some(Hand::Right), [false, true]);
        assert_eq!(w.observe(Some(Hand::Right), [false, true]), None);
        // Tracking is lost: the watch stands down, and the masked grip reads open.
        w.disarm();
        assert_eq!(w.latch_press(Some(Hand::Right), [false; 2]), None);
        assert_eq!(
            w.observe(Some(Hand::Right), [false; 2]),
            None,
            "not dropped"
        );
        // The hand comes back and closes on the gun.
        assert_eq!(
            w.latch_press(Some(Hand::Right), [false, true]),
            Some(Hand::Right)
        );
        assert_eq!(w.armed(), Some(Hand::Right));
    }

    #[test]
    fn a_gun_that_changed_hands_disarms_the_watch() {
        let mut w = ReleaseWatch::new();
        w.arm(Hand::Right);
        assert_eq!(w.observe(Some(Hand::Left), [false; 2]), None);
        assert_eq!(w.armed(), None);
        w.arm(Hand::Right);
        assert_eq!(w.observe(None, [false; 2]), None);
        assert_eq!(w.armed(), None);
    }

    #[test]
    fn a_new_gun_under_a_held_grip_waits_for_a_fresh_press() {
        let mut w = ReleaseWatch::new();
        let held = [true, false];
        assert_eq!(w.latch_press(Some(Hand::Left), held), Some(Hand::Left));
        // Another gun comes into the hand while the grip stays closed.
        w.unarm();
        assert_eq!(w.latch_press(Some(Hand::Left), held), None);
        assert_eq!(w.observe(Some(Hand::Left), held), None);
        assert_eq!(
            w.observe(Some(Hand::Left), [false; 2]),
            None,
            "opening the grip does not let the new gun go"
        );
        w.latch_press(Some(Hand::Left), [false; 2]);
        assert_eq!(w.latch_press(Some(Hand::Left), held), Some(Hand::Left));
    }
}
