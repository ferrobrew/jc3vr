//! The animation-modifier detours behind the weapon-agnostic body
//! ([`crate::controllers::roomscale::stance::unarmed`]) and the standing stance's relaxed idle
//! ([`crate::controllers::roomscale::stance`]).
//!
//! [`Character::SetCharacterModifiers`] sets the wielded weapon's held or aimed modifier, and its
//! equipped modifier, through [`Character::SetModifier`], which forwards to every rule system of
//! the animated model. For the local player those are the only `SetModifier` calls it makes, so the
//! calls made inside it can be filtered by rule system ([`keeps_weapon_modifier`]):
//!
//! - while the unarmed body applies, only the upper-body action layer takes them, so every other
//!   layer plays its unarmed clips and the actions keep the weapon's clip lengths;
//! - otherwise, while the relaxed idle applies, the body's rule system (the first) goes without and
//!   the others keep the weapon's arm and grip poses.
//!
//! The detour on `SetCharacterModifiers` also records which idle variant the modifiers select, for
//! the readout.

use std::{cell::Cell, ptr};

use detours_macro::detour;
use jc3gi::{animation::rule_system::AnimationModifierType, character::character::Character};
use re_utilities::hook_library::HookLibrary;

use crate::controllers::roomscale::stance::{
    self,
    logic::idle_variant,
    unarmed::{self, ModifierSuppression, keeps_weapon_modifier},
};

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
    let suppression = if unarmed::applies() {
        ModifierSuppression::AllButActions
    } else if stance::relaxed_idle_applies() {
        ModifierSuppression::BaseLayer
    } else {
        ModifierSuppression::None
    };
    // The same queries the engine makes inside the call, made just before it, for the readout.
    let trying_to_aim = unsafe { local.IsTryingToAim() };
    let tether = unsafe { stance::tether(local) };
    SCOPE.set(Some(Scope {
        character,
        suppression,
        weapon_calls: 0,
    }));
    CHARACTER_SET_CHARACTER_MODIFIERS
        .get()
        .unwrap()
        .call(character);
    let weapon = SCOPE.take().is_some_and(|scope| scope.weapon_calls > 0);
    let body_suppressed = suppression != ModifierSuppression::None;
    stance::record_modifiers(
        weapon && suppression == ModifierSuppression::BaseLayer,
        weapon && suppression == ModifierSuppression::AllButActions,
        idle_variant(weapon, trying_to_aim, tether, body_suppressed),
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
        if scope.suppression != ModifierSuppression::None
            && let Some(modifier_type) = modifier_type_of(modifier_type)
            && let Some(character) = unsafe { character.as_mut() }
        {
            let rule_systems = unsafe { character.m_AnimatedModel.m_RuleSystems.as_slice() };
            for (layer, rule_system) in rule_systems.iter().enumerate() {
                if !keeps_weapon_modifier(scope.suppression, layer) {
                    continue;
                }
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
    suppression: ModifierSuppression,
    /// How many `SetModifier` calls the engine made inside the call: the weapon's, when it has one.
    weapon_calls: u32,
}

thread_local! {
    static SCOPE: Cell<Option<Scope>> = const { Cell::new(None) };
}

/// The modifier type for a raw value from the engine, `None` for a value this build does not use.
fn modifier_type_of(value: i32) -> Option<AnimationModifierType> {
    [
        AnimationModifierType::FRAME_BOUND,
        AnimationModifierType::STATE_BOUND,
    ]
    .into_iter()
    .find(|t| *t as i32 == value)
}
