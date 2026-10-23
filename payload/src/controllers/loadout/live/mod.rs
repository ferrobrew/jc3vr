//! The live table: the one [`Loadout`] of the local player's items, fed by the game and the hands,
//! and published as a snapshot each input tick.
//!
//! For now the table is dual-written beside the old weapon handling
//! ([`crate::controllers::weapons`]), which still moves the weapons, and only the readouts and the
//! pins of the weapons lying in the world read it. Every move the old handling makes is also sent
//! here as an event:
//!
//! - the hand moves (the draws, holsters, stows, takes, hand-overs, and drops of the holder of the
//!   wielded weapon and of the second weapon) at the input tick that sees them ([`hands`]);
//! - what the game did with a weapon the old handling no longer keeps in a slot, the carry set, or
//!   a hand (its fate, from the inventory's weak reference to it): a weapon the game dropped lies in
//!   the world, and one gone or taken by another character leaves;
//! - a weapon lying in the world the pins left to the game leaves;
//! - the spawn reset's begin and end, the store of the save, the mission's weapon cache, its
//!   removal and its give-back, and the weapons the sidecar restores or the split pistol pair adds.
//!
//! The divergence readout ([`divergence`]) compares the table with the old handling's slots, carry
//! set, and hands each tick, so a move the dual-write misses shows before anything is made to read
//! the table. The weapons lying in the world are the table's alone: the pins read them from it
//! ([`crate::controllers::weapons::inventory::pins`]). Until the heavy
//! weapons are the mod's to hold, the table takes only the weapons the old inventory tracks.
//!
//! The table is behind one lock. The input tick and the detours (the reset's, the store's, and the
//! mission cache's, on whatever thread the game runs them) fold their events under it, and make no
//! engine call and take no other lock while they hold it. The readouts read the published status,
//! a leaf lock of its own.

mod divergence;
mod hands;

use std::sync::Arc;

use parking_lot::Mutex;

pub use crate::controllers::loadout::live::divergence::Divergence;
pub(crate) use crate::controllers::loadout::live::{
    divergence::OldPlace,
    hands::{Held, Whereabouts},
};
use crate::controllers::{
    frame::Hand,
    loadout::{
        binding::{Awaiting, Binding, ObjectAddress},
        event::Event,
        live::divergence::diverging,
        projection::StoreProjection,
        reset::{CacheKind, ResetKind, SlotObject},
        socket::{BodyLayout, Socket},
        table::{Loadout, LoadoutSnapshot, Place},
        transition::{Removal, RemovalWhy, Restored, TakeSource},
    },
};

/// The live table as the readouts show it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LiveStatus {
    /// The table as of the last input tick, while the table runs.
    pub snapshot: Option<Arc<LoadoutSnapshot>>,
    /// Where the table and the old handling disagree, as of the last input tick.
    pub divergence: Vec<Divergence>,
    /// Transitions applied and events refused since the payload loaded.
    pub transitions: u64,
    pub refusals: u64,
    /// The last store's slots and sidecar.
    pub last_store: Option<StoreProjection>,
    /// The latest transitions and refusals, the newest last.
    pub log: Vec<String>,
}

pub fn status() -> LiveStatus {
    STATUS.lock().clone()
}

/// What became of a weapon the old handling no longer keeps.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Fate {
    /// It lies in the world.
    Lying,
    /// Another character holds it.
    HeldByOther,
    /// The object is gone.
    Gone,
}

/// One input tick of the old handling, as the table is fed and checked against it.
pub(crate) struct Tick<'a> {
    /// The weapons in the slots the old inventory tracks.
    pub slots: &'a [SlotObject],
    /// The carry set's weapons: each one's object, its spawn tag's hash, and its socket.
    pub carried: &'a [(ObjectAddress, u32, Socket)],
    /// What each hand holds, by [`Hand::index`]: the wielded weapon in its holder's hand, the
    /// second weapon in its own.
    pub hands: [Option<Held>; 2],
    /// Where a weapon the table does not know came from when it comes into each hand.
    pub sources: [TakeSource; 2],
    /// What became of the table's weapons the old handling no longer keeps in a slot, the carry
    /// set, or a hand.
    pub fates: &'a [(ObjectAddress, Fate)],
    /// Where the old handling has every weapon it keeps.
    pub old: &'a [(ObjectAddress, OldPlace)],
}

