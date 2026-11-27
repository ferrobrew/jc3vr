//! Traversal leaves the hands as they are: the base layer's weapon state tasks are refused for the
//! local player while the loadout table runs, and the neutral state the game forces on a teleport,
//! a revive, or a cutscene's setup is undone. The design is the two-handed inventory's "No
//! automatic moves" (`docs/mod/input/two-handed-inventory.md`), the engine side in
//! `docs/engine/character/animation-graph.md`, "The base layer's weapon tasks".
//!
//! - [`NWeaponUnequipTask_OnEnter`] puts the wielded weapon away on entering a vehicle, a seat, a
//!   ride, the wingsuit, a free fall, a dive, a hit reaction's flight, the ragdoll, and dying;
//! - [`NWeaponHideInVehicleTask_OnEnter`] hides the two-handed and special slots' holstered
//!   weapons in a seat of a vehicle that hides them;
//! - [`NStopUsingWeaponStateTask_OnEnter`] stops the selected weapon's use in the ragdoll, a crash,
//!   dying, drowning, and the get-up;
//! - [`NDropWeaponStateTask_OnEnter`] drops the heavy slot's weapon on a grapple, a throw, a fall,
//!   the parachute's and the wingsuit's opening, a vehicle's entry, and swimming.
//!
//! Each is refused for the local player, so the table's hands and sockets stand through them. A
//! heavy weapon of the table's is beyond the slots, so none of the tasks reaches it: it stays in
//! its hand through a swim, a hand climb, a grapple, and a vehicle's entry, as any other gun does.
//! The drop task is let through for a heavy weapon the game wields from its slot, which is the
//! game's own rather than the table's.
//!
//! The forced neutral state ([`Character::ForceNeutralState`]) unequips the wielded weapon; its
//! detour tells the live table which weapon it took ([`live::neutral_forced`]), so the next input
//! tick keeps it in its hand rather than taking it for a holster, and the reconcile wields or arms
//! it again.
//!
//! While the table does not run, the tasks and the forced neutral state are the game's, and the
//! table follows what they do: a weapon put away docks on its socket, and a heavy weapon dropped
//! lies in the world.
//!
//! The tasks run on the animation's update and the forced neutral state wherever the game calls
//! it; the detours read a published switch and the character, and fold the table's one event.
//!
//! [`NWeaponUnequipTask_OnEnter`]: jc3gi::equipment::weapon_animation::NWeaponUnequipTask_OnEnter
//! [`NWeaponHideInVehicleTask_OnEnter`]: jc3gi::equipment::weapon_animation::NWeaponHideInVehicleTask_OnEnter
//! [`NStopUsingWeaponStateTask_OnEnter`]: jc3gi::equipment::weapon_animation::NStopUsingWeaponStateTask_OnEnter
//! [`NDropWeaponStateTask_OnEnter`]: jc3gi::equipment::weapon_animation::NDropWeaponStateTask_OnEnter

use std::{
    ffi::c_void,
    sync::atomic::{AtomicBool, Ordering},
};

use detours_macro::detour;
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::{
        weapon::{WeaponBase, WeaponState},
        weapon_animation::{
            NDropWeaponStateTask_OnEnter_ADDRESS, NStopUsingWeaponStateTask_OnEnter_ADDRESS,
            NWeaponHideInVehicleTask_OnEnter_ADDRESS, NWeaponUnequipTask_OnEnter_ADDRESS,
        },
    },
    state::StateContext,
};
use jc3vr_body::hooks::input::locomotion::character_from_context;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    loadout::live,
    weapons::inventory::{engine, slots},
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&WEAPON_UNEQUIP_TASK_BINDER)
        .with_static_binder(&WEAPON_HIDE_IN_VEHICLE_TASK_BINDER)
        .with_static_binder(&STOP_USING_WEAPON_TASK_BINDER)
        .with_static_binder(&DROP_WEAPON_TASK_BINDER)
        .with_static_binder(&FORCE_NEUTRAL_STATE_BINDER)
}

/// Publish whether the tasks are refused, which they are while the table runs. Set on each input
/// tick the table runs, and cleared when it stops.
pub(crate) fn publish(keep: bool) {
    KEEP.store(keep, Ordering::Relaxed);
}

static KEEP: AtomicBool = AtomicBool::new(false);

/// Whether a task run for the character of `context` is refused: the local player's, while the
/// tasks are refused.
///
/// # Safety
///
/// `context` must be the state context the task was given.
unsafe fn refuse(context: *mut StateContext) -> bool {
    KEEP.load(Ordering::Relaxed)
        && unsafe { character_from_context(context) }.is_some_and(|c| c.m_IsLocalCharacter)
}

#[detour(address = NWeaponUnequipTask_OnEnter_ADDRESS)]
fn weapon_unequip_task(context: *mut StateContext, properties: *mut c_void, instance: *mut c_void) {
    if !unsafe { refuse(context) } {
        WEAPON_UNEQUIP_TASK
            .get()
            .unwrap()
            .call(context, properties, instance);
    }
}

