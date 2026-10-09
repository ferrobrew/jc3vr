//! The pre-simulation half of the hand bodies' tick: make, add, park, and drive the bodies toward
//! where their palms will be at the end of the step, then keep the held guns' bodies on them.

use std::{
    sync::atomic::{AtomicU32, Ordering},
    time::Instant,
};

use glam::{Quat, Vec2, Vec3};
use jc3gi::{character::character::Character, physics::PhysicsSystem};

use crate::{
    config::Config,
    controllers::{
        aim::{
            ray::Ray,
            raycast::{self, CastOptions},
        },
        frame::{Hand, Pose},
        physical_hands::{
            climb,
            physics::{
                body::{BodyShape, BodySpec, Capsule, HandBody, player_system_group},
                contact,
                drive::{DriveTuning, Goal, PushTuning, contact_gain, lag_push, smooth_push},
                guns::{self, step::GunStep},
                joint::Joint,
                lifecycle::{ActRequest, HANDS, RenderTuning},
                render::RenderOffset,
                state::{
                    MAX_LEAD_S, PARK_AFTER, READOUT_PERIOD, RENDER_MAX_TURN_RAD,
                    RENDER_TURN_DEADBAND_RAD, SHARED, STALE_AFTER, enabled, gun_tuning,
                    holding_tuning, press_load,
                },
                types::{BodyPhase, HandBodyStatus},
            },
            vault,
        },
    },
    hooks, sim_rate,
};