/// Start the input tick's feed: start the table from the slots on its first tick, complete a spawn
/// reset or a load since the last tick, and watch for a mission's give-back. `slots` are the
/// tracked slots' weapons. Called before anything restores a weapon this tick, so a restore lands
/// in the table that the reset left.
pub(crate) fn begin_tick(dominant: Hand, slots: &[SlotObject]) {
    let mut live = LIVE.lock();
    live.table.set_body(BodyLayout::full(dominant));
    if !live.started {
        live.started = true;
        live.hands = [None; 2];
        live.reset = ResetWatch::default();
        live.fold(Event::ResetCompleted {
            kind: ResetKind::Load,
            slots: slots.to_vec(),
        });
        return;
    }
    if live.reset.completed {
        let kind = if live.reset.died {
            ResetKind::Respawn
        } else if live.reset.loaded {
            ResetKind::Load
        } else {
            ResetKind::Travel
        };
        live.reset = ResetWatch::default();
        live.fold(Event::ResetCompleted {
            kind,
            slots: slots.to_vec(),
        });
    }
    match live.mission {
        MissionWatch::Taken if slots.is_empty() => live.mission = MissionWatch::Emptied,
        MissionWatch::Emptied if !slots.is_empty() => {
            live.mission = MissionWatch::Idle;
            live.fold(Event::CacheRestored {
                cache: CacheKind::Mission,
                slots: slots.to_vec(),
            });
        }
        MissionWatch::Idle | MissionWatch::Taken | MissionWatch::Emptied => {}
    }
}

/// Fold one input tick of the old handling into the table, check the table against it, and
/// publish the table.
pub(crate) fn input_tick(tick: &Tick) {
    let mut live = LIVE.lock();
    if !live.started {
        return;
    }
    live.rebind_kept(tick.carried);
    live.hand_moves(tick);
    for &(object, fate) in tick.fates {
        let place = live.place_of(object);
        let event = match (fate, place) {
            (_, None) | (Fate::Lying, Some(Place::World(_))) => continue,
            (Fate::Lying, Some(_)) => Event::Dropped { object },
            (Fate::HeldByOther, Some(_)) => Event::Removed {
                target: Removal::Object(object),
                why: RemovalWhy::TakenByOther,
            },
            (Fate::Gone, Some(_)) => Event::Removed {
                target: Removal::Object(object),
                why: RemovalWhy::Gone,
            },
        };
        live.fold(event);
    }
    let snapshot = live.table.snapshot();
    let divergence = diverging(tick.old, &snapshot);
    if divergence.len() != live.diverged {
        tracing::debug!(
            target: "controllers",
            divergent = divergence.len(),
            "loadout: the table and the old handling disagree about {} weapons",
            divergence.len()
        );
        live.diverged = divergence.len();
    }
    let published = live.published();
    drop(live);
    let mut status = STATUS.lock();
    *status = LiveStatus {
        snapshot: Some(Arc::new(snapshot)),
        divergence,
        ..published
    };
}

/// Every live object the table's items are bound to, and where each is, as of now.
pub(crate) fn bound() -> Vec<(ObjectAddress, Place)> {
    let live = LIVE.lock();
    live.table
        .items()
        .filter_map(|item| Some((item.binding.address()?, item.place)))
        .collect()
}

/// The pins left `objects`, lying in the world, to the game: they leave the table.
pub(crate) fn forgotten(objects: &[ObjectAddress]) {
    let mut live = LIVE.lock();
    for &object in objects {
        if matches!(live.place_of(object), Some(Place::World(_))) {
            live.fold(Event::Removed {
                target: Removal::Object(object),
                why: RemovalWhy::Forgotten,
            });
        }
    }
}

