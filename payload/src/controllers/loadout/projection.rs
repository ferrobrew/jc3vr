//! The game's slots as a projection of the table: what each of the eight slots should hold, which
//! is selected, and, for a store or a cache, what the sidecar records beside them. Pure.
//!
//! The game reads its slots for the consumers the mod feeds rather than patches (the HUD's
//! counter, the AI's threat rating, the store, the saves), so the slots follow the table, never
//! the other way round:
//!
//! - each weapon slot holds one of the table's weapons of that slot on the body: the primary
//!   hand's, then the other hand's, then the one on the socket the game docks that slot on, then
//!   the other sockets' in order, the oldest first among equals. Only a weapon bound to a live
//!   object goes into a slot, and a weapon lying in the world never does;
//! - the primary hand's weapon is selected;
//! - the heavy slot stays empty, in play and for a store or a cache alike: a weapon in it replaces
//!   the body's idle and strafe with the minigun's, its unequip drops it, the save stores its type
//!   as none, and the graph script's restore skips it, so a heavy weapon is always beyond the
//!   slots, kept by the carry set and recorded by the sidecar;
//! - for a store or a cache the sidecar records every weapon on the body with its hand or socket
//!   and its slot, if any, since the save keeps only the slots;
//! - the grenade, the planted explosive, the beacon, and the other consumables' slots stay the
//!   game's.

use jc3gi::character::inventory::WeaponSlot;

use crate::controllers::{
    frame::Hand,
    loadout::{
        binding::Binding,
        item::{ItemId, WeaponType},
        socket::Socket,
        table::{Loadout, Place},
    },
};

/// What one of the game's slots should hold.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SlotContent {
    Weapon {
        item: ItemId,
        weapon: WeaponType,
    },
    Empty,
    /// The game's own: a consumable slot the table never holds.
    Game,
}

/// One slot's content.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SlotFill {
    pub slot: WeaponSlot,
    pub content: SlotContent,
}

/// What the game's eight slots should hold.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Projection {
    pub slots: [SlotFill; SLOTS.len()],
    /// The slot to select: the primary hand's weapon's, when that is in its slot.
    pub selected: Option<WeaponSlot>,
}

impl Projection {
    /// The slot `item` is projected into.
    pub fn slot_of(&self, item: ItemId) -> Option<WeaponSlot> {
        self.slots
            .iter()
            .find(|fill| matches!(fill.content, SlotContent::Weapon { item: i, .. } if i == item))
            .map(|fill| fill.slot)
    }
}

/// Where on the body a weapon the sidecar records is.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum BodyPlace {
    Hand(Hand),
    Socket(Socket),
}

/// One weapon the sidecar records.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SidecarEntry {
    pub item: ItemId,
    pub weapon: WeaponType,
    pub place: BodyPlace,
    /// The slot the store projection put it in, or `None` for a weapon beyond the slots.
    pub slot: Option<WeaponSlot>,
}

/// What a store or a cache writes: the slots, and the sidecar's record of the rest.
#[derive(Debug, Clone, PartialEq)]
pub struct StoreProjection {
    pub projection: Projection,
    pub sidecar: Vec<SidecarEntry>,
}

/// The game's slots, in their index order.
pub const SLOTS: [WeaponSlot; 8] = [
    WeaponSlot::E_SLOT_DUAL_WIELD,
    WeaponSlot::E_SLOT_TWO_HANDED,
    WeaponSlot::E_SLOT_HEAVY,
    WeaponSlot::E_SLOT_HAND_GRENADE,
    WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE,
    WeaponSlot::E_SLOT_FOW,
    WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL,
    WeaponSlot::E_SLOT_BEACON,
];

/// What the game's slots should hold, in play and for a store or a cache alike.
pub fn project(loadout: &Loadout) -> Projection {
    let candidates = candidates(loadout);
    let slots = SLOTS.map(|slot| SlotFill {
        slot,
        content: match slot {
            WeaponSlot::E_SLOT_HEAVY => SlotContent::Empty,
            WeaponSlot::E_SLOT_DUAL_WIELD
            | WeaponSlot::E_SLOT_TWO_HANDED
            | WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL => candidates
                .iter()
                .filter(|c| c.weapon.slot() == slot)
                .min_by_key(|c| (c.rank, c.item))
                .map_or(SlotContent::Empty, |c| SlotContent::Weapon {
                    item: c.item,
                    weapon: c.weapon,
                }),
            _ => SlotContent::Game,
        },
    });
    let mut projection = Projection {
        slots,
        selected: None,
    };
    projection.selected = loadout
        .primary()
        .and_then(|hand| loadout.held(hand))
        .and_then(|item| projection.slot_of(item.id));
    projection
}

/// What a store or a cache writes: the store projection, and every weapon on the body for the
/// sidecar.
pub fn project_store(loadout: &Loadout) -> StoreProjection {
    let projection = project(loadout);
    let sidecar = loadout
        .items()
        .filter_map(|item| {
            let weapon = item.kind.weapon()?;
            let place = match item.place {
                Place::Hand(hand) => BodyPlace::Hand(hand),
                Place::Socket(socket) => BodyPlace::Socket(socket),
                Place::World(_) => return None,
            };
            Some(SidecarEntry {
                item: item.id,
                weapon,
                place,
                slot: projection.slot_of(item.id),
            })
        })
        .collect();
    StoreProjection {
        projection,
        sidecar,
    }
}

/// A weapon on the body that may go into its slot, and how strongly.
struct Candidate {
    item: ItemId,
    weapon: WeaponType,
    /// Lower goes first.
    rank: usize,
}

fn candidates(loadout: &Loadout) -> Vec<Candidate> {
    let primary = loadout.primary();
    loadout
        .items()
        .filter(|item| matches!(item.binding, Binding::Live(_)))
        .filter_map(|item| {
            let weapon = item.kind.weapon()?;
            let rank = match item.place {
                Place::Hand(hand) if Some(hand) == primary => 0,
                Place::Hand(_) => 1,
                Place::Socket(socket) if socket.is_native(weapon.slot()) => 2,
                Place::Socket(socket) => 3 + socket.index(),
                Place::World(_) => return None,
            };
            Some(Candidate {
                item: item.id,
                weapon,
                rank,
            })
        })
        .collect()
}
