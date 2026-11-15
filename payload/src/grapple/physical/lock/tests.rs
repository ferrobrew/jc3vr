use glam::{Quat, Vec3};

use crate::{
    controllers::bindings::{grapple_button::LockButton, yank::HandLine},
    grapple::{
        physical::{
            config::{GRAPPLE, LockConfig},
            lock::model::{Held, Pump, PumpSample, SwingInput, TakeIn, armed, swing},
        },
        rope::{GRAVITY_MPS2, RopeFault},
    },
};

fn pump_sample(hand: Vec3, head: Vec3) -> PumpSample {
    PumpSample {
        hand,
        head,
        rotation: Quat::IDENTITY,
        world_scale: 1.0,
        recenters: 0,
    }
}

#[test]
fn pump_ignores_shared_head_motion_and_its_first_sample() {
    let tuning = GRAPPLE.swing;
    let mut pump = Pump::default();
    pump.sample(Some(pump_sample(Vec3::X, Vec3::ZERO)), DT, &tuning);
    assert_eq!(pump.take(DT, Vec3::Y * 5.1, 5.0, &tuning), Vec3::ZERO);
    pump.sample(Some(pump_sample(Vec3::X + Vec3::Z, Vec3::Z)), DT, &tuning);
    assert_eq!(pump.take(DT, Vec3::Y * 5.1, 5.0, &tuning), Vec3::ZERO);
}

#[test]
fn pump_is_tangential_bounded_and_has_one_input_ticks_budget() {
    let tuning = GRAPPLE.swing;
    let mut pump = Pump::default();
    pump.sample(Some(pump_sample(Vec3::ZERO, Vec3::ZERO)), DT, &tuning);
    pump.sample(Some(pump_sample(Vec3::X * 0.2, Vec3::ZERO)), DT, &tuning);
    let mut whole = pump;
    let offset = Vec3::Y * 5.1;
    let first = pump.take(DT * 0.5, offset, 5.0, &tuning);
    let second = pump.take(DT * 0.5, offset, 5.0, &tuning);
    assert!(first.x < 0.0);
    assert_eq!(first.y, 0.0);
    assert!((first + second).abs_diff_eq(whole.take(DT, offset, 5.0, &tuning), 1e-6));
    assert!((first + second).length() <= 0.35 * GRAVITY_MPS2 * DT + 1e-6);
    assert_eq!(pump.take(DT, offset, 5.0, &tuning), Vec3::ZERO);
}

#[test]
fn pump_does_not_assist_slack_rope_or_replay_its_slack_budget() {
    let tuning = GRAPPLE.swing;
    let mut pump = Pump::default();
    pump.sample(Some(pump_sample(Vec3::ZERO, Vec3::ZERO)), DT, &tuning);
    pump.sample(Some(pump_sample(Vec3::X * 0.1, Vec3::ZERO)), DT, &tuning);
    assert_eq!(pump.take(DT, Vec3::Y * 4.0, 5.0, &tuning), Vec3::ZERO);
    assert_eq!(pump.take(DT, Vec3::Y * 5.1, 5.0, &tuning), Vec3::ZERO);
}

#[test]
fn lateral_pump_keeps_rope_length_and_adds_no_tangent_target_reaction() {
    let tuning = GRAPPLE.swing;
    let rider = Vec3::Y * -5.1;
    let mut held = Held::new(Vec3::ZERO, Vec3::Y * -5.0, &CONFIG).unwrap();
    let length = held.rope.length_m;
    let mut pump = Pump::default();
    pump.sample(Some(pump_sample(Vec3::ZERO, Vec3::ZERO)), DT, &tuning);
    pump.sample(Some(pump_sample(Vec3::X * 0.1, Vec3::ZERO)), DT, &tuning);
    let velocity = pump.take(DT, rider, length, &tuning);
    let mut tick = input(rider, velocity, Vec3::ZERO, Vec3::ZERO);
    tick.target_inverse_mass = 1.0 / MASS;
    let out = swing(&mut held, tick, &CONFIG).unwrap();
    assert!(out.velocity.x < 0.0);
    assert_eq!(held.rope.length_m, length);
    assert_eq!(out.target_impulse.x, 0.0);
    assert_eq!(out.target_impulse.z, 0.0);
}

