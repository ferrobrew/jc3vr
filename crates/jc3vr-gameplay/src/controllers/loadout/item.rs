//! The items the loadout holds: what each is, and the identity it keeps for the session. Pure.
//!
//! An item's identity is the table's own ([`ItemId`]), assigned when the item enters the table and
//! never reused, so it survives the spawn reset's replacement of a slot's weapon object with a new
//! one of the same type: the table re-binds the identity to the new object
//! ([`crate::controllers::loadout::binding`]).

use std::{fmt, num::NonZeroU32};

use jc3gi::{character::inventory::WeaponSlot, equipment::weapon::WeaponClass};

/// The name of an inventory slot, for the readouts and the log.
pub const fn slot_label(slot: WeaponSlot) -> &'static str {
    match slot {
        WeaponSlot::E_SLOT_DUAL_WIELD => "pistols",
        WeaponSlot::E_SLOT_TWO_HANDED => "two-handed",
        WeaponSlot::E_SLOT_HEAVY => "heavy",
        WeaponSlot::E_SLOT_HAND_GRENADE => "grenade",
        WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE => "explosives",
        WeaponSlot::E_SLOT_FOW => "FOW",
        WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL => "special",
        WeaponSlot::E_SLOT_BEACON => "beacon",
        _ => "none",
    }
}

/// An item's identity, assigned by the table and stable for the session.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemId(NonZeroU32);

impl ItemId {
    /// The first identity a table hands out.
    pub(super) const FIRST: ItemId = ItemId(NonZeroU32::MIN);

    /// The identity after this one. Identities count up for the whole session, so a log line never
    /// names two items with one identity; a table that ran out would have taken billions of items.
    pub(super) const fn next(self) -> ItemId {
        match self.0.checked_add(1) {
            Some(next) => ItemId(next),
            None => ItemId::FIRST,
        }
    }
}

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// How big a weapon is, for the body's sockets: a socket takes items of its own class or smaller,
/// in this order.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SizeClass {
    /// The pistols and revolvers.
    Pistol,
    /// The submachine guns.
    Smg,
    /// The two-handed and special weapons.
    LongGun,
    /// The heavy weapons, which go on the back.
    Heavy,
}

impl SizeClass {
    pub const fn label(self) -> &'static str {
        match self {
            SizeClass::Pistol => "pistol",
            SizeClass::Smg => "submachine gun",
            SizeClass::LongGun => "long gun",
            SizeClass::Heavy => "heavy weapon",
        }
    }
}

/// A weapon's type: its spawn tag's hash (the form the save keeps a weapon's type in), its slot,
/// and its size class. Only the weapons a hand holds have one; see [`WeaponType::new`].
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct WeaponType {
    tag_hash: u32,
    slot: WeaponSlot,
    size: SizeClass,
}

impl WeaponType {
    /// The type of a weapon of spawn tag `tag_hash`, in `slot`, of the raw weapon class `class`
    /// (read as the raw value, since an unset class is out of the enum's range). The pistol slot's
    /// submachine guns are the one class that changes the size. The grenade, the planted explosive,
    /// the beacon, and the other consumables' slots are the game's, not weapons a hand holds.
    pub fn new(tag_hash: u32, slot: WeaponSlot, class: i32) -> Result<WeaponType, NotAWeapon> {
        let size = match slot {
            WeaponSlot::E_SLOT_DUAL_WIELD if class == WeaponClass::E_WEAPON_CLASS_SMG as i32 => {
                SizeClass::Smg
            }
            WeaponSlot::E_SLOT_DUAL_WIELD => SizeClass::Pistol,
            WeaponSlot::E_SLOT_TWO_HANDED | WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL => {
                SizeClass::LongGun
            }
            WeaponSlot::E_SLOT_HEAVY => SizeClass::Heavy,
            _ => return Err(NotAWeapon { slot }),
        };
        Ok(WeaponType {
            tag_hash,
            slot,
            size,
        })
    }

    pub const fn tag_hash(self) -> u32 {
        self.tag_hash
    }

    pub const fn slot(self) -> WeaponSlot {
        self.slot
    }

    pub const fn size(self) -> SizeClass {
        self.size
    }
}

impl fmt::Display for WeaponType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a {} of the {} slot ({:#010x})",
            self.size.label(),
            slot_label(self.slot),
            self.tag_hash
        )
    }
}

/// A weapon type was asked for a slot whose items are not weapons a hand holds.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct NotAWeapon {
    pub slot: WeaponSlot,
}

impl fmt::Display for NotAWeapon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "loadout: the {} slot holds no weapon a hand holds",
            slot_label(self.slot)
        )
    }
}

impl std::error::Error for NotAWeapon {}

/// What an item is.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ItemKind {
    /// A weapon, which docks on a socket that fits it and lies in the world when let go of.
    Weapon(WeaponType),
    /// A loose object a hand picked up: no socket takes it, and letting go of it drops it out of
    /// the table.
    Prop,
    /// A hand grenade, while a hand holds it.
    Grenade,
    /// A C4 charge, while a hand holds it.
    Charge,
}

impl ItemKind {
    /// The weapon's type, for a weapon.
    pub const fn weapon(self) -> Option<WeaponType> {
        match self {
            ItemKind::Weapon(weapon) => Some(weapon),
            ItemKind::Prop | ItemKind::Grenade | ItemKind::Charge => None,
        }
    }
}

impl fmt::Display for ItemKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ItemKind::Weapon(weapon) => weapon.fmt(f),
            ItemKind::Prop => f.write_str("a prop"),
            ItemKind::Grenade => f.write_str("a grenade"),
            ItemKind::Charge => f.write_str("a C4 charge"),
        }
    }
}
