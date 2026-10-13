//! The engine calls behind docking a weapon and letting go of it. The engine side is in
//! `docs/engine/gameplay/weapon-equip-and-drop.md`.
//!
//! - A draw without the game's act equips the slot's holstered weapon in one call
//!   ([`WeaponBase::OnEquip`]), after unequipping the wielded one, and starts the snap from the
//!   weapon's dock into the hand ([`crate::controllers::weapons::hold::dock`]).
//! - A holster without the game's act unequips the wielded weapon ([`WeaponBase::OnUnequip`]) once
//!   its snap from the hand to the dock has run.
//! - A drop runs the weapon's drop at once while the inventory still holds it
//!   ([`WeaponBase::OnDrop`], as a swimming character's pickup does to a slot's occupant), gives
//!   its body the hand's motion, and then clears the slot ([`Inventory::DropWeapon`], whose own
//!   deferred drop does nothing to a weapon already dropped). A heavy weapon is dropped through
//!   the slot alone: unequipping it drops it through the inventory, and the game's own drop
//!   already gives it the hand's motion.
//!
//! Everything here runs on the input tick, on the main thread outside the world update, with the
//! local player's character.

use std::{fmt, num::NonZeroUsize};

use glam::{Mat4, Vec3};
use jc3gi::{
    character::{
        character::Character,
        inventory::{Inventory, WeaponSlot},
    },
    equipment::weapon::{WeaponBase, WeaponData, WeaponState},
};

use crate::controllers::{
    aim::gun,
    frame::{Hand, Pose},
    hands::{grip, pose as hand_pose},
    physical_hands,
    weapons::{
        dual_wield::{self, single},
        engine::slot_from_raw,
        hold::layout,
    },
};

/// Why a dock or a drop was not done.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DockRefusal {
    /// There is no local player.
    NoCharacter,
    /// The slot holds no weapon.
    EmptySlot,
    /// The slot's weapon is not on the body.
    NotHolstered,
    /// No weapon is in the hands.
    NothingHeld,
    /// The character's animation forbids drawing or holstering now.
    AnimationForbids,
    /// The weapon is not one this handles: the dual-wield pair, or a slot without a holster.
    Unsupported,
    /// The call ran, but the weapon is not where it should be afterwards.
    NotDone,
}

impl fmt::Display for DockRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self {
            DockRefusal::NoCharacter => "there is no local player",
            DockRefusal::EmptySlot => "the slot holds no weapon",
            DockRefusal::NotHolstered => "the slot's weapon is not on the body",
            DockRefusal::NothingHeld => "no weapon is in the hands",
            DockRefusal::AnimationForbids => "the animation forbids it",
            DockRefusal::Unsupported => "the weapon is not one this handles",
            DockRefusal::NotDone => "the weapon is not where it should be afterwards",
        };
        write!(f, "hold: {why}")
    }
}

impl std::error::Error for DockRefusal {}

/// What a draw found about the weapon's dock, for its snap.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Docked {
    pub weapon: usize,
    /// The dock bone (an `NBone::ESafeBoneIndex`).
    pub dock: u32,
    /// The first gun's pose relative to the dock bone, in the skeleton's model space.
    pub offset: Option<Pose>,
}

/// The long guns on the shoulders: the slots a second weapon is taken from.
pub fn handles(slot: WeaponSlot) -> bool {
    matches!(
        slot,
        WeaponSlot::E_SLOT_TWO_HANDED | WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL
    )
}

/// The slots a draw or a holster without the game's act handles: the long guns on the shoulders
/// and the pistols on the hips.
pub fn docks(slot: WeaponSlot) -> bool {
    handles(slot) || slot == WeaponSlot::E_SLOT_DUAL_WIELD
}