#[test]
fn pump_reseeds_after_recenter_tracking_gap_or_implausible_jump() {
    let tuning = GRAPPLE.swing;
    for discontinuity in 0..5 {
        let mut pump = Pump::default();
        pump.sample(Some(pump_sample(Vec3::ZERO, Vec3::ZERO)), DT, &tuning);
        let mut next = pump_sample(Vec3::X * 0.1, Vec3::ZERO);
        match discontinuity {
            0 => next.recenters += 1,
            1 => next.rotation = Quat::from_rotation_y(0.5),
            2 => next.hand = Vec3::X * 2.0,
            3 => next.world_scale = 2.0,
            _ => pump.sample(None, DT, &tuning),
        }
        pump.sample(Some(next), DT, &tuning);
        assert_eq!(pump.take(DT, Vec3::Y * 5.1, 5.0, &tuning), Vec3::ZERO);
    }
}

const DT: f32 = 1.0 / 30.0;
const MASS: f32 = 100.0;
const CONFIG: LockConfig = LockConfig::new();

fn input(rider: Vec3, rider_velocity: Vec3, hook: Vec3, hook_velocity: Vec3) -> SwingInput {
    SwingInput {
        rider,
        rider_velocity,
        rider_mass_kg: MASS,
        hook,
        hook_velocity,
        target_inverse_mass: 0.0,
        dt: DT,
    }
}

/// Run the swing from a static hook at the origin for `ticks` under `config`, returning the
/// rider's path.
fn fly_with(
    held: &mut Held,
    mut rider: Vec3,
    mut velocity: Vec3,
    ticks: usize,
    config: &LockConfig,
) -> Vec<(Vec3, Vec3)> {
    (0..ticks)
        .map(|_| {
            velocity = swing(held, input(rider, velocity, Vec3::ZERO, Vec3::ZERO), config)
                .unwrap()
                .velocity;
            rider += velocity * DT;
            (rider, velocity)
        })
        .collect()
}

/// Run the swing from a static hook at the origin for `ticks` under the default config, returning
/// the rider's path.
fn fly(held: &mut Held, rider: Vec3, velocity: Vec3, ticks: usize) -> Vec<(Vec3, Vec3)> {
    fly_with(held, rider, velocity, ticks, &CONFIG)
}

#[test]
fn a_static_hook_swings_a_bounded_pendulum_that_keeps_its_energy_roughly() {
    let start = Vec3::new(5.0, 0.0, 0.0);
    let mut held = Held::new(Vec3::ZERO, start, &CONFIG).unwrap();
    let path = fly(&mut held, start, Vec3::ZERO, 300);
    let (lowest, fastest) = path.iter().fold((0.0_f32, 0.0_f32), |(low, fast), (p, v)| {
        (low.min(p.y), fast.max(v.length()))
    });
    for (p, _) in &path {
        assert!(p.length() < 5.0 + 0.6, "the line holds: {}", p.length());
    }
    // A 5 m pendulum dropped from level reaches about sqrt(2 g L) at the bottom.
    assert!(lowest < -4.5, "{lowest}");
    assert!(fastest > 8.0 && fastest < 12.0, "{fastest}");
}

