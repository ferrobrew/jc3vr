//! The weapon handling's engine reads and calls: the local player's wielded weapon, the foregrip
//! geometry measured from the gun's placement, and the holster call. Everything here runs in the
//! game stage ([`crate::controllers::bindings::stage`]), on the game thread before the world
//! update.

use glam::{Mat4, Quat, Vec3};
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::{AmmunitionType, WeaponBase, WeaponGripHand, WeaponState},
    game::GameObject,
    types::math::Matrix4,
};

use crate::controllers::{
    aim::{gun, ray, sights},
    config::AimConfig,
    frame::{Hand, Pose},
    hands::{ik::TickTarget, late, pose as hand_pose},
    loadout::SLOTS,
    weapons::{
        foregrip::{FOREGRIP, GripGeometry},
        heft::HeftClass,
    },
};

/// A weapon in the hands: its slot and the hands holding one of its guns.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Wielded {
    pub slot: WeaponSlot,
    /// Which hands hold one of its guns, indexed by [`Hand::index`].
    pub hands: [bool; 2],
}

/// A weapon of the local player's in the hands: the wielded one, or one armed in a hand.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HeldWeapon {
    pub address: usize,
    pub wielded: Wielded,
    /// The hand its first gun rides.
    pub holder: Option<Hand>,
    /// Whether it is held in both hands' reach: a two-handed, special, or heavy weapon, wielded
    /// rather than mounted, or armed in a hand.
    pub two_handed: bool,
    /// The weight class its one-handed tuning comes from.
    pub class: HeftClass,
}

/// The gun in each hand, by [`Hand::index`]: the wielded weapon in the hand its first gun rides,
/// and the weapon armed in a hand (`armed`, by [`Hand::index`]) in its own hand. A hand holds one
/// gun at most; the wielded weapon's second gun (the game's pistol pair) is not a gun of the hand
/// it rides.
pub fn in_hands(
    wielded: Option<HeldWeapon>,
    armed: [Option<HeldWeapon>; 2],
) -> [Option<HeldWeapon>; 2] {
    Hand::BOTH.map(|hand| match wielded {
        Some(w) if w.holder == Some(hand) => Some(w),
        _ => armed[hand.index()]
            .filter(|a| wielded.is_none_or(|w| w.address != a.address))
            .filter(|a| a.holder == Some(hand)),
    })
}

/// The gun held in one hand while the other is free, of the gun in each hand (`guns`) and whether
/// each hand is free (`free`, holding no gun or other item), both by [`Hand::index`]: the gun the
/// free hand may take over, touch, or, for a long gun, brace. With a gun in each hand neither hand
/// is free, so there is one at most.
pub fn lone_gun(guns: [Option<HeldWeapon>; 2], free: [bool; 2]) -> Option<HeldWeapon> {
    Hand::BOTH
        .into_iter()
        .find_map(|hand| guns[hand.index()].filter(|_| free[hand.other().index()]))
}

/// Whether a weapon of `slot` is a long gun, held in both hands' reach and by its stock and
/// handguard as well as its grip: the two-handed, the special, and the heavy slots.
pub fn long_slot(slot: WeaponSlot) -> bool {
    matches!(
        slot,
        WeaponSlot::E_SLOT_TWO_HANDED
            | WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL
            | WeaponSlot::E_SLOT_HEAVY
    )
}

/// Whether a weapon of `slot` is a gun a hand takes from a character or a rack: a pistol, a
/// two-handed, or a special weapon. A heavy weapon is held at a mount or by the game's own carry,
/// and the grenade, the planted explosive, and the beacon are not guns.
pub(crate) fn takeable_gun_slot(slot: WeaponSlot) -> bool {
    matches!(
        slot,
        WeaponSlot::E_SLOT_DUAL_WIELD
            | WeaponSlot::E_SLOT_TWO_HANDED
            | WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL
    )
}

/// The IK weight at which a hand's target counts as reached in full, rather than blended toward.
pub(crate) const FULL_WEIGHT: f32 = 0.999;

/// `weapon`'s slot, read as the raw value: an unset slot is out of the enum's range.
///
/// # Safety
///
/// `weapon` must be live.
pub(crate) unsafe fn slot_of(weapon: &WeaponBase) -> Option<WeaponSlot> {
    slot_from_raw(unsafe { (&raw const weapon.m_EquipSlot).cast::<i32>().read() })
}

