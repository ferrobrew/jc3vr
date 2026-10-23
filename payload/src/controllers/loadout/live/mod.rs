//! The live table: the one [`Loadout`] of the local player's items, fed by the game and the hands,
//! and published as a snapshot each input tick.
//!
//! The table is the only writer of where the player's weapons are. The game's slots follow it
//! ([`crate::controllers::weapons::inventory::reconcile`]), the placement reads its hands and its
//! places on the body ([`published`]), and a weapon the game wields lands in the hand a grab
//! expected, or in a free hand for the game's own draw ([`expect`]). What the hands and the game do
//! comes here as events:
//!
//! - the hand moves (the draws, docks, takes, hand-overs, and drops), at once from the call that
//!   makes them, and the game's own holster or draw of the wielded weapon at the input tick that
//!   sees it ([`hands`]);
//! - what the game did with a weapon of the table's that is in no slot, the carry set, or a hand
//!   (its fate, from the inventory's weak reference to it): a weapon the game dropped lies in the
//!   world, and one gone or taken by another character leaves;
//! - a weapon lying in the world the pins left to the game leaves;
//! - the spawn reset's begin and end, the store of the save, the mission's weapon cache, its
//!   removal and its give-back, and the weapons the sidecar restores or the split pistol pair adds.
//!
//! The weapons lying in the world are the table's alone: the pins read them from it
//! ([`crate::controllers::weapons::inventory::pins`]). The table takes the weapons of the slots the
//! inventory tracks, and a heavy weapon while a hand holds it.
//!
//! The table is behind one lock. The input tick and the detours (the reset's, the store's, and the
//! mission cache's, on whatever thread the game runs them) fold their events under it, and make no
//! engine call and take no other lock while they hold it but the published places' and the
//! status's, leaf locks of their own, which the placement and the readouts read.

mod expect;
mod hands;
mod published;

use std::{sync::Arc, time::Instant};

use parking_lot::Mutex;

pub(crate) use crate::controllers::loadout::live::{
    expect::{Expect, Pending},
    hands::{Held, Whereabouts},
    published::{hand_of, in_hand, item_of, on_body, place_of},
};
use crate::controllers::{
    frame::Hand,
    loadout::{
        binding::{Awaiting, Binding, ObjectAddress},
        event::Event,
        item::ItemKind,
        live::expect::landing,
        projection::{StoreProjection, project_store},
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

/// What became of a weapon of the table's that is in no slot, the carry set, or a hand.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Fate {
    /// It lies in the world.
    Lying,
    /// Another character holds it.
    HeldByOther,
    /// The object is gone.
    Gone,
}

/// One input tick of the game, as the table is fed from it.
pub(crate) struct Tick<'a> {
    /// The weapons in the slots the inventory tracks.
    pub slots: &'a [SlotObject],
    /// The carry set's weapons: each one's object and its spawn tag's hash.
    pub carried: &'a [(ObjectAddress, u32)],
    /// The weapon the game wields in the hands.
    pub wielded: Option<Held>,
    /// The weapon armed in each hand, by [`Hand::index`].
    pub armed: [Option<ObjectAddress>; 2],
    /// What became of the table's weapons that are in no slot, the carry set, or a hand.
    pub fates: &'a [(ObjectAddress, Fate)],
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
        live.pending = None;
        live.origins = [None; 2];
        live.last = None;
        live.reset = ResetWatch::default();
        live.fold(Event::ResetCompleted {
            kind: ResetKind::Load,
            slots: slots.to_vec(),
        });
        live.publish_places();
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
    live.publish_places();
}

/// A weapon the game wields that came into a hand this tick, and the expectation it met (`None`
/// for the game's own draw).
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct Arrival {
    pub hand: Hand,
    pub expect: Option<Expect>,
}

/// Fold one input tick of the game into the table, and publish the table. Returns the wielded weapon's arrival in a hand, when it came into one.
pub(crate) fn input_tick(tick: &Tick) -> Option<Arrival> {
    let mut live = LIVE.lock();
    if !live.started {
        return None;
    }
    let now = Instant::now();
    let dt_s = live
        .last
        .map_or(0.0, |t| now.saturating_duration_since(t).as_secs_f32());
    live.last = Some(now);
    live.pending = live.pending.and_then(|p| p.aged(dt_s));
    live.rebind_kept(tick.carried);
    let arrival = live.hand_moves(tick);
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
    live.publish_places();
    let published = live.published();
    drop(live);
    let mut status = STATUS.lock();
    *status = LiveStatus {
        snapshot: Some(Arc::new(snapshot)),
        ..published
    };
    arrival
}