/// Equip `slot`'s holstered weapon at once, unequipping the wielded one first. Returns where its
/// gun `gun` (the drawing hand's pistol of the pair, or a long gun's only gun) was docked.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn draw(
    owner: &mut Character,
    slot: WeaponSlot,
    gun: usize,
) -> Result<Docked, DockRefusal> {
    if !docks(slot) {
        return Err(DockRefusal::Unsupported);
    }
    if !unsafe { owner.m_Inventory.GetAnimationAllowsDrawing() } {
        return Err(DockRefusal::AnimationForbids);
    }
    let weapon = unsafe { slot_weapon(&owner.m_Inventory, slot) }.ok_or(DockRefusal::EmptySlot)?;
    if weapon.m_State != WeaponState::E_WS_HOLSTERED {
        return Err(DockRefusal::NotHolstered);
    }
    let address = weapon as *mut WeaponBase as usize;
    let docked =
        unsafe { dock_of(owner, weapon, gun, WeaponState::E_WS_HOLSTERED) }.unwrap_or(Docked {
            weapon: address,
            dock: 0,
            offset: None,
        });
    if let Some(held) = unsafe { wielded_mut(owner) } {
        unsafe { held.OnUnequip() };
        unsafe { skip_interpolation(held) };
    }
    unsafe { weapon.OnEquip(owner) };
    let equipped = unsafe { gun::held_weapon(owner) }
        .is_some_and(|w| w as *const WeaponBase as usize == address);
    if !equipped {
        return Err(DockRefusal::NotDone);
    }
    Ok(docked)
}

/// Where the wielded weapon's gun `gun` docks, for a holster's snap: its holstered bone, and no
/// offset (the caller knows it from the draw, when there was one). A long gun has one gun; for the
/// pistols it is the holstering hand's.
///
/// # Safety
///
/// As [`draw`].
pub(crate) unsafe fn holster_target(owner: &Character, gun: usize) -> Result<Docked, DockRefusal> {
    if !unsafe { owner.m_Inventory.GetAnimationAllowsHolstering() } {
        return Err(DockRefusal::AnimationForbids);
    }
    let weapon = unsafe { gun::held_weapon(owner) }.ok_or(DockRefusal::NothingHeld)?;
    let slot = unsafe { slot_of(weapon) }.ok_or(DockRefusal::Unsupported)?;
    if !docks(slot) {
        return Err(DockRefusal::Unsupported);
    }
    let gun = if slot == WeaponSlot::E_SLOT_DUAL_WIELD {
        gun
    } else {
        0
    };
    let dock = unsafe { gun::guns(weapon) }
        .nth(gun)
        .flatten()
        .and_then(|gun| {
            gun.m_lBoneAttachementPerWeaponState
                .get(WeaponState::E_WS_HOLSTERED as usize)
                .copied()
        })
        .unwrap_or(0);
    Ok(Docked {
        weapon: weapon as *const WeaponBase as usize,
        dock,
        offset: None,
    })
}

/// Where `hand`'s pistol of the wielded pair sits on its hip, while the pair is wielded and the
/// pistol is not drawn: the pistol's current bone and its pose there, for a draw's snap.
///
/// # Safety
///
/// As [`draw`].
pub(crate) unsafe fn pistol_dock(owner: &Character, hand: Hand) -> Result<Docked, DockRefusal> {
    let (weapon, gun) = unsafe { pair_gun(owner, hand) }?;
    let docked =
        unsafe { dock_of(owner, weapon, gun, weapon.m_State) }.ok_or(DockRefusal::NotDone)?;
    if layout::dock_bone(docked.dock).is_none() {
        return Err(DockRefusal::NotHolstered);
    }
    Ok(docked)
}

/// The hip `hand`'s pistol of the wielded pair goes back to while the other hand keeps its own,
/// for a holster's snap: the hip holster bone on the hand's side, and no offset.
///
/// # Safety
///
/// As [`draw`].
pub(crate) unsafe fn pistol_hip(owner: &Character, hand: Hand) -> Result<Docked, DockRefusal> {
    let (weapon, _) = unsafe { pair_gun(owner, hand) }?;
    Ok(Docked {
        weapon: weapon as *const WeaponBase as usize,
        dock: layout::hip_bone(hand),
        offset: None,
    })
}

/// Unequip the wielded weapon at once, when it is still `weapon`.
///
/// # Safety
///
/// As [`draw`].
pub(crate) unsafe fn holster(owner: &Character, weapon: usize) -> Result<(), DockRefusal> {
    let held = unsafe { wielded_mut(owner) }.ok_or(DockRefusal::NothingHeld)?;
    if held as *mut WeaponBase as usize != weapon {
        return Err(DockRefusal::NothingHeld);
    }
    unsafe { held.OnUnequip() };
    unsafe { skip_interpolation(held) };
    if held.m_State != WeaponState::E_WS_HOLSTERED {
        return Err(DockRefusal::NotDone);
    }
    Ok(())
}

