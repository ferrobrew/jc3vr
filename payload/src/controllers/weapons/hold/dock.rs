//! Docking a held weapon on the body and taking it off, as one object: the short snap between the
//! dock and the hand, and the grip release that holsters or drops the weapon. Pure.
//!
//! A draw or a holster without the game's act equips or unequips the weapon in one call, which
//! moves its guns between a dock bone and a hand attach bone at a stroke. [`Snap`] hides the
//! switch: for its duration the rigid grip places the gun's hand attach bone part of the way between
//! the dock's pose and the hand's grip ([`Snap::toward_hand`]), easing in and out, so the gun slides
//! out of the holster into the hand, or out of the hand into the holster, and the bone table
//! switches where the two poses meet. A draw switches at its start and slides toward the hand; a
//! holster slides toward the dock and switches at its end.
//!
//! A gun taken from the world (off the ground, out of a character's hands, or off a rack) slides
//! the same way from where it was in the world ([`SnapFrom::World`]), so it is seen to leave its
//! place rather than appear in the hand.
//!
//! With the grip held to hold a gun, [`ReleaseWatch`] reports the holding hand's grip opening
//! after it took the gun by a grab, and [`on_release`] decides what the release does.

use crate::controllers::frame::{Hand, Pose};

/// Which way a snap moves the gun.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SnapKind {
    /// From the dock into the hand.
    Draw,
    /// From the hand onto the dock.
    Holster,
}

impl SnapKind {
    pub const fn label(self) -> &'static str {
        match self {
            SnapKind::Draw => "draw",
            SnapKind::Holster => "holster",
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
}

impl ReleaseWatch {
    pub const fn new() -> Self {
        Self { armed: None }
    }

    /// `hand` took the gun by a grab: its grip holds it from now on.
    pub fn arm(&mut self, hand: Hand) {
        self.armed = Some(hand);
    }

    /// Stop watching.
    pub fn disarm(&mut self) {
        self.armed = None;
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

/// What opening the grip on a held gun does.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Release {
    /// Holster it: the hand is at the weapon's own holster.
    Holster,
    /// Let it fall.
    Drop,
    /// Keep it: the weapon cannot be let go of (the dual-wield pair, whose pistols stay together).
    Keep,
}

/// What a release does, with the hand in the weapon's own holster when `at_own_holster`, and a
/// weapon that can be let fall when `droppable`.
pub fn on_release(at_own_holster: bool, droppable: bool) -> Release {
    match (at_own_holster, droppable) {
        (true, _) => Release::Holster,
        (false, true) => Release::Drop,
        (false, false) => Release::Keep,
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
    fn a_release_holsters_at_the_holster_and_drops_elsewhere() {
        assert_eq!(on_release(true, true), Release::Holster);
        assert_eq!(on_release(true, false), Release::Holster);
        assert_eq!(on_release(false, true), Release::Drop);
        assert_eq!(on_release(false, false), Release::Keep);
    }
}
