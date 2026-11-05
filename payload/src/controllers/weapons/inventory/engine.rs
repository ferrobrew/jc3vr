//! The VR inventory's engine reads: a weapon's kind and magazine, what became of a weapon outside
//! the slots, and the strong and weak references the inventory keeps weapons by.
//!
//! Everything here runs in the game stage, on the game thread before the world update, unless a
//! function says otherwise.

use std::ptr::NonNull;

use glam::Vec3;
use jc3gi::{
    character::{
        character::Character,
        inventory::{Inventory, WeaponSlot},
    },
    equipment::weapon::{WeaponBase, WeaponState},
    spawn_system::SpawnSystem,
    types::shared_ptr::SharedPtr,
};

use crate::controllers::{
    loadout::{ObjectAddress, Restored, Socket, WeaponType},
    weapons::{
        engine::{character_object, gun_position, object_of, slot_of, weapon_at},
        hold::refs::{Strong, Weak},
    },
};

/// A live weapon object's address, the key the inventory's stores keep it by.
pub type ObjectKey = usize;

/// What kind of weapon a weapon is: its spawn tag's hash, the form the save keeps a weapon's type
/// in, and its inventory slot.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct WeaponKind {
    pub tag_hash: u32,
    pub slot: WeaponSlot,
}

/// A weapon seen in an inventory slot or in the mod's keeping.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Seen {
    pub object: ObjectKey,
    pub kind: WeaponKind,
    pub magazine: i32,
}

/// What became of a weapon that is in no slot and not carried.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Fate {
    /// It lies in the world (or anywhere else outside a character), at this distance from the
    /// player when it could be read.
    Lying { distance_m: Option<f32> },
    /// Another character holds it, at this distance from the player when it could be read.
    HeldByOther { distance_m: Option<f32> },
    /// The object is gone.
    Gone,
}

/// The slots whose weapons the inventory tracks and carries: the pistols, the two-handed weapons,
/// the special weapons, and the heavy weapons, which it keeps beyond the slots, docks on the back,
/// and arms in a hand, never wielding them from their slot, whose unequip drops them. The grenade, the
/// planted explosive, and the beacon are in use while holstered, and the sun gun drops itself, so
/// the game keeps those.
pub(crate) const TRACKED_SLOTS: [WeaponSlot; 4] = [
    WeaponSlot::E_SLOT_DUAL_WIELD,
    WeaponSlot::E_SLOT_TWO_HANDED,
    WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL,
    WeaponSlot::E_SLOT_HEAVY,
];

/// Whether the inventory tracks `slot`'s weapons now.
pub(crate) fn tracks(slot: WeaponSlot) -> bool {
    TRACKED_SLOTS.contains(&slot)
}

/// The slots the inventory tracks now.
pub(crate) fn tracked_slots() -> impl Iterator<Item = WeaponSlot> {
    TRACKED_SLOTS.into_iter().filter(|&slot| tracks(slot))
}

/// The original native slot pointer matching an identity, without a pointee borrow.
/// The caller must retain the slot source and serialize native access while using the handle.
pub(crate) fn slot_handle(inventory: &Inventory, object: ObjectKey) -> Option<NonNull<WeaponBase>> {
    inventory
        .m_Weapons
        .iter()
        .find(|slot| slot.px as usize == object)
        .and_then(|slot| NonNull::new(slot.px))
}

/// The weapon `strong` refers to, which it keeps alive.
pub(crate) fn weapon_of(strong: &Strong) -> *mut WeaponBase {
    weapon_at(strong.object())
}

/// A weak reference to `weapon`.
///
/// # Safety
///
/// `weapon` must be live.
pub(crate) unsafe fn weak_to(weapon: &WeaponBase) -> Weak {
    unsafe { Weak::of(&weapon.m_WeakThis) }
}

/// A strong reference to `weapon`, while it lives.
///
/// # Safety
///
/// `weapon` must be live.
pub(crate) unsafe fn strong_to(weapon: &WeaponBase) -> Option<Strong> {
    unsafe { Strong::lock(&weapon.m_WeakThis) }
}