#[test]
fn the_take_leaves_the_velocity_alone_and_follows_the_rider_in() {
    let start = Vec3::new(20.0, 0.0, 0.0);
    let rush = Vec3::new(-12.0, 0.0, 6.0);
    let mut held = Held::new(Vec3::ZERO, start, &CONFIG).unwrap();
    assert!(held.coasting());
    let out = swing(
        &mut held,
        input(start, rush, Vec3::ZERO, Vec3::ZERO),
        &CONFIG,
    )
    .unwrap();
    assert!(
        (out.velocity - (rush - Vec3::Y * GRAVITY_MPS2 * DT)).length() < 1e-4,
        "nothing but gravity on the take's first tick: {}",
        out.velocity
    );
    assert_eq!(out.tension_n, 0.0);
    let mut rider = start + out.velocity * DT;
    let mut velocity = out.velocity;
    for _ in 0..5 {
        velocity = swing(
            &mut held,
            input(rider, velocity, Vec3::ZERO, Vec3::ZERO),
            &CONFIG,
        )
        .unwrap()
        .velocity;
        rider += velocity * DT;
    }
    assert!(
        (velocity.z - rush.z).abs() < 1e-3,
        "the speed across the line is never turned: {velocity}"
    );
    // The length follows the rider to where it was at the tick's start, a tick's travel behind.
    assert!(
        (held.rope.length_m - rider.length()).abs() < rush.length() * DT + 0.05,
        "the held length follows the rider in: {} at {}",
        held.rope.length_m,
        rider.length()
    );
}

/// Run the take from a static hook at the origin to its end, returning the rider and velocity.
fn take(held: &mut Held, mut rider: Vec3, mut velocity: Vec3) -> (Vec3, Vec3) {
    for _ in 0..2000 {
        if !held.coasting() {
            break;
        }
        velocity = swing(
            held,
            input(rider, velocity, Vec3::ZERO, Vec3::ZERO),
            &CONFIG,
        )
        .unwrap()
        .velocity;
        rider += velocity * DT;
    }
    (rider, velocity)
}

#[test]
fn the_line_goes_taut_where_the_approach_ends_with_no_jump() {
    // A rush in toward the hook with some speed across the line.
    let start = Vec3::new(20.0, 0.0, 0.0);
    let rush = Vec3::new(-6.0, 0.0, 12.0);
    let mut held = Held::new(Vec3::ZERO, start, &CONFIG).unwrap();
    let mut rider = start;
    let mut velocity = rush;
    let mut largest_change = 0.0_f32;
    for _ in 0..200 {
        let next = swing(
            &mut held,
            input(rider, velocity, Vec3::ZERO, Vec3::ZERO),
            &CONFIG,
        )
        .unwrap()
        .velocity;
        largest_change = largest_change.max((next - velocity).length());
        velocity = next;
        rider += velocity * DT;
    }
    assert!(!held.coasting(), "the take ends once the approach does");
    assert!(
        largest_change < 2.0,
        "no tick changes the velocity by more than a smooth swing does: {largest_change}"
    );
    assert!(
        rider.length() < held.rope.length_m + 0.6,
        "the line holds the swing: {} against {}",
        rider.length(),
        held.rope.length_m
    );
}

#[test]
fn a_rush_at_a_level_hook_falls_under_it_on_a_line_that_bears_the_swing() {
    // Gravity carries the coast under the hook, where the approach ends with the speed across the
    // line. The coast stops no shorter than the line can swing that speed on.
    let start = Vec3::new(20.0, 0.0, 0.0);
    let rush = Vec3::new(-25.0, 0.0, 0.0);
    let mut held = Held::new(Vec3::ZERO, start, &CONFIG).unwrap();
    let (rider, velocity) = take(&mut held, start, rush);
    assert!(rider.y < 0.0, "under the hook: {rider}");
    assert!(
        held.rope.length_m < start.length() && held.rope.length_m >= CONFIG.min_length_m,
        "the line took up the approach: {}",
        held.rope.length_m
    );
    let swing_g = velocity.length_squared() / held.rope.length_m / GRAVITY_MPS2 + 1.0;
    assert!(
        swing_g < CONFIG.max_accel_g,
        "the line bears the swing: {swing_g} g at {velocity} on {}",
        held.rope.length_m
    );
    assert!(
        velocity.length() > 5.0,
        "the speed carries into the swing: {velocity}"
    );
}

