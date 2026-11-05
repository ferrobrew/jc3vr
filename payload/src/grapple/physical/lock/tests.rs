use glam::Vec3;

use crate::{
    controllers::bindings::yank::HandLine,
    grapple::{
        physical::{
            config::LockConfig,
            lock::model::{Held, SwingInput, TakeIn, swing},
        },
        rope::{GRAVITY_MPS2, RopeFault},
    },
};

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
    held.brake_left_s = 0.0;
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
fn the_brake_turns_the_rush_toward_the_hook_into_a_swing() {
    let start = Vec3::new(20.0, 0.0, 0.0);
    let rush = Vec3::new(-25.0, 0.0, 6.0);
    let mut held = Held::new(Vec3::ZERO, start, &CONFIG).unwrap();
    let path = fly(&mut held, start, rush, 30);
    let (rider, velocity) = *path.last().unwrap();
    let outward = rider.normalize();
    assert!(
        velocity.dot(outward) > -1.0,
        "no rush left: {}",
        velocity.dot(outward)
    );
    assert!(velocity.z > 4.0, "the tangent stays: {velocity}");
    assert!(held.rope.length_m < 20.0 && held.rope.length_m > 13.0);
    assert!(!held.braking());
}

#[test]
fn a_hook_on_a_moving_target_keeps_the_rider_at_its_offset() {
    let carried = Vec3::new(20.0, 0.0, -8.0);
    let mut hook = Vec3::new(0.0, 50.0, 0.0);
    let mut rider = hook - Vec3::Y * 6.0;
    let mut velocity = carried;
    let mut held = Held::new(hook, rider, &CONFIG).unwrap();
    held.brake_left_s = 0.0;
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
fn the_target_feels_the_equal_and_opposite_pull() {
    let mut held = Held::new(Vec3::ZERO, Vec3::Y * -4.0, &CONFIG).unwrap();
    held.brake_left_s = 0.0;
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
    held.brake_left_s = 0.0;
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
    held.brake_left_s = 0.0;
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
    held.take_in(1.0, &CONFIG);
    assert!((held.rope.length_m - 2.0).abs() < 1e-6);
    held.take_in(5.0, &CONFIG);
    assert_eq!(held.rope.length_m, CONFIG.min_length_m);
    held.take_in(f32::NAN, &CONFIG);
    held.take_in(-2.0, &CONFIG);
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
