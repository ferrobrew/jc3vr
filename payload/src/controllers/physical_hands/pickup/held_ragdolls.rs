//! The hands' holds of ragdolls across pickup ticks: holding the game's get-up off while a hand
//! holds a character ([`characters::get_up`]), and logging how each hold ended, with the time held
//! and whether the get-up ended it.

use std::num::NonZeroUsize;

use parking_lot::Mutex;

use crate::controllers::{
    frame::Hand,
    physical_hands::{
        characters::{
            CharactersConfig,
            get_up::{self, REQUEUE_S, RequeueClock},
        },
        pickup::{state::PickupState, tick::TickOutcome},
    },
};

/// Follow each hand's hold of a ragdoll after a pickup tick of `dt` seconds of game time: queue
/// the ragdoll act again on a held character when it is due, once per character however many
/// hands hold it, and log each hold that ended.
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
    let mut requeued: [Option<NonZeroUsize>; 2] = [None; 2];
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
            clock: RequeueClock::default(),
            requeues: 0,
            accepted: 0,
        });
        hold.body = body;
        if !hold.clock.advance(dt, REQUEUE_S)
            || !config.hold_off_get_up
            || requeued.contains(&Some(character))
        {
            continue;
        }
        requeued[i] = Some(character);
        // SAFETY: forwarded from the caller; the body's owner is checked first.
        let queued = unsafe { get_up::requeue(character, body) };
        hold.requeues += 1;
        hold.accepted += u32::from(queued);
        if hold.requeues == 1 || !queued {
            tracing::info!(
                target: "controllers",
                ?hand,
                body,
                held_s = hold.clock.held_s,
                queued,
                requeues = hold.requeues,
                "physical hands: characters: queued the ragdoll again on a held character to \
                 hold off its get-up"
            );
        }
    }
}

/// One hand's hold of a character's ragdoll.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Hold {
    character: NonZeroUsize,
    /// The body the hand holds, which proves the character alive.
    body: u32,
    clock: RequeueClock,
    /// How often the ragdoll act was queued again, and how often the state took it.
    requeues: u32,
    accepted: u32,
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
            held_s = hold.clock.held_s,
            since_requeue_s = hold.clock.since_s,
            requeues = hold.requeues,
            accepted = hold.accepted,
            loss,
            end = end.label(),
            "physical hands: characters: the game's get-up ended a hand's hold of a ragdoll"
        );
    } else {
        tracing::info!(
            target: "controllers",
            ?hand,
            body = hold.body,
            held_s = hold.clock.held_s,
            since_requeue_s = hold.clock.since_s,
            requeues = hold.requeues,
            accepted = hold.accepted,
            loss,
            end = end.label(),
            "physical hands: characters: a hand's hold of a ragdoll ended"
        );
    }
}

/// Each hand's hold of a ragdoll at the last pickup tick.
static HOLDS: Mutex<[Option<Hold>; 2]> = Mutex::new([None; 2]);