#[test]
fn a_coast_that_would_reach_the_hook_stops_at_the_least_length() {
    let start = Vec3::NEG_Y * 10.0;
    let mut held = Held::new(Vec3::ZERO, start, &CONFIG).unwrap();
    let mut rider = start;
    let mut velocity = Vec3::Y * 40.0;
    let mut nearest = f32::INFINITY;
    for _ in 0..120 {
        velocity = swing(
            &mut held,
            input(rider, velocity, Vec3::ZERO, Vec3::ZERO),
            &CONFIG,
        )
        .unwrap()
        .velocity;
        rider += velocity * DT;
        nearest = nearest.min(rider.length());
    }
    assert!(
        nearest > CONFIG.min_length_m * 0.5 && rider.y < 0.0,
        "the coast never reaches the hook: {nearest}, {rider}"
    );
}

#[test]
fn a_rush_up_to_a_high_hook_keeps_its_way_across_the_line() {
    let elevation = 63.0_f32.to_radians();
    let start = Vec3::new(-elevation.cos(), -elevation.sin(), 0.0) * 50.0;
    let direction = start.normalize();
    let across = Vec3::Z * 5.0;
    let rush = -direction * 30.0 + across;
    let mut held = Held::new(Vec3::ZERO, start, &CONFIG).unwrap();
    let out = swing(
        &mut held,
        input(start, rush, Vec3::ZERO, Vec3::ZERO),
        &CONFIG,
    )
    .unwrap();
    assert!(
        (out.velocity.z - across.z).abs() < 1e-3
            && (out.velocity.y - (rush.y - GRAVITY_MPS2 * DT)).abs() < 1e-3,
        "the rush carries on up, its crossing untouched: {}",
        out.velocity
    );
}

#[test]
fn a_rush_straight_up_under_its_hook_stops_and_hangs() {
    let start = Vec3::NEG_Y * 30.0;
    let mut held = Held::new(Vec3::ZERO, start, &CONFIG).unwrap();
    let (_, velocity) = take(&mut held, start, Vec3::Y * 25.0);
    assert!(velocity.length() < 1.0, "no way across: {velocity}");
}

#[test]
fn a_hang_starts_at_rest_under_the_ceiling_and_lowers_the_rider_to_its_length() {
    // Arrived under a ceiling hook, rushing up into it and a little across.
    let start = Vec3::new(0.2, -0.8, 0.0);
    let rush = Vec3::new(6.0, 20.0, 0.0);
    let mut held = Held::hang(Vec3::ZERO, start, 3.5, &CONFIG).unwrap();
    assert!((held.rope.length_m - start.length().max(CONFIG.min_length_m)).abs() < 1e-3);
    let path = fly(&mut held, start, rush, 150);
    let (rider, velocity) = *path.last().unwrap();
    assert!(
        (held.rope.length_m - 3.5).abs() < 1e-3,
        "{}",
        held.rope.length_m
    );
    assert!(rider.y < -3.0, "the rider hangs below the hook: {rider}");
    assert!(
        path.iter().all(|(p, _)| p.y < 0.5),
        "the rush never carries the rider over the hook"
    );
    assert!(
        velocity.length() < 8.0,
        "no fling from the rush: {velocity}"
    );
}

#[test]
fn a_hang_taken_far_out_is_reeled_in_to_its_length() {
    let start = Vec3::new(3.0, -9.0, 0.0);
    let mut held = Held::hang(Vec3::ZERO, start, 3.5, &CONFIG).unwrap();
    fly(&mut held, start, Vec3::ZERO, 300);
    assert!(
        (held.rope.length_m - 3.5).abs() < 1e-3,
        "{}",
        held.rope.length_m
    );
}

