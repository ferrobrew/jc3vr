//! The engine calls behind docking a weapon and letting go of it. The engine side is in
//! `docs/engine/gameplay/weapon-equip-and-drop.md`.
//!
//! - A dock of the wielded weapon unequips it ([`WeaponBase::OnUnequip`]) once its snap from the
//!   hand to the socket has run ([`crate::controllers::weapons::hold::dock`]); the reconcile does
//!   everything else a draw or a dock moves.
//! - A drop runs the weapon's drop at once while the inventory still holds it
//!   ([`WeaponBase::OnDrop`], as a swimming character's pickup does to a slot's occupant), gives
//!   its body the copied release-time hand and rig motion, and then clears the slot
//!   ([`Inventory::DropWeapon`]). A strong reference retains a heavy weapon through the slot
//!   removal its unequip can perform inside `OnDrop`. The immediate drop fulfils any deferred
//!   drop queued by that recursive unequip.
//!
//! Everything here runs on the input tick, on the main thread outside the world update, with the
//! local player's character.

use std::{fmt, num::NonZeroUsize};

use glam::{Mat4, Vec3};
use jc3gi::{
    character::{
        character::{Character, SafeBoneIndex},
        inventory::{Inventory, WeaponSlot},
    },
    equipment::weapon::{WeaponBase, WeaponData, WeaponState},
};

use crate::controllers::{
    aim::gun,
    frame::Pose,
    hands::{grip, pose as hand_pose},
    physical_hands,
    weapons::{
        dual_wield::single,
        engine::slot_from_raw,
        hold::release_diagnostics::{DropObservation, NativeWrite, ReleaseMotion, SampleStage},
        inventory::engine::strong_to,
    },
};

/// Why a dock or a drop was not done.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DockRefusal {
    /// There is no local player.
    NoCharacter,
    /// The loadout table does not run, so nothing is drawn or docked.
    NoTable,
    /// Nothing the hands can take is on the socket.
    NothingThere,
    /// The socket does not take what the hand holds.
    DoesNotFit,
    /// The weapon is not on the body.
    NotHolstered,
    /// No weapon is in the hands.
    NothingHeld,
    /// The character's animation forbids drawing or holstering now.
    AnimationForbids,
    /// The weapon is not one this handles: the dual-wield pair, which cannot fall, or a weapon of
    /// a slot the table does not take.
    Unsupported,
    /// The call ran, but the weapon is not where it should be afterwards.
    NotDone,
}

impl fmt::Display for DockRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self {
            DockRefusal::NoCharacter => "there is no local player",
            DockRefusal::NoTable => "the loadout table does not run",
            DockRefusal::NothingThere => "nothing the hands can take is on the socket",
            DockRefusal::DoesNotFit => "the socket does not take what the hand holds",
            DockRefusal::NotHolstered => "the weapon is not on the body",
            DockRefusal::NothingHeld => "no weapon is in the hands",
            DockRefusal::AnimationForbids => "the animation forbids it",
            DockRefusal::Unsupported => "the weapon is not one this handles",
            DockRefusal::NotDone => "the weapon is not where it should be afterwards",
        };
        write!(f, "hold: {why}")
    }
}

impl std::error::Error for DockRefusal {}

/// Unequip the wielded weapon at once, when it is still `weapon`.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
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

/// The reported drop motion and copied observations, without a native velocity readback.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Thrown {
    /// The sanitized release motion requested on the native body.
    pub linear: Vec3,
    pub angular: Vec3,
    /// Whether the existing native setter returned success. Later persistence is unknown.
    pub set: bool,
    pub diagnostic: DropObservation,
}

/// Drop the wielded weapon, requesting its copied release motion with the linear part capped at
/// `max_speed_mps`.
///
/// # Safety
///
/// As [`holster`].
pub(crate) unsafe fn drop_held(
    owner: &mut Character,
    max_speed_mps: f32,
    release: ReleaseMotion,
) -> Result<Thrown, DockRefusal> {
    // The inventory retains the weapon until the strong reference is acquired below.
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
    let diagnostic =
        DropObservation::sampled(address, Some(slot), SampleStage::Wielded, motion, body);
    // OnDrop can unequip a heavy weapon through its slot, so retain it through the write.
    let strong = unsafe { strong_to(weapon) }.ok_or(DockRefusal::NotDone)?;
    let mut thrown = unsafe { throw(weapon, max_speed_mps, diagnostic, release) };
    if owner
        .m_Inventory
        .m_Weapons
        .get(slot as usize)
        .is_some_and(|entry| entry.px as usize == address)
    {
        unsafe { owner.m_Inventory.DropWeapon(slot, false) };
    }
    thrown.diagnostic.slot_clear_returned = true;
    drop(strong);
    Ok(thrown)
}

