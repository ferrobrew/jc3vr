//! Holding a held ragdoll's get-up off while a hand holds it, and telling what ended a hold.
//!
//! The animation graph stands a ragdolling character up once its ragdolling timers say so
//! ([`RagdollingControllerParams`]): the time spent ragdolling, against a threshold, and the time
//! spent settled, its limbs slow against each other and near the ground
//! (`docs/engine/character/ragdoll.md`, "Out of the ragdoll"). On the 14 November take the get-up
//! fired about six seconds after a topple while two hands held the soldier, and ended the hold.
//! So each pickup tick of a hold writes the held character's timers back to where no get-up
//! condition holds ([`hold_timers`]). Each hold that ends is classed by what the character does
//! next ([`HoldEnd`]), so the log shows whether a get-up ended it anyway.
//!
//! [`RagdollingControllerParams`]: jc3gi::character::control_parameters::RagdollingControllerParams

use std::num::NonZeroUsize;

use jc3gi::character::character::Character;

use crate::controllers::physical_hands::{bodies::owner_of, characters::engine};

/// Write back the ragdolling timers of `character`, held by its body `body`, so no get-up
/// condition holds: the total below zero, as the ragdolling state task keeps it while the
/// ragdoll's owner moves it (where the timer condition never holds, whichever way it compares),
/// and the settled time at zero. Returns whether the timers were written; a protected or dead
/// character, or one that no longer ragdolls, is left alone.
///
/// # Safety
///
/// The game thread, outside the physics step and the game's update. The body's current owner is
/// checked first.
pub(crate) unsafe fn hold_timers(character: NonZeroUsize, body: u32) -> bool {
    if unsafe { owner_of(body) } != Some(character) {
        return false;
    }
    // SAFETY: the body's owner proves the character live.
    let character = unsafe { &mut *(character.get() as *mut Character) };
    if unsafe { engine::protection(character) }.is_some()
        || unsafe { engine::dead(character) }
        || !unsafe { character.IsRagdolling() }
    {
        return false;
    }
    let Some(parameters) = (unsafe { character.m_ControlParameters.as_mut() }) else {
        return false;
    };
    let timers = &mut parameters.m_RagdollingControllerParameters;
    timers.m_TotalRagdollingTime = HELD_TOTAL_S;
    timers.m_SettledRagdollingTime = 0.0;
    true
}

/// The total ragdolling time a held character is kept at: below zero, which the timer condition
/// reads as the ragdoll not being its own.
const HELD_TOTAL_S: f32 = -1.0;

/// What a held character does once a hand's hold of its ragdoll ended.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum HoldEnd {
    /// The game's get-up runs: the character stands up out of its ragdoll.
    GettingUp,
    /// The character, alive, no longer ragdolls and is not getting up: it already stood.
    Standing,
    /// It still ragdolls: the hold ended by the hand or the joint, not by the game.
    Ragdolling,
    /// It is dead.
    Dead,
    /// The held body no longer belongs to it: the character or its ragdoll went away.
    Gone,
}

impl HoldEnd {
    /// The class of a hold's end from what the character's state says: whether its body is still
    /// its own, whether it is dead, getting up, or ragdolling.
    pub(crate) fn classify(owned: bool, dead: bool, getting_up: bool, ragdolling: bool) -> Self {
        if !owned {
            HoldEnd::Gone
        } else if dead {
            HoldEnd::Dead
        } else if getting_up {
            HoldEnd::GettingUp
        } else if ragdolling {
            HoldEnd::Ragdolling
        } else {
            HoldEnd::Standing
        }
    }

    /// Whether the game took the character back to its feet.
    pub(crate) const fn by_get_up(self) -> bool {
        matches!(self, HoldEnd::GettingUp | HoldEnd::Standing)
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            HoldEnd::GettingUp => "the character is getting up",
            HoldEnd::Standing => "the character stands",
            HoldEnd::Ragdolling => "the character still ragdolls",
            HoldEnd::Dead => "the character is dead",
            HoldEnd::Gone => "the held body is no longer the character's",
        }
    }
}

/// What `character`, whose body `body` a hand held, does now that the hold ended.
///
/// # Safety
///
/// The game thread, outside the physics step. The body's current owner is checked first.
pub(crate) unsafe fn hold_end(character: NonZeroUsize, body: u32) -> HoldEnd {
    if unsafe { owner_of(body) } != Some(character) {
        return HoldEnd::Gone;
    }
    // SAFETY: the body's owner proves the character live.
    let character = unsafe { &*(character.get() as *const Character) };
    HoldEnd::classify(
        true,
        unsafe { engine::dead(character) },
        unsafe { engine::getting_up(character) },
        unsafe { character.IsRagdolling() },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hold_end_is_classed_by_what_the_character_does() {
        assert_eq!(HoldEnd::classify(false, false, true, false), HoldEnd::Gone);
        assert_eq!(HoldEnd::classify(true, true, false, true), HoldEnd::Dead);
        assert_eq!(
            HoldEnd::classify(true, false, true, false),
            HoldEnd::GettingUp
        );
        assert_eq!(
            HoldEnd::classify(true, false, false, true),
            HoldEnd::Ragdolling
        );
        assert_eq!(
            HoldEnd::classify(true, false, false, false),
            HoldEnd::Standing
        );
        assert!(HoldEnd::GettingUp.by_get_up() && HoldEnd::Standing.by_get_up());
        assert!(!HoldEnd::Ragdolling.by_get_up() && !HoldEnd::Gone.by_get_up());
    }
}