/// Stop the table: the old inventory is off, so the game's slots are the player's weapons again.
/// The next tick with the inventory on starts it again from the slots.
pub(crate) fn stop() {
    let mut live = LIVE.lock();
    if !live.started {
        return;
    }
    live.started = false;
    live.table = Loadout::new(live.table.body());
    live.mission = MissionWatch::Idle;
    live.push("loadout: the table stopped, the inventory is off".to_owned());
    let published = live.published();
    drop(live);
    *STATUS.lock() = published;
}

/// The spawn reset is about to replace the slots' weapons, and the old handling releases the carry
/// set's weapons (`released`), which the reset then removes. `died` is whether the local player's
/// health was spent, the reset then being a respawn.
pub(crate) fn reset_began(died: bool, released: &[ObjectAddress]) {
    let mut live = LIVE.lock();
    if !live.started {
        return;
    }
    live.reset.died |= died;
    live.fold(Event::ResetBegan);
    for &object in released {
        if live.place_of(object).is_some() {
            live.fold(Event::Removed {
                target: Removal::Object(object),
                why: RemovalWhy::Gone,
            });
        }
    }
}

/// The spawn reset is done: the next input tick reports the slots it left.
pub(crate) fn reset_ended() {
    LIVE.lock().reset.completed = true;
}

/// The game loaded the player's save data: the next input tick starts the table again from the
/// slots the load put in the inventory.
pub(crate) fn loaded() {
    let mut live = LIVE.lock();
    live.reset.loaded = true;
    live.reset.completed = true;
}

/// The game is about to store the player's save data.
pub(crate) fn store_imminent() {
    let mut live = LIVE.lock();
    if live.started {
        live.fold(Event::StoreImminent);
    }
}

/// A mission is about to cache the local player's slots.
pub(crate) fn mission_caching() {
    let mut live = LIVE.lock();
    if live.started {
        live.fold(Event::Cached(CacheKind::Mission));
    }
}

/// A mission took the local player's weapons with its cache: the give-back is watched for, as the
/// slots holding a weapon again after they were seen empty.
pub(crate) fn mission_took() {
    let mut live = LIVE.lock();
    if !live.started {
        return;
    }
    live.mission = MissionWatch::Taken;
    live.fold(Event::Removed {
        target: Removal::AllWeapons,
        why: RemovalWhy::Mission,
    });
}

/// The mod spawned `restored` to stand for a weapon the player has, and the old handling carries it
/// on its socket.
pub(crate) fn restored(restored: Restored) {
    let mut live = LIVE.lock();
    if live.started {
        live.fold(Event::Restored(restored));
    }
}

/// How many log lines the readouts keep.
const LOG_LINES: usize = 24;

struct Live {
    table: Loadout,
    /// Whether the table runs: it starts from the slots on the first tick with the inventory on.
    started: bool,
    /// What the old hand authorities held at the last tick, by [`Hand::index`].
    hands: [Option<ObjectAddress>; 2],
    reset: ResetWatch,
    mission: MissionWatch,
    transitions: u64,
    refusals: u64,
    last_store: Option<StoreProjection>,
    log: Vec<String>,
    /// How many weapons the last check found divergent, to log a change once.
    diverged: usize,
}

/// A spawn reset or a load waiting for the next input tick to report the slots it left.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
struct ResetWatch {
    completed: bool,
    /// The player's health was spent at the reset's begin.
    died: bool,
    /// The game loaded the player's save data since the last completion.
    loaded: bool,
}

/// The give-back of a mission that took the player's weapons, which has no seam of its own.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum MissionWatch {
    Idle,
    /// The mission took the weapons; the slots are not yet seen empty.
    Taken,
    /// The slots have been seen empty since; the next weapon in them is the give-back.
    Emptied,
}

