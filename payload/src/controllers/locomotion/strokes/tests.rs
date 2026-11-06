//! Tests for the strokes' model.

use glam::Vec3;

use crate::controllers::locomotion::{
    config::SwimStrokeConfig,
    strokes::model::{StrokeHand, StrokeInput, StrokeModel, StrokeStep, propel},
};

const DT: f32 = 1.0 / 60.0;
const HEAD: Vec3 = Vec3::ZERO;
/// A palm facing back, as one pulling water toward the feet does.
const PALM_BACK: Vec3 = Vec3::Z;
/// A palm facing down, edge-on to a hand moving forward or back.
const PALM_DOWN: Vec3 = Vec3::NEG_Y;

/// Where a hand starts its stroke on `side` (-1 left, +1 right): out in front at chest height.
fn start(side: f32) -> Vec3 {
    Vec3::new(0.3 * side, -0.35, -0.5)
}

/// Run `ticks` ticks of the hands moving by `velocity` each (metres per second), palms `palm`,
/// and return every step.
fn run(
    model: &mut StrokeModel,
    hands: [Option<(Vec3, Vec3, Vec3)>; 2],
    ticks: usize,
    underwater: bool,
) -> Vec<StrokeStep> {
    let config = SwimStrokeConfig::new();
    (0..ticks)
        .map(|tick| {
            let t = tick as f32 * DT;
            let hands = hands.map(|h| {
                h.map(|(from, velocity, palm)| StrokeHand {
                    position: from + velocity * t,
                    palm,
                })
            });
            model.step(
                &StrokeInput {
                    hands,
                    head: HEAD,
                    underwater,
                    dt: DT,
                },
                &config,
            )
        })
        .collect()
}

fn total_turn(steps: &[StrokeStep]) -> f32 {
    steps.iter().map(|s| s.turn_right).sum()
}

#[test]
fn a_breaststroke_swims_forward_without_turning() {
    let mut model = StrokeModel::new();
    let back = Vec3::new(0.0, 0.0, 2.0);
    let steps = run(
        &mut model,
        [
            Some((start(-1.0), back, PALM_BACK)),
            Some((start(1.0), back, PALM_BACK)),
        ],
        24,
        false,
    );
    let last = steps.last().unwrap();
    assert!(last.speed > 1.0, "{}", last.speed);
    assert!(total_turn(&steps).abs() < 1e-4, "{}", total_turn(&steps));
    // Both hands knocked once, on the same tick.
    let caught: Vec<_> = steps.iter().filter(|s| s.caught[0].is_some()).collect();
    assert_eq!(caught.len(), 1);
    assert!(caught[0].caught[1].is_some());
}

#[test]
fn the_speed_glides_out_after_the_stroke() {
    let mut model = StrokeModel::new();
    let back = Vec3::new(0.0, 0.0, 2.0);
    let stroke = [
        Some((start(-1.0), back, PALM_BACK)),
        Some((start(1.0), back, PALM_BACK)),
    ];
    let peak = run(&mut model, stroke, 24, false).last().unwrap().speed;
    let held = [
        Some((start(-1.0) + back * (23.0 * DT), Vec3::ZERO, PALM_BACK)),
        Some((start(1.0) + back * (23.0 * DT), Vec3::ZERO, PALM_BACK)),
    ];
    let glide = run(&mut model, held, 60, false);
    let after = glide.last().unwrap().speed;
    assert!(after < peak * 0.6 && after > 0.0, "{peak} {after}");
    // Once the hands' motion has settled, the drive only falls.
    assert!(
        glide[10..]
            .windows(2)
            .all(|w| w[1].speed <= w[0].speed + 1e-6)
    );
}

#[test]
fn a_one_sided_stroke_turns_away_from_its_side() {
    let back = Vec3::new(0.0, 0.0, 2.0);
    let mut model = StrokeModel::new();
    let left = run(
        &mut model,
        [Some((start(-1.0), back, PALM_BACK)), None],
        60,
        false,
    );
    assert!(total_turn(&left) > 0.1, "{}", total_turn(&left));
    let mut model = StrokeModel::new();
    let right = run(
        &mut model,
        [None, Some((start(1.0), back, PALM_BACK))],
        60,
        false,
    );
    assert!((total_turn(&right) + total_turn(&left)).abs() < 1e-4);
    assert!(left.last().unwrap().speed > 0.0);
}

#[test]
fn a_palm_slicing_edge_on_pushes_nothing() {
    let mut model = StrokeModel::new();
    let back = Vec3::new(0.0, 0.0, 2.0);
    let steps = run(
        &mut model,
        [
            Some((start(-1.0), back, PALM_DOWN)),
            Some((start(1.0), back, PALM_DOWN)),
        ],
        24,
        false,
    );
    assert!(
        steps
            .iter()
            .all(|s| s.speed == 0.0 && s.caught == [None; 2])
    );
}

