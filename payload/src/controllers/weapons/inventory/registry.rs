//! The registry of the weapons the player owns, holds, or has let go of nearby: one entry per
//! weapon object, with where it is. Pure.
//!
//! The game stays the source of truth for its inventory slots, which the save, the HUD, the weapon
//! wheel, and the missions read, so the registry mirrors them: each input tick it folds in an
//! [`Observation`] of the slots, the hands, and the fate of every other weapon it knows
//! ([`Registry::observe`]). It is the source of truth for everything else: which hand drives a
//! weapon outside the game's own wielding, and which weapons lying in the world the player has
//! touched. A weapon is keyed by its object's address while it lives; an entry whose object is
//! gone is dropped.

use jc3gi::character::inventory::WeaponSlot;

use crate::controllers::{frame::Hand, weapons::inventory::sockets::SocketId};

/// An entry's identity, stable for the session.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntryId(pub u32);

/// A live weapon object's address, the registry's key for it.
pub type ObjectKey = usize;

/// What kind of weapon an entry is: its spawn tag's hash, the form the save keeps a weapon's type
/// in, and its inventory slot.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct WeaponKind {
    pub tag_hash: u32,
    pub slot: WeaponSlot,
}

/// How a weapon in a hand is driven.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Role {
    /// The game's wielded weapon, which the game fires.
    Wielded,
    /// A weapon the mod fires from the holstered state.
    Driven,
}

/// Where a weapon is.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Location {
    /// In its inventory slot, on the body.
    MainSlot(WeaponSlot),
    /// In a hand.
    Hand(Hand, Role),
    /// Carried beyond the slots, docked on a socket of the body.
    Socket(SocketId),
    /// Lying in the world after the player let go of it; `pinned` while the mod keeps it from the
    /// game's despawn.
    World { pinned: bool },
    /// Held by another character.
    Npc,
}

impl Location {
    /// Whether the player owns a weapon here: it is kept by a strong reference and has the player
    /// as its holder.
    pub const fn owned(self) -> bool {
        matches!(
            self,
            Location::MainSlot(_) | Location::Hand(..) | Location::Socket(_)
        )
    }

    pub fn label(self) -> String {
        match self {
            Location::MainSlot(_) => "in its slot".to_owned(),
            Location::Hand(hand, Role::Wielded) => format!("in the {} hand", hand.label()),
            Location::Hand(hand, Role::Driven) => {
                format!("in the {} hand, driven by the mod", hand.label())
            }
            Location::Socket(socket) => format!("carried on the {}", socket.label()),
            Location::World { pinned: true } => "in the world, pinned".to_owned(),
            Location::World { pinned: false } => "in the world".to_owned(),
            Location::Npc => "with another character".to_owned(),
        }
    }
}

/// One weapon the registry knows.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Entry {
    pub id: EntryId,
    pub object: ObjectKey,
    pub kind: WeaponKind,
    /// The magazine as last observed.
    pub magazine: i32,
    pub location: Location,
    /// How far the weapon is from the player, metres, for a weapon in the world.
    pub distance_m: Option<f32>,
}

/// A weapon seen in an inventory slot or in the mod's keeping.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Seen {
    pub object: ObjectKey,
    pub kind: WeaponKind,
    pub magazine: i32,
}

/// What became of a known weapon that is in no slot.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Fate {
    /// It lies in the world (or anywhere else outside a character), at this distance from the
    /// player when it could be read.
    Lying {
        distance_m: Option<f32>,
        magazine: i32,
    },
    /// Another character holds it.
    HeldByOther,
    /// The object is gone.
    Gone,
}

/// One input tick's view of the player's weapons.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Observation {
    /// The weapons in the inventory slots the registry tracks.
    pub slots: Vec<Seen>,
    /// The weapons the mod carries beyond the slots, and the socket each is docked on.
    pub carried: Vec<(Seen, SocketId)>,
    /// The weapon in each hand, by [`Hand::index`], and how it is driven.
    pub hands: [Option<(ObjectKey, Role)>; 2],
    /// The fate of each known weapon that is in no slot, from [`Registry::unplaced`].
    pub fates: Vec<(ObjectKey, Fate)>,
}

