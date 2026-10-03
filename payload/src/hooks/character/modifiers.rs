//! The animation-modifier detours behind the standing stance's relaxed idle
//! ([`crate::controllers::roomscale::stance`]).
//!
//! [`Character::SetCharacterModifiers`] sets the wielded weapon's held or aimed modifier, and its
//! equipped modifier, through [`Character::SetModifier`], which forwards to every rule system of
//! the animated model. For the local player those are the only `SetModifier` calls it makes, so
//! while the relaxed idle applies, the calls made inside it skip the body's rule system (the first)
//! and reach the others, which keep the weapon's arm and grip poses. The detour on
//! `SetCharacterModifiers` also records which idle variant the modifiers select, for the readout.

use std::{cell::Cell, ptr};

use detours_macro::detour;
use jc3gi::{animation::rule_system::AnimationModifierType, character::character::Character};
use re_utilities::hook_library::HookLibrary;

use crate::controllers::roomscale::stance::{self, logic::idle_variant};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&CHARACTER_SET_CHARACTER_MODIFIERS_BINDER)
        .with_static_binder(&CHARACTER_SET_MODIFIER_BINDER)
}

#[detour(address = jc3gi::character::character::Character::SetCharacterModifiers_ADDRESS)]
fn character_set_character_modifiers(character: *mut Character) {
    let Some(local) = (unsafe { character.as_ref() }).filter(|c| c.m_IsLocalCharacter) else {
        CHARACTER_SET_CHARACTER_MODIFIERS
            .get()
            .unwrap()
            .call(character);
        return;
    };
    let suppress = stance::relaxed_idle_applies();
    // The same queries the engine makes inside the call, made just before it, for the readout.
    let trying_to_aim = unsafe { local.IsTryingToAim() };
    let tether = unsafe { stance::tether(local) };
    SCOPE.set(Some(Scope {
        character,
        suppress,
        weapon_calls: 0,
    }));
    CHARACTER_SET_CHARACTER_MODIFIERS
        .get()
        .unwrap()
        .call(character);
    let weapon = SCOPE.take().is_some_and(|scope| scope.weapon_calls > 0);
    stance::record_modifiers(
        suppress && weapon,
        idle_variant(weapon, trying_to_aim, tether, suppress),
    );
}

#[detour(address = jc3gi::character::character::Character::SetModifier_ADDRESS)]
fn character_set_modifier(character: *mut Character, modifier: *const u32, modifier_type: i32) {
    let scope = SCOPE
        .get()
        .filter(|scope| ptr::eq(scope.character, character));
    if let Some(mut scope) = scope {
        scope.weapon_calls += 1;
        SCOPE.set(Some(scope));
        if scope.suppress
            && let Some(modifier_type) = modifier_type_of(modifier_type)
            && let Some(character) = unsafe { character.as_mut() }
        {
            let rule_systems = unsafe { character.m_AnimatedModel.m_RuleSystems.as_slice() };
            for rule_system in rule_systems.iter().skip(BODY_RULE_SYSTEMS) {
                if let Some(rule_system) = unsafe { rule_system.px.as_mut() } {
                    unsafe { rule_system.SetModifier(modifier, modifier_type) };
                }
            }
            return;
        }
    }
    CHARACTER_SET_MODIFIER
        .get()
        .unwrap()
        .call(character, modifier, modifier_type);
}

/// The local player's `SetCharacterModifiers` call in progress on this thread. Characters update on
/// worker threads, so the scope is per thread.
#[derive(Copy, Clone)]
struct Scope {
    character: *mut Character,
    suppress: bool,
    /// How many `SetModifier` calls the engine made inside the call: the weapon's, when it has one.
    weapon_calls: u32,
}

thread_local! {
    static SCOPE: Cell<Option<Scope>> = const { Cell::new(None) };
}

/// How many rule systems at the front of the animated model's list belong to the body: the first,
/// which drives the base state machine.
const BODY_RULE_SYSTEMS: usize = 1;

/// The modifier type for a raw value from the engine, `None` for a value this build does not use.
fn modifier_type_of(value: i32) -> Option<AnimationModifierType> {
    [
        AnimationModifierType::FRAME_BOUND,
        AnimationModifierType::STATE_BOUND,
    ]
    .into_iter()
    .find(|t| *t as i32 == value)
}