/// Whether `weapon` is a hand grenade.
///
/// # Safety
///
/// `weapon` must be live.
pub(crate) unsafe fn is_hand_grenade(weapon: &WeaponBase) -> bool {
    (unsafe { slot_of(weapon) }) == Some(WeaponSlot::E_SLOT_HAND_GRENADE)
}

/// `weapon`'s `CGameObject` subobject, the object that references, holders, and spawn calls name
/// it by.
pub(crate) fn object_of(weapon: *const WeaponBase) -> *mut GameObject {
    weapon
        .cast::<u8>()
        .wrapping_add(WeaponBase::GAME_OBJECT_BASE_OFFSET as usize)
        .cast::<GameObject>()
        .cast_mut()
}

/// The weapon whose `CGameObject` subobject is `object`, or null for a null object. The caller
/// establishes that the object is a weapon.
pub(crate) fn weapon_at(object: *const GameObject) -> *mut WeaponBase {
    if object.is_null() {
        return std::ptr::null_mut();
    }
    object
        .cast::<u8>()
        .wrapping_sub(WeaponBase::GAME_OBJECT_BASE_OFFSET as usize)
        .cast::<WeaponBase>()
        .cast_mut()
}

/// `character`'s `CGameObject` subobject, as a weapon's holder names it and the pickups take it
/// as their activator.
pub(crate) fn character_object(character: *const Character) -> *mut GameObject {
    character
        .cast::<u8>()
        .wrapping_add(Character::GAME_OBJECT_BASE_OFFSET as usize)
        .cast::<GameObject>()
        .cast_mut()
}

/// `character`'s world transform, as its last simulation tick placed it.
pub(crate) fn character_frame(character: &Character) -> Mat4 {
    Mat4::from(character.m_WorldMatrixT1)
}

/// `character`'s world position: the translation of its world transform ([`character_frame`]).
pub(crate) fn character_position(character: &Character) -> Vec3 {
    character_frame(character).w_axis.truncate()
}

/// The world pose of `weapon`'s first gun, without its scale.
///
/// # Safety
///
/// `weapon` must be live.
pub(crate) unsafe fn gun_pose(weapon: &WeaponBase) -> Option<Pose> {
    let first = unsafe { gun::guns(weapon) }.next().flatten()?;
    let (_, orientation, position) =
        Mat4::from(first.m_WorldMatrixT1).to_scale_rotation_translation();
    (position.is_finite() && orientation.is_finite()).then(|| Pose {
        position,
        orientation: orientation.normalize(),
    })
}

/// The world position of `weapon`'s first gun. Unlike [`gun_pose`], it needs no orientation, so a
/// gun whose transform does not decompose still has one.
///
/// # Safety
///
/// `weapon` must be live.
pub(crate) unsafe fn gun_position(weapon: &WeaponBase) -> Option<Vec3> {
    let first = unsafe { gun::guns(weapon) }.next().flatten()?;
    let at = Mat4::from(first.m_WorldMatrixT1).w_axis.truncate();
    at.is_finite().then_some(at)
}

/// `weapon`'s ammunition type when a reserve counts it: read as the raw value, so an unset or
/// unknown type is `None`, as is the unlimited type, whose rounds are never counted.
///
/// # Safety
///
/// `weapon` must be live.
pub(crate) unsafe fn limited_ammo(weapon: &WeaponBase) -> Option<AmmunitionType> {
    let raw = unsafe { (&raw const weapon.m_AmmunitionType).cast::<i32>().read() };
    ((0..AmmunitionType::NOF_AMMO_TYPES as i32).contains(&raw)
        && raw != AmmunitionType::AMMO_UNLIMITED as i32)
        .then_some(weapon.m_AmmunitionType)
}