impl Observation {
    /// Whether `object` is in a slot or carried.
    fn placed(&self, object: ObjectKey) -> bool {
        self.slots.iter().any(|s| s.object == object)
            || self.carried.iter().any(|(s, _)| s.object == object)
    }
}

/// Why an entry left the registry.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum GoneWhy {
    /// The object no longer exists.
    Destroyed,
    /// It lay in the world, unpinned and far away, so it was left to the game.
    Forgotten,
    /// The spawn system reset, which replaces or removes every weapon.
    Reset,
}

impl GoneWhy {
    pub const fn label(self) -> &'static str {
        match self {
            GoneWhy::Destroyed => "the object is gone",
            GoneWhy::Forgotten => "left to the game",
            GoneWhy::Reset => "the spawn system reset",
        }
    }
}

/// A change the registry made, for the log.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Transition {
    Added {
        id: EntryId,
        kind: WeaponKind,
        to: Location,
    },
    Moved {
        id: EntryId,
        kind: WeaponKind,
        from: Location,
        to: Location,
    },
    Gone {
        id: EntryId,
        kind: WeaponKind,
        from: Location,
        why: GoneWhy,
    },
}

/// The registry.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    entries: Vec<Entry>,
    next_id: u32,
}

impl Registry {
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn get(&self, id: EntryId) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// The entry of `object`.
    pub fn find(&self, object: ObjectKey) -> Option<&Entry> {
        self.entries.iter().find(|e| e.object == object)
    }

    /// The known weapons that are neither among `slots` nor `carried`, whose fate the next
    /// observation must say.
    pub fn unplaced(&self, slots: &[Seen], carried: &[(Seen, SocketId)]) -> Vec<ObjectKey> {
        self.entries
            .iter()
            .filter(|e| {
                !slots.iter().any(|s| s.object == e.object)
                    && !carried.iter().any(|(s, _)| s.object == e.object)
            })
            .map(|e| e.object)
            .collect()
    }

    /// Fold in one tick's observation. A weapon in a slot is added when new; a hand's weapon is
    /// in that hand; a known weapon outside the slots follows its fate, and one without a fate is
    /// taken for gone.
    pub fn observe(&mut self, observation: &Observation) -> Vec<Transition> {
        let mut out = Vec::new();
        let hand_of = |object: ObjectKey| {
            Hand::BOTH.into_iter().find_map(|hand| {
                observation.hands[hand.index()]
                    .filter(|(o, _)| *o == object)
                    .map(|(_, role)| (hand, role))
            })
        };
        for seen in &observation.slots {
            let to = match hand_of(seen.object) {
                Some((hand, role)) => Location::Hand(hand, role),
                None => Location::MainSlot(seen.kind.slot),
            };
            out.extend(self.place(*seen, to));
        }
        for &(seen, socket) in &observation.carried {
            let to = match hand_of(seen.object) {
                Some((hand, _)) => Location::Hand(hand, Role::Driven),
                None => Location::Socket(socket),
            };
            out.extend(self.place(seen, to));
        }
        let mut gone = Vec::new();
        for entry in &mut self.entries {
            if observation.placed(entry.object) {
                continue;
            }
            let fate = observation
                .fates
                .iter()
                .find(|(o, _)| *o == entry.object)
                .map_or(Fate::Gone, |&(_, fate)| fate);
            let to = match fate {
                Fate::Lying {
                    distance_m,
                    magazine,
                } => {
                    entry.distance_m = distance_m;
                    entry.magazine = magazine;
                    match entry.location {
                        Location::World { pinned } => Location::World { pinned },
                        _ => Location::World { pinned: false },
                    }
                }
                Fate::HeldByOther => {
                    entry.distance_m = None;
                    Location::Npc
                }
                Fate::Gone => {
                    gone.push(entry.id);
                    continue;
                }
            };
            if entry.location != to {
                out.push(Transition::Moved {
                    id: entry.id,
                    kind: entry.kind,
                    from: entry.location,
                    to,
                });
                entry.location = to;
            }
        }
        for id in gone {
            out.extend(self.remove(id, GoneWhy::Destroyed));
        }
        out
    }

