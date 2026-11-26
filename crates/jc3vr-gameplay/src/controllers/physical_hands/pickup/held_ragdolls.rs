//! The hands' holds of ragdolls across pickup ticks: holding the game's get-up off while a hand
//! holds a character ([`get_up`]), and logging how each hold ended, with the time held and whether
//! the get-up ended it.

use std::num::NonZeroUsize;

use parking_lot::Mutex;

use crate::controllers::{
    frame::Hand,
    physical_hands::{
        characters::{CharactersConfig, get_up},
        pickup::{state::PickupState, tick::TickOutcome},
    },
};

/// Follow each hand's hold of a ragdoll after a pickup tick of `dt` seconds of game time: write
/// back each held character's ragdolling timers, once per character however many hands hold it,
/// and log each hold that ended.
///
/// # Safety
///
/// The game thread, outside the physics step and the game's update, after the pickup tick.
pub(super) unsafe fn follow(
    state: &PickupState,
    outcome: &TickOutcome,
    config: &CharactersConfig,
    dt: f32,
) {
    let mut holds = HOLDS.lock();
    let mut held: [Option<NonZeroUsize>; 2] = [None; 2];
    for hand in Hand::BOTH {
        let i = hand.index();
        let now = state.held[i].and_then(|held| held.ragdoll.map(|c| (c, held.body)));
        if let Some(hold) = holds[i]
            && now.is_none_or(|(character, _)| character != hold.character)
        {
            holds[i] = None;
            // SAFETY: forwarded from the caller; the body's owner is checked first.
            unsafe { log_end(hand, hold, outcome) };
        }
        let Some((character, body)) = now else {
            continue;
        };
        let hold = holds[i].get_or_insert(Hold {
            character,
            body,
            held_s: 0.0,
            timers_held: false,
        });
        hold.body = body;
        if dt.is_finite() && dt > 0.0 {
            hold.held_s += dt;
        }
        if !config.hold_off_get_up || held.contains(&Some(character)) {
            continue;
        }
        held[i] = Some(character);
        // SAFETY: forwarded from the caller; the body's owner is checked first.
        let written = unsafe { get_up::hold_timers(character, body) };
        if written && !hold.timers_held {
            tracing::info!(
                target: "controllers",
                ?hand,
                body,
                "physical hands: characters: holding a held character's ragdolling timers off \
                 its get-up"
            );
        }
        hold.timers_held |= written;
    }
}

/// One hand's hold of a character's ragdoll.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Hold {
    character: NonZeroUsize,
    /// The body the hand holds, which proves the character alive.
    body: u32,
    /// The game time the hold has lasted, seconds.
    held_s: f32,
    /// Whether the hold wrote the character's ragdolling timers back.
    timers_held: bool,
}

/// Log how `hand`'s hold `hold` ended: what the character does now, how long it was held, and why
/// the hold ended when it was lost rather than let go.
///
/// # Safety
///
/// As [`follow`].
unsafe fn log_end(hand: Hand, hold: Hold, outcome: &TickOutcome) {
    let end = unsafe { get_up::hold_end(hold.character, hold.body) };
    let loss = outcome.losses[hand.index()].map(|loss| loss.label());
    if end.by_get_up() {
        tracing::warn!(
            target: "controllers",
            ?hand,
            body = hold.body,
            held_s = hold.held_s,
            timers_held = hold.timers_held,
            loss,
            end = end.label(),
            "physical hands: characters: the game's get-up ended a hand's hold of a ragdoll"
        );
    } else {
        tracing::info!(
            target: "controllers",
            ?hand,
            body = hold.body,
            held_s = hold.held_s,
            timers_held = hold.timers_held,
            loss,
            end = end.label(),
            "physical hands: characters: a hand's hold of a ragdoll ended"
        );
    }
}

/// Each hand's hold of a ragdoll at the last pickup tick.
static HOLDS: Mutex<[Option<Hold>; 2]> = Mutex::new([None; 2]);