/// The pre-simulation step: make, add, park, and drive the hand bodies. Called by the callback with
/// the time the physics update was given, `given_dt`.
///
/// # Safety
///
/// The thread that steps the physics world, outside the step.
pub(crate) unsafe fn step(given_dt: f32) {
    let now = Instant::now();
    // The step the world is about to take, which the drives, the climb, and the proxy's velocity
    // read: the engine's 30 Hz constant, or the tick's step under the sim-rate experiment.
    let step_s = sim_rate::world_step_seconds(Some(given_dt));
    STEP_S.store(step_s.to_bits(), Ordering::Relaxed);
    let config = Config::lock_query(|c| c.controllers.physical_hands);
    contact::step_tick();
    let mut hands = HANDS.lock();
    hands.steps += 1;
    hands.placed = [false; 2];
    let reason = if !enabled(&config) {
        Some("the hand bodies are off")
    } else if !hooks::in_gameplay() {
        Some("the game left gameplay")
    } else if crate::is_shutting_down() {
        Some("the payload is shutting down")
    } else {
        None
    };
    if let Some(reason) = reason {
        // SAFETY: forwarded from the caller; no hand body acts, so every grip lets go.
        unsafe { climb::step([None; 2]) };
        vault::stand_down();
        let destroyed = unsafe { hands.destroy_all(reason) };
        guns::step::stand_down();
        let mut shared = SHARED.lock();
        shared.status.destroyed += destroyed;
        shared.status.steps = hands.steps;
        shared.status.hands = [HandBodyStatus::default(); 2];
        shared.status.push = Vec2::ZERO;
        shared.push = Vec2::ZERO;
        shared.world_offsets = [RenderOffset::NONE; 2];
        shared.offset_timeline.reset();
        shared.loads = [0.0; 2];
        shared.body_poses = [None; 2];
        return;
    }

    let (
        targets,
        published,
        world_scale,
        mut push,
        (holds, pins),
        rig_moves,
        park_now,
        follow_through,
        pressed,
    ) = {
        let mut shared = SHARED.lock();
        (
            shared.targets,
            shared.published,
            shared.world_scale,
            shared.push,
            (shared.holds, shared.pins),
            std::mem::take(&mut shared.rig_moves),
            shared
                .park_now
                .filter(|(at, _)| now.saturating_duration_since(*at) <= STALE_AFTER)
                .map_or([false; 2], |(_, park)| park),
            shared
                .follow_through
                .map(|until| until.is_some_and(|at| now < at)),
            shared.loads,
        )
    };
    let fresh = published.is_some_and(|at| now.saturating_duration_since(at) <= STALE_AFTER);
    let pins = if fresh { pins } else { [None; 2] };
    // The hands on a gun act to hold it, with a holding hand's strength.
    let gun_holds = if fresh {
        guns::holds_for_step(&config)
    } else {
        guns::GunHolds::default()
    };
    let on_gun = gun_holds.hands();
    let mut joint_lost = [false; 2];
    let holds = if !fresh {
        [None; 2]
    } else if config.bodies.joint_grabs {
        holds
    } else {
        // A grip that holds through a joint while the joints are switched off holds nothing.
        joint_lost = holds.map(|h| h.is_some());
        [None; 2]
    };
    let age_s = published
        .map_or(0.0, |at| now.saturating_duration_since(at).as_secs_f32())
        .min(MAX_LEAD_S);
    let lead_s = age_s + step_s;
    // SAFETY: forwarded from the caller.
    let group = unsafe { player_system_group() };
    let b = &config.bodies;
    let scale = if world_scale.is_finite() && world_scale > 0.0 {
        world_scale
    } else {
        1.0
    };
    let tuning = DriveTuning {
        mass_kg: b.mass_kg,
        max_force_n: b.force_n(),
        max_torque_nm: b.torque_nm(),
        linear_gain: b.linear_gain,
        angular_gain: b.angular_gain,
        max_speed_mps: b.max_speed_mps,
        max_angular_speed_rps: b.max_angular_speed_rps,
    };
    let render = RenderTuning {
        max_shift_m: b.break_distance_m * scale,
        deadband_rad: RENDER_TURN_DEADBAND_RAD,
        max_turn_rad: RENDER_MAX_TURN_RAD,
    };
    // A hand on a gun is drawn turned exactly as its body is, since the drawn gun follows the hand.
    let render_on_gun = RenderTuning {
        deadband_rad: 0.0,
        ..render
    };
    let push_tuning = PushTuning {
        stiffness_n_per_m: b.push_stiffness_n_per_m,
        max_force_n: b.force_n(),
        body_mass_kg: b.body_mass_kg,
        deadband_m: b.push_deadband_m * scale,
        min_horizontal: b.min_push_horizontal,
    };

    // The climb takes its grips where the acting bodies are now, and pins the gripping hands there.
    let poses = Hand::BOTH.map(|hand| {
        let i = hand.index();
        // A hand on a gun neither climbs nor reaches.
        let acting = fresh && group.is_some() && targets[i].is_some() && !on_gun[i];
        let body = hands.bodies[i]
            .as_ref()
            .filter(|_| acting && hands.phase(hand) == BodyPhase::Acting)?;
        // SAFETY: forwarded from the caller.
        let motion = unsafe { body.motion() }.ok()?;
        Some((motion.position, motion.orientation))
    });
    // SAFETY: forwarded from the caller; the characters' proxy updates ran before this callback.
    let climbs = unsafe { climb::step(poses) };
    // A hand gripping what it cannot move yet, a latched door, is held at its grab point as a
    // climbing hand is held on the wall.
    let grips = Hand::BOTH.map(|hand| {
        let i = hand.index();
        climbs[i].or_else(|| {
            let pin = pins[i]?;
            let orientation = poses[i].map_or(Quat::IDENTITY, |(_, o)| o);
            Some(Goal {
                position: pin,
                orientation,
            })
        })
    });
    // A gripping hand's body stays on the wall; every other acting body moves with the rig.
    for hand in Hand::BOTH {
        let i = hand.index();
        if let Some((from, to)) = rig_moves[i]
            && grips[i].is_none()
        {
            // SAFETY: forwarded from the caller.
            unsafe { hands.carry(hand, from, to) };
        }
    }
    let climbing = climbs.iter().any(Option::is_some);

    let mut statuses = [HandBodyStatus::default(); 2];
    let mut offsets = [RenderOffset::NONE; 2];
    let mut loads = [0.0; 2];
    let mut created = 0;
    let mut destroyed = 0;
    let mut error = None;
    let mut hand_push = Vec2::ZERO;
    let mut press_lags = [None; 2];
    for hand in Hand::BOTH {
        let i = hand.index();
        let target = targets[i].filter(|_| fresh);
        match (target, group) {
            (Some(target), Some(group)) => {
                hands.missing_since[i] = None;
                let spec = BodySpec {
                    shape: BodyShape::Capsule(Capsule {
                        half_length_m: b.half_length_m * scale,
                        radius_m: b.radius_m * scale,
                    }),
                    mass_kg: b.mass_kg,
                    system_group: group,
                    contact_events: b.hits.enabled,
                    gravity: false,
                };
                let palm = Goal {
                    position: target.position + target.linear * lead_s,
                    orientation: Quat::from_scaled_axis(target.angular * lead_s)
                        * target.orientation,
                };
                // A hand that holds an object through a joint is moved by the keep-out, and pulls
                // and twists with a holding hand's strength, the same budget as a carried hold.
                let holding = holds[i];
                let goal = grips[i].unwrap_or(Goal {
                    position: palm.position + holding.map_or(Vec3::ZERO, |h| h.keep_out),
                    ..palm
                });
                let held = holds[i].map_or(0, |h| h.object.get());
                let mut clear = |from: Vec3, to: Vec3| unsafe { path_clear(from, to, held) };
                // A hand held back rests on what holds it, and one that just struck hard follows
                // through rather than stopping dead.
                let mut linear_gain = contact_gain(tuning.linear_gain, b.contact_gain, pressed[i]);
                if follow_through[i] {
                    linear_gain *= b.hits.follow_through_gain.clamp(0.0, 1.0);
                }
                let held_tuning = if on_gun[i] {
                    gun_tuning(&tuning, guns::mass(hand))
                } else {
                    holding.map_or(tuning, |h| holding_tuning(&tuning, &config, h.shared))
                };
                let hand_tuning = DriveTuning {
                    linear_gain,
                    ..held_tuning
                };
                let request = ActRequest {
                    hand,
                    spec,
                    goal,
                    palm,
                    render: if on_gun[i] { &render_on_gun } else { &render },
                    anchor: grips[i].map_or(target.position, |g| g.position),
                    clear: &mut clear,
                    break_distance_m: b.break_distance_m * scale,
                    tuning: &hand_tuning,
                    step_s,
                };
                // SAFETY: forwarded from the caller.
                match unsafe { hands.act(request) } {
                    Ok(acted) => {
                        created += u64::from(acted.created);
                        destroyed += u64::from(acted.destroyed);
                        // SAFETY: forwarded from the caller.
                        joint_lost[i] |= acted.joint_lost || unsafe { hands.join(hand, holds[i]) };
                        statuses[i] = acted.status;
                        statuses[i].joint = hands.joints[i].as_ref().map(Joint::id);
                        offsets[i] = acted.render;
                        loads[i] = press_load(acted.lag, b, scale);
                        // A hand that grips or holds something does not push the body up.
                        if grips[i].is_none() && holds[i].is_none() && !on_gun[i] {
                            press_lags[i] = Some(acted.lag);
                        }
                        if b.push {
                            hand_push += lag_push(acted.lag, &push_tuning, step_s);
                        }
                    }
                    Err(e) => {
                        error = Some(e.to_string());
                        // SAFETY: forwarded from the caller.
                        joint_lost[i] |= unsafe { hands.unjoin(hand, "the hand body failed") };
                        statuses[i].phase = hands.phase(hand);
                    }
                }
            }
            (_, group) => {
                let since = *hands.missing_since[i].get_or_insert(now);
                let park = group.is_none()
                    || park_now[i]
                    || now.saturating_duration_since(since) >= PARK_AFTER;
                let reason = if group.is_none() {
                    "the player has no collision group"
                } else if park_now[i] {
                    "the hand took a gun or carries what it holds"
                } else {
                    "the hand stopped acting"
                };
                // SAFETY: forwarded from the caller.
                joint_lost[i] |= unsafe { hands.unjoin(hand, "the hand stopped acting") };
                if park {
                    // SAFETY: forwarded from the caller.
                    unsafe { hands.park(hand, reason) };
                } else {
                    // SAFETY: forwarded from the caller.
                    unsafe { hands.hold(hand) };
                }
                statuses[i].phase = hands.phase(hand);
                statuses[i].body_id = hands.bodies[i].as_ref().map(HandBody::body_id);
            }
        }
    }
    // SAFETY: forwarded from the caller; the hands acted above, so a gun hangs only from a body
    // that acts.
    unsafe {
        guns::step::step(
            &mut hands,
            GunStep {
                holds: gun_holds,
                config: &b.guns,
                contact_events: b.hits.enabled,
                group,
                palms: targets.map(|target| {
                    target.filter(|_| fresh).map(|t| Pose {
                        position: t.position,
                        orientation: t.orientation,
                    })
                }),
            },
        )
    };
    // The hands pressed down on what does not give push the body up, unless they climb.
    if !climbing {
        let press = vault::Press {
            lags: press_lags,
            supports: climb::static_reaches(),
            scale,
            dt: step_s,
        };
        // SAFETY: forwarded from the caller; the characters' proxy updates ran before this
        // callback.
        unsafe { vault::step(press, &config.vault) };
    }
    push = if b.push && !climbing {
        smooth_push(
            push,
            hand_push,
            step_s,
            b.push_smoothing_s,
            b.max_push_speed_mps,
        )
    } else {
        Vec2::ZERO
    };
    let log_now = hands
        .last_log
        .is_none_or(|at| now.saturating_duration_since(at) >= READOUT_PERIOD);
    if log_now && statuses.iter().any(|s| s.phase == BodyPhase::Acting) {
        hands.last_log = Some(now);
        for hand in Hand::BOTH {
            let s = &statuses[hand.index()];
            if s.phase == BodyPhase::Acting {
                tracing::debug!(
                    hand = hand.label(),
                    lag_mm = s.lag_m * 1000.0,
                    speed_mps = s.speed_mps,
                    force_share = s.force_share,
                    torque_share = s.torque_share,
                    push_x = push.x,
                    push_z = push.y,
                    "hand bodies: drive"
                );
            }
        }
    }

    let mut shared = SHARED.lock();
    for (lost, now_lost) in shared.joint_lost.iter_mut().zip(joint_lost) {
        *lost |= now_lost;
    }
    shared.world_offsets = offsets;
    shared.offset_timeline.push(offsets, Instant::now());
    shared.loads = loads;
    shared.body_poses = poses;
    shared.push = push;
    shared.stepped = Some(now);
    let status = &mut shared.status;
    status.steps = hands.steps;
    status.system_group = group;
    status.hands = statuses;
    status.push = push;
    status.created += created;
    status.destroyed += destroyed;
    if let Some(error) = error {
        status.last_error = Some(error);
    }
}