    /// Put `seen` at `to`, adding its entry when it is new.
    fn place(&mut self, seen: Seen, to: Location) -> Option<Transition> {
        match self.entries.iter_mut().find(|e| e.object == seen.object) {
            Some(entry) => {
                entry.kind = seen.kind;
                entry.magazine = seen.magazine;
                entry.distance_m = None;
                if entry.location == to {
                    return None;
                }
                let from = entry.location;
                entry.location = to;
                Some(Transition::Moved {
                    id: entry.id,
                    kind: entry.kind,
                    from,
                    to,
                })
            }
            None => {
                let id = self.allocate();
                self.entries.push(Entry {
                    id,
                    object: seen.object,
                    kind: seen.kind,
                    magazine: seen.magazine,
                    location: to,
                    distance_m: None,
                });
                Some(Transition::Added {
                    id,
                    kind: seen.kind,
                    to,
                })
            }
        }
    }

    /// Pin or unpin the weapon of `id`, when it lies in the world.
    pub fn set_pinned(&mut self, id: EntryId, pinned: bool) -> Option<Transition> {
        let entry = self.entries.iter_mut().find(|e| e.id == id)?;
        let Location::World { pinned: was } = entry.location else {
            return None;
        };
        if was == pinned {
            return None;
        }
        let to = Location::World { pinned };
        let from = entry.location;
        entry.location = to;
        Some(Transition::Moved {
            id,
            kind: entry.kind,
            from,
            to,
        })
    }

    /// Drop the entry of `id`.
    pub fn remove(&mut self, id: EntryId, why: GoneWhy) -> Option<Transition> {
        let index = self.entries.iter().position(|e| e.id == id)?;
        let entry = self.entries.remove(index);
        Some(Transition::Gone {
            id,
            kind: entry.kind,
            from: entry.location,
            why,
        })
    }

    /// Drop every entry, for a spawn system reset: it removes the weapons in the world and
    /// replaces the slots' weapons with new objects.
    pub fn reset(&mut self) -> Vec<Transition> {
        let ids: Vec<EntryId> = self.entries.iter().map(|e| e.id).collect();
        ids.into_iter()
            .filter_map(|id| self.remove(id, GoneWhy::Reset))
            .collect()
    }

    fn allocate(&mut self) -> EntryId {
        let id = EntryId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1).max(1);
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RIFLE: ObjectKey = 0x1000;
    const SNIPER: ObjectKey = 0x2000;
    const PISTOLS: ObjectKey = 0x3000;

    fn kind(slot: WeaponSlot) -> WeaponKind {
        WeaponKind {
            tag_hash: 0xABCD_0000 | slot as u32,
            slot,
        }
    }

    fn seen(object: ObjectKey, slot: WeaponSlot) -> Seen {
        Seen {
            object,
            kind: kind(slot),
            magazine: 30,
        }
    }

    fn lying(distance_m: f32) -> Fate {
        Fate::Lying {
            distance_m: Some(distance_m),
            magazine: 12,
        }
    }

    fn location(r: &Registry, object: ObjectKey) -> Option<Location> {
        r.find(object).map(|e| e.location)
    }

    #[test]
    fn the_slots_are_mirrored_and_the_wielded_weapon_is_in_its_hand() {
        let mut r = Registry::new();
        let t = r.observe(&Observation {
            carried: vec![],
            slots: vec![
                seen(RIFLE, WeaponSlot::E_SLOT_TWO_HANDED),
                seen(PISTOLS, WeaponSlot::E_SLOT_DUAL_WIELD),
            ],
            hands: [Some((RIFLE, Role::Wielded)), None],
            fates: vec![],
        });
        assert_eq!(t.len(), 2);
        assert!(t.iter().all(|t| matches!(t, Transition::Added { .. })));
        assert_eq!(
            location(&r, RIFLE),
            Some(Location::Hand(Hand::Left, Role::Wielded))
        );
        assert_eq!(
            location(&r, PISTOLS),
            Some(Location::MainSlot(WeaponSlot::E_SLOT_DUAL_WIELD))
        );
        // The same view again changes nothing.
        let again = r.observe(&Observation {
            carried: vec![],
            slots: vec![
                seen(RIFLE, WeaponSlot::E_SLOT_TWO_HANDED),
                seen(PISTOLS, WeaponSlot::E_SLOT_DUAL_WIELD),
            ],
            hands: [Some((RIFLE, Role::Wielded)), None],
            fates: vec![],
        });
        assert!(again.is_empty());
    }