/// `hand` expects the next weapon the game wields, or `weapon` when it is named, to come into it
/// as `expect` says: a grab started a draw or a take that the game carries out.
pub(crate) fn expect(hand: Hand, expect: Expect, weapon: Option<ObjectAddress>) {
    LIVE.lock().pending = Some(Pending::new(hand, expect, weapon));
}

/// The hand expecting a weapon, while one is.
pub(crate) fn pending() -> Option<Pending> {
    LIVE.lock().pending
}

/// What brought the weapon in `hand` there, as far as the hands know: the expectation it met, or
/// `None` for the game's own draw.
pub(crate) fn origin(hand: Hand) -> Option<Expect> {
    LIVE.lock().origins[hand.index()]
}

/// The game now wields `held`, which a call the mod made put into the hands (an equip or a
/// pickup): it lands in the hand that expected it, or in a free one, at once rather than at the
/// next input tick, so that the next placement already puts it there. Left to the next input tick
/// when that hand still holds the weapon the call replaced, whose departure the tick sees first.
/// Returns its arrival, when it landed now.
pub(crate) fn adopt(held: Held) -> Option<Arrival> {
    let mut live = LIVE.lock();
    if !live.started {
        return None;
    }
    let arrival = live.land(held);
    live.publish_places();
    arrival
}

/// Hand the weapon at `object` from the other hand to `to`, when the table has it there and `to`
/// is empty. Returns whether it moved.
pub(crate) fn hand_over(to: Hand, object: ObjectAddress) -> bool {
    let mut live = LIVE.lock();
    let from = to.other();
    let movable = live.started
        && live.table.held(to).is_none()
        && live
            .table
            .held(from)
            .is_some_and(|item| item.binding == Binding::Live(object));
    if !movable {
        return false;
    }
    live.fold(Event::HandOver { from });
    let moved = live.hand_holding(object) == Some(to);
    if moved {
        live.origins[to.index()] = live.origins[from.index()].take();
    }
    live.publish_places();
    moved
}

/// The hands put `held` into `hand` (a draw, or a take off the body): the table
/// takes the move at once. Returns whether the table holds it there.
pub(crate) fn arrive(hand: Hand, held: Held) -> bool {
    let mut live = LIVE.lock();
    if !live.started {
        return false;
    }
    if let Some(event) = hands::arrival(&live.table, hand, held, TakeSource::World) {
        live.fold(event);
        if live.hand_holding(held.object) == Some(hand) {
            live.origins[hand.index()] = Some(Expect::Draw);
        }
    }
    live.publish_places();
    live.hand_holding(held.object) == Some(hand)
}

/// The hands took the weapon at `object` out of `hand` to `whereabouts` (a dock, a put-back, or a
/// let-go): the table takes the move at once.
pub(crate) fn depart(hand: Hand, object: ObjectAddress, whereabouts: Whereabouts) {
    let mut live = LIVE.lock();
    if !live.started {
        return;
    }
    if let Some(event) = hands::departure(&live.table, hand, object, whereabouts) {
        live.fold(event);
    }
    live.publish_places();
}

/// The hands docked the weapon at `object`, held in `hand`, on `socket`: the table takes the move at
/// once. Returns whether the table has it there.
pub(crate) fn dock(hand: Hand, object: ObjectAddress, socket: Socket) -> bool {
    let mut live = LIVE.lock();
    if !live.started {
        return false;
    }
    let holds = live
        .table
        .held(hand)
        .is_some_and(|item| item.binding == Binding::Live(object));
    if holds {
        live.fold(Event::Dock { hand, socket });
    }
    live.publish_places();
    live.place_of(object) == Some(Place::Socket(socket))
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
    live.pending = None;
    live.origins = [None; 2];
    live.push("loadout: the table stopped, the inventory is off".to_owned());
    published::clear();
    let published = live.published();
    drop(live);
    *STATUS.lock() = published;
}

/// The spawn reset is about to replace the slots' weapons, and the inventory releases the carry
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
    live.publish_places();
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
/// Returns the slots and the sidecar the store should be given.
pub(crate) fn store_imminent() -> Option<StoreProjection> {
    let mut live = LIVE.lock();
    if !live.started {
        return None;
    }
    live.fold(Event::StoreImminent)
}