impl Live {
    /// Fold `event`, logging what it did or why it was refused.
    fn fold(&mut self, event: Event) {
        let store = matches!(event, Event::StoreImminent);
        match self.table.fold(event) {
            Ok(folded) => {
                for transition in &folded.transitions {
                    tracing::info!(target: "controllers", "{transition}");
                    self.push(transition.to_string());
                }
                self.transitions += folded.transitions.len() as u64;
                if store {
                    self.last_store = folded.store;
                }
            }
            Err(error) => {
                tracing::info!(target: "controllers", %error, "loadout: an event was refused");
                self.refusals += 1;
                self.push(format!("refused: {error}"));
            }
        }
    }

    /// The old hands' moves since the last tick: first each weapon that left a hand for somewhere
    /// no hand is, then each that came into one.
    fn hand_moves(&mut self, tick: &Tick) {
        let now = tick.hands.map(|held| held.map(|h| h.object));
        let before = std::mem::replace(&mut self.hands, now);
        for hand in Hand::BOTH {
            let Some(object) = before[hand.index()] else {
                continue;
            };
            if now.contains(&Some(object)) {
                continue;
            }
            let whereabouts = whereabouts(tick, object);
            if let Some(event) = hands::departure(&self.table, hand, object, whereabouts) {
                self.fold(event);
            }
        }
        for hand in Hand::BOTH {
            let Some(held) = tick.hands[hand.index()] else {
                continue;
            };
            if before[hand.index()] == Some(held.object) {
                continue;
            }
            if let Some(event) = hands::arrival(&self.table, hand, held, tick.sources[hand.index()])
            {
                self.fold(event);
            }
        }
    }

    /// Bind each item waiting to be spawned again to a weapon of its type the carry set kept and
    /// the table does not know: a mission's stash keeps the carried weapons' objects, which its
    /// give-back brings back.
    fn rebind_kept(&mut self, carried: &[(ObjectAddress, u32, Socket)]) {
        let waiting: Vec<_> = self
            .table
            .items()
            .filter(|item| item.binding == Binding::Awaiting(Awaiting::Respawn))
            .filter_map(|item| Some((item.id, item.kind.weapon()?.tag_hash())))
            .collect();
        for (item, tag) in waiting {
            let kept = carried
                .iter()
                .find(|&&(object, kept_tag, _)| kept_tag == tag && self.place_of(object).is_none());
            if let Some(&(object, _, _)) = kept {
                self.fold(Event::Spawned { item, object });
            }
        }
    }

    fn place_of(&self, object: ObjectAddress) -> Option<Place> {
        self.table
            .items()
            .find(|item| item.binding == Binding::Live(object))
            .map(|item| item.place)
    }

    fn push(&mut self, line: String) {
        self.log.push(line);
        if self.log.len() > LOG_LINES {
            let excess = self.log.len() - LOG_LINES;
            self.log.drain(..excess);
        }
    }

    /// The status to publish, without the tick's snapshot and divergence.
    fn published(&self) -> LiveStatus {
        LiveStatus {
            snapshot: None,
            divergence: Vec::new(),
            transitions: self.transitions,
            refusals: self.refusals,
            last_store: self.last_store.clone(),
            log: self.log.clone(),
        }
    }
}

/// Where the old handling has `object` now that it left the hands.
fn whereabouts(tick: &Tick, object: ObjectAddress) -> Whereabouts {
    if let Some(&(_, _, socket)) = tick.carried.iter().find(|&&(o, _, _)| o == object) {
        Whereabouts::Carried(socket)
    } else if tick.slots.iter().any(|slot| slot.object == object) {
        Whereabouts::Slot
    } else {
        Whereabouts::Away
    }
}

static LIVE: Mutex<Live> = Mutex::new(Live {
    table: Loadout::new(BodyLayout::full(Hand::Right)),
    started: false,
    hands: [None; 2],
    reset: ResetWatch {
        completed: false,
        died: false,
        loaded: false,
    },
    mission: MissionWatch::Idle,
    transitions: 0,
    refusals: 0,
    last_store: None,
    log: Vec::new(),
    diverged: 0,
});

static STATUS: Mutex<LiveStatus> = Mutex::new(LiveStatus {
    snapshot: None,
    divergence: Vec::new(),
    transitions: 0,
    refusals: 0,
    last_store: None,
    log: Vec::new(),
});
