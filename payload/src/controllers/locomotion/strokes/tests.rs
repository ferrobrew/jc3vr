//! Tests for the strokes' model.

use glam::Vec3;

use crate::controllers::locomotion::{
    config::SwimStrokeConfig,
    strokes::model::{StrokeHand, StrokeInput, StrokeModel, StrokeStep},
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
    assert!(last.forward > 0.5, "{}", last.forward);
    assert!(total_turn(&steps).abs() < 1e-4, "{}", total_turn(&steps));
    // Both hands knocked once, on the same tick.
    let caught: Vec<_> = steps.iter().filter(|s| s.caught[0].is_some()).collect();
    assert_eq!(caught.len(), 1);
    assert!(caught[0].caught[1].is_some());
}

#[test]
fn the_drive_glides_out_after_the_stroke() {
    let mut model = StrokeModel::new();
    let back = Vec3::new(0.0, 0.0, 2.0);
    let stroke = [
        Some((start(-1.0), back, PALM_BACK)),
        Some((start(1.0), back, PALM_BACK)),
    ];
    let peak = run(&mut model, stroke, 24, false).last().unwrap().drive;
    let held = [
        Some((start(-1.0) + back * (23.0 * DT), Vec3::ZERO, PALM_BACK)),
        Some((start(1.0) + back * (23.0 * DT), Vec3::ZERO, PALM_BACK)),
    ];
    let glide = run(&mut model, held, 60, false);
    let after = glide.last().unwrap().drive;
    assert!(after < peak * 0.6 && after > 0.0, "{peak} {after}");
    // Once the hands' motion has settled, the drive only falls.
    assert!(
        glide[10..]
            .windows(2)
            .all(|w| w[1].drive <= w[0].drive + 1e-6)
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
    assert!(left.last().unwrap().forward > 0.0);
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
            .all(|s| s.drive == 0.0 && s.caught == [None; 2])
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
            .all(|s| s.drive == 0.0 && s.in_water == [false; 2])
    );
    let mut model = StrokeModel::new();
    let wet = run(&mut model, hands, 24, true);
    assert!(wet.last().unwrap().forward > 0.5);
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
    assert!(steps.iter().all(|s| s.drive == 0.0 && s.forward == 0.0));
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
    assert!(steps.iter().all(|s| s.drive == 0.0));
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