/// A mission is about to cache the local player's slots. Returns the slots the cache should take.
pub(crate) fn mission_caching() -> Option<StoreProjection> {
    let mut live = LIVE.lock();
    if !live.started {
        return None;
    }
    live.fold(Event::Cached(CacheKind::Mission))
}

/// What a store would write now, while the table runs, without telling the table of a store: for
/// putting the table back into the game's slots when the table stops.
pub(crate) fn store_projection_now() -> Option<StoreProjection> {
    let live = LIVE.lock();
    live.started.then(|| project_store(&live.table))
}

/// The table as it is now, while it runs, for a pass that must see the moves the hands made since
/// the last input tick.
pub(crate) fn snapshot_now() -> Option<LoadoutSnapshot> {
    let live = LIVE.lock();
    live.started.then(|| live.table.snapshot())
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
    live.publish_places();
}

/// The mod spawned `restored` to stand for a weapon the player has, and the carry set keeps it
/// on its socket.
pub(crate) fn restored(restored: Restored) {
    let mut live = LIVE.lock();
    if live.started {
        live.fold(Event::Restored(restored));
        live.publish_places();
    }
}

/// How many log lines the readouts keep.
const LOG_LINES: usize = 24;

struct Live {
    table: Loadout,
    /// Whether the table runs: it starts from the slots on the first tick with the inventory on.
    started: bool,
    /// The hand expecting the next weapon the game wields: per-hand runtime state, not a place.
    pending: Option<Pending>,
    /// What brought each hand's weapon there, by [`Hand::index`], for the readouts.
    origins: [Option<Expect>; 2],
    /// When the last input tick ran, for the expectation's age.
    last: Option<Instant>,
    reset: ResetWatch,
    mission: MissionWatch,
    transitions: u64,
    refusals: u64,
    last_store: Option<StoreProjection>,
    log: Vec<String>,
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
    const fn new() -> Live {
        Live {
            table: Loadout::new(BodyLayout::full(Hand::Right)),
            started: false,
            pending: None,
            origins: [None; 2],
            last: None,
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
        }
    }

    /// Fold `event`, logging what it did or why it was refused. Returns, for a store or a cache,
    /// the slots and the sidecar the game should be given.
    fn fold(&mut self, event: Event) -> Option<StoreProjection> {
        let store = matches!(event, Event::StoreImminent);
        match self.table.fold(event) {
            Ok(folded) => {
                for transition in &folded.transitions {
                    tracing::info!(target: "controllers", "{transition}");
                    self.push(transition.to_string());
                }
                self.transitions += folded.transitions.len() as u64;
                if store {
                    self.last_store.clone_from(&folded.store);
                }
                folded.store
            }
            Err(error) => {
                tracing::info!(target: "controllers", %error, "loadout: an event was refused");
                self.refusals += 1;
                self.push(format!("refused: {error}"));
                None
            }
        }
    }

    /// The game's moves of the hands' weapons the table has not seen yet: first each weapon in a
    /// hand of the table's that the game no longer wields and that is not armed there (the game
    /// holstered or dropped it on its own), then the wielded weapon landing in a hand when the table
    /// has it in none. Returns the wielded weapon's arrival.
    fn hand_moves(&mut self, tick: &Tick) -> Option<Arrival> {
        let wielded = tick.wielded.map(|h| h.object);
        for hand in Hand::BOTH {
            let Some(object) = self
                .table
                .held(hand)
                .and_then(|item| item.binding.address())
            else {
                continue;
            };
            if Some(object) == wielded || tick.armed.contains(&Some(object)) {
                continue;
            }
            let Some(whereabouts) = whereabouts(tick, object) else {
                continue;
            };
            if let Some(event) = hands::departure(&self.table, hand, object, whereabouts) {
                self.fold(event);
            }
        }
        self.land(tick.wielded?)
    }

    /// Land the weapon `held` the game wields in a hand, when the table has it in none: the hand
    /// that expected it, or a free one. Nothing is folded while that hand holds something, and the
    /// expectation is kept: the weapon is left to the game's layout until the hand empties, rather
    /// than refused anew every tick.
    fn land(&mut self, held: Held) -> Option<Arrival> {
        if self.hand_holding(held.object).is_some() {
            return None;
        }
        let free = Hand::BOTH.map(|hand| self.table.held(hand).is_none());
        let (hand, expect, used) = landing(
            self.pending,
            held.object,
            free,
            self.table.body().dominant(),
        );
        if !free[hand.index()] {
            return None;
        }
        if used {
            self.pending = None;
        }
        let source = match expect {
            Some(Expect::Take(source)) => source,
            Some(Expect::Draw) | None => TakeSource::Give,
        };
        let event = hands::arrival(&self.table, hand, held, source)?;
        self.fold(event);
        if self.hand_holding(held.object) != Some(hand) {
            return None;
        }
        self.origins[hand.index()] = expect;
        Some(Arrival { hand, expect })
    }

