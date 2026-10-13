//! The legs over whole movements: a run's touchdowns, a walk's planted feet, a standing sway, a
//! step in the room, a turn in place, and a stop.

use glam::{Quat, Vec3};

use crate::controllers::body::rig::{
    config::LegsConfig,
    legs::{
        Legs, LegsInput, LegsTuning,
        gait::CrouchGait,
        swing::{FootPose, Ground},
    },
};

const DT: f32 = 1.0 / 30.0;
const LEG_M: f32 = 0.9;
const HIP_M: f32 = 0.95;
const HALF_STANCE_M: f32 = 0.12;

fn tuning() -> LegsTuning {
    LegsTuning::of(&LegsConfig::new(), CrouchGait::UPRIGHT, 25f32.to_radians())
}

fn flat(_: Vec3) -> Option<Ground> {
    None
}

/// The pelvis at a place and a heading.
#[derive(Debug, Copy, Clone)]
struct Pelvis {
    at: Vec3,
    yaw: f32,
}

fn rest(pelvis: Pelvis) -> [FootPose; 2] {
    let turn = Quat::from_rotation_y(pelvis.yaw);
    [-1.0, 1.0].map(|side| FootPose {
        position: pelvis.at + turn * Vec3::new(side * HALF_STANCE_M, 0.0, 0.0),
        rotation: turn,
    })
}

fn min_jerk(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    u * u * u * (10.0 - 15.0 * u + 6.0 * u * u)
}

/// What one tick produced.
#[derive(Debug, Copy, Clone)]
struct Tick {
    t: f32,
    feet: [FootPose; 2],
    planted: [bool; 2],
}

/// Run the legs along `path(t)` for `seconds`, calling `each` with every tick.
fn run(
    legs: &mut Legs,
    seconds: f32,
    path: impl Fn(f32) -> Pelvis,
    mut each: impl FnMut(&Legs, Tick),
) {
    let tuning = tuning();
    let mut previous = path(0.0);
    let ticks = (seconds / DT).round() as usize;
    for k in 1..=ticks {
        let t = k as f32 * DT;
        let pelvis = path(t);
        let velocity = (pelvis.at - previous.at) / DT;
        previous = pelvis;
        let input = LegsInput {
            rest: rest(pelvis),
            root_height: 0.0,
            velocity,
            crouch: 0.0,
            hip_height_m: HIP_M,
            leg_length_m: LEG_M,
            dt: DT,
        };
        let feet = legs.step(&input, &tuning, &mut flat);
        each(
            legs,
            Tick {
                t,
                feet,
                planted: legs.planted(),
            },
        );
    }
}

/// A straight run along -Z, speeding up to `speed` over `ramp_s` with a minimum-jerk profile.
fn straight(speed: f32, ramp_s: f32) -> impl Fn(f32) -> Pelvis {
    move |t| {
        // The distance covered: the ramp's integral, then constant speed.
        let ramp = t.min(ramp_s);
        let u = ramp / ramp_s;
        // The integral of the minimum-jerk speed profile over the ramp.
        let covered = speed * ramp_s * (u.powi(4) * 2.5 - u.powi(5) * 3.0 + u.powi(6));
        let distance = covered + speed * (t - ramp_s).max(0.0);
        Pelvis {
            at: Vec3::new(0.0, 0.0, -distance),
            yaw: 0.0,
        }
    }
}

/// How far each planted foot moved while it stayed planted, the worst over the run.
#[derive(Default)]
struct Skate {
    held: [Option<Vec3>; 2],
    worst: f32,
}

impl Skate {
    fn observe(&mut self, tick: &Tick) {
        for i in 0..2 {
            if tick.planted[i] {
                let at = tick.feet[i].position;
                let start = *self.held[i].get_or_insert(at);
                self.worst = self.worst.max(start.distance(at));
            } else {
                self.held[i] = None;
            }
        }
    }
}

