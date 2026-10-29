//! The game's own inventory as the readouts show it: each slot's weapon with its ammunition, the
//! selected slot, the wielded weapon, the pistol slot's pair or single pistol, and the grenade and
//! explosive stock. Captured on the input tick and published with the inventory's readout, so the
//! debug UI never reads the game's memory.
//!
//! A weapon's display name is looked up on the same tick ([`weapon_name`]), on the hypothesis that
//! the HUD names a weapon by passing [`WeaponBase::m_WeaponNameHash`] to the localisation store as
//! a key hash, as [`HUDUI::OnPickupWeapon`](jc3gi::ui::hud_ui::HUDUI::OnPickupWeapon) takes its name
//! from that field. The hypothesis is unverified in-game; a hash the store does not have shows as
//! the bare hash.

use std::collections::BTreeMap;

use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::{AmmunitionType, WeaponBase, WeaponState},
    ui::locator::Locale,
};
use parking_lot::Mutex;
use smol_str::SmolStr;

use crate::controllers::{
    loadout::ObjectAddress,
    summary::localise::{current_locale, game_string_by_hash, renders},
    weapons::{
        dual_wield::single, engine as weapons_engine, engine::slot_from_raw, inventory::traversal,
    },
};

/// The game's inventory as of one input tick.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GameInventory {
    /// Each slot, in the slots' index order.
    pub slots: [GameSlot; SLOT_COUNT],
    /// The character's selected slot.
    pub selected: Option<WeaponSlot>,
    /// The weapon wielded in the hands.
    pub wielded: Option<ObjectAddress>,
    /// Whether the inventory's weapon updates skip the wielded weapon now
    /// ([`traversal::wield_skipped`]), so that the hands' weapons are armed rather than wielded.
    pub wield_skipped: bool,
    pub pistols: Pistols,
    /// The reserves of hand grenades and planted explosives.
    pub grenades: i32,
    pub explosives: i32,
}

/// One of the game's slots.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GameSlot {
    pub slot: WeaponSlot,
    pub weapon: Option<GameWeapon>,
    /// Whether a spawn of the slot's weapon is on its way.
    pub pending_spawn: bool,
}

/// A weapon in one of the game's slots.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GameWeapon {
    pub object: ObjectAddress,
    /// The weapon type's name hash.
    pub name_hash: u32,
    /// The weapon type's display name in the game's language ([`weapon_name`]), or `None` when the
    /// lookup finds none or the debug UI's fonts cannot draw the language.
    pub name: Option<SmolStr>,
    /// The weapon class and the weapon state, read as raw values: an unset one is out of the
    /// enum's range.
    pub class: i32,
    pub state: i32,
    pub magazine: i32,
    /// The holder's reserve of the weapon's ammunition type, for a type a reserve holds.
    pub reserve: Option<i32>,
}

impl GameWeapon {
    /// Whether the player mans the weapon as a mounted gun, which the game keeps in the heavy slot
    /// meanwhile.
    pub fn mounted(&self) -> bool {
        self.state == WeaponState::E_WS_MOUNTED as i32
    }
}

/// What the pistol slot holds.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Pistols {
    Empty,
    /// The game's pair: one weapon with a gun in each hand.
    Pair,
    /// A single pistol ([`single`]).
    Single,
}

/// Read `owner`'s inventory.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn capture(owner: &Character) -> GameInventory {
    let inventory = &owner.m_Inventory;
    let reserve = |raw: i32| {
        usize::try_from(raw)
            .ok()
            .filter(|&index| index < AmmunitionType::NOF_AMMO_TYPES as usize)
            .and_then(|index| inventory.m_Ammunition.get(index).copied())
    };
    let slots = std::array::from_fn(|index| {
        let slot = slot_from_raw(index as i32).unwrap_or(WeaponSlot::E_NONE);
        // SAFETY: a slot's strong reference keeps its weapon alive.
        let weapon = unsafe { inventory.m_Weapons[index].px.as_ref() }.and_then(|weapon| {
            // SAFETY: the weapon is live, as above.
            let raw = |field: *const i32| unsafe { field.read() };
            let ammo = raw((&raw const weapon.m_AmmunitionType).cast());
            Some(GameWeapon {
                object: ObjectAddress::from_address(weapon as *const WeaponBase as usize)?,
                name_hash: weapon.m_WeaponNameHash,
                // SAFETY: the input tick is a game thread past startup.
                name: unsafe { weapon_name(weapon.m_WeaponNameHash) },
                class: raw((&raw const weapon.m_Class).cast()),
                state: raw((&raw const weapon.m_State).cast()),
                magazine: weapon.m_MagazineAmmo,
                reserve: reserve(ammo),
            })
        });
        GameSlot {
            slot,
            weapon,
            pending_spawn: inventory.m_WeaponPendingSpawn[index],
        }
    });
    let pistols = match &slots[WeaponSlot::E_SLOT_DUAL_WIELD as usize].weapon {
        None => Pistols::Empty,
        Some(weapon) if single::is(weapon.object.get()) => Pistols::Single,
        Some(_) => Pistols::Pair,
    };
    // SAFETY: as this function's own contract.
    let selected = unsafe { (&raw const owner.m_SelectedWeaponSlot).cast::<i32>().read() };
    GameInventory {
        slots,
        selected: slot_from_raw(selected),
        // SAFETY: as this function's own contract.
        wielded: unsafe { weapons_engine::held_weapon(owner) }
            .and_then(|held| ObjectAddress::from_address(held.address)),
        // SAFETY: as this function's own contract.
        wield_skipped: unsafe { traversal::wield_skipped(owner) },
        pistols,
        grenades: reserve(AmmunitionType::AMMO_HAND_GRENADE as i32).unwrap_or(0),
        explosives: reserve(AmmunitionType::AMMO_EXPLOSIVES as i32).unwrap_or(0),
    }
}

const SLOT_COUNT: usize = WeaponSlot::E_NUM_WEAPON_SLOTS as usize;

/// The display name for the weapon type whose name hash is `hash`, looked up once per hash and
/// language: the localisation store's string for `hash` used as a key hash. Unverified in-game (see
/// the module docs).
///
/// # Safety
///
/// Call from a game thread past startup.
unsafe fn weapon_name(hash: u32) -> Option<SmolStr> {
    // SAFETY: per the contract.
    let locale = unsafe { current_locale() }?;
    if !renders(locale) {
        return None;
    }
    let mut cache = NAMES.lock();
    if cache.locale != Some(locale) {
        cache.locale = Some(locale);
        cache.names.clear();
    }
    cache
        .names
        .entry(hash)
        // SAFETY: per the contract.
        .or_insert_with(|| unsafe { game_string_by_hash(hash) }.map(SmolStr::from))
        .clone()
}

/// The names [`weapon_name`] has looked up, by name hash, for the language they were looked up in;
/// a change of language empties it. A miss is kept too, so a hash the store lacks is asked once.
struct NameCache {
    locale: Option<Locale>,
    names: BTreeMap<u32, Option<SmolStr>>,
}

static NAMES: Mutex<NameCache> = Mutex::new(NameCache {
    locale: None,
    names: BTreeMap::new(),
});