    /// The hand the table holds the object at `object` in.
    fn hand_holding(&self, object: ObjectAddress) -> Option<Hand> {
        Hand::BOTH.into_iter().find(|&hand| {
            self.table
                .held(hand)
                .is_some_and(|item| item.binding == Binding::Live(object))
        })
    }

    /// Publish the hands and the places on the body for the detours, forgetting the origin of a
    /// hand that holds nothing.
    fn publish_places(&mut self) {
        for hand in Hand::BOTH {
            let weapon = self
                .table
                .held(hand)
                .is_some_and(|item| matches!(item.kind, ItemKind::Weapon(_)));
            if !weapon {
                self.origins[hand.index()] = None;
            }
        }
        published::publish(&self.table);
    }

    /// Bind each item waiting to be spawned again to a weapon of its type the carry set kept and
    /// the table does not know: a mission's stash keeps the carried weapons' objects, which its
    /// give-back brings back.
    fn rebind_kept(&mut self, carried: &[(ObjectAddress, u32)]) {
        let waiting: Vec<_> = self
            .table
            .items()
            .filter(|item| item.binding == Binding::Awaiting(Awaiting::Respawn))
            .filter_map(|item| Some((item.id, item.kind.weapon()?.tag_hash())))
            .collect();
        for (item, tag) in waiting {
            let kept = carried
                .iter()
                .find(|&&(object, kept_tag)| kept_tag == tag && self.place_of(object).is_none());
            if let Some(&(object, _)) = kept {
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

    /// The status to publish, without the tick's snapshot.
    fn published(&self) -> LiveStatus {
        LiveStatus {
            snapshot: None,
            transitions: self.transitions,
            refusals: self.refusals,
            last_store: self.last_store.clone(),
            log: self.log.clone(),
        }
    }
}

/// Where `object` went when it left the hands, or `None` while the carry set keeps it off the slots.
fn whereabouts(tick: &Tick, object: ObjectAddress) -> Option<Whereabouts> {
    if tick.carried.iter().any(|&(o, _)| o == object) {
        // Off the slots the game neither wields nor holsters it: the hand keeps it, and the
        // reconcile arms it.
        None
    } else if tick.slots.iter().any(|slot| slot.object == object) {
        Some(Whereabouts::Slot)
    } else {
        Some(Whereabouts::Away)
    }
}

static LIVE: Mutex<Live> = Mutex::new(Live::new());

static STATUS: Mutex<LiveStatus> = Mutex::new(LiveStatus {
    snapshot: None,
    transitions: 0,
    refusals: 0,
    last_store: None,
    log: Vec::new(),
});

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use jc3gi::{character::inventory::WeaponSlot, equipment::weapon::WeaponClass};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::controllers::loadout::item::WeaponType;

    fn object(n: usize) -> ObjectAddress {
        ObjectAddress::new(NonZeroUsize::MIN.saturating_add(n * 0x10))
    }

    fn rifle() -> SlotObject {
        SlotObject {
            object: object(1),
            weapon: WeaponType::new(
                0x301,
                WeaponSlot::E_SLOT_TWO_HANDED,
                WeaponClass::E_WEAPON_CLASS_ASSAULT_RIFLE as i32,
            )
            .unwrap(),
        }
    }

    fn pistol() -> SlotObject {
        SlotObject {
            object: object(2),
            weapon: WeaponType::new(
                0x302,
                WeaponSlot::E_SLOT_DUAL_WIELD,
                WeaponClass::E_WEAPON_CLASS_PISTOL as i32,
            )
            .unwrap(),
        }
    }

    fn held(slot: SlotObject) -> Held {
        Held {
            object: slot.object,
            weapon: slot.weapon,
        }
    }

    /// A running table started from `slots`, as a load does.
    fn started(slots: &[SlotObject]) -> Live {
        let mut live = Live::new();
        live.started = true;
        live.fold(Event::ResetCompleted {
            kind: ResetKind::Load,
            slots: slots.to_vec(),
        });
        live
    }

    fn tick<'a>(
        slots: &'a [SlotObject],
        wielded: Option<SlotObject>,
        armed: Option<(Hand, SlotObject)>,
    ) -> Tick<'a> {
        Tick {
            slots,
            carried: &[],
            wielded: wielded.map(held),
            armed: Hand::BOTH.map(|h| {
                armed
                    .filter(|(hand, _)| *hand == h)
                    .map(|(_, slot)| slot.object)
            }),
            fates: &[],
        }
    }