/// The velocities a drop gave the weapon's body.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Thrown {
    pub linear: Vec3,
    pub angular: Vec3,
    /// Whether the mod set them; otherwise the game's drop did.
    pub set: bool,
}

/// Drop the wielded weapon from the hands, its body leaving with the hand's motion, the linear part
/// capped at `max_speed_mps`.
///
/// # Safety
///
/// As [`draw`].
pub(crate) unsafe fn drop_held(
    owner: &mut Character,
    max_speed_mps: f32,
) -> Result<Thrown, DockRefusal> {
    // The wielded weapon is live in the inventory until the slot is cleared below.
    let weapon = unsafe { wielded_mut(owner) }.ok_or(DockRefusal::NothingHeld)?;
    let slot = unsafe { slot_of(weapon) }.ok_or(DockRefusal::Unsupported)?;
    // The game's pair is one weapon and cannot fall; a single pistol is a weapon alone.
    let pair = !single::is(weapon as *mut WeaponBase as usize);
    if slot == WeaponSlot::E_SLOT_DUAL_WIELD && pair {
        return Err(DockRefusal::Unsupported);
    }
    let first = unsafe { gun::guns(weapon) }.next().flatten();
    let motion = first.map(|gun| {
        (
            Vec3::from_array(gun.m_MeasuredWorldVelocity.data),
            Vec3::from_array(gun.m_MeasuredAngularVelocity.data),
        )
    });
    let body = first.and_then(|gun| unsafe { body_of(gun) });
    let address = weapon as *mut WeaponBase as usize;
    if slot == WeaponSlot::E_SLOT_HEAVY {
        unsafe { owner.m_Inventory.DropWeapon(slot, false) };
        let (linear, angular) = motion.unwrap_or_default();
        return Ok(Thrown {
            linear,
            angular,
            set: false,
        });
    }
    let thrown = unsafe { throw(weapon, motion, body, address, max_speed_mps) };
    unsafe { owner.m_Inventory.DropWeapon(slot, false) };
    Ok(thrown)
}

/// Let `weapon` fall with the hand's motion: drop it at once ([`WeaponBase::OnDrop`]), then give
/// its first gun's body `motion` (the gun's measured velocity and angular velocity, read before
/// the drop), the linear part capped at `max_speed_mps`.
///
/// # Safety
///
/// `weapon` must be live, `body` its first gun's body as read before the drop, on the main thread
/// outside the world update.
unsafe fn throw(
    weapon: &mut WeaponBase,
    motion: Option<(Vec3, Vec3)>,
    body: Option<u32>,
    address: usize,
    max_speed_mps: f32,
) -> Thrown {
    unsafe { weapon.OnDrop() };
    match (motion, body, NonZeroUsize::new(address)) {
        (Some((linear, angular)), Some(body), Some(owner_object)) => {
            let linear = finite_or_zero(linear).clamp_length_max(max_speed_mps.max(0.0));
            let angular = finite_or_zero(angular);
            // SAFETY: the input tick runs outside the physics step.
            let set = unsafe {
                physical_hands::set_body_velocities(body, owner_object, linear, angular)
            }
            .inspect_err(|e| {
                tracing::debug!(target: "controllers", error = %e, "hold: the dropped weapon keeps the game's velocity");
            })
            .is_ok();
            Thrown {
                linear,
                angular,
                set,
            }
        }
        _ => Thrown {
            linear: Vec3::ZERO,
            angular: Vec3::ZERO,
            set: false,
        },
    }
}

/// Let a weapon no inventory slot holds fall from the hand, with the hand's motion capped at
/// `max_speed_mps`: the drop a held weapon gets, without the slot to clear.
///
/// # Safety
///
/// `weapon` must be live, on the main thread outside the world update.
pub(crate) unsafe fn drop_loose(weapon: &mut WeaponBase, max_speed_mps: f32) -> Thrown {
    let first = unsafe { gun::guns(weapon) }.next().flatten();
    let motion = first.map(|gun| {
        (
            Vec3::from_array(gun.m_MeasuredWorldVelocity.data),
            Vec3::from_array(gun.m_MeasuredAngularVelocity.data),
        )
    });
    let body = first.and_then(|gun| unsafe { body_of(gun) });
    let address = weapon as *mut WeaponBase as usize;
    unsafe { throw(weapon, motion, body, address, max_speed_mps) }
}

