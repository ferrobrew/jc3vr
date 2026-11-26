//! The animation-modifier detours behind the weapon-agnostic body
//! ([`crate::controllers::body::unarmed`]).
//!
//! [`Character::SetCharacterModifiers`] sets the wielded weapon's held or aimed modifier, and its
//! equipped modifier, through [`Character::SetModifier`], which forwards to every rule system of
//! the animated model. For the local player those are the only `SetModifier` calls it makes, so the
//! calls made inside it can be filtered by rule system ([`keeps_weapon_modifier`]):
//!
//! while the unarmed body applies, only the upper-body action layer takes them, so every other
//! layer plays its unarmed clips and the actions keep the weapon's clip lengths. The detour on
//! `SetCharacterModifiers` records whether it kept a weapon's modifiers off, for the readout.

use std::{
    cell::Cell,
    ptr,
    sync::atomic::{AtomicBool, Ordering},
};

use detours_macro::detour;
use jc3gi::{
    animation::rule_system::{AnimationModifierType, AnimationRuleSystem},
    character::character::{
        Character, get_Character_StuntCombatModifier, get_Character_StuntNoncombatModifier,
    },
};
use jc3vr_view::headpose::{Source, source};
use re_utilities::hook_library::HookLibrary;

use crate::controllers::body::unarmed::{self, ModifierSuppression, keeps_weapon_modifier};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&CHARACTER_SET_CHARACTER_MODIFIERS_BINDER)
        .with_static_binder(&CHARACTER_SET_MODIFIER_BINDER)
        .with_static_binder(&RULE_SYSTEM_SET_MODIFIER_BINDER)
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
    } else {
        ModifierSuppression::None
    };
    // A standing stunt area plays the on-foot animation state, so the stunt modifiers that swap
    // its clips for the stunt idles and moves must not reach it.
    let suppress_stunt = source() == Source::Vr && unsafe { standing_stunt(local) };
    if SUPPRESS_STUNT.swap(suppress_stunt, Ordering::Relaxed) != suppress_stunt {
        tracing::info!(
            target: "controllers",
            suppress = suppress_stunt,
            "animation: the local player's stunt clip modifiers are {}",
            if suppress_stunt { "kept off" } else { "restored" }
        );
    }
    SCOPE.set(Some(Scope {
        character,
        suppression,
        suppress_stunt,
        weapon_calls: 0,
    }));
    CHARACTER_SET_CHARACTER_MODIFIERS
        .get()
        .unwrap()
        .call(character);
    let weapon = SCOPE.take().is_some_and(|scope| scope.weapon_calls > 0);
    unarmed::record_modifiers(weapon && suppression == ModifierSuppression::AllButActions);
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

/// Whether the stunt clip modifiers were last kept off a standing stunt, for the one log line.
static SUPPRESS_STUNT: AtomicBool = AtomicBool::new(false);

/// The local player's `SetCharacterModifiers` call in progress on this thread. Characters update on
/// worker threads, so the scope is per thread.
#[derive(Copy, Clone)]
struct Scope {
    character: *mut Character,
    suppression: ModifierSuppression,
    /// Whether the stunt modifiers are being kept off the local player's on-foot animation.
    suppress_stunt: bool,
    /// How many `SetModifier` calls the engine made inside the call: the weapon's, when it has one.
    weapon_calls: u32,
}

/// Whether the local player stands on a vehicle's stunt area, rather than hanging from a
/// helicopter or crossing the area: its animation is the on-foot one there.
///
/// # Safety
///
/// `character` must be the live local character, on the thread running its update.
unsafe fn standing_stunt(character: &Character) -> bool {
    unsafe {
        character.IsInStuntArea() && !character.IsStuntHanging() && !character.IsStuntTraversing()
    }
}

/// The rule system's `SetModifier`, which `SetCharacterModifiers` also calls directly for the
/// stunt modifiers. Drop those two while the local player stands in a stunt area, so the on-foot
/// state selects its normal clips; the engine clears frame-bound modifiers and re-sets them every
/// tick, so the bit is never left behind.
#[detour(address = jc3gi::animation::rule_system::AnimationRuleSystem::SetModifier_ADDRESS)]
fn rule_system_set_modifier(
    rule_system: *mut AnimationRuleSystem,
    modifier: *const u32,
    modifier_type: i32,
) {
    if SCOPE.get().is_some_and(|scope| scope.suppress_stunt)
        && (ptr::eq(modifier, unsafe {
            get_Character_StuntCombatModifier() as *const u32
        }) || ptr::eq(modifier, unsafe {
            get_Character_StuntNoncombatModifier() as *const u32
        }))
    {
        return;
    }
    RULE_SYSTEM_SET_MODIFIER
        .get()
        .unwrap()
        .call(rule_system, modifier, modifier_type);
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