/// Drop `weapon` at once ([`WeaponBase::OnDrop`]), then request the copied release motion on its
/// first gun's body, with the linear part capped at `max_speed_mps`, without a native readback.
///
/// # Safety
///
/// `weapon` must be live, and `diagnostic` must contain its first gun's motion and body as read
/// before the drop, on the main thread outside the world update.
unsafe fn throw(
    weapon: &mut WeaponBase,
    max_speed_mps: f32,
    mut diagnostic: DropObservation,
    release: ReleaseMotion,
) -> Thrown {
    let body = diagnostic.native_body_id;
    let address = diagnostic.weapon;
    unsafe { weapon.OnDrop() };
    // The immediate drop fulfils a request queued by a heavy weapon's recursive unequip.
    weapon.m_DeferredDrop = false;
    diagnostic.on_drop_returned = true;
    let requested = diagnostic.prepare_release(release, max_speed_mps);
    let linear = requested.linear;
    let angular = requested.angular;
    match (body, NonZeroUsize::new(address)) {
        (Some(body), Some(owner_object)) => {
            // SAFETY: the input tick runs outside the physics step.
            let result = unsafe {
                physical_hands::set_body_velocities(body, owner_object, linear, angular)
            }
            .inspect_err(|e| {
                tracing::debug!(target: "controllers", error = %e, "hold: the dropped weapon keeps the game's velocity");
            });
            diagnostic.write = match result {
                Ok(()) => NativeWrite::SetterReturned,
                Err(error) => NativeWrite::Rejected(error),
            };
            Thrown {
                linear,
                angular,
                set: result.is_ok(),
                diagnostic,
            }
        }
        _ => {
            diagnostic.requested_world_motion = None;
            diagnostic.write = if body.is_none() {
                NativeWrite::MissingBody
            } else {
                NativeWrite::MissingOwner
            };
            Thrown {
                linear,
                angular,
                set: false,
                diagnostic,
            }
        }
    }
}

/// Drop a weapon without a hand-release estimate, requesting rest motion for inventory cleanup.
/// The linear request still observes `max_speed_mps`.
///
/// # Safety
///
/// `weapon` must be live, on the main thread outside the world update.
pub(crate) unsafe fn drop_loose(weapon: &mut WeaponBase, max_speed_mps: f32) -> Thrown {
    unsafe { release_loose(weapon, max_speed_mps, ReleaseMotion::REST) }
}

/// Drop a carried weapon using motion copied before its disarm.
///
/// # Safety
///
/// As [`drop_loose`]. The caller retains a slot or strong reference through the call.
pub(crate) unsafe fn release_loose(
    weapon: &mut WeaponBase,
    max_speed_mps: f32,
    release: ReleaseMotion,
) -> Thrown {
    let first = unsafe { gun::guns(weapon) }.next().flatten();
    let motion = first.map(|gun| {
        (
            Vec3::from_array(gun.m_MeasuredWorldVelocity.data),
            Vec3::from_array(gun.m_MeasuredAngularVelocity.data),
        )
    });
    let body = first.and_then(|gun| unsafe { body_of(gun) });
    let address = weapon as *mut WeaponBase as usize;
    let diagnostic =
        DropObservation::sampled(address, None, SampleStage::OutsideInventory, motion, body);
    unsafe { throw(weapon, max_speed_mps, diagnostic, release) }
}

/// Drop the weapon in `slot` onto the ground before another weapon replaces it there, so that it
/// falls rather than despawns. Returns whether there was one to drop.
///
/// # Safety
///
/// As [`holster`].
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

/// The pose of `weapon`'s first gun on `owner`'s bone `bone`, in the skeleton's model space: where
/// it sits there, for a snap between the bone and a hand.
///
/// # Safety
///
/// `owner` and `weapon` must be live, on the main thread outside the world update.
pub(crate) unsafe fn pose_on_bone(
    owner: &Character,
    weapon: &WeaponBase,
    bone: SafeBoneIndex,
) -> Option<Pose> {
    let gun = unsafe { gun::guns(weapon) }.next().flatten()?;
    let on_bone = unsafe { grip::bone_model_pose(owner, bone) }?;
    let in_model = grip::relative_pose(
        Mat4::from(owner.m_WorldMatrixT1),
        Mat4::from(gun.m_WorldMatrixT1),
    )?;
    Some(hand_pose::mul(hand_pose::inverse(on_bone), in_model))
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
