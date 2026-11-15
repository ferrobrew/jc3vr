//! Damping what the hands hold within itself, after each pickup tick: a held ragdoll's bones
//! ([`characters::damp_held_ragdoll`]) and an articulated object's assembly
//! ([`articulated::damp_assembly`]). A hand drives only the body it holds, and the bodies hung off
//! it on constraints of their own kept their momentum and rang against the hand.

use std::num::NonZeroUsize;

use parking_lot::Mutex;

use crate::controllers::{
    frame::Hand,
    physical_hands::{
        articulated, characters, physics,
        pickup::state::{HeldKind, PickupState},
    },
};

/// Damp each assembly the hands hold once, however many hands hold it, over a tick of `dt`
/// seconds.
///
/// # Safety
///
/// The main thread, outside the physics step, after the pickup tick kept or let go the holds.
pub(super) unsafe fn damp_held(state: &PickupState, dt: f32) {
    let mut ragdolls: [Option<NonZeroUsize>; 2] = [None; 2];
    let mut assemblies: [Option<u32>; 2] = [None; 2];
    let hand_bodies = physics::status().hands.map(|h| h.body_id);
    let mut noted = NOTED.lock();
    for hand in Hand::BOTH {
        let i = hand.index();
        let Some(held) = state.held[i] else {
            noted[i] = None;
            continue;
        };
        if let Some(character) = held.ragdoll {
            if !ragdolls.contains(&Some(character)) {
                ragdolls[i] = Some(character);
                // SAFETY: forwarded from the caller; the hold kept the body this tick.
                unsafe { characters::damp_held_ragdoll(character, held.body, dt) };
            }
            continue;
        }
        if held.kind != HeldKind::Articulated || assemblies.contains(&Some(held.body)) {
            continue;
        }
        assemblies[i] = Some(held.body);
        // SAFETY: forwarded from the caller.
        let damping = unsafe {
            articulated::damp_assembly(held.body, hand_bodies, ASSEMBLY_DAMPING_PER_S, dt)
        };
        if noted[i] != Some(held.body) {
            noted[i] = Some(held.body);
            tracing::info!(
                target: "controllers",
                ?hand,
                body = held.body,
                bodies = damping.map(|d| d.bodies),
                mass_kg = damping.map(|d| d.mass_kg),
                "physical hands: damping a held articulated object's assembly within itself"
            );
        }
    }
}

/// How fast a held articulated object's motion within its assembly dies down, per second: the
/// rate a held ragdoll's bones are damped at.
const ASSEMBLY_DAMPING_PER_S: f32 = 4.0;

/// The body each hand's articulated hold was last logged for, so each hold is logged once.
static NOTED: Mutex<[Option<u32>; 2]> = Mutex::new([None; 2]);