#[test]
fn a_hang_under_a_moving_hook_stops_swinging_across_the_line() {
    // Under a helicopter flying along +X, swinging across its path.
    let hook_velocity = Vec3::X * 10.0;
    let mut held = Held::hang(Vec3::ZERO, Vec3::NEG_Y * 3.5, 3.5, &CONFIG).unwrap();
    let mut hook = Vec3::ZERO;
    let mut rider = Vec3::NEG_Y * 3.5;
    let mut velocity = hook_velocity + Vec3::Z * 4.0;
    for _ in 0..240 {
        velocity = swing(
            &mut held,
            input(rider, velocity, hook, hook_velocity),
            &CONFIG,
        )
        .unwrap()
        .velocity;
        rider += velocity * DT;
        hook += hook_velocity * DT;
    }
    let relative = velocity - hook_velocity;
    assert!(relative.z.abs() < 0.5, "the swing dies away: {relative}");
}

#[test]
fn taking_line_in_stops_a_hangs_pay_out() {
    let start = Vec3::NEG_Y * 2.0;
    let mut held = Held::hang(Vec3::ZERO, start, 3.5, &CONFIG).unwrap();
    held.take_in(0.1, DT, &CONFIG);
    let before = held.rope.length_m;
    fly(&mut held, start, Vec3::ZERO, 30);
    assert!(
        held.rope.length_m <= before + 1e-5,
        "{}",
        held.rope.length_m
    );
}

#[test]
fn a_tangential_swing_velocity_survives_the_next_lock() {
    // At the bottom of the arc the swing's velocity is across the line. A release hands that
    // velocity on, and the next lock's brake has nothing to remove from it, so a chained swing
    // does not stop at the join.
    let rider = Vec3::NEG_Y * 5.0;
    let exit = Vec3::X * 6.0;
    let mut next = Held::new(Vec3::ZERO, rider, &CONFIG).unwrap();
    assert!(next.coasting());
    let out = swing(
        &mut next,
        input(rider, exit, Vec3::ZERO, Vec3::ZERO),
        &CONFIG,
    )
    .unwrap();
    let direction = rider.normalize();
    let tangential = out.velocity - direction * out.velocity.dot(direction);
    assert!((tangential - exit).length() < 1e-3, "{out:?}");
}

#[test]
fn a_mid_swing_release_hands_on_the_swings_whole_velocity() {
    let start = Vec3::X * 5.0;
    let mut held = Held::new(Vec3::ZERO, start, &CONFIG).unwrap();
    let path = fly(&mut held, start, Vec3::ZERO, 30);
    let (rider, exit) = *path.last().unwrap();
    assert!(exit.length() > 1.0, "the swing carries speed: {exit}");
    // The captured velocity is the character's whole motion, gravity included: the next tick of
    // the same swing at the same position continues from it.
    let mut again = Held::new(Vec3::ZERO, rider, &CONFIG).unwrap();
    let out = swing(
        &mut again,
        input(rider, exit, Vec3::ZERO, Vec3::ZERO),
        &CONFIG,
    )
    .unwrap();
    assert!(out.velocity.is_finite());
    assert!(
        out.velocity.length() > exit.length() * 0.8,
        "the momentum carries: {out:?}"
    );
}

#[test]
fn a_hook_on_a_moving_target_keeps_the_rider_at_its_offset() {
    let carried = Vec3::new(20.0, 0.0, -8.0);
    let mut hook = Vec3::new(0.0, 50.0, 0.0);
    let mut rider = hook - Vec3::Y * 6.0;
    let mut velocity = carried;
    let mut held = Held::new(hook, rider, &CONFIG).unwrap();
    for _ in 0..150 {
        velocity = swing(&mut held, input(rider, velocity, hook, carried), &CONFIG)
            .unwrap()
            .velocity;
        rider += velocity * DT;
        hook += carried * DT;
        let offset = rider - hook;
        assert!(offset.length() < 6.0 + 0.4, "{}", offset.length());
        assert!(offset.x.abs() < 0.5 && offset.z.abs() < 0.5, "{offset}");
    }
    assert!((velocity - carried).length() < 1.0, "{velocity}");
}

