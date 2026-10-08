//! The weapon handling's engine reads and calls: the local player's wielded weapon, the foregrip
//! geometry measured from the gun's placement, and the holster call. Everything here runs on the
//! input tick, on the main thread outside the world update.

use glam::{Mat4, Quat};
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::{WeaponBase, WeaponGripHand, WeaponState},
    types::math::Matrix4,
};

use crate::controllers::{
    aim::{gun, ray, sights},
    config::AimConfig,
    frame::{Hand, Pose},
    hands::{ik::TickTarget, late, pose as hand_pose},
    weapons::{config::ForegripConfig, foregrip::GripGeometry, heft::HeftClass, holster::Wielded},
};

/// The local player's wielded weapon.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HeldWeapon {
    pub address: usize,
    pub wielded: Wielded,
    /// The hand its first gun rides.
    pub holder: Option<Hand>,
    /// Whether it is held in both hands' reach: a two-handed, special, or heavy weapon, wielded
    /// rather than mounted.
    pub two_handed: bool,
    /// The weight class its one-handed tuning comes from.
    pub class: HeftClass,
}

/// The local player character.
///
/// # Safety
///
/// Main thread, outside the world update.
pub unsafe fn local_player() -> Option<&'static mut Character> {
    unsafe { Character::GetLocalPlayerCharacter().as_mut() }
}

/// The weapon `owner` wields in the hands.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
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
    let two_handed = weapon.m_State == WeaponState::E_WS_WIELDING
        && matches!(
            slot,
            WeaponSlot::E_SLOT_TWO_HANDED
                | WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL
                | WeaponSlot::E_SLOT_HEAVY
        );
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
/// `weapon` must be the live weapon [`held_weapon`] returned, read on the main thread outside the
/// world update.
pub unsafe fn measure_geometry(
    weapon: &HeldWeapon,
    target: TickTarget,
    aim: &AimConfig,
    foregrip: &ForegripConfig,
    world_scale: f32,
) -> Option<GripGeometry> {
    const FULL_WEIGHT: f32 = 0.999;
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
        barrel * foregrip.fallback_distance_m
    };
    (foregrip.is_finite() && sight.is_finite()).then_some(GripGeometry {
        foregrip,
        barrel,
        sight,
        native,
    })
}

/// Holster the wielded weapon, as the game's own holster input does.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub unsafe fn holster(owner: &mut Character) {
    unsafe { owner.HolsterWeapon() };
}

/// The inventory slot of a raw slot value, or `None` for one out of the enum's range.
pub(super) fn slot_from_raw(slot: i32) -> Option<WeaponSlot> {
    const SLOTS: [WeaponSlot; 8] = [
        WeaponSlot::E_SLOT_DUAL_WIELD,
        WeaponSlot::E_SLOT_TWO_HANDED,
        WeaponSlot::E_SLOT_HEAVY,
        WeaponSlot::E_SLOT_HAND_GRENADE,
        WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE,
        WeaponSlot::E_SLOT_FOW,
        WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL,
        WeaponSlot::E_SLOT_BEACON,
    ];
    SLOTS.into_iter().find(|&s| s as i32 == slot)
}
