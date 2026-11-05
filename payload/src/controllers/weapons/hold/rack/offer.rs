//! The game's offer of a second one-handed weapon of a kind the player already carries. The
//! design is in `docs/mod/input/physical-weapons.md`, "Racks"; the engine side in
//! `docs/engine/gameplay/weapon-equip-and-drop.md`, "Racks".
//!
//! A pickup's interaction graph is offered to a character only while its entry conditions pass, and
//! one of them ([`InteractionGraphParentWeaponNotInInventory`]) fails while the character's slot for
//! the offered weapon holds a weapon of the same kind. The game holds one weapon per slot, so a
//! second of a kind would only replace the first. Under single pistols each hand holds its own
//! weapon of the dual-wield slot, and the free hand can take a second of the same kind beside the
//! first: a second submachine gun off the same rack, for instance. The condition would hide that
//! rack from the player's valid interaction entries, which are how the rack's weapons are found
//! ([`crate::controllers::weapons::hold::rack`]), so for the local player the detour lets it pass
//! for a weapon of the dual-wield slot while pistols are single. A weapon of another kind in the
//! slot already passes, so the second of a kind is then offered and taken exactly as one of
//! another kind is: by a free hand's grab, or by the game's use-item pickup, which the table routes
//! into a free hand ([`crate::controllers::weapons::hold::use_item`]).
//!
//! The condition is evaluated wherever the character's interactions are updated, so the detour
//! reads only the entries and the weapon they offer, as the condition itself does, and the
//! single-pistol switch.

use detours_macro::detour;
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    interaction::{InteractionGraphParentWeaponNotInInventory, InteractionUserProxy},
};
use re_utilities::hook_library::HookLibrary;

use crate::controllers::weapons::{
    dual_wield::single,
    engine::{slot_of, weapon_at},
    hold::rack::offered,
};

pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&PARENT_WEAPON_NOT_IN_INVENTORY_EVALUATE_BINDER)
}

/// Whether a pickup whose not-carried condition evaluated to `passed` is offered: as the game has
/// it, or, for the local player while pistols are single, whenever the offered weapon is of the
/// dual-wield slot, which each hand fills on its own.
pub(crate) fn offered_to_player(
    passed: bool,
    local_player: bool,
    singles: bool,
    slot: Option<WeaponSlot>,
) -> bool {
    passed || (local_player && singles && slot == Some(WeaponSlot::E_SLOT_DUAL_WIELD))
}

#[detour(address = jc3gi::interaction::InteractionGraphParentWeaponNotInInventory::Evaluate_ADDRESS)]
fn parent_weapon_not_in_inventory_evaluate(
    condition: *mut InteractionGraphParentWeaponNotInInventory,
    user: *mut InteractionUserProxy,
    evaluation_type: u32,
) -> bool {
    let passed = PARENT_WEAPON_NOT_IN_INVENTORY_EVALUATE.get().unwrap().call(
        condition,
        user,
        evaluation_type,
    );
    if passed || !single::active() {
        return passed;
    }
    // SAFETY: the game passes the live user proxy it evaluates the condition for.
    let Some(user) = (unsafe { user.as_ref() }) else {
        return passed;
    };
    let local_player = !user.m_Character.is_null()
        && std::ptr::eq(user.m_Character, unsafe {
            Character::GetLocalPlayerCharacter()
        });
    if !local_player {
        return passed;
    }
    // SAFETY: the user's entries are live while the game evaluates their conditions.
    let slot = unsafe { first_offered_slot(user) };
    offered_to_player(passed, local_player, true, slot)
}

/// The slot of the weapon the first of `user`'s entries offers, read from the list the condition
/// reads.
///
/// # Safety
///
/// `user`'s entry lists must be live.
unsafe fn first_offered_slot(user: &InteractionUserProxy) -> Option<WeaponSlot> {
    let entries = if user.m_IsValidatingEntry {
        &user.m_PotentialEntries
    } else {
        &user.m_ValidEntries
    };
    let (mut at, end) = (entries.m_Begin, entries.m_End);
    if at.is_null() || end < at {
        return None;
    }
    let mut seen = 0;
    while at < end && seen < MAX_ENTRIES {
        // SAFETY: the pointer is inside the proxy's live entry array.
        let entry = unsafe { *at };
        at = unsafe { at.add(1) };
        seen += 1;
        let Some(strong) = (unsafe { offered(entry) }) else {
            continue;
        };
        let weapon = unsafe { weapon_at(strong.object()).as_ref() }?;
        return unsafe { slot_of(weapon) };
    }
    None
}

/// The most entries the walk visits, a bound against a corrupt array.
const MAX_ENTRIES: usize = 256;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_weapon_of_the_dual_wield_slot_is_offered_to_the_local_player_under_singles() {
        assert!(offered_to_player(
            false,
            true,
            true,
            Some(WeaponSlot::E_SLOT_DUAL_WIELD)
        ));
    }

    #[test]
    fn the_game_decides_for_other_slots_other_characters_and_paired_pistols() {
        for slot in [
            Some(WeaponSlot::E_SLOT_TWO_HANDED),
            Some(WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL),
            None,
        ] {
            assert!(!offered_to_player(false, true, true, slot));
        }
        assert!(!offered_to_player(
            false,
            false,
            true,
            Some(WeaponSlot::E_SLOT_DUAL_WIELD)
        ));
        assert!(!offered_to_player(
            false,
            true,
            false,
            Some(WeaponSlot::E_SLOT_DUAL_WIELD)
        ));
    }

    #[test]
    fn a_passing_condition_always_stays_passed() {
        for local_player in [false, true] {
            for singles in [false, true] {
                for slot in [
                    None,
                    Some(WeaponSlot::E_SLOT_DUAL_WIELD),
                    Some(WeaponSlot::E_SLOT_TWO_HANDED),
                ] {
                    assert!(offered_to_player(true, local_player, singles, slot));
                }
            }
        }
    }
}
