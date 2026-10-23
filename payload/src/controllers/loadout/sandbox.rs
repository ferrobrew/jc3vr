//! A table of the debug UI's own, driven by hand with made-up objects.
//!
//! The core is not wired into the game yet: no hook sends it events, and the old weapon handling
//! still runs. Until it is, the debug UI's loadout section drives this sandbox, so the
//! transitions, their refusals, the primary hand, and the projections can be watched as the hooks
//! will produce them. Nothing here reads or writes the game; the objects are counters, never
//! dereferenced.

use std::num::NonZeroUsize;

use jc3gi::{character::inventory::WeaponSlot, equipment::weapon::WeaponClass};
use parking_lot::Mutex;

use crate::controllers::{
    frame::Hand,
    loadout::{
        binding::{Awaiting, Binding, ObjectAddress},
        event::Event,
        item::WeaponType,
        projection::{SlotContent, StoreProjection},
        reset::{CacheKind, SlotObject},
        socket::BodyLayout,
        table::{Loadout, LoadoutSnapshot},
    },
};

/// The sandbox as the debug UI shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct SandboxStatus {
    pub snapshot: LoadoutSnapshot,
    /// The snapshots the table holds.
    pub caches: Vec<CacheKind>,
    /// The last store or cache's slots and sidecar.
    pub last_store: Option<StoreProjection>,
    /// The latest transitions and refusals, the newest last.
    pub log: Vec<String>,
}

pub fn status() -> SandboxStatus {
    let sandbox = SANDBOX.lock();
    let table = &sandbox.table;
    SandboxStatus {
        snapshot: table.snapshot(),
        caches: table.caches_held().collect(),
        last_store: sandbox.stores.last().map(|(_, store)| store.clone()),
        log: sandbox.log.clone(),
    }
}

/// The made-up weapons the sandbox offers, with their labels.
pub fn presets() -> Vec<(&'static str, WeaponType)> {
    let pistol = WeaponClass::E_WEAPON_CLASS_PISTOL as i32;
    let smg = WeaponClass::E_WEAPON_CLASS_SMG as i32;
    let rifle = WeaponClass::E_WEAPON_CLASS_ASSAULT_RIFLE as i32;
    let sniper = WeaponClass::E_WEAPON_CLASS_SNIPER as i32;
    let rpg = WeaponClass::E_WEAPON_CLASS_RPG as i32;
    [
        ("Pistol", 0x5100, WeaponSlot::E_SLOT_DUAL_WIELD, pistol),
        ("Submachine gun", 0x5200, WeaponSlot::E_SLOT_DUAL_WIELD, smg),
        (
            "Assault rifle",
            0x5300,
            WeaponSlot::E_SLOT_TWO_HANDED,
            rifle,
        ),
        (
            "Sniper rifle",
            0x5400,
            WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL,
            sniper,
        ),
        ("Launcher", 0x5500, WeaponSlot::E_SLOT_HEAVY, rpg),
    ]
    .into_iter()
    .filter_map(|(label, tag, slot, class)| {
        WeaponType::new(tag, slot, class)
            .ok()
            .map(|weapon| (label, weapon))
    })
    .collect()
}

/// A new made-up object.
pub fn object() -> ObjectAddress {
    let mut sandbox = SANDBOX.lock();
    let object = ObjectAddress::new(sandbox.next_object);
    sandbox.next_object = sandbox
        .next_object
        .checked_add(OBJECT_STRIDE)
        .unwrap_or(FIRST_OBJECT);
    object
}

/// The made-up object of the item on the snapshot's `index`th row, for an event about it.
pub fn object_of(index: usize) -> Option<ObjectAddress> {
    status()
        .snapshot
        .items
        .get(index)
        .and_then(|item| item.binding.address())
}

/// Fold `event` into the sandbox and log what it did.
pub fn send(event: Event) {
    let mut sandbox = SANDBOX.lock();
    let cache = match event {
        Event::Cached(kind) => Some(kind),
        _ => None,
    };
    let folded = sandbox.table.fold(event);
    match folded {
        Ok(folded) => {
            let lines: Vec<String> = folded.transitions.iter().map(ToString::to_string).collect();
            if lines.is_empty() {
                sandbox.push("loadout: nothing moved".to_owned());
            }
            for line in lines {
                sandbox.push(line);
            }
            if let Some(store) = folded.store {
                sandbox.stores.retain(|(kind, _)| *kind != cache);
                sandbox.stores.push((cache, store));
            }
        }
        Err(error) => sandbox.push(format!("refused: {error}")),
    }
}

/// What the game's slots hold after a spawn reset: a new object of each waiting item's type.
pub fn slots_after_reset() -> Vec<SlotObject> {
    let items = status().snapshot.items;
    items
        .iter()
        .filter_map(|item| match (item.binding, item.kind.weapon()) {
            (Binding::Awaiting(Awaiting::Slot(_)), Some(weapon)) => Some(weapon),
            _ => None,
        })
        .map(|weapon| SlotObject {
            object: object(),
            weapon,
        })
        .collect()
}

/// What the game's slots hold after it restores `cache`: a new object of each weapon the cache's
/// store put in a slot.
pub fn slots_for_restore(cache: CacheKind) -> Vec<SlotObject> {
    let weapons: Vec<WeaponType> = SANDBOX
        .lock()
        .stores
        .iter()
        .filter(|(kind, _)| *kind == Some(cache))
        .flat_map(|(_, store)| store.projection.slots)
        .filter_map(|fill| match fill.content {
            SlotContent::Weapon { weapon, .. } => Some(weapon),
            SlotContent::Empty | SlotContent::Game => None,
        })
        .collect();
    weapons
        .into_iter()
        .map(|weapon| SlotObject {
            object: object(),
            weapon,
        })
        .collect()
}

/// Change the sandbox's body, keeping its items.
pub fn set_body(body: BodyLayout) {
    SANDBOX.lock().table.set_body(body);
}

/// Start the sandbox again, with an empty table for `body`.
pub fn clear(body: BodyLayout) {
    let mut sandbox = SANDBOX.lock();
    sandbox.table = Loadout::new(body);
    sandbox.stores.clear();
    sandbox.log.clear();
}

/// How many log lines the sandbox keeps.
const LOG_LINES: usize = 24;
const FIRST_OBJECT: NonZeroUsize = NonZeroUsize::new(0x1000).unwrap();
const OBJECT_STRIDE: usize = 0x10;

struct Sandbox {
    table: Loadout,
    /// The last store's projection, and each cache's, by the cache it was for.
    stores: Vec<(Option<CacheKind>, StoreProjection)>,
    log: Vec<String>,
    next_object: NonZeroUsize,
}

impl Sandbox {
    fn push(&mut self, line: String) {
        self.log.push(line);
        if self.log.len() > LOG_LINES {
            let excess = self.log.len() - LOG_LINES;
            self.log.drain(..excess);
        }
    }
}

static SANDBOX: Mutex<Sandbox> = Mutex::new(Sandbox {
    table: Loadout::new(BodyLayout::full(Hand::Right)),
    stores: Vec::new(),
    log: Vec::new(),
    next_object: FIRST_OBJECT,
});
