//! A gun's shape read from the game: its model's box, its fire point, its other hand's grip point,
//! and the wells table's magazine well and action, all in the gun's own frame.

use glam::{Mat4, Vec3};
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::{WeaponBase, WeaponGripHand, WeaponState},
    types::math::{Matrix4, Vector3},
};

use crate::controllers::{
    aim::gun,
    frame::{Hand, Pose},
    grab::HolsterSite,
    weapons::{
        engine::slot_from_raw,
        hold::{pickup::gun_pose, second},
        holster::site_slot,
        regions::{
            WorldTouch,
            shape::{Carry, GunRegion, GunShape, MagazineWell, classify, into_gun, turn_on_wrist},
        },
        reload::wells,
    },
};

/// The shape of `weapon`'s first gun, for a gun held in (or taken by) `hand`: the other hand's grip
/// point is the one the game gives for the hand opposite it. `None` when the gun has no model or
/// its box is unusable.
///
/// # Safety
///
/// `weapon` must be live, read on the main thread outside the world update.
pub(crate) unsafe fn shape(weapon: &WeaponBase, hand: Hand) -> Option<GunShape> {
    let first = unsafe { gun::guns(weapon) }.next().flatten()?;
    let model = unsafe { first.m_BaseModelInstance.as_ref() }?;
    let mut min = Vector3 { data: [0.0; 3] };
    let mut max = Vector3 { data: [0.0; 3] };
    unsafe { model.GetModelSpaceBoundingBox(&mut min, &mut max) };
    let muzzle = unsafe { gun::fire_point(first) }.w_axis.truncate();
    // The rigid part of the gun's matrix: a holstered gun's is drawn scaled, and its model's box is
    // in the unscaled frame.
    let world = unsafe { gun_pose(weapon) }?;
    let to_gun = Mat4::from_rotation_translation(world.orientation, world.position).inverse();
    let grip_hand = match hand.other() {
        Hand::Left => WeaponGripHand::E_WEAPON_GRIP_HAND_LEFT,
        Hand::Right => WeaponGripHand::E_WEAPON_GRIP_HAND_RIGHT,
    };
    let mut grip = Matrix4::from(Mat4::IDENTITY);
    let foregrip = unsafe { weapon.GetGripPosition(grip_hand, &mut grip) }
        .then(|| to_gun.transform_point3(Mat4::from(grip).w_axis.truncate()))
        .filter(|p| p.is_finite());
    let entry = wells::lookup(weapon.m_WeaponNameHash).filter(|e| e.grade.trusted());
    let magazine = entry.and_then(|e| {
        Some(MagazineWell {
            mouth: e.mouth_m?,
            axis: e.insert_axis?,
        })
    });
    let action = entry.and_then(|e| e.handle_m);
    // Read as the raw value: an unset slot is out of the enum's range.
    let slot = unsafe { (&raw const weapon.m_EquipSlot).cast::<i32>().read() };
    let long = slot_from_raw(slot).is_some_and(is_long);
    let shape = GunShape {
        min: Vec3::from_array(min.data),
        max: Vec3::from_array(max.data),
        muzzle,
        foregrip,
        magazine,
        action,
        long,
    };
    shape.is_usable().then_some(shape)
}

/// Whether a weapon of `slot` is a long gun, held by its stock and handguard as well as its grip.
pub(crate) fn is_long(slot: WeaponSlot) -> bool {
    matches!(
        slot,
        WeaponSlot::E_SLOT_TWO_HANDED
            | WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL
            | WeaponSlot::E_SLOT_HEAVY
    )
}

/// How `hand` carries the long gun `weapon`, whose first gun is at `gun` in the world, taking it
/// where `touch` says: `None` at its grip, for a pistol, or for a touch off the gun.
///
/// # Safety
///
/// `weapon` must be live, read on the main thread outside the world update.
pub(crate) unsafe fn world_carry(
    weapon: &WeaponBase,
    hand: Hand,
    gun: Pose,
    touch: &WorldTouch,
) -> Option<Carry> {
    let shape = unsafe { shape(weapon, hand) }?;
    if !shape.long {
        return None;
    }
    let hit = classify(
        &shape,
        into_gun(gun, touch.point, 1.0),
        touch.grip_radius,
        touch.reach,
    )?;
    (hit.region != GunRegion::Grip).then(|| Carry {
        region: hit.region,
        anchor: hit.anchor,
        turn: turn_on_wrist(touch.wrist, gun.orientation),
    })
}

/// A long gun docked on a shoulder.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct DockedLongGun {
    pub site: HolsterSite,
    pub weapon: usize,
    /// The gun's pose in the world.
    pub pose: Pose,
    pub shape: GunShape,
}

/// The long guns docked on `owner`'s shoulders (the shoulders by `dominant`), in the holstered
/// state, with their poses and shapes. The second weapon is in a hand, not docked.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn docked_long_guns(owner: &Character, dominant: Hand) -> Vec<DockedLongGun> {
    let mut out = Vec::new();
    for side in Hand::BOTH {
        let site = HolsterSite::Shoulder(side);
        let slot = site_slot(site, dominant);
        let Some(weapon) = (unsafe { owner.m_Inventory.m_Weapons[slot as usize].px.as_ref() })
        else {
            continue;
        };
        let address = weapon as *const WeaponBase as usize;
        if weapon.m_State != WeaponState::E_WS_HOLSTERED || second::is(address) {
            continue;
        }
        // A docked gun is taken by the hand that draws it, with its other hand's grip opposite.
        let (Some(pose), Some(shape)) = (unsafe { gun_pose(weapon) }, unsafe {
            shape(weapon, dominant)
        }) else {
            continue;
        };
        out.push(DockedLongGun {
            site,
            weapon: address,
            pose,
            shape,
        });
    }
    out
}
