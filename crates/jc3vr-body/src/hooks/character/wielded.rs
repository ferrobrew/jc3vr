//! The weapon-wielded condition's detour behind the relaxed locomotion
//! ([`crate::controllers::body::unarmed::relaxes_wielded`]).
//!
//! [`WeaponWieldedCondition::Evaluate`] answers the animation graph's `C_WEAPON_WIELDED` for the
//! character whose state machine asks. For the local player, while the relaxed locomotion holds,
//! the any-slot condition reads false on every rule system but the upper-body actions'. The
//! condition runs on the characters' update threads, so the detour reads only the atomic the
//! unarmed body publishes and the character's own rule systems.

use std::ptr;

use detours_macro::detour;
use jc3gi::{
    animation::rule_system::StateMachineInstance, character::character::Character,
    equipment::weapon_animation::WeaponWieldedCondition,
};
use re_utilities::hook_library::HookLibrary;

use crate::controllers::body::unarmed;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&WEAPON_WIELDED_CONDITION_EVALUATE_BINDER)
}

#[detour(address = jc3gi::equipment::weapon_animation::WeaponWieldedCondition::Evaluate_ADDRESS)]
fn weapon_wielded_condition_evaluate(
    condition: *const WeaponWieldedCondition,
    instance: *const StateMachineInstance,
) -> bool {
    let wielded = WEAPON_WIELDED_CONDITION_EVALUATE
        .get()
        .unwrap()
        .call(condition, instance);
    if !wielded {
        return wielded;
    }
    let relaxed = unarmed::wielded_relaxed();
    if !relaxed {
        return wielded;
    }
    // SAFETY: the graph evaluates its own live condition for its own live instance.
    let relaxed = unsafe { relaxes(relaxed, condition, instance) };
    if relaxed {
        unarmed::count_relaxed_wielded();
    }
    !relaxed
}

/// Whether the wielded condition `condition`, asked by `instance`, reads false for the local
/// player while the locomotion is `relaxed`.
///
/// # Safety
///
/// `condition` and `instance` must be null or live for the evaluation.
unsafe fn relaxes(
    relaxed: bool,
    condition: *const WeaponWieldedCondition,
    instance: *const StateMachineInstance,
) -> bool {
    let (Some(condition), Some(instance)) =
        (unsafe { condition.as_ref() }, unsafe { instance.as_ref() })
    else {
        return false;
    };
    let Some(character) = owner(instance) else {
        return false;
    };
    // SAFETY: the instance's own character is live while its graph evaluates.
    let character = unsafe { &*character };
    if !character.m_IsLocalCharacter {
        return false;
    }
    let rule_systems = unsafe { character.m_AnimatedModel.m_RuleSystems.as_slice() };
    let layer = rule_systems.iter().position(|rule_system| {
        unsafe { rule_system.px.as_ref() }
            .is_some_and(|r| ptr::eq(r.m_StateMachineInstance.px, instance))
    });
    unarmed::relaxes_wielded(relaxed, condition.m_WeaponType < 0, layer)
}

/// The character `instance` animates: its user data is the character's game-object subobject.
fn owner(instance: &StateMachineInstance) -> Option<*const Character> {
    let game_object = instance.m_UserData as usize;
    if game_object == 0 {
        return None;
    }
    let character = game_object.checked_sub(Character::GAME_OBJECT_BASE_OFFSET as usize)?;
    Some(character as *const Character)
}