/// A non-owning `boost::shared_ptr<CWeaponBase>` to the weapon `strong` keeps alive, for the calls
/// that take one by reference and copy it; valid while `strong` is held.
pub(crate) fn shared_view(strong: &Strong) -> SharedPtr<WeaponBase> {
    SharedPtr {
        px: weapon_of(strong),
        pn: strong.control_block(),
    }
}

/// `weapon`'s kind: its spawn tag's hash (zero without the spawn system) and its slot.
///
/// # Safety
///
/// `weapon` must be live.
pub(crate) unsafe fn kind_of(weapon: &WeaponBase) -> Option<WeaponKind> {
    let slot = unsafe { slot_of(weapon) }?;
    // SAFETY: a live weapon's `CGameObject` subobject is live.
    let index = unsafe { (*object_of(weapon)).GetUniqueSpawnTagIndex() };
    let tag_hash = unsafe { SpawnSystem::get() }
        .map_or(0, |system| unsafe { system.GetTagHashFromTagIndex(index) });
    Some(WeaponKind { tag_hash, slot })
}

/// `weapon`'s address, as the loadout binds an item to it.
pub(crate) fn object_address(weapon: &WeaponBase) -> Option<ObjectAddress> {
    ObjectAddress::from_address(weapon as *const WeaponBase as usize)
}

/// The restoration of `weapon` onto `socket`, as the table admits it: `None` for a weapon whose
/// address or type the loadout cannot hold.
///
/// # Safety
///
/// `weapon` must be live.
pub(crate) unsafe fn restored(weapon: &WeaponBase, socket: Socket) -> Option<Restored> {
    Some(Restored {
        object: object_address(weapon)?,
        weapon: unsafe { weapon_type(weapon) }?,
        socket,
    })
}

/// `weapon`'s type as the loadout knows it: its spawn tag's hash, its slot, and its size class. `None`
/// for a weapon of a slot whose items a hand does not hold.
///
/// # Safety
///
/// `weapon` must be live.
pub(crate) unsafe fn weapon_type(weapon: &WeaponBase) -> Option<WeaponType> {
    let kind = unsafe { kind_of(weapon) }?;
    // Read as the raw value: an unset or unknown class is out of the enum's range.
    let class = unsafe { (&raw const weapon.m_Class).cast::<i32>().read() };
    WeaponType::new(kind.tag_hash, kind.slot, class).ok()
}

/// The weapons in `owner`'s tracked slots.
///
/// # Safety
///
/// `owner` must be the live local player character.
pub(crate) unsafe fn slots(owner: &Character) -> Vec<(Seen, &'static WeaponBase)> {
    tracked_slots()
        .filter_map(|slot| {
            let weapon = unsafe { owner.m_Inventory.m_Weapons.get(slot as usize)?.px.as_ref() }?;
            // A mounted gun in the heavy slot is the game's while the player mans it.
            if weapon.m_State == WeaponState::E_WS_MOUNTED {
                return None;
            }
            let kind = unsafe { kind_of(weapon) }?;
            Some((
                Seen {
                    object: weapon as *const WeaponBase as ObjectKey,
                    kind,
                    magazine: weapon.m_MagazineAmmo,
                },
                weapon,
            ))
        })
        .collect()
}

/// What became of the weapon `weak` refers to, seen from `owner` at `player`: gone, held by
/// another character, or lying anywhere else.
///
/// # Safety
///
/// `owner` must be the live local player character.
pub(crate) unsafe fn fate(weak: &Weak, owner: &Character, player: Vec3) -> Fate {
    let Some(strong) = weak.lock() else {
        return Fate::Gone;
    };
    // SAFETY: the strong reference keeps the weapon alive while it is read.
    let weapon = unsafe { &*weapon_of(&strong) };
    let holder = weapon.m_Holder.px as usize;
    let distance_m = unsafe { gun_position(weapon) }.map(|at| at.distance(player));
    if weapon.m_State != WeaponState::E_WS_DROPPED
        && holder != 0
        && holder != character_object(owner) as usize
    {
        return Fate::HeldByOther { distance_m };
    }
    Fate::Lying { distance_m }
}
