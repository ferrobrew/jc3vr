//! Each hand's grab hint as the weapon handling's input tick publishes it for the HUD
//! (`hud::grab_hint`): what the hand's grip would take, or swap, and where on the body.
//!
//! The input tick publishes each hand's hint ([`publish`]), with the icons named by weapon id, and
//! the HUD reads the latest one ([`fresh`]) on the render thread. A hint is published only while
//! the game shows its gameplay HUD, which the HUD decides through the gate it registers
//! ([`set_publish_gate`]).

use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

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

/// Publish each hand's hint, by [`Hand::index`](jc3vr_input::frame::Hand::index), or
/// forget them while the publish gate is shut. Called each input tick of the weapon handling.
pub(crate) fn publish(hints: [Option<Hint>; 2]) {
    let open = PUBLISH_GATE.get().is_none_or(|gate| gate());
    *SAMPLE.lock() = open.then(|| (hints, Instant::now()));
}

/// Forget the hints: the weapon handling stood down.
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

/// Register whether a hint may be published at the moment of publishing. Only one gate is held:
/// the first registration wins and a later one is refused with a warning. With no gate registered,
/// every hint is published.
pub fn set_publish_gate(gate: fn() -> bool) {
    if PUBLISH_GATE.set(gate).is_err() {
        tracing::warn!("grab hint: a publish gate is already registered; the new one is ignored");
    }
}

static SAMPLE: Mutex<Option<([Option<Hint>; 2], Instant)>> = Mutex::new(None);

static PUBLISH_GATE: OnceLock<fn() -> bool> = OnceLock::new();