/// The reserve of `weapon`'s ammunition in its user's inventory: zero without a user, or for a type
/// out of the inventory's range.
///
/// # Safety
///
/// `weapon` must be live, on the game thread before the world update.
pub(crate) unsafe fn reserve(weapon: &WeaponBase) -> i32 {
    let raw = unsafe { (&raw const weapon.m_AmmunitionType).cast::<i32>().read() };
    if !(0..AmmunitionType::NOF_AMMO_TYPES as i32).contains(&raw) {
        return 0;
    }
    // `m_User` is a `weak_ptr<CCharacter>`; its first word is the character.
    let Some(user) = (unsafe { (weapon.m_User[0] as *const Character).as_ref() }) else {
        return 0;
    };
    unsafe { user.m_Inventory.GetAmmoCount(weapon.m_AmmunitionType) }
}

/// Add `rounds` of `weapon`'s ammunition to `owner`'s reserve, up to its cap, as the game's pickup
/// does ([`Inventory::AddAmmo`](jc3gi::character::inventory::Inventory::AddAmmo)); nothing for no rounds or a type no reserve counts. Unlike a write
/// of the reserve
/// ([`Inventory::SetAmmoCount`](jc3gi::character::inventory::Inventory::SetAmmoCount)), it starts no reload of an empty wielded weapon and
/// pushes nothing to the game's weapon readout.
///
/// # Safety
///
/// `owner` must be the live local player character and `weapon` live, on the game thread before
/// the world update.
pub(crate) unsafe fn credit_reserve(owner: &mut Character, weapon: &WeaponBase, rounds: i32) {
    if rounds <= 0 {
        return;
    }
    if let Some(ammo) = unsafe { limited_ammo(weapon) } {
        unsafe { owner.m_Inventory.AddAmmo(ammo, rounds) };
    }
}

/// Move `weapon`'s stored rounds to `owner`'s reserve ([`credit_reserve`]) and clear them, as the
/// game's pickup does.
///
/// # Safety
///
/// As [`credit_reserve`].
pub(crate) unsafe fn credit_stored(owner: &mut Character, weapon: &mut WeaponBase) {
    let stored = std::mem::take(&mut weapon.m_StoredNonMagazineAmmo);
    unsafe { credit_reserve(owner, weapon, stored) };
}

/// The local player character.
///
/// # Safety
///
/// Game thread, before the world update.
pub unsafe fn local_player() -> Option<&'static mut Character> {
    unsafe { Character::GetLocalPlayerCharacter().as_mut() }
}

/// The weapon `owner` wields in the hands.
///
/// # Safety
///
/// `owner` must be the live local player character, on the game thread before the world update.
pub unsafe fn held_weapon(owner: &Character) -> Option<HeldWeapon> {
    let weapon = unsafe { gun::held_weapon(owner) }?;
    let slot = unsafe { (&raw const owner.m_SelectedWeaponSlot).cast::<i32>().read() };
    let slot = slot_from_raw(slot)?;
    let mut hands = [false; 2];
    let mut holder = None;
    for gun in unsafe { gun::guns(weapon) }.flatten() {
        let Some(hand) = gun
            .m_lBoneAttachementPerWeaponState
            .get(weapon.m_State as usize)
            .and_then(|&bone| late::attach_hand(bone))
        else {
            continue;
        };
        hands[hand.index()] = true;
        holder.get_or_insert(hand);
    }
    let two_handed = weapon.m_State == WeaponState::E_WS_WIELDING && long_slot(slot);
    // Read as the raw value: an unset or unknown class is out of the enum's range.
    let class = unsafe { (&raw const weapon.m_Class).cast::<i32>().read() };
    Some(HeldWeapon {
        address: weapon as *const WeaponBase as usize,
        wielded: Wielded { slot, hands },
        holder,
        two_handed,
        class: HeftClass::of(class, two_handed),
    })
}

