//! A gun's shape read from the game: its model's box, its fire point, its other hand's grip point,
//! and the wells table's magazine well and action, all in the gun's own frame.

use glam::{Mat4, Vec3};
use jc3gi::{
    character::character::Character,
    equipment::weapon::{WeaponBase, WeaponGripHand},
    types::math::{Matrix4, Vector3},
};

use crate::controllers::{
    aim::gun,
    frame::{Hand, Pose},
    loadout::{LoadoutSnapshot, Socket},
    weapons::{
        docks,
        engine::{gun_pose, long_slot, slot_of},
        regions::shape::{GunShape, MagazineWell},
        reload::wells,
    },
};

/// The shape of `weapon`'s first gun, for a gun held in (or taken by) `hand`: the other hand's grip
/// point is the one the game gives for the hand opposite it. `None` when the gun has no model or
/// its box is unusable.
///
/// # Safety
///
/// `weapon` must be live, read on the game thread before the world update.
pub(crate) unsafe fn shape(weapon: &WeaponBase, hand: Hand) -> Option<GunShape> {
    let first = unsafe { gun::guns(weapon) }.next().flatten()?;
    let (min, max) = unsafe { model_box(weapon) }?;
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
    let long = unsafe { slot_of(weapon) }.is_some_and(long_slot);
    let shape = GunShape {
        min,
        max,
        muzzle,
        foregrip,
        magazine,
        action,
        long,
    };
    shape.is_usable().then_some(shape)
}

/// The model box (minimum and maximum corners) of `weapon`'s first gun, in the gun's own frame,
/// metres. `None` when the gun has no model.
///
/// # Safety
///
/// `weapon` must be live, read on the game thread before the world update.
pub(crate) unsafe fn model_box(weapon: &WeaponBase) -> Option<(Vec3, Vec3)> {
    let first = unsafe { gun::guns(weapon) }.next().flatten()?;
    let model = unsafe { first.m_BaseModelInstance.as_ref() }?;
    let mut min = Vector3 { data: [0.0; 3] };
    let mut max = Vector3 { data: [0.0; 3] };
    unsafe { model.GetModelSpaceBoundingBox(&mut min, &mut max) };
    Some((Vec3::from_array(min.data), Vec3::from_array(max.data)))
}

/// A long gun docked on a socket of the body.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct DockedLongGun {
    pub site: Socket,
    pub weapon: usize,
    /// The gun's pose in the world.
    pub pose: Pose,
    pub shape: GunShape,
}

/// The long guns the loadout table has docked on `owner`'s sockets, with their poses and shapes.
///
/// # Safety
///
/// `owner` must be the live local player character, on the game thread before the world update.
pub(crate) unsafe fn docked_long_guns(
    owner: &Character,
    snapshot: &LoadoutSnapshot,
    dominant: Hand,
) -> Vec<DockedLongGun> {
    let mut out = Vec::new();
    for (site, weapon) in unsafe { docks::socket_weapons(owner, snapshot) } {
        if !unsafe { slot_of(weapon) }.is_some_and(long_slot) {
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
            weapon: weapon as *const WeaponBase as usize,
            pose,
            shape,
        });
    }
    out
}