#[test]
fn arm_take_in_adds_pull_velocity_without_losing_the_tangent() {
    let rider = Vec3::NEG_Y * 4.0;
    let mut held = Held::new(Vec3::ZERO, rider, &CONFIG).unwrap();
    held.take_in(0.05, DT, &CONFIG);
    let mut stationary = held;
    stationary.take_in(0.0, DT, &CONFIG);
    let before = Vec3::X * 5.0;
    let moving = swing(
        &mut held,
        input(rider, before, Vec3::ZERO, Vec3::ZERO),
        &CONFIG,
    )
    .unwrap();
    let still = swing(
        &mut stationary,
        input(rider, before, Vec3::ZERO, Vec3::ZERO),
        &CONFIG,
    )
    .unwrap();
    assert!(moving.velocity.y > still.velocity.y);
    assert_eq!(moving.velocity.x, before.x);
    assert!(moving.tension_n > still.tension_n);
    let gravity = Vec3::NEG_Y * GRAVITY_MPS2 * DT;
    assert!(((moving.velocity - before - gravity) * MASS + moving.target_impulse).length() < 1e-2);
}

#[test]
fn the_target_feels_the_equal_and_opposite_pull() {
    let mut held = Held::new(Vec3::ZERO, Vec3::Y * -4.0, &CONFIG).unwrap();
    let before = Vec3::new(0.0, -3.0, 0.0);
    let out = swing(
        &mut held,
        SwingInput {
            target_inverse_mass: 1.0 / 1500.0,
            ..input(Vec3::Y * -4.1, before, Vec3::ZERO, Vec3::ZERO)
        },
        &CONFIG,
    )
    .unwrap();
    let gravity = Vec3::Y * -GRAVITY_MPS2 * DT;
    let rider_impulse = (out.velocity - before - gravity) * MASS;
    assert!(out.tension_n > 0.0);
    assert!((rider_impulse + out.target_impulse).length() < 1e-2);
    assert!(
        out.target_impulse.y < 0.0,
        "the target is pulled toward the rider"
    );
}

#[test]
fn the_tension_is_capped_at_the_lines_strength() {
    let mut held = Held::new(Vec3::ZERO, Vec3::X * 4.0, &CONFIG).unwrap();
    let out = swing(
        &mut held,
        input(Vec3::X * 6.0, Vec3::X * 30.0, Vec3::ZERO, Vec3::ZERO),
        &CONFIG,
    )
    .unwrap();
    assert_eq!(out.tension_n, out.max_tension_n);
    assert!((out.max_tension_n - CONFIG.max_accel_g * GRAVITY_MPS2 * MASS).abs() < 1e-2);
}

#[test]
fn an_overstretched_line_breaks() {
    let mut held = Held::new(Vec3::ZERO, Vec3::X * 4.0, &CONFIG).unwrap();
    assert_eq!(
        swing(
            &mut held,
            input(Vec3::X * 7.5, Vec3::ZERO, Vec3::ZERO, Vec3::ZERO),
            &CONFIG,
        ),
        Err(RopeFault::Stretched)
    );
}

#[test]
fn the_line_is_never_held_shorter_than_its_least_length() {
    let held = Held::new(Vec3::ZERO, Vec3::X * 0.5, &CONFIG).unwrap();
    assert_eq!(held.rope.length_m, CONFIG.min_length_m);
    let mut held = Held::new(Vec3::ZERO, Vec3::X * 3.0, &CONFIG).unwrap();
    held.take_in(1.0, DT, &CONFIG);
    assert!((held.rope.length_m - 2.0).abs() < 1e-6);
    held.take_in(5.0, DT, &CONFIG);
    assert_eq!(held.rope.length_m, CONFIG.min_length_m);
    held.take_in(f32::NAN, DT, &CONFIG);
    held.take_in(-2.0, DT, &CONFIG);
    assert_eq!(held.rope.length_m, CONFIG.min_length_m);
}

