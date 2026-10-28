//! The pre-simulation half of the hand climbing's tick: take and end the grips, and move the
//! player while a hand grips.

use std::time::Instant;

use glam::{Quat, Vec3};
use jc3gi::character::character::Character;

use crate::{
    config::Config,
    controllers::{
        frame::Hand,
        physical_hands::{
            climb::{
                engine, native,
                solve::{
                    ClimbTuning, Follow, MantleTuning, body_target, carry_velocity, climb_velocity,
                    fling_carry, is_ledge, release_velocity, smooth_motion,
                },
                state::{
                    Carry, Event, Grip, LOG_PERIOD, MAX_MANTLE_STEPS, MAX_STALE_STEPS, MantleStep,
                    MantleTick, Request, STATE, enabled,
                },
            },
            physics::{self, drive::Goal},
            stunt,
        },
        roomscale::teleport,
    },
    sim_rate::scale_ticks,
};

/// The climb's part of the hand bodies' pre-simulation step: take and end the grips, and move the
/// player while a hand grips. `bodies` holds each acting hand body's pose, game space, `None` for
/// a hand whose body does not act this step. Returns where each gripping hand's body is held.
///
/// # Safety
///
/// The thread that steps the physics world, outside the step, after the characters' proxy
/// updates (the pre-simulation callback).
pub(crate) unsafe fn step(bodies: [Option<(Vec3, Quat)>; 2]) -> [Option<Goal>; 2] {
    let config = Config::lock_query(|c| c.controllers.physical_hands);
    let context = unsafe { engine::gate_context() };
    native::observe(context);
    let mut state = STATE.lock();
    state.stepped = Some(Instant::now());
    let character = unsafe { Character::GetLocalPlayerCharacter() };
    let player =
        unsafe { character.as_ref() }.map(|c| (character as usize, c.m_PfxCharacter.px as usize));
    if state.player != player {
        state.requests = [None; 2];
        state.carry = None;
        state.drop_all("the local player's proxy changed");
        state.finish_climb();
        state.player = player;
    }
    if state.publishes == state.stepped_publish {
        state.stale_steps = state.stale_steps.saturating_add(1);
    } else {
        state.stale_steps = 0;
        state.stepped_publish = state.publishes;
    }
    // The game holds the player in a stunt position: it moves the player with the vehicle, so the
    // grips hold the hands where they took hold and move nothing. Grips taken before it did were
    // taken for a body it has since moved, so they let go.
    let held_by_game = stunt::game_holds();
    if held_by_game && !state.held_by_game {
        native::clear();
        state.drop_all("the game took the player into a stunt position");
        state.carry = None;
        state.finish_climb();
    }
    state.held_by_game = held_by_game;
    let reason = if !enabled(&config) {
        Some("climbing is off")
    } else if teleport::epoch() != state.epoch {
        state.epoch = teleport::epoch();
        Some("the player was teleported")
    } else if state.input_at.is_none_or(|at| {
        Instant::now().saturating_duration_since(at)
            > crate::controllers::physical_hands::climb::state::STALE_AFTER
    }) {
        Some("the hand input stopped publishing")
    } else if state.stale_steps > scale_ticks(MAX_STALE_STEPS) {
        Some("the hands stopped publishing")
    } else {
        None
    };
    if let Some(reason) = reason {
        native::clear();
        state.requests = [None; 2];
        state.carry = None;
        state.drop_all(reason);
        state.finish_climb();
        return [None; 2];
    }

    // SAFETY: forwarded from the caller.
    let place = unsafe { engine::player_place() };
    // SAFETY: forwarded from the caller; the proxy's update ran before this callback, and found
    // this support afresh rather than the one the last step wrote.
    let support = unsafe { engine::support() };
    let step_s = physics::step_seconds();
    let mantle_tuning = MantleTuning {
        start_m: config.climb.mantle_start_m,
        clearance_m: config.climb.mantle_clearance_m,
        forward_m: config.climb.mantle_forward_m,
        speed_mps: config.climb.mantle_speed_mps,
        min_speed_mps: config.climb.mantle_min_speed_mps,
        min_climb_m: config.climb.mantle_min_climb_m,
        land_past_m: config.climb.mantle_land_past_m,
        step_s,
    };
    let rig_epoch = state.rig_epoch;
    let mut released_by_grip = None;
    let drops_before = state.status.drops;
    for hand in Hand::BOTH {
        let i = hand.index();
        if state.grips[i].is_some_and(|g| !unsafe { g.body_holds() }) {
            state.drop_hand(hand, "the gripped body's identity changed");
            state.smoothed = Vec3::ZERO;
            state.smoothed_carry = Vec3::ZERO;
            released_by_grip = None;
        }
        match state.requests[i].take() {
            Some(Request::Grip(hold))
                if state.grips[i].is_none() && state.input_held[i] && unsafe { hold.valid() } =>
            {
                // SAFETY: forwarded from the caller.
                let frame = unsafe { engine::body_frame(hold.body) }.filter(|f| {
                    engine::climbable(f, config.climb.moving, config.climb.min_moving_mass_kg)
                });
                let grip = match (bodies[i], state.palms[i], place, frame) {
                    (Some(pin), Some(palm), Ok(place), Some(frame)) => Some(Grip {
                        hand,
                        ledge: is_ledge(
                            hold.normal.y,
                            hold.point.y,
                            place.feet.y,
                            support.is_some(),
                            config.climb.min_ledge_normal_y,
                            &mantle_tuning,
                        ),
                        hold,
                        body: place.body,
                        palm,
                        pin,
                        frame: (!frame.is_static).then_some(frame.transform),
                        rig_epoch,
                        missing_steps: 0,
                    }),
                    _ => None,
                };
                if let Some(grip) = grip {
                    tracing::info!(
                        hand = hand.label(),
                        body = hold.body,
                        moving = grip.frame.is_some(),
                        point = ?hold.point,
                        normal = ?hold.normal,
                        "hand climbing: a hand gripped the world"
                    );
                    state.grips[i] = Some(grip);
                    state.status.grips += 1;
                    state.status.moving_grips += u64::from(grip.frame.is_some());
                    state.events.push((hand, Event::Gripped));
                } else {
                    tracing::debug!(
                        hand = hand.label(),
                        body = hold.body,
                        has_body = bodies[i].is_some(),
                        "hand climbing: a grip found nothing to take"
                    );
                }
            }
            Some(Request::Release) if state.grips[i].take().is_some() => {
                released_by_grip = Some(hand);
                state.status.releases += 1;
                state.events.push((hand, Event::Released));
                tracing::debug!(hand = hand.label(), "hand climbing: a hand let go");
            }
            _ => {}
        }
        // A hand whose body stops acting for a step or two (a flicker of the mode, a dropped
        // tracking sample) keeps its grip; one that stays out lets go.
        if let Some(grip) = state.grips[i].as_mut() {
            grip.missing_steps = if bodies[i].is_some() {
                0
            } else {
                grip.missing_steps.saturating_add(1)
            };
            if grip.missing_steps > scale_ticks(MAX_STALE_STEPS) {
                state.drop_hand(hand, "the hand's body stopped acting");
            }
        }
        // The ground written for the proxy below must stay the body the hand took: a static body
        // of the world, or a moving body that still exists.
        // SAFETY: forwarded from the caller.
        if let Some(grip) = state.grips[i]
            && !unsafe { grip.body_holds() }
        {
            state.drop_hand(hand, "the gripped body is gone, or no longer static");
        }
    }

    if state.status.drops != drops_before {
        released_by_grip = None;
    }
    if state.grips.iter().all(Option::is_none) {
        if let Some(hand) = released_by_grip
            && !held_by_game
            && state.climbing
            && (config.climb.fling || state.smoothed_carry != Vec3::ZERO)
        {
            let kept = release_velocity(
                state.smoothed,
                state.smoothed_carry,
                if config.climb.fling && state.mantle.is_none() {
                    config.climb.fling_scale
                } else {
                    0.0
                },
                config.climb.max_fling_speed_mps,
            );
            // SAFETY: forwarded from the caller.
            match unsafe { engine::set_velocity(kept) } {
                Ok(()) => {
                    state.status.last_fling_mps = Some(kept.length());
                    state.carry = Some(Carry { kept, steps: 0 });
                    let max = config.climb.max_fling_speed_mps;
                    let share = if max > 0.0 {
                        (kept.length() / max).min(1.0)
                    } else {
                        0.0
                    };
                    state.events.push((hand, Event::Flung(share)));
                    tracing::info!(
                        speed_mps = kept.length(),
                        velocity = ?kept,
                        "hand climbing: let go of the world"
                    );
                }
                Err(e) => state.status.last_error = Some(e.to_string()),
            }
        } else if let Some(carry) = state.carry.filter(|_| !held_by_game) {
            // The game may take a fling's velocity back on the steps after the release, so the
            // fling keeps carrying the body as a throw for a moment, until it stands somewhere.
            let elapsed_s = (carry.steps + 1) as f32 * step_s;
            if support.is_some() || elapsed_s > config.climb.fling_carry_s {
                state.carry = None;
            } else {
                let velocity = fling_carry(carry.kept, elapsed_s, config.pickup.gravity_mps2);
                // SAFETY: forwarded from the caller.
                match unsafe { engine::set_velocity(velocity) } {
                    Ok(()) => {
                        state.carry = Some(Carry {
                            steps: carry.steps + 1,
                            ..carry
                        })
                    }
                    Err(e) => {
                        state.status.last_error = Some(e.to_string());
                        state.carry = None;
                    }
                }
            }
        }
        state.finish_climb();
        return [None; 2];
    }
    state.carry = None;

    let place = match place {
        Ok(place) => place,
        Err(e) => {
            state.status.last_error = Some(e.to_string());
            state.drop_all("the player has no body to move");
            state.finish_climb();
            return [None; 2];
        }
    };
    if held_by_game {
        // SAFETY: forwarded from the caller.
        return unsafe { state.hold_only() };
    }
    if !state.climbing {
        state.climbing = true;
        state.smoothed = Vec3::ZERO;
        state.start_feet_y = place.feet.y;
        tracing::info!(feet = ?place.feet, "hand climbing: climbing starts");
    }
    let tuning = ClimbTuning {
        gain: config.climb.gain,
        max_speed_mps: config.climb.max_speed_mps,
        step_s,
    };
    let palms = state.palms;
    for grip in state.grips.iter_mut().flatten() {
        if grip.rig_epoch != rig_epoch {
            // The rig moved under the palms: hold the body where it is from here, and carry the
            // grip with its body from here.
            grip.rig_epoch = rig_epoch;
            grip.body = place.body;
            grip.palm = palms[grip.hand.index()].unwrap_or(grip.palm);
            // SAFETY: forwarded from the caller.
            if let Some(carried) = unsafe { grip.carried() } {
                grip.hold.point = carried.point;
                grip.pin = carried.pin;
                grip.frame = Some(carried.frame);
            }
        }
    }
    // A grip on a moving body is carried with it: where the hands would have the player moves as
    // the gripped point does, and the point's own velocity carries the player along.
    // SAFETY: forwarded from the caller.
    let carried = state
        .grips
        .map(|grip| grip.and_then(|g| unsafe { g.carried() }));
    let carry =
        carry_velocity(
            state.grips.iter().enumerate().filter_map(|(i, grip)| {
                grip.map(|_| carried[i].map_or(Vec3::ZERO, |c| c.velocity))
            }),
            config.climb.max_carry_speed_mps,
        );
    let follows = Hand::BOTH.map(|hand| {
        let i = hand.index();
        let grip = state.grips[i]?;
        let palm = state.palms[i]?;
        let moved = carried[i].map_or(Vec3::ZERO, |c| c.point - grip.hold.point);
        Some(Follow {
            body: grip.body + moved,
            grip: grip.palm,
            palm,
        })
    });
    let gripping = follows.iter().flatten().count();
    let target = body_target(follows.into_iter().flatten()).unwrap_or(place.body);
    let gap = target - place.body;
    state.status.gap_m = gap.length();
    // The mantle carries the body away from where the hands would have it, on purpose.
    if state.mantle.is_none() && gap.length() > config.climb.break_distance_m {
        state.drop_all("the body could not follow the hands");
        state.finish_climb();
        return [None; 2];
    }
    let mut velocity = climb_velocity(target, place.body, &tuning) + carry;

    let stands = support.is_some_and(|y| y >= config.climb.min_ledge_normal_y);
    if config.climb.mantle {
        let mantle = state.step_mantle(
            MantleTick {
                feet: place.feet,
                supported: support.is_some(),
                stands,
                tuning: &mantle_tuning,
                max_steps: scale_ticks(MAX_MANTLE_STEPS),
                context,
            },
            |hold| {
                let valid = unsafe { hold.valid() };
                let frame = valid
                    .then(|| unsafe { engine::body_frame(hold.body) })
                    .flatten();
                (valid, frame)
            },
        );
        match mantle {
            MantleStep::Inactive => {}
            MantleStep::Move(over) => velocity = over + carry,
            MantleStep::Completed(landing) => {
                if let (Some((owner, landing)), Some(context)) = (landing, context) {
                    unsafe { native::landed(context, owner, landing, Instant::now()) };
                }
                for hand in Hand::BOTH {
                    if state.grips[hand.index()].take().is_some() {
                        state.events.push((hand, Event::Released));
                    }
                }
                state.finish_climb();
                return [None; 2];
            }
            MantleStep::Cancelled(reason) => {
                state.drop_all(reason);
                state.finish_climb();
                return [None; 2];
            }
        }
    }

    let ground = Hand::BOTH
        .into_iter()
        .find_map(|hand| state.grips[hand.index()].map(|g| g.hold.body));
    let Some(ground) = ground else {
        // The check above leaves a grip, but a climb with none to stand on ends rather than panics.
        state.finish_climb();
        return [None; 2];
    };
    // SAFETY: forwarded from the caller; the ground is a body the step checked above.
    if let Err(e) = unsafe { engine::hold(velocity, ground, carry) } {
        state.status.last_error = Some(e.to_string());
        state.drop_all("the player could not be held");
        state.finish_climb();
        return [None; 2];
    }
    let motion_grips = state.grips.map(|g| g.map(|g| g.hold));
    (state.smoothed, state.smoothed_carry) = smooth_motion(
        (state.smoothed, state.smoothed_carry),
        (velocity, carry),
        motion_grips != state.motion_grips,
        step_s,
        config.climb.fling_smoothing_s,
    );
    state.motion_grips = motion_grips;
    if let Some(context) = context {
        unsafe { native::hold(context, motion_grips, Instant::now()) };
    }
    state.status.velocity = velocity;
    state.status.climbing = true;
    state.status.mantling = state.mantle.is_some();
    let now = Instant::now();
    if state
        .logged_at
        .is_none_or(|at| now.saturating_duration_since(at) >= LOG_PERIOD)
    {
        state.logged_at = Some(now);
        tracing::debug!(
            velocity = ?velocity,
            gap_m = gap.length(),
            hands = gripping,
            mantling = state.mantle.is_some(),
            stands,
            "hand climbing: climb"
        );
    }
    for hand in Hand::BOTH {
        state.status.hands[hand.index()].grip = state.grips[hand.index()].map(|g| g.hold);
    }
    Hand::BOTH.map(|hand| {
        let i = hand.index();
        let grip = state.grips[i]?;
        let (position, orientation) = carried[i].map_or(grip.pin, |c| c.pin);
        Some(Goal {
            position,
            orientation,
        })
    })
}