/// Only the entry is refused: the exit puts back a weapon the entry hid, and does nothing to one it
/// did not.
#[detour(address = NWeaponHideInVehicleTask_OnEnter_ADDRESS)]
fn weapon_hide_in_vehicle_task(
    context: *mut StateContext,
    properties: *mut c_void,
    instance: *mut c_void,
) {
    if !unsafe { refuse(context) } {
        WEAPON_HIDE_IN_VEHICLE_TASK
            .get()
            .unwrap()
            .call(context, properties, instance);
    }
}

#[detour(address = NStopUsingWeaponStateTask_OnEnter_ADDRESS)]
fn stop_using_weapon_task(
    context: *mut StateContext,
    properties: *mut c_void,
    instance: *mut c_void,
) {
    if !unsafe { refuse(context) } {
        STOP_USING_WEAPON_TASK
            .get()
            .unwrap()
            .call(context, properties, instance);
    }
}

/// Refused only while the heavy slot holds no weapon the game wields: the table never puts a
/// heavy weapon there, so one wielded from it is the game's own (picked up while the table did not
/// run, which the reconcile leaves to the game), and the game drops it as it would. A heavy weapon
/// of the table's, in a hand or on a socket, is beyond the slots and untouched either way; one the
/// reconcile has yet to take out of the slot, holstered, is kept.
#[detour(address = NDropWeaponStateTask_OnEnter_ADDRESS)]
fn drop_weapon_task(context: *mut StateContext, properties: *mut c_void, instance: *mut c_void) {
    let refused =
        unsafe { refuse(context) } && refuses_heavy_drop(unsafe { heavy_occupant(context) });
    if !refused {
        DROP_WEAPON_TASK
            .get()
            .unwrap()
            .call(context, properties, instance);
    }
}

/// What the heavy slot of the task's character holds.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum HeavyOccupant {
    Empty,
    /// A weapon in any state but wielding, holstered included.
    Kept,
    Wielded,
}

/// Whether the drop task is refused for a local player whose heavy slot holds `occupant`.
fn refuses_heavy_drop(occupant: HeavyOccupant) -> bool {
    occupant != HeavyOccupant::Wielded
}

/// What the heavy slot of the character of `context` holds.
///
/// # Safety
///
/// `context` must be the state context the task was given.
unsafe fn heavy_occupant(context: *mut StateContext) -> HeavyOccupant {
    let Some(weapon) = (unsafe { character_from_context(context) })
        .and_then(|c| slots::weapon(&c.m_Inventory, WeaponSlot::E_SLOT_HEAVY))
        // SAFETY: the slot's strong reference keeps its weapon alive.
        .map(|weapon| unsafe { weapon.as_ref() })
    else {
        return HeavyOccupant::Empty;
    };
    if matches!(
        weapon.m_State,
        WeaponState::E_WS_WIELDING | WeaponState::E_WS_DUAL_WIELDING
    ) {
        HeavyOccupant::Wielded
    } else {
        HeavyOccupant::Kept
    }
}

/// The forced neutral state unequips the wielded weapon: for the local player, while the tasks are
/// refused, the table is told which weapon it took, so the hand keeps it.
#[detour(address = jc3gi::character::character::Character::ForceNeutralState_ADDRESS)]
fn force_neutral_state(character: *mut Character) {
    // SAFETY: the game calls it on a live character.
    let wielded = unsafe { character.as_ref() }
        .filter(|c| c.m_IsLocalCharacter && KEEP.load(Ordering::Relaxed))
        .and_then(|c| unsafe { c.GetWieldingWeapon(true).as_ref() })
        .and_then(|weapon| unsafe { weapon.px.as_ref() })
        .and_then(|weapon: &WeaponBase| engine::object_address(weapon));
    FORCE_NEUTRAL_STATE.get().unwrap().call(character);
    if wielded.is_some_and(live::neutral_forced) {
        tracing::info!(
            target: "controllers",
            "inventory: the forced neutral state put the wielded weapon away; its hand keeps it"
        );
    }
}

/// Whether the inventory's weapon updates skip the wielded weapon for `owner` now: in a seat other
/// than a motorcycle's, or flying the boost wingsuit, whose guns are updated in its place. The
/// mod's own update then fires a gun in a hand, so the reconcile arms it rather than wield it.
///
/// # Safety
///
/// `owner` must be the live local player character, on the game thread.
pub(crate) unsafe fn wield_skipped(owner: &Character) -> bool {
    if !KEEP.load(Ordering::Relaxed) {
        return false;
    }
    let seated = unsafe { owner.IsInRidingInVehicleState() && !owner.IsRidingMC() };
    let boost = unsafe { owner.HasBoostWingsuit() && owner.IsUsingWingsuit() };
    seated || boost
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_drop_task_drops_only_a_heavy_weapon_the_game_wields() {
        assert!(refuses_heavy_drop(HeavyOccupant::Empty));
        assert!(refuses_heavy_drop(HeavyOccupant::Kept));
        assert!(!refuses_heavy_drop(HeavyOccupant::Wielded));
    }
}