    /// Draw `slot`'s weapon off its socket into `hand`, as a take of the second weapon does.
    fn draw(live: &mut Live, hand: Hand, slot: SlotObject) {
        let event = hands::arrival(&live.table, hand, held(slot), TakeSource::World).unwrap();
        live.fold(event);
    }

    #[test]
    fn a_drawn_weapon_lands_in_the_hand_that_expected_it() {
        let slots = [rifle()];
        let mut live = started(&slots);
        live.pending = Some(Pending::new(Hand::Left, Expect::Draw, None));
        let arrival = live.hand_moves(&tick(&slots, Some(rifle()), None));
        assert_eq!(
            arrival,
            Some(Arrival {
                hand: Hand::Left,
                expect: Some(Expect::Draw)
            })
        );
        assert_eq!(live.hand_holding(rifle().object), Some(Hand::Left));
        assert_eq!(live.pending, None, "the expectation is used up");
        assert_eq!(
            live.hand_moves(&tick(&slots, Some(rifle()), None)),
            None,
            "it stays where it landed"
        );
    }

    #[test]
    fn the_game_s_own_draw_lands_in_the_dominant_hand() {
        let slots = [rifle()];
        let mut live = started(&slots);
        let arrival = live.hand_moves(&tick(&slots, Some(rifle()), None));
        assert_eq!(
            arrival,
            Some(Arrival {
                hand: Hand::Right,
                expect: None
            })
        );
    }

    #[test]
    fn a_weapon_the_game_puts_away_leaves_its_hand_before_the_next_lands() {
        let slots = [rifle(), pistol()];
        let mut live = started(&slots);
        live.hand_moves(&tick(&slots, Some(rifle()), None));
        assert_eq!(live.hand_holding(rifle().object), Some(Hand::Right));
        live.hand_moves(&tick(&slots, Some(pistol()), None));
        assert_eq!(live.hand_holding(pistol().object), Some(Hand::Right));
        let docked = live
            .table
            .items()
            .find(|item| item.binding == Binding::Live(rifle().object))
            .map(|item| item.place);
        assert!(
            matches!(docked, Some(Place::Socket(socket)) if socket.is_native(WeaponSlot::E_SLOT_TWO_HANDED)),
            "the rifle docks on its own socket, not {docked:?}"
        );
    }

    #[test]
    fn an_armed_weapon_stays_in_its_hand_beside_the_wielded_one() {
        let slots = [rifle(), pistol()];
        let mut live = started(&slots);
        live.hand_moves(&tick(&slots, Some(pistol()), None));
        draw(&mut live, Hand::Left, rifle());
        live.hand_moves(&tick(&slots, Some(pistol()), Some((Hand::Left, rifle()))));
        assert_eq!(live.hand_holding(pistol().object), Some(Hand::Right));
        assert_eq!(
            live.hand_holding(rifle().object),
            Some(Hand::Left),
            "an armed weapon is not taken for a departure"
        );
        live.hand_moves(&tick(&slots, Some(pistol()), None));
        assert_ne!(
            live.hand_holding(rifle().object),
            Some(Hand::Left),
            "a weapon neither wielded nor armed, back in its slot, docks"
        );
    }

    #[test]
    fn a_weapon_the_game_wields_waits_while_its_hand_is_full() {
        let slots = [rifle(), pistol()];
        let mut live = started(&slots);
        live.hand_moves(&tick(&slots, Some(pistol()), None));
        draw(&mut live, Hand::Left, rifle());
        live.pending = Some(Pending::new(Hand::Left, Expect::Draw, Some(object(9))));
        let refusals = live.refusals;
        assert_eq!(
            live.land(Held {
                object: object(9),
                weapon: rifle().weapon
            }),
            None
        );
        assert_eq!(live.refusals, refusals, "nothing is refused");
        assert!(live.pending.is_some(), "the expectation is kept");
    }
}