/// Drop the weapon in `slot` onto the ground before another weapon replaces it there, so that it
/// falls rather than despawns. Returns whether there was one to drop.
///
/// # Safety
///
/// As [`draw`].
pub(crate) unsafe fn drop_occupant(owner: &mut Character, slot: WeaponSlot) -> bool {
    let Some(occupant) = (unsafe { slot_weapon(&owner.m_Inventory, slot) }) else {
        return false;
    };
    if slot != WeaponSlot::E_SLOT_HEAVY && occupant.m_State != WeaponState::E_WS_DROPPED {
        unsafe { occupant.OnDrop() };
    }
    unsafe { owner.m_Inventory.DropWeapon(slot, false) };
    true
}

/// The weapon `owner` wields, to act on.
unsafe fn wielded_mut(owner: &Character) -> Option<&'static mut WeaponBase> {
    let address = unsafe { gun::held_weapon(owner) }? as *const WeaponBase as usize;
    unsafe { (address as *mut WeaponBase).as_mut() }
}

/// The weapon in `slot`.
unsafe fn slot_weapon(inventory: &Inventory, slot: WeaponSlot) -> Option<&'static mut WeaponBase> {
    unsafe { inventory.m_Weapons.get(slot as usize)?.px.as_mut() }
}

/// The weapon's slot, read as the raw value: an unset slot is out of the enum's range.
unsafe fn slot_of(weapon: &WeaponBase) -> Option<WeaponSlot> {
    slot_from_raw(unsafe { (&raw const weapon.m_EquipSlot).cast::<i32>().read() })
}

/// The wielded pair and the index of `hand`'s pistol.
unsafe fn pair_gun(owner: &Character, hand: Hand) -> Result<(&WeaponBase, usize), DockRefusal> {
    let weapon = unsafe { gun::held_weapon(owner) }.ok_or(DockRefusal::NothingHeld)?;
    if unsafe { slot_of(weapon) } != Some(WeaponSlot::E_SLOT_DUAL_WIELD) {
        return Err(DockRefusal::Unsupported);
    }
    let gun = dual_wield::attach::gun_of(hand).ok_or(DockRefusal::Unsupported)?;
    Ok((weapon, gun))
}

/// Gun `index` of the weapon on its dock: the bone its table names for `state`, and its pose on
/// that bone.
unsafe fn dock_of(
    owner: &Character,
    weapon: &WeaponBase,
    index: usize,
    state: WeaponState,
) -> Option<Docked> {
    let gun = unsafe { gun::guns(weapon) }.nth(index).flatten()?;
    let dock = *gun.m_lBoneAttachementPerWeaponState.get(state as usize)?;
    let offset = layout::dock_bone(dock).and_then(|safe| {
        let bone = unsafe { grip::bone_model_pose(owner, safe) }?;
        let in_model = grip::relative_pose(
            Mat4::from(owner.m_WorldMatrixT1),
            Mat4::from(gun.m_WorldMatrixT1),
        )?;
        Some(hand_pose::mul(hand_pose::inverse(bone), in_model))
    });
    Some(Docked {
        weapon: weapon as *const WeaponBase as usize,
        dock,
        offset,
    })
}

/// The gun's body id.
unsafe fn body_of(gun: &WeaponData) -> Option<u32> {
    unsafe { gun.m_PfxInstance.as_ref() }.map(|body| body.m_BodyId)
}

/// Skip every gun's render interpolation for one frame, so a gun that changed bones does not
/// streak between them.
unsafe fn skip_interpolation(weapon: &mut WeaponBase) {
    let count = unsafe { gun::guns(weapon) }.count();
    for index in 0..count {
        if let Ok(index) = i32::try_from(index) {
            unsafe { weapon.SkipRenderInterpolationForOneFrame(index) };
        }
    }
}

fn finite_or_zero(v: Vec3) -> Vec3 {
    if v.is_finite() { v } else { Vec3::ZERO }
}
