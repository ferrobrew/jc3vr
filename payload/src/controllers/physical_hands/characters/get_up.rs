//! Holding a held ragdoll's get-up off while a hand holds it, and telling what ended a hold.
//!
//! The animation graph stands a ragdolling character up once its ragdolling state's timers say so:
//! the time spent ragdolling, against a threshold, and the time spent settled, its limbs slow
//! against each other and near the ground (`docs/engine/character/ragdoll.md`, "Out of the
//! ragdoll"). On the 14 November take the get-up fired about six seconds after a topple while two
//! hands held the soldier, and ended the hold. Those timers live in the character's control
//! parameters, which no binding reaches, so the hold instead queues the ragdoll act again on the
//! held character every [`REQUEUE_S`] of the hold ([`engine::requeue_ragdoll`]), on the hypothesis
//! that re-entering a ragdoll state resets them. Each hold that ends is classed by what the
//! character does next ([`HoldEnd`]), so the log shows whether a get-up ended it.

use std::num::NonZeroUsize;

use jc3gi::character::character::Character;

use crate::controllers::physical_hands::{bodies::owner_of, characters::engine};

/// How often, in seconds of the hold, the ragdoll act is queued again on a held character: well
/// within the six seconds after which the get-up fired on the 14 November take.
pub(crate) const REQUEUE_S: f32 = 1.5;

/// When a hold of a ragdoll queues the ragdoll act again: every [`REQUEUE_S`] of game time held.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(crate) struct RequeueClock {
    /// The game time the hold has lasted, seconds.
    pub held_s: f32,
    /// The game time since the act was last queued, or since the hold began, seconds.
    pub since_s: f32,
}

impl RequeueClock {
    /// Advance the clock by `dt` seconds of game time; returns whether the act is due now, which
    /// restarts the wait. A step that is not a positive number advances nothing.
    pub(crate) fn advance(&mut self, dt: f32, every_s: f32) -> bool {
        if !(dt.is_finite() && dt > 0.0) {
            return false;
        }
        self.held_s += dt;
        self.since_s += dt;
        if self.since_s >= every_s {
            self.since_s = 0.0;
            true
        } else {
            false
        }
    }
}

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

/// Queue the ragdoll act again on `character`, held by its body `body`
/// ([`engine::requeue_ragdoll`]). Returns whether an act was queued.
///
/// # Safety
///
/// The game thread, outside the physics step and the game's update. The body's current owner is
/// checked first.
pub(crate) unsafe fn requeue(character: NonZeroUsize, body: u32) -> bool {
    if unsafe { owner_of(body) } != Some(character) {
        return false;
    }
    // SAFETY: the body's owner proves the character live.
    let character = unsafe { &mut *(character.get() as *mut Character) };
    unsafe { engine::requeue_ragdoll(character) }
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
    fn the_act_is_requeued_every_interval_of_the_hold() {
        let mut clock = RequeueClock::default();
        let dt = 0.25;
        let due: Vec<bool> = (0..12).map(|_| clock.advance(dt, 1.5)).collect();
        assert_eq!(due.iter().filter(|d| **d).count(), 2);
        assert!(due[5] && due[11]);
        assert!((clock.held_s - 3.0).abs() < 1e-5);
        assert!(!clock.advance(f32::NAN, 1.5));
        assert!(!clock.advance(-1.0, 1.5));
        assert!((clock.held_s - 3.0).abs() < 1e-5);
    }

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
