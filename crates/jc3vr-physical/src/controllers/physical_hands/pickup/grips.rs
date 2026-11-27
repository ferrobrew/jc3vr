//! The grips a hand keeps on what it cannot simply carry: a latched door until its latch gives, a
//! character until a yank, and a yanked character until the hand takes hold of the ragdoll it falls
//! as. Each runs from the pickup tick, on the main thread outside the physics step.

use jc3vr_body::controllers::{
    grab::world_reach::{Reach, SeatAccess, TargetKind},
    physics::{bodies, error::PhysicsError, vehicles},
};
use jc3vr_core::published::Stamp;
use jc3vr_input::frame::Hand;

use crate::controllers::physical_hands::{
    characters,
    characters::{GunnerOut, Regrip, SeatOut, Seize, SeizeStep, tuning::CHARACTERS},
    doors,
    doors::LatchStep,
    grasp,
    grasp::{Grip, PalmMotion},
    physics,
    physics::{Shove, ShoveForce},
    pickup::{
        Held, HeldKind, JointInputs, PickupState, PickupTuning, TickOutcome, tick::held_mass,
    },
    world::World,
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
    unlatch_pull_m: f32,
    config: &PickupTuning,
    outcome: &mut TickOutcome,
) {
    let i = hand.index();
    let (Some(mut latch), Some(palm)) = (state.latches[i], palm) else {
        return;
    };
    // SAFETY: forwarded from the caller.
    match unsafe { latch.step(palm.pose.position, unlatch_pull_m) } {
        LatchStep::Pin(pin) => {
            state.latches[i] = Some(latch);
            outcome.pins[i] = Some(pin);
        }
        LatchStep::Opened { object, body, grab } => {
            state.latches[i] = None;
            // SAFETY: forwarded from the caller; the door's body was found just now.
            match unsafe { bodies::read(body, object) } {
                Ok(read) => {
                    let acquired =
                        match unsafe { doors::tearing::TearGrip::take(latch.door, object, body) } {
                            Ok(acquired) => acquired,
                            Err(refusal) => {
                                tracing::info!(
                                    hand = hand.label(),
                                    slot = doors::slot_label(latch.door.slot),
                                    part = latch.door.part_index,
                                    body,
                                    %refusal,
                                    "physical hands: doors: the latch gave, but the opened door's \
                                     identity changed; the hand lets go"
                                );
                                crate::controllers::physical_hands::pickup::tearing::acquired(
                                    state, i, None,
                                );
                                outcome.released[i] = true;
                                outcome.lost += 1;
                                outcome.error = Some(
                                    "physical hands: doors: the opened door's identity changed"
                                        .to_owned(),
                                );
                                return;
                            }
                        };
                    tracing::info!(
                        hand = hand.label(),
                        slot = doors::slot_label(latch.door.slot),
                        part = latch.door.part_index,
                        body,
                        "physical hands: doors: the latch gave, and the hand holds the door"
                    );
                    let world = unsafe { vehicles::vehicle_frame(latch.door.vehicle) };
                    state.carry_body(object, body);
                    state.set_held(
                        i,
                        Some(Held {
                            object,
                            body,
                            grip: Grip::of(latch.handle_palm(world), read.pose, grab),
                            mass_kg: read.mass_kg.unwrap_or(f32::INFINITY),
                            ragdoll: None,
                            // A new fixed joint would retain the pre-opening relative pose and
                            // discard the pull that undid the latch. The carry drive keeps that
                            // pull active.
                            joined: false,
                            kind: HeldKind::Door,
                        }),
                    );
                    crate::controllers::physical_hands::pickup::tearing::acquired(
                        state,
                        i,
                        Some(acquired),
                    );
                    outcome.unlatched[i] = true;
                }
                Err(e) => {
                    let now = Stamp::now();
                    let opening_at = *latch.opening_at.get_or_insert(now);
                    if matches!(
                        e,
                        PhysicsError::NoBody | PhysicsError::NotDynamic | PhysicsError::BadMotion
                    ) && now.saturating_duration_since(opening_at).as_secs_f32()
                        < config.unlatch_settle_s
                    {
                        let world = unsafe { vehicles::vehicle_frame(latch.door.vehicle) };
                        state.latches[i] = Some(latch);
                        outcome.pins[i] = Some(latch.handle_palm(world).position);
                    } else {
                        tracing::info!(
                            hand = hand.label(),
                            slot = doors::slot_label(latch.door.slot),
                            part = latch.door.part_index,
                            body,
                            error = %e,
                            "physical hands: doors: the latch gave, but the door's own body could \
                             not be read; the hand lets go"
                        );
                        outcome.released[i] = true;
                        outcome.lost += 1;
                        outcome.error = Some(e.to_string());
                    }
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
/// vehicle entry, which hijacks the seat; the grip lets go either way. A yank on one reached
/// through a broken window takes them out of the seat as a ragdoll, which the hand then takes.
///
/// # Safety
///
/// Main thread, outside the physics step.
pub(super) unsafe fn keep_seize(
    state: &mut PickupState,
    hand: Hand,
    palm: Option<PalmMotion>,
    world_scale: f32,
    outcome: &mut TickOutcome,
) {
    let i = hand.index();
    let (Some(mut seize), Some(palm)) = (state.seizes[i], palm) else {
        return;
    };
    let palm = palm.pose.position;
    if let Some(reach) = unsafe { seize.ragdoll_reach(palm) } {
        // The regrip holds the bone the reach found from here on, so a capsule the game frees as
        // the character ragdolls no longer has to prove the character live.
        if let TargetKind::Character(target) = reach.kind {
            seize.target = target;
        }
        state.seizes[i] = None;
        state.regrips[i] = Some(Regrip::new(seize));
        outcome.pins[i] = unsafe { seize.point() };
        return;
    }
    // SAFETY: forwarded from the caller.
    let tuning = &CHARACTERS.get().characters;
    match unsafe { seize.step(palm, tuning.yank_m * world_scale) } {
        SeizeStep::Pin(pin) => outcome.pins[i] = Some(pin),
        SeizeStep::Yank { at, direction } => {
            state.seizes[i] = None;
            if seize.target.seated.is_some() {
                // SAFETY: forwarded from the caller; the target's owner is checked first.
                match unsafe { characters::gunner_out(seize.target) } {
                    GunnerOut::NotAGunner => {}
                    GunnerOut::Queued => {
                        // The gunner falls where they sit, and the hand takes hold of the ragdoll
                        // as of any fallen character.
                        tracing::info!(
                            hand = hand.label(),
                            "physical hands: characters: a hand pulled a mounted gun's gunner out \
                             of the seat"
                        );
                        outcome.yanks += 1;
                        state.regrips[i] = Some(Regrip::new(seize));
                        return;
                    }
                    refused @ (GunnerOut::Refused | GunnerOut::NoTransition) => {
                        // A gunner is never hijacked: the vehicle-entry action would seat the
                        // player in another seat and leave the gunner where they are.
                        tracing::info!(
                            hand = hand.label(),
                            reason = refused.label(),
                            "physical hands: characters: a mounted gun's gunner stays in the seat"
                        );
                        outcome.released[i] = true;
                        return;
                    }
                }
            }
            if seize
                .target
                .seated
                .is_some_and(|seat| seat.access == SeatAccess::Pane)
            {
                // SAFETY: forwarded from the caller; the access and the target's owner are checked
                // first.
                let out = unsafe { characters::occupant_access(seize.target, palm, world_scale) }
                    .and_then(|_| unsafe { characters::unseat(seize.target) });
                match out {
                    Some(SeatOut::Queued) => {
                        // The occupant falls where they sit, and the hand takes hold of the
                        // ragdoll as of any fallen character, to pull out through the opening.
                        tracing::info!(
                            hand = hand.label(),
                            ?direction,
                            "physical hands: characters: a hand pulled an occupant out of the seat \
                             through a broken window"
                        );
                        outcome.yanks += 1;
                        state.regrips[i] = Some(Regrip::new(seize));
                    }
                    refused => {
                        tracing::info!(
                            hand = hand.label(),
                            reason = refused
                                .map_or("no longer seated in the same vehicle", SeatOut::label),
                            "physical hands: characters: an occupant reached through a broken window \
                             stays in the seat"
                        );
                        outcome.released[i] = true;
                    }
                }
                return;
            }
            let exposed = seize
                .target
                .seated
                .is_some_and(|seat| seat.access == SeatAccess::Exposed)
                && unsafe { characters::occupant_access(seize.target, palm, world_scale) }
                    .is_some_and(|seat| seat.access == SeatAccess::Exposed);
            if seize.target.seated.is_none() || exposed {
                // An exposed rider is pulled by the hit reaction, not the seat-entry action.
                // The anatomical grip stays pending until the resulting ragdoll is dynamic.
                physics::queue_shove(Shove {
                    hand,
                    character: seize.target.character,
                    body: seize.target.body,
                    at,
                    direction,
                    force: ShoveForce::Yank {
                        strength: tuning.yank_strength,
                    },
                });
                outcome.yanks += 1;
                tracing::info!(
                    hand = hand.label(),
                    hostile = seize.target.hostile,
                    "physical hands: characters: a hand yanked a character"
                );
                state.regrips[i] = Some(Regrip::new(seize));
                return;
            }
            outcome.released[i] = true;
            // SAFETY: the current attachment is read only after rechecking character ownership.
            match unsafe { characters::occupant_access(seize.target, palm, world_scale) } {
                Some(seated) => {
                    tracing::info!(
                        hand = hand.label(),
                        door = seated
                            .access
                            .door()
                            .map(|(slot, _)| doors::slot_label(slot)),
                        open_deg = seated.access.door().map(|(_, open_deg)| open_deg),
                        ?direction,
                        "physical hands: hijack: a hand pulled an accessible vehicle occupant"
                    );
                    state.hijack_clicks[i] = true;
                    outcome.hijacks += 1;
                }
                None => {
                    tracing::info!(
                        hand = hand.label(),
                        "physical hands: hijack: the occupant is no longer accessible before the yank"
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
pub(super) fn shove_on_release(hand: Hand, seize: &Seize, palm: &PalmMotion) {
    let speed_mps = palm.velocity.length();
    if seize.target.seated.is_some()
        || !speed_mps.is_finite()
        || speed_mps < CHARACTERS.get().characters.shove_min_speed_mps
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
/// the character's ragdoll, hold that body as a loose object; give up when the regrip has run out
/// ([`Regrip::run_down`]) or the character is gone.
///
/// # Safety
///
/// Main thread, outside the physics step.
pub(super) unsafe fn keep_regrip(
    state: &mut PickupState,
    hand: Hand,
    palm: Option<PalmMotion>,
    _reach: Option<Reach>,
    joints: &JointInputs,
    config: &PickupTuning,
    outcome: &mut TickOutcome,
) {
    let i = hand.index();
    let (Some(regrip), Some(palm)) = (state.regrips[i], palm) else {
        return;
    };
    if let Some(reach) = unsafe { regrip.seize.ragdoll_reach(palm.pose.position) } {
        state.regrips[i] = None;
        // SAFETY: forwarded from the caller; the reach found the body this tick.
        match unsafe { bodies::read(reach.body, reach.object) } {
            Ok(body) if body.mass_kg.is_some_and(|m| grasp::can_hold(m, config)) => {
                let Some(mut world) = (unsafe { World::get() }) else {
                    outcome.released[i] = true;
                    return;
                };
                unsafe { world.world_mut().activateBody(reach.body) };
                let mass_kg = held_mass(&reach.kind, body.mass_kg.unwrap_or(f32::INFINITY));
                tracing::info!(
                    hand = hand.label(),
                    body = reach.body,
                    mass_kg,
                    joined = joints.join[i],
                    "physical hands: characters: a hand took hold of a fallen character's ragdoll"
                );
                let (kind, joined) =
                    state.body_mode(reach.object, reach.body, HeldKind::Ragdoll, joints.join[i]);
                state.set_held(
                    i,
                    Some(Held {
                        object: reach.object,
                        body: reach.body,
                        grip: Grip::regrip(palm.pose, body.pose, reach.point, regrip.seize.in_palm),
                        mass_kg,
                        ragdoll: Some(regrip.seize.target.character),
                        joined,
                        kind,
                    }),
                );
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
    // A topple the hit reaction ignored leaves the character standing: retry its ragdoll once.
    if regrip.retry_due() {
        // SAFETY: forwarded from the caller.
        let queued = unsafe { characters::retry_ragdoll(regrip.seize.target) };
        tracing::info!(
            hand = hand.label(),
            queued,
            "physical hands: characters: the topple left the character standing; retried the ragdoll"
        );
        if let Some(regrip) = state.regrips[i].as_mut() {
            regrip.retried = true;
        }
    }
    if regrip.expired() {
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
