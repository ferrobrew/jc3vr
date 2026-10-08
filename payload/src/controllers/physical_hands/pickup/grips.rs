//! The grips a hand keeps on what it cannot simply carry: a latched door until its latch gives, a
//! character until a yank, and a yanked character until the hand takes hold of the ragdoll it falls
//! as. Each runs from the pickup tick, on the main thread outside the physics step.

use std::time::{Duration, Instant};

use crate::controllers::{
    frame::Hand,
    physical_hands::{
        bodies,
        characters::{self, CharactersConfig, Regrip, Seize, SeizeStep},
        config::PickupConfig,
        doors::{self, LatchStep},
        grasp::{self, Grip, PalmMotion, Reach},
        physics::{self, Shove, ShoveForce},
        pickup::{Held, JointInputs, PickupState, TickOutcome},
    },
};

/// Keep `hand`'s grip on a latched door: hold the hand at the grab point while the latch holds,
/// and once it gives, hold the door's own body where the hand took it.
///
/// # Safety
///
/// Main thread, outside the physics step.
pub(super) unsafe fn keep_latch(
    state: &mut PickupState,
    hand: Hand,
    palm: Option<PalmMotion>,
    joints: &JointInputs,
    unlatch_pull_m: f32,
    outcome: &mut TickOutcome,
) {
    let i = hand.index();
    let (Some(latch), Some(palm)) = (state.latches[i], palm) else {
        return;
    };
    // SAFETY: forwarded from the caller.
    match unsafe { latch.step(palm.pose.position, unlatch_pull_m) } {
        LatchStep::Pin(pin) => outcome.pins[i] = Some(pin),
        LatchStep::Opened { object, body, grab } => {
            state.latches[i] = None;
            // SAFETY: forwarded from the caller; the door's body was found just now.
            match unsafe { bodies::read(body, object) } {
                Ok(read) => {
                    tracing::info!(
                        hand = hand.label(),
                        slot = doors::slot_label(latch.door.slot),
                        part = latch.door.part_index,
                        body,
                        joined = joints.join[i],
                        "physical hands: doors: the latch gave, and the hand holds the door"
                    );
                    state.held[i] = Some(Held {
                        object,
                        body,
                        grip: Grip::of(palm.pose, read.pose, grab),
                        mass_kg: read.mass_kg.unwrap_or(f32::INFINITY),
                        joined: joints.join[i],
                    });
                    outcome.unlatched[i] = true;
                }
                Err(e) => {
                    outcome.released[i] = true;
                    outcome.lost += 1;
                    outcome.error = Some(e.to_string());
                }
            }
        }
        LatchStep::Lost(e) => {
            tracing::info!(
                hand = hand.label(),
                slot = doors::slot_label(latch.door.slot),
                error = %e,
                "physical hands: doors: a grip on a latched door was lost"
            );
            state.latches[i] = None;
            outcome.released[i] = true;
            outcome.lost += 1;
            outcome.error = Some(e.to_string());
        }
    }
}

/// Keep `hand`'s grip on a character: hold the hand at the grab point, and act on a yank. A yank on
/// a seated character, through a door still open beside the hand, has the input tick click the
/// vehicle entry, which hijacks the seat; the grip lets go either way.
///
/// # Safety
///
/// Main thread, outside the physics step.
pub(super) unsafe fn keep_seize(
    state: &mut PickupState,
    hand: Hand,
    palm: Option<PalmMotion>,
    world_scale: f32,
    config: &CharactersConfig,
    outcome: &mut TickOutcome,
) {
    let i = hand.index();
    let (Some(seize), Some(palm)) = (state.seizes[i], palm) else {
        return;
    };
    let palm = palm.pose.position;
    // SAFETY: forwarded from the caller.
    match unsafe { seize.step(palm, config.yank_m * world_scale) } {
        SeizeStep::Pin(pin) => outcome.pins[i] = Some(pin),
        SeizeStep::Yank { at, direction } => {
            state.seizes[i] = None;
            let Some(seated) = seize.target.seated else {
                // A character on foot is thrown off its feet along the yank by its own hit
                // reaction; the hand then looks for the ragdoll it falls as, if the player is
                // hostile to it, and lets go otherwise.
                physics::queue_shove(Shove {
                    hand,
                    character: seize.target.character,
                    body: seize.target.body,
                    at,
                    direction,
                    force: ShoveForce::Yank {
                        strength: config.yank_strength,
                    },
                });
                outcome.yanks += 1;
                tracing::info!(
                    hand = hand.label(),
                    hostile = seize.target.hostile,
                    "physical hands: characters: a hand yanked a character"
                );
                if seize.target.hostile {
                    state.regrips[i] = Some(Regrip {
                        seize,
                        until: Instant::now()
                            + Duration::from_secs_f32(config.regrip_s.clamp(0.0, MAX_REGRIP_S)),
                    });
                } else {
                    outcome.released[i] = true;
                }
                return;
            };
            outcome.released[i] = true;
            // SAFETY: forwarded from the caller; the character, and with it its vehicle, was
            // checked alive by the step.
            match unsafe { characters::door_still_open(&seated, palm, world_scale, config) } {
                Some((door, open_deg)) => {
                    tracing::info!(
                        hand = hand.label(),
                        door = doors::slot_label(door),
                        open_deg,
                        ?direction,
                        "physical hands: hijack: a hand pulled an occupant out through the open door"
                    );
                    state.hijack_clicks[i] = true;
                    outcome.hijacks += 1;
                }
                None => {
                    tracing::info!(
                        hand = hand.label(),
                        "physical hands: hijack: the door beside the hand closed before the yank"
                    );
                }
            }
        }
        SeizeStep::Lost => {
            state.seizes[i] = None;
            outcome.released[i] = true;
            outcome.lost += 1;
            outcome.error = Some("physical hands: characters: the character is gone".to_owned());
        }
    }
}

