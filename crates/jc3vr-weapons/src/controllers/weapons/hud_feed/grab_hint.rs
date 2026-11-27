//! Each hand's grab hint as the weapon handling's input tick publishes it for the HUD
//! (`hud::grab_hint`): what the hand's grip would take, or swap, and where on the body.
//!
//! The input tick publishes each hand's hint ([`publish`]), with the icons named by weapon id, and
//! the HUD reads the latest one ([`fresh`]) on the render thread, when it shows hints at all.

use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::controllers::{loadout::Socket, weapons::hud_feed::icon::IconId};

/// What one hand's grip would take, or swap, and where on the body.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Hint {
    /// The grip takes the item with this icon from `place`.
    Take { icon: IconId, place: Place },
    /// Letting go docks the held weapon (`incoming`) on `place`, and the weapon docked there
    /// (`outgoing`) falls clear.
    Swap {
        incoming: IconId,
        outgoing: IconId,
        place: Place,
    },
}

impl Hint {
    /// The icons the hint shows, in drawing order.
    #[inline]
    pub fn icons(self) -> impl Iterator<Item = IconId> {
        let (first, second) = match self {
            Hint::Take { icon, .. } => (icon, None),
            Hint::Swap {
                incoming, outgoing, ..
            } => (incoming, Some(outgoing)),
        };
        std::iter::once(first).chain(second)
    }

    /// The name of the place the hint is about, drawn above it.
    pub fn label(self) -> &'static str {
        match self {
            Hint::Take { place, .. } | Hint::Swap { place, .. } => place.name(),
        }
    }
}

/// A place on the body a hand takes from or docks on.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Place {
    Socket(Socket),
    GrenadePouch,
    C4Pouch,
    MagazinePouch,
}

impl Place {
    /// The place's name as the hint shows it, in sentence case.
    pub const fn name(self) -> &'static str {
        match self {
            Place::Socket(socket) => socket.display_name(),
            Place::GrenadePouch => "Grenade pouch",
            Place::C4Pouch => "C4 pouch",
            Place::MagazinePouch => "Magazine pouch",
        }
    }
}

/// Publish each hand's hint, by [`Hand::index`](jc3vr_input::frame::Hand::index). Called each
/// input tick of the weapon handling.
pub(crate) fn publish(hints: [Option<Hint>; 2]) {
    *SAMPLE.lock() = Some((hints, Instant::now()));
}

/// Forget the hints: the weapon handling stood down, or the HUD shows none.
pub fn clear() {
    *SAMPLE.lock() = None;
}

/// Each hand's hint published no longer than `stale` before `now`, or none for a hint that is
/// older or was forgotten.
pub fn fresh(now: Instant, stale: Duration) -> [Option<Hint>; 2] {
    SAMPLE
        .lock()
        .filter(|(_, at)| now.duration_since(*at) <= stale)
        .map_or([None; 2], |(hints, _)| hints)
}

static SAMPLE: Mutex<Option<([Option<Hint>; 2], Instant)>> = Mutex::new(None);