#[test]
fn the_hand_takes_line_in_only_pulling_back_and_no_faster_than_the_reel() {
    let at = |z: f32| {
        Some(HandLine {
            grip: Vec3::new(0.0, 1.2, z),
            aim: Vec3::NEG_Z,
        })
    };
    let mut take = TakeIn::default();
    assert_eq!(
        take.step(DT, at(-0.6), 3.0),
        0.0,
        "the first sample starts it"
    );
    let slow = take.step(DT, at(-0.55), 3.0);
    assert!((slow - 0.05).abs() < 1e-5, "{slow}");
    assert!((take.step(DT, at(-0.2), 3.0) - 0.1).abs() < 1e-5, "capped");
    assert_eq!(
        take.step(DT, at(-0.7), 3.0),
        0.0,
        "reaching out pays nothing out"
    );
    assert!(take.step(DT, at(-0.6), 3.0) > 0.0, "hand over hand");
    assert_eq!(take.step(DT, None, 3.0), 0.0);
    assert_eq!(
        take.step(DT, at(-0.1), 3.0),
        0.0,
        "tracking back restarts it"
    );
}

#[test]
fn bad_samples_are_refused() {
    let mut held = Held::new(Vec3::ZERO, Vec3::X * 4.0, &CONFIG).unwrap();
    let mut bad = input(Vec3::X * 4.0, Vec3::ZERO, Vec3::ZERO, Vec3::ZERO);
    bad.rider_mass_kg = 0.0;
    assert_eq!(swing(&mut held, bad, &CONFIG), Err(RopeFault::Invalid));
    bad.rider_mass_kg = MASS;
    bad.hook = Vec3::NAN;
    assert_eq!(swing(&mut held, bad, &CONFIG), Err(RopeFault::Invalid));
    assert!(Held::new(Vec3::NAN, Vec3::ZERO, &CONFIG).is_none());
}

#[test]
fn a_press_in_the_wind_up_stays_armed_while_held_and_disarms_on_release() {
    let held = [
        Some(LockButton::Pressed),
        Some(LockButton::Held),
        Some(LockButton::Held),
    ];
    let mut state = false;
    for button in held {
        state = armed(state, button);
        assert!(state, "a held press stays armed until the reel carries");
    }
    for up in [Some(LockButton::Tapped), Some(LockButton::LetGo), None] {
        assert!(!armed(true, up));
    }
}

#[test]
fn a_held_button_never_rearms_after_the_lock_took_hold_or_refused() {
    // The tick clears the arming when it takes hold or refuses; a held button does not undo that.
    assert!(!armed(false, Some(LockButton::Held)));
}

#[test]
fn no_rush_onto_the_lock_breaks_the_line_in_the_swing_after_it() {
    // Level rushes, a rush up a diagonal line, and one straight up a steep one: each takes the
    // coast in and swings on for ten seconds on a line that holds.
    for (start, rush) in [
        (Vec3::new(20.0, 0.0, 0.0), Vec3::new(-25.0, 0.0, 0.0)),
        (Vec3::new(20.0, 0.0, 0.0), Vec3::new(-15.0, 0.0, 0.0)),
        (Vec3::new(20.0, 0.0, 0.0), Vec3::new(-6.0, 0.0, 12.0)),
        (Vec3::new(14.0, -14.0, 0.0), Vec3::new(-15.0, 15.0, 4.0)),
        (Vec3::new(10.0, -25.0, 0.0), Vec3::new(-10.0, 25.0, 0.0)),
        (Vec3::new(30.0, 5.0, 10.0), Vec3::new(-30.0, -2.0, -8.0)),
    ] {
        let mut held = Held::new(Vec3::ZERO, start, &CONFIG).unwrap();
        let mut rider = start;
        let mut velocity = rush;
        for tick in 0..600 {
            velocity = swing(
                &mut held,
                input(rider, velocity, Vec3::ZERO, Vec3::ZERO),
                &CONFIG,
            )
            .unwrap_or_else(|fault| panic!("{start} at {rush}: {fault:?} on tick {tick}"))
            .velocity;
            rider += velocity * DT;
        }
        assert!(
            rider.length() < held.rope.length_m + CONFIG.break_stretch_m,
            "{start} at {rush}: the line holds"
        );
    }
}