#[test]
fn a_hand_out_of_the_water_pushes_nothing_unless_underwater() {
    let back = Vec3::new(0.0, 0.0, 2.0);
    let high = |side: f32| start(side) + Vec3::new(0.0, 0.5, 0.0);
    let hands = [
        Some((high(-1.0), back, PALM_BACK)),
        Some((high(1.0), back, PALM_BACK)),
    ];
    let mut model = StrokeModel::new();
    let dry = run(&mut model, hands, 24, false);
    assert!(
        dry.iter()
            .all(|s| s.speed == 0.0 && s.in_water == [false; 2])
    );
    let mut model = StrokeModel::new();
    let wet = run(&mut model, hands, 24, true);
    assert!(wet.last().unwrap().speed > 1.0);
}

#[test]
fn a_recovery_with_the_palm_forward_brakes_and_never_swims_backward() {
    let mut model = StrokeModel::new();
    let forward = Vec3::new(0.0, 0.0, -2.0);
    let palm_forward = Vec3::NEG_Z;
    let steps = run(
        &mut model,
        [
            Some((start(-1.0) + Vec3::Z * 0.6, forward, palm_forward)),
            Some((start(1.0) + Vec3::Z * 0.6, forward, palm_forward)),
        ],
        24,
        false,
    );
    assert!(steps.iter().all(|s| s.speed == 0.0));
}

#[test]
fn a_slow_drift_and_a_hitch_do_not_swim() {
    let mut model = StrokeModel::new();
    let drift = Vec3::new(0.0, 0.0, 0.2);
    let steps = run(
        &mut model,
        [
            Some((start(-1.0), drift, PALM_BACK)),
            Some((start(1.0), drift, PALM_BACK)),
        ],
        60,
        false,
    );
    assert!(steps.iter().all(|s| s.speed == 0.0));
    // A non-finite or zero tick changes nothing.
    let config = SwimStrokeConfig::new();
    let before = model;
    for dt in [0.0, f32::NAN] {
        let step = model.step(
            &StrokeInput {
                hands: [None; 2],
                head: HEAD,
                underwater: false,
                dt,
            },
            &config,
        );
        assert_eq!(step.turn_right, 0.0);
        assert_eq!(model, before);
    }
}

#[test]
fn the_turn_is_limited_to_the_fastest_turn() {
    let config = SwimStrokeConfig::new();
    let mut model = StrokeModel::new();
    let hard = Vec3::new(0.0, 0.0, 20.0);
    let steps = run(
        &mut model,
        [Some((start(-1.0) * 3.0, hard, PALM_BACK)), None],
        30,
        false,
    );
    let max = config.max_turn_deg_s.to_radians() * DT;
    assert!(steps.iter().all(|s| s.turn_right <= max + 1e-6));
    assert!(steps.iter().any(|s| (s.turn_right - max).abs() < 1e-6));
}

/// Run `cycles` breaststrokes: both hands pull back at `pull_mps` for `pull_s`, palms back, then
/// come forward edge-on for the rest of each one-second cycle. Returns every step.
fn breaststrokes(
    model: &mut StrokeModel,
    pull_mps: f32,
    pull_s: f32,
    cycles: usize,
) -> Vec<StrokeStep> {
    const CYCLE_S: f32 = 1.0;
    let config = SwimStrokeConfig::new();
    let ticks = (CYCLE_S / DT).round() as usize;
    let pull_ticks = (pull_s / DT).round() as usize;
    let recover_mps = pull_mps * pull_s / (CYCLE_S - pull_s);
    let mut offset = 0.0f32;
    let mut steps = Vec::new();
    for _ in 0..cycles {
        for tick in 0..ticks {
            let (velocity, palm) = if tick < pull_ticks {
                (pull_mps, PALM_BACK)
            } else {
                (-recover_mps, PALM_DOWN)
            };
            offset += velocity * DT;
            let hands = [-1.0, 1.0].map(|side| {
                Some(StrokeHand {
                    position: start(side) + Vec3::Z * offset,
                    palm,
                })
            });
            steps.push(model.step(
                &StrokeInput {
                    hands,
                    head: HEAD,
                    underwater: false,
                    dt: DT,
                },
                &config,
            ));
        }
    }
    steps
}

/// The mean speed over the last two seconds of `steps`.
fn settled_speed(steps: &[StrokeStep]) -> f32 {
    let tail = &steps[steps.len() - (2.0 / DT) as usize..];
    tail.iter().map(|s| s.speed).sum::<f32>() / tail.len() as f32
}