#[test]
fn a_run_lands_its_feet_under_the_body_not_ahead_of_it() {
    let mut legs = Legs::new();
    let mut leads = Vec::new();
    let mut last = None;
    run(&mut legs, 5.0, straight(5.0, 1.0), |legs, tick| {
        if tick.t > 2.0
            && legs.touchdown() != last
            && let Some((_, lead)) = legs.touchdown()
        {
            leads.push(lead);
        }
        last = legs.touchdown();
    });
    assert!(leads.len() >= 6, "{leads:?}");
    let worst = leads.iter().copied().fold(f32::MIN, f32::max);
    let mean = leads.iter().sum::<f32>() / leads.len() as f32;
    assert!(
        worst <= 0.42 * LEG_M + 1e-4,
        "a touchdown {worst} m ahead: {leads:?}"
    );
    assert!(
        mean <= 0.35,
        "touchdowns {mean} m ahead on average: {leads:?}"
    );
    assert!(mean > 0.1, "{mean}");
}

#[test]
fn a_walk_never_skates_a_planted_foot() {
    let mut legs = Legs::new();
    let mut skate = Skate::default();
    run(&mut legs, 4.0, straight(1.4, 0.6), |_, tick| {
        skate.observe(&tick)
    });
    assert!(skate.worst < 0.01, "a planted foot moved {} m", skate.worst);
    assert!(legs.steps() >= 4);
}

#[test]
fn a_standing_sway_steps_nothing() {
    let mut legs = Legs::new();
    run(
        &mut legs,
        10.0,
        |t| Pelvis {
            at: Vec3::new(
                0.05 * (std::f32::consts::TAU * 0.5 * t).sin(),
                0.0,
                0.05 * (std::f32::consts::TAU * 0.3 * t).cos() - 0.05,
            ),
            yaw: 0.0,
        },
        |_, _| {},
    );
    assert_eq!(legs.steps(), 0);
}

#[test]
fn a_step_across_the_room_steps_each_foot_at_most_once() {
    let mut legs = Legs::new();
    let mut last = None;
    run(
        &mut legs,
        3.0,
        |t| Pelvis {
            at: Vec3::new(0.3 * min_jerk(t / 0.6), 0.0, 0.0),
            yaw: 0.0,
        },
        |_, tick| last = Some(tick),
    );
    let steps = legs.steps();
    assert!((1..=2).contains(&steps), "{steps} steps");
    let last = last.unwrap();
    let rest = rest(Pelvis {
        at: Vec3::new(0.3, 0.0, 0.0),
        yaw: 0.0,
    });
    assert_eq!(last.planted, [true, true]);
    for (foot, rest) in last.feet.iter().zip(rest) {
        assert!(foot.position.distance(rest.position) < 0.22);
    }
}

#[test]
fn a_quarter_turn_in_place_steps_two_or_three_times() {
    let mut legs = Legs::new();
    run(
        &mut legs,
        2.5,
        |t| Pelvis {
            at: Vec3::ZERO,
            yaw: std::f32::consts::FRAC_PI_2 * min_jerk(t),
        },
        |_, _| {},
    );
    let steps = legs.steps();
    assert!((2..=3).contains(&steps), "{steps} steps");
}

#[test]
fn stopping_never_slides_a_planted_foot_and_plants_both() {
    let mut legs = Legs::new();
    let mut skate = Skate::default();
    let walk = straight(1.4, 0.6);
    let stop_at = 3.0;
    let stopped = walk(stop_at).at.z;
    let mut last = None;
    run(
        &mut legs,
        5.0,
        move |t| {
            if t <= stop_at {
                walk(t)
            } else {
                // Slow to a stop over a third of a second.
                let u = ((t - stop_at) / 0.33).min(1.0);
                let extra = 1.4 * 0.33 * (u - u * u * 0.5);
                Pelvis {
                    at: Vec3::new(0.0, 0.0, stopped - extra),
                    yaw: 0.0,
                }
            }
        },
        |legs, tick| {
            skate.observe(&tick);
            last = Some((legs.moving(), tick));
        },
    );
    assert!(skate.worst < 0.01, "a planted foot moved {} m", skate.worst);
    let (moving, tick) = last.unwrap();
    assert!(!moving);
    assert_eq!(tick.planted, [true, true]);
}
