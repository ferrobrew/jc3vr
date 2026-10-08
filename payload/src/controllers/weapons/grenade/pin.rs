//! The grenade's pin. Pure.
//!
//! A grenade taken from the pouch comes out with its pin in: the grenade's fuse
//! ([`Grenade::m_ShouldTickDetonationTimer`]) is held, and full, from the update that primes it.
//! The other hand pulls the pin with a grip press at the grenade, or the holding hand's trigger
//! does, and from the next update the fuse runs while the grenade is still in the hand: the
//! grenade cooks. A grenade thrown with its pin in has its fuse started at the throw, so it is
//! never a dud that lies live in the world forever.
//!
//! The input tick asks for the pull ([`pull`]); the grenade weapon's update, the one place the
//! grenade is known to be live, writes the fuse ([`before_update`], [`on_primed`]).
//!
//! [`Grenade::m_ShouldTickDetonationTimer`]: jc3gi::equipment::explosive::Grenade::m_ShouldTickDetonationTimer

/// Where a held grenade's pin is.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Pin {
    /// In: the fuse is held.
    In,
    /// Pulled on the input tick; the next weapon update starts the fuse.
    Pulling,
    /// Out: the fuse runs.
    Out,
}

impl Pin {
    pub const fn label(self) -> &'static str {
        match self {
            Pin::In => "in",
            Pin::Pulling => "being pulled",
            Pin::Out => "out",
        }
    }
}

/// What to do to the held grenade's fuse.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FuseWrite {
    /// Stop the fuse and fill it again.
    Hold,
    /// Let the fuse run.
    Run,
}

/// Who pulled the pin.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Puller {
    /// The other hand, with a grip press at the grenade.
    OtherHand,
    /// The holding hand's trigger.
    Trigger,
}

impl Puller {
    pub const fn label(self) -> &'static str {
        match self {
            Puller::OtherHand => "the other hand",
            Puller::Trigger => "the trigger",
        }
    }
}

/// The pin of a grenade just primed: in, with its fuse held, when the pin is on; `None` when the
/// fuse runs from the prime, as the game's own throw has it.
pub fn on_primed(enabled: bool) -> (Option<Pin>, Option<FuseWrite>) {
    if enabled {
        (Some(Pin::In), Some(FuseWrite::Hold))
    } else {
        (None, None)
    }
}

/// Pull the pin. Returns the pin's new place and whether this pull took it out.
pub fn pull(pin: Option<Pin>) -> (Option<Pin>, bool) {
    match pin {
        Some(Pin::In) => (Some(Pin::Pulling), true),
        other => (other, false),
    }
}

/// The fuse write before the grenade weapon's update: a pulled pin starts the fuse, and so does a
/// throw with the pin still in. `throwing` is whether this update fires the grenade.
pub fn before_update(pin: Option<Pin>, throwing: bool) -> (Option<Pin>, Option<FuseWrite>) {
    match pin {
        Some(Pin::Pulling) => (Some(Pin::Out), Some(FuseWrite::Run)),
        Some(Pin::In) if throwing => (Some(Pin::Out), Some(FuseWrite::Run)),
        other => (other, None),
    }
}

/// Whether the pin's grab volume is placed: the pin is in, and the hand that would pull it holds
/// nothing.
pub fn volume_placed(pin: Option<Pin>, other_hand_free: bool) -> bool {
    pin == Some(Pin::In) && other_hand_free
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_primed_grenade_holds_its_fuse_only_with_the_pin_on() {
        assert_eq!(on_primed(true), (Some(Pin::In), Some(FuseWrite::Hold)));
        assert_eq!(on_primed(false), (None, None));
    }

    #[test]
    fn a_pulled_pin_starts_the_fuse_at_the_next_update() {
        let (pin, pulled) = pull(Some(Pin::In));
        assert!(pulled);
        assert_eq!(pin, Some(Pin::Pulling));
        assert_eq!(
            before_update(pin, false),
            (Some(Pin::Out), Some(FuseWrite::Run))
        );
        // Out stays out, and the fuse is written once.
        assert_eq!(before_update(Some(Pin::Out), false), (Some(Pin::Out), None));
        assert_eq!(before_update(Some(Pin::Out), true), (Some(Pin::Out), None));
    }

    #[test]
    fn a_pin_comes_out_once() {
        assert_eq!(pull(Some(Pin::Pulling)), (Some(Pin::Pulling), false));
        assert_eq!(pull(Some(Pin::Out)), (Some(Pin::Out), false));
        assert_eq!(pull(None), (None, false));
    }

    #[test]
    fn a_throw_with_the_pin_in_starts_the_fuse() {
        assert_eq!(
            before_update(Some(Pin::In), true),
            (Some(Pin::Out), Some(FuseWrite::Run))
        );
        assert_eq!(before_update(Some(Pin::In), false), (Some(Pin::In), None));
        assert_eq!(before_update(None, true), (None, None));
    }

    #[test]
    fn the_pin_volume_needs_the_pin_in_and_a_free_hand() {
        assert!(volume_placed(Some(Pin::In), true));
        assert!(!volume_placed(Some(Pin::In), false));
        assert!(!volume_placed(Some(Pin::Out), true));
        assert!(!volume_placed(None, true));
    }
}