    #[test]
    fn a_holstered_weapon_goes_back_to_its_slot_and_a_driven_one_is_marked() {
        let mut r = Registry::new();
        let slots = vec![
            seen(RIFLE, WeaponSlot::E_SLOT_TWO_HANDED),
            seen(SNIPER, WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL),
        ];
        r.observe(&Observation {
            carried: vec![],
            slots: slots.clone(),
            hands: [Some((SNIPER, Role::Driven)), Some((RIFLE, Role::Wielded))],
            fates: vec![],
        });
        assert_eq!(
            location(&r, SNIPER),
            Some(Location::Hand(Hand::Left, Role::Driven))
        );
        let t = r.observe(&Observation {
            carried: vec![],
            slots,
            hands: [None, Some((RIFLE, Role::Wielded))],
            fates: vec![],
        });
        assert_eq!(
            t,
            vec![Transition::Moved {
                id: r.find(SNIPER).unwrap().id,
                kind: kind(WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL),
                from: Location::Hand(Hand::Left, Role::Driven),
                to: Location::MainSlot(WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL),
            }]
        );
    }

    #[test]
    fn a_weapon_let_go_of_lies_in_the_world_until_it_is_gone() {
        let mut r = Registry::new();
        r.observe(&Observation {
            carried: vec![],
            slots: vec![seen(RIFLE, WeaponSlot::E_SLOT_TWO_HANDED)],
            hands: [None, Some((RIFLE, Role::Wielded))],
            fates: vec![],
        });
        assert_eq!(r.unplaced(&[], &[]), vec![RIFLE]);
        r.observe(&Observation {
            carried: vec![],
            slots: vec![],
            hands: [None; 2],
            fates: vec![(RIFLE, lying(1.5))],
        });
        let entry = *r.find(RIFLE).unwrap();
        assert_eq!(entry.location, Location::World { pinned: false });
        assert_eq!(entry.distance_m, Some(1.5));
        assert_eq!(entry.magazine, 12, "the magazine follows the object");
        let t = r.observe(&Observation {
            carried: vec![],
            slots: vec![],
            hands: [None; 2],
            fates: vec![(RIFLE, Fate::Gone)],
        });
        assert!(matches!(
            t.as_slice(),
            [Transition::Gone {
                why: GoneWhy::Destroyed,
                ..
            }]
        ));
        assert!(r.entries().is_empty());
    }

    #[test]
    fn a_weapon_without_a_fate_is_taken_for_gone() {
        let mut r = Registry::new();
        r.observe(&Observation {
            slots: vec![seen(RIFLE, WeaponSlot::E_SLOT_TWO_HANDED)],
            ..Observation::default()
        });
        r.observe(&Observation::default());
        assert!(r.entries().is_empty());
    }

    #[test]
    fn a_weapon_another_character_takes_is_theirs_and_comes_back_when_taken_back() {
        let mut r = Registry::new();
        r.observe(&Observation {
            slots: vec![seen(RIFLE, WeaponSlot::E_SLOT_TWO_HANDED)],
            ..Observation::default()
        });
        let id = r.find(RIFLE).unwrap().id;
        r.observe(&Observation {
            fates: vec![(RIFLE, lying(3.0))],
            ..Observation::default()
        });
        r.observe(&Observation {
            fates: vec![(RIFLE, Fate::HeldByOther)],
            ..Observation::default()
        });
        assert_eq!(location(&r, RIFLE), Some(Location::Npc));
        assert!(!Location::Npc.owned());
        r.observe(&Observation {
            carried: vec![],
            slots: vec![seen(RIFLE, WeaponSlot::E_SLOT_TWO_HANDED)],
            hands: [Some((RIFLE, Role::Wielded)), None],
            fates: vec![],
        });
        let entry = r.find(RIFLE).unwrap();
        assert_eq!(entry.id, id, "the same weapon keeps its entry");
        assert!(entry.location.owned());
    }