#[test]
fn sustained_strokes_swim_faster_the_harder_they_pull() {
    let config = SwimStrokeConfig::new();
    let speeds: Vec<f32> = [(0.6, 0.3), (0.9, 0.3), (1.2, 0.3), (2.5, 0.3)]
        .iter()
        .map(|&(pull, time)| settled_speed(&breaststrokes(&mut StrokeModel::new(), pull, time, 8)))
        .collect();
    assert!(speeds.windows(2).all(|w| w[1] > w[0]), "{speeds:?}");
    // A gentle rhythm swims at a moderate pace, below the strokes' fastest.
    assert!(
        speeds[0] > 0.5 && speeds[0] < config.max_speed_mps * 0.4,
        "{speeds:?}"
    );
    // A hard one reaches the fastest and holds near it through the recoveries.
    assert!(speeds[3] > config.max_speed_mps * 0.75, "{speeds:?}");
    assert!(speeds.iter().all(|&s| s <= config.max_speed_mps));
}

#[test]
fn one_stroke_is_a_burst_that_glides_to_a_stop() {
    let mut model = StrokeModel::new();
    let steps = breaststrokes(&mut model, 2.0, 0.3, 1);
    let pull_end = (0.3 / DT) as usize;
    let peak = steps[..pull_end + 4]
        .iter()
        .map(|s| s.speed)
        .fold(0.0, f32::max);
    // The burst arrives within the stroke.
    assert!(peak > 1.5, "{peak}");
    // Edge-on recovery and then still hands only glide down.
    let held = [-1.0f32, 1.0].map(|side| Some((start(side), Vec3::ZERO, PALM_DOWN)));
    let glide = run(&mut model, held, (4.0 / DT) as usize, false);
    assert!(glide.last().unwrap().speed < peak * 0.1);
}

#[test]
fn a_palm_first_recovery_brakes_at_half_the_pull() {
    let back = Vec3::new(0.0, 0.0, 2.0);
    let forward = -back;
    let both = |velocity: Vec3, palm: Vec3, at: f32| {
        [-1.0f32, 1.0].map(|side| Some((start(side) + Vec3::Z * at, velocity, palm)))
    };
    let mut model = StrokeModel::new();
    let pulled = run(&mut model, both(back, PALM_BACK, 0.0), 12, false)
        .last()
        .unwrap()
        .speed;
    let reached = back.z * 11.0 * DT;
    let mut gliding = model;
    let after_glide = run(
        &mut gliding,
        both(Vec3::ZERO, PALM_DOWN, reached),
        12,
        false,
    )
    .last()
    .unwrap()
    .speed;
    let braked = run(&mut model, both(forward, Vec3::NEG_Z, reached), 12, false)
        .last()
        .unwrap()
        .speed;
    let lost_to_brake = after_glide - braked;
    assert!(lost_to_brake > 0.0, "{after_glide} {braked}");
    assert!(lost_to_brake < pulled * 0.75, "{pulled} {lost_to_brake}");
}

#[test]
fn propel_raises_only_the_share_along_the_facing() {
    let facing = Vec3::new(0.0, 0.0, -1.0);
    // Treading water with buoyancy and a drift across: the strokes add speed along the facing and
    // keep the rest.
    let core = Vec3::new(0.3, 0.2, 0.0);
    let out = propel(core, facing, 2.5);
    assert!((out.dot(facing) - 2.5).abs() < 1e-5);
    assert!((out.x - 0.3).abs() < 1e-6 && (out.y - 0.2).abs() < 1e-6);
    // A crawl already faster along the facing keeps its own speed.
    let crawl = Vec3::new(0.0, 0.0, -4.0);
    assert_eq!(propel(crawl, facing, 2.5), crawl);
    // A core moving backward is brought up to the strokes' speed forward.
    let drifting_back = Vec3::new(0.0, 0.0, 0.5);
    assert!((propel(drifting_back, facing, 1.0).dot(facing) - 1.0).abs() < 1e-5);
}

#[test]
fn propel_follows_a_pitched_facing_underwater() {
    let facing = Vec3::new(0.0, 0.5, -0.866_025_4);
    let out = propel(Vec3::ZERO, facing, 2.0);
    assert!((out - facing * 2.0).length() < 1e-5);
}

#[test]
fn propel_leaves_the_velocity_without_a_speed_or_a_facing() {
    let core = Vec3::new(0.1, -0.2, 0.3);
    let facing = Vec3::NEG_Z;
    for speed in [0.0, -1.0, f32::NAN] {
        assert_eq!(propel(core, facing, speed), core);
    }
    assert_eq!(propel(core, Vec3::ZERO, 2.0), core);
    let bad = Vec3::new(f32::NAN, 0.0, 0.0);
    assert!(propel(bad, facing, 2.0).x.is_nan());
}
