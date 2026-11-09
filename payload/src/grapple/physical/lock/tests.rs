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

/// Run the swing from a static hook at the origin for `ticks`, returning the rider's path.
fn fly(held: &mut Held, mut rider: Vec3, mut velocity: Vec3, ticks: usize) -> Vec<(Vec3, Vec3)> {
    (0..ticks)
        .map(|_| {
            velocity = swing(
                held,
                input(rider, velocity, Vec3::ZERO, Vec3::ZERO),
                &CONFIG,
            )
            .unwrap()
            .velocity;
            rider += velocity * DT;
            (rider, velocity)
        })
        .collect()
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
fn locking_keeps_the_inward_rush_and_tangential_momentum() {
    let start = Vec3::new(20.0, 0.0, 0.0);
    let rush = Vec3::new(-25.0, 0.0, 6.0);
    let mut held = Held::new(Vec3::ZERO, start, &CONFIG).unwrap();
    let out = swing(
        &mut held,
        input(start, rush, Vec3::ZERO, Vec3::ZERO),
        &CONFIG,
    )
    .unwrap();
    assert!((out.velocity - (rush - Vec3::Y * GRAVITY_MPS2 * DT)).length() < 1e-5);
    assert_eq!(out.tension_n, 0.0);
    assert_eq!(held.rope.length_m, 20.0);
    let path = fly(&mut held, start, rush, 10);
    assert!((path.last().unwrap().1.x - rush.x).abs() < 1e-5);
    assert_eq!(held.rope.length_m, 20.0);
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