    #[test]
    fn pinning_acts_only_on_weapons_in_the_world_and_keeps_through_ticks() {
        let mut r = Registry::new();
        r.observe(&Observation {
            slots: vec![seen(RIFLE, WeaponSlot::E_SLOT_TWO_HANDED)],
            ..Observation::default()
        });
        let id = r.find(RIFLE).unwrap().id;
        assert_eq!(
            r.set_pinned(id, true),
            None,
            "a slot's weapon is not pinned"
        );
        r.observe(&Observation {
            fates: vec![(RIFLE, lying(2.0))],
            ..Observation::default()
        });
        assert!(r.set_pinned(id, true).is_some());
        assert_eq!(r.set_pinned(id, true), None, "pinned once");
        r.observe(&Observation {
            fates: vec![(RIFLE, lying(2.5))],
            ..Observation::default()
        });
        assert_eq!(location(&r, RIFLE), Some(Location::World { pinned: true }));
    }

    #[test]
    fn a_reset_drops_everything() {
        let mut r = Registry::new();
        r.observe(&Observation {
            carried: vec![],
            slots: vec![seen(RIFLE, WeaponSlot::E_SLOT_TWO_HANDED)],
            fates: vec![],
            hands: [None; 2],
        });
        let t = r.reset();
        assert_eq!(t.len(), 1);
        assert!(r.entries().is_empty());
        // Identities keep counting up, so a log line never names two weapons with one id.
        r.observe(&Observation {
            slots: vec![seen(SNIPER, WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL)],
            ..Observation::default()
        });
        assert_eq!(r.find(SNIPER).unwrap().id, EntryId(2));
    }

    #[test]
    fn a_carried_weapon_is_on_its_socket_and_in_a_hand_while_driven() {
        let mut r = Registry::new();
        let carried = vec![(
            seen(SNIPER, WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL),
            SocketId::BackOuter,
        )];
        r.observe(&Observation {
            slots: vec![seen(RIFLE, WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL)],
            carried: carried.clone(),
            hands: [None; 2],
            fates: vec![],
        });
        assert_eq!(
            location(&r, SNIPER),
            Some(Location::Socket(SocketId::BackOuter))
        );
        assert!(Location::Socket(SocketId::BackOuter).owned());
        assert!(r.unplaced(&[], &carried).contains(&RIFLE));
        assert!(!r.unplaced(&[], &carried).contains(&SNIPER));
        r.observe(&Observation {
            slots: vec![seen(RIFLE, WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL)],
            carried,
            hands: [Some((SNIPER, Role::Wielded)), None],
            fates: vec![],
        });
        assert_eq!(
            location(&r, SNIPER),
            Some(Location::Hand(Hand::Left, Role::Driven)),
            "a carried weapon in a hand is the mod's to drive"
        );
    }

    #[test]
    fn a_promotion_swaps_a_slot_and_a_socket_without_losing_either() {
        let mut r = Registry::new();
        r.observe(&Observation {
            slots: vec![seen(RIFLE, WeaponSlot::E_SLOT_TWO_HANDED)],
            carried: vec![(
                seen(SNIPER, WeaponSlot::E_SLOT_TWO_HANDED),
                SocketId::BackFirst,
            )],
            hands: [None; 2],
            fates: vec![],
        });
        let ids = (r.find(RIFLE).unwrap().id, r.find(SNIPER).unwrap().id);
        let t = r.observe(&Observation {
            slots: vec![seen(SNIPER, WeaponSlot::E_SLOT_TWO_HANDED)],
            carried: vec![(
                seen(RIFLE, WeaponSlot::E_SLOT_TWO_HANDED),
                SocketId::BackFirst,
            )],
            hands: [None, Some((SNIPER, Role::Wielded))],
            fates: vec![],
        });
        assert_eq!(t.len(), 2);
        assert!(t.iter().all(|t| matches!(t, Transition::Moved { .. })));
        assert_eq!(r.find(RIFLE).unwrap().id, ids.0);
        assert_eq!(r.find(SNIPER).unwrap().id, ids.1);
        assert_eq!(
            location(&r, RIFLE),
            Some(Location::Socket(SocketId::BackFirst))
        );
    }
}