/// The shove a hand gives the character it lets go of, when it lets go moving at least the
/// shove's least speed: a blow without damage along the hand's motion, dealt on the next frame
/// tick. A seated character is not shoved.
pub(super) fn shove_on_release(
    hand: Hand,
    seize: &Seize,
    palm: &PalmMotion,
    config: &CharactersConfig,
) {
    let speed_mps = palm.velocity.length();
    if seize.target.seated.is_some()
        || !speed_mps.is_finite()
        || speed_mps < config.shove_min_speed_mps.max(0.0)
    {
        return;
    }
    let Some(direction) = palm.velocity.try_normalize() else {
        return;
    };
    physics::queue_shove(Shove {
        hand,
        character: seize.target.character,
        body: seize.target.body,
        at: palm.pose.position,
        direction,
        force: ShoveForce::Push { speed_mps },
    });
}

/// Keep `hand` at the character it yanked while it falls: once the hand's reach finds a body of
/// the character's ragdoll, hold that body as a loose object; give up when the regrip runs out or
/// the character is gone.
///
/// # Safety
///
/// Main thread, outside the physics step.
pub(super) unsafe fn keep_regrip(
    state: &mut PickupState,
    hand: Hand,
    palm: Option<PalmMotion>,
    reach: Option<Reach>,
    joints: &JointInputs,
    config: &PickupConfig,
    outcome: &mut TickOutcome,
) {
    let i = hand.index();
    let (Some(regrip), Some(palm)) = (state.regrips[i], palm) else {
        return;
    };
    if let Some(reach) = reach.filter(|r| regrip.takes(r)) {
        state.regrips[i] = None;
        // SAFETY: forwarded from the caller; the reach found the body this tick.
        match unsafe { bodies::read(reach.body, reach.object) } {
            Ok(body) if body.mass_kg.is_some_and(|m| grasp::can_hold(m, config)) => {
                tracing::info!(
                    hand = hand.label(),
                    body = reach.body,
                    joined = joints.join[i],
                    "physical hands: characters: a hand took hold of a fallen character's ragdoll"
                );
                state.held[i] = Some(Held {
                    object: reach.object,
                    body: reach.body,
                    grip: Grip::of(palm.pose, body.pose, reach.point),
                    mass_kg: body.mass_kg.unwrap_or(f32::INFINITY),
                    joined: joints.join[i],
                });
                outcome.regrips += 1;
            }
            Ok(_) => {
                outcome.released[i] = true;
                outcome.error =
                    Some("physical hands: characters: the ragdoll's body is too heavy".to_owned());
            }
            Err(e) => {
                outcome.released[i] = true;
                outcome.error = Some(e.to_string());
            }
        }
        return;
    }
    if Instant::now() >= regrip.until {
        state.regrips[i] = None;
        outcome.released[i] = true;
        tracing::info!(
            hand = hand.label(),
            "physical hands: characters: no ragdoll came within the hand's reach after a yank"
        );
        return;
    }
    // SAFETY: forwarded from the caller.
    match unsafe { regrip.seize.step(palm.pose.position, f32::INFINITY) } {
        SeizeStep::Pin(pin) => outcome.pins[i] = Some(pin),
        SeizeStep::Yank { .. } | SeizeStep::Lost => {
            state.regrips[i] = None;
            outcome.released[i] = true;
        }
    }
}

/// The longest a hand waits for a yanked character's ragdoll, seconds.
const MAX_REGRIP_S: f32 = 5.0;