/// Measure a two-handed weapon's geometry in its holding wrist's frame: the gun's last placement
/// followed the wrist target `target` exactly, so the gun's transform relative to that target is
/// its transform relative to the wrist. `None` unless the target was reached at full weight.
///
/// # Safety
///
/// `weapon` must be the live weapon [`held_weapon`] returned, read on the game thread before the
/// world update.
pub unsafe fn measure_geometry(
    weapon: &HeldWeapon,
    target: TickTarget,
    aim: &AimConfig,
    world_scale: f32,
) -> Option<GripGeometry> {
    let holder = weapon.holder?;
    if target.weight < FULL_WEIGHT || world_scale.is_nan() || world_scale <= 0.0 {
        return None;
    }
    let base = unsafe { (weapon.address as *const WeaponBase).as_ref() }?;
    let first = unsafe { gun::guns(base) }.next().flatten()?;
    let sight = sights::resolve(&aim.sights, base.m_WeaponNameHash).sight;
    let fire_point = unsafe { gun::fire_point(first) };
    let barrel_world = ray::sight_ray(
        Mat4::from(first.m_WorldMatrixT1),
        fire_point,
        sight.as_ref(),
    )?;
    let to_wrist = hand_pose::inverse(target.wrist);
    let barrel = (to_wrist.orientation * barrel_world.direction).try_normalize()?;
    let sight = (to_wrist.position + to_wrist.orientation * barrel_world.origin) / world_scale;
    let muzzle_world = (Mat4::from(first.m_WorldMatrixT1) * fire_point)
        .w_axis
        .truncate();
    let muzzle = (to_wrist.position + to_wrist.orientation * muzzle_world) / world_scale;

    let grip_hand = match holder.other() {
        Hand::Left => WeaponGripHand::E_WEAPON_GRIP_HAND_LEFT,
        Hand::Right => WeaponGripHand::E_WEAPON_GRIP_HAND_RIGHT,
    };
    let mut grip = Matrix4::from(Mat4::IDENTITY);
    let native = unsafe { base.GetGripPosition(grip_hand, &mut grip) };
    let foregrip = if native {
        let world = Mat4::from(grip).w_axis.truncate();
        let local = hand_pose::mul(
            to_wrist,
            Pose {
                position: world,
                orientation: Quat::IDENTITY,
            },
        );
        local.position / world_scale
    } else {
        barrel * FOREGRIP.fallback_distance_m
    };
    let (_, gun_rotation, gun_position) =
        Mat4::from(first.m_WorldMatrixT1).to_scale_rotation_translation();
    let gun = hand_pose::mul(
        to_wrist,
        Pose {
            position: gun_position,
            orientation: gun_rotation.normalize(),
        },
    );
    let gun = Pose {
        position: gun.position / world_scale,
        orientation: gun.orientation,
    };
    let finite = foregrip.is_finite()
        && sight.is_finite()
        && muzzle.is_finite()
        && gun.position.is_finite()
        && gun.orientation.is_finite();
    finite.then_some(GripGeometry {
        foregrip,
        barrel,
        sight,
        muzzle,
        native,
        gun,
    })
}

/// Holster the wielded weapon, as the game's own holster input does.
///
/// # Safety
///
/// `owner` must be the live local player character, on the game thread before the world update.
pub unsafe fn holster(owner: &mut Character) {
    unsafe { owner.HolsterWeapon() };
}