/// The step the physics world takes this step, seconds, as [`step`] published it before driving
/// anything. Read by the code the step calls (the climb and the proxy's velocity), on the thread
/// that steps the world.
pub(crate) fn step_seconds() -> f32 {
    f32::from_bits(STEP_S.load(Ordering::Relaxed))
}

/// The bits of [`step_seconds`]; the engine's constant until the first step.
static STEP_S: AtomicU32 = AtomicU32::new(PhysicsSystem::STEP_SECONDS.to_bits());

/// Whether nothing solid lies between `from` and `to`, game space, for the aim's ray filter, which
/// never hits the player nor the hand bodies; `held` is an object a hand holds, also passed
/// through, or zero. A cast that fails counts as blocked.
///
/// # Safety
///
/// The thread that steps the physics world, outside the step.
unsafe fn path_clear(from: Vec3, to: Vec3, held: usize) -> bool {
    let Some(ray) = Ray::new(from, to - from) else {
        return true;
    };
    let player = unsafe { Character::GetLocalPlayerCharacter() } as usize;
    let options = CastOptions {
        ignore: [player, held],
        see_through: false,
        material: false,
    };
    // SAFETY: forwarded from the caller; the ignored objects are the live player and a held
    // object the pickups checked this tick, or zero.
    matches!(
        unsafe { raycast::cast(&ray, from.distance(to), &options) },
        Ok(None)
    )
}