/// The inventory slot of a raw slot value, or `None` for one out of the enum's range.
pub(crate) fn slot_from_raw(slot: i32) -> Option<WeaponSlot> {
    SLOTS.into_iter().find(|&s| s as i32 == slot)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gun(address: usize, slot: WeaponSlot, hand: Hand) -> HeldWeapon {
        let two_handed = long_slot(slot);
        HeldWeapon {
            address,
            wielded: Wielded {
                slot,
                hands: Hand::BOTH.map(|h| h == hand),
            },
            holder: Some(hand),
            two_handed,
            class: HeftClass::of(0, two_handed),
        }
    }

    #[test]
    fn each_hand_has_its_own_gun_whichever_the_game_wields() {
        let pistol = gun(0x10, WeaponSlot::E_SLOT_DUAL_WIELD, Hand::Right);
        let rifle = gun(0x20, WeaponSlot::E_SLOT_TWO_HANDED, Hand::Left);
        let mut armed = [None; 2];
        armed[Hand::Left.index()] = Some(rifle);
        let hands = in_hands(Some(pistol), armed);
        assert_eq!(hands[Hand::Right.index()], Some(pistol));
        assert_eq!(hands[Hand::Left.index()], Some(rifle));
        // Mirrored: the long gun wielded in the left hand, a pistol armed in the right.
        let mut armed = [None; 2];
        armed[Hand::Right.index()] = Some(gun(0x10, WeaponSlot::E_SLOT_DUAL_WIELD, Hand::Right));
        let left_rifle = gun(0x20, WeaponSlot::E_SLOT_TWO_HANDED, Hand::Left);
        let hands = in_hands(Some(left_rifle), armed);
        assert_eq!(hands[Hand::Left.index()], Some(left_rifle));
        assert_eq!(hands[Hand::Right.index()].map(|w| w.address), Some(0x10));
        // A gun armed in one hand alone is that hand's.
        let mut armed = [None; 2];
        armed[Hand::Left.index()] = Some(rifle);
        assert_eq!(in_hands(None, armed), armed);
    }

    #[test]
    fn the_pair_is_a_gun_of_its_first_hand_alone() {
        let mut pair = gun(0x10, WeaponSlot::E_SLOT_DUAL_WIELD, Hand::Right);
        pair.wielded.hands = [true; 2];
        assert_eq!(in_hands(Some(pair), [None; 2]), [None, Some(pair)]);
    }

    #[test]
    fn the_wielded_weapon_is_never_also_an_armed_one() {
        let rifle = gun(0x20, WeaponSlot::E_SLOT_TWO_HANDED, Hand::Right);
        // A weapon armed in the hand the wielded weapon left, being disarmed this tick.
        let mut armed = [None; 2];
        armed[Hand::Left.index()] = Some(gun(0x20, WeaponSlot::E_SLOT_TWO_HANDED, Hand::Left));
        assert_eq!(in_hands(Some(rifle), armed), [None, Some(rifle)]);
    }

    #[test]
    fn the_gun_with_the_other_hand_free_is_the_lone_one() {
        let (left, right) = (Hand::Left.index(), Hand::Right.index());
        let rifle = gun(0x20, WeaponSlot::E_SLOT_TWO_HANDED, Hand::Left);
        let pistol = gun(0x10, WeaponSlot::E_SLOT_DUAL_WIELD, Hand::Right);
        let mut guns = [None; 2];
        guns[left] = Some(rifle);
        // Armed or wielded alike: a gun with the other hand free is the lone gun.
        assert_eq!(lone_gun(guns, [false, true]), Some(rifle));
        assert_eq!(lone_gun([None, Some(pistol)], [true, false]), Some(pistol));
        // With the other hand holding anything, there is none.
        assert_eq!(lone_gun(guns, [false; 2]), None);
        guns[right] = Some(pistol);
        assert_eq!(lone_gun(guns, [false; 2]), None);
        // Two long guns leave neither hand free.
        guns[right] = Some(gun(
            0x30,
            WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL,
            Hand::Right,
        ));
        assert_eq!(lone_gun(guns, [false; 2]), None);
        assert_eq!(lone_gun([None; 2], [true; 2]), None);
    }

    #[test]
    fn only_the_long_slots_are_long_guns() {
        assert!(long_slot(WeaponSlot::E_SLOT_TWO_HANDED));
        assert!(long_slot(WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL));
        assert!(long_slot(WeaponSlot::E_SLOT_HEAVY));
        assert!(!long_slot(WeaponSlot::E_SLOT_DUAL_WIELD));
        assert!(!long_slot(WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE));
    }

    #[test]
    fn a_hand_takes_pistols_and_long_guns_but_not_heavy_weapons() {
        assert!(takeable_gun_slot(WeaponSlot::E_SLOT_DUAL_WIELD));
        assert!(takeable_gun_slot(WeaponSlot::E_SLOT_TWO_HANDED));
        assert!(takeable_gun_slot(WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL));
        assert!(!takeable_gun_slot(WeaponSlot::E_SLOT_HEAVY));
        assert!(!takeable_gun_slot(WeaponSlot::E_SLOT_HAND_GRENADE));
        assert!(!takeable_gun_slot(WeaponSlot::E_SLOT_BEACON));
    }

    #[test]
    fn every_slot_reads_back_from_its_raw_value() {
        for slot in SLOTS {
            assert_eq!(slot_from_raw(slot as i32), Some(slot));
        }
        assert_eq!(slot_from_raw(-1), None);
        assert_eq!(slot_from_raw(WeaponSlot::E_NONE as i32), None);
    }

    #[test]
    fn the_object_casts_round_trip_and_keep_null() {
        let weapon = 0x1000 as *const WeaponBase;
        assert_eq!(weapon_at(object_of(weapon)).cast_const(), weapon);
        assert!(weapon_at(std::ptr::null()).is_null());
    }
}
