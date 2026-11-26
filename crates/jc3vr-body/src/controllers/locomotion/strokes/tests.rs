use glam::{Quat, Vec3};

use crate::controllers::locomotion::strokes::model::{
    StrokeHand, StrokeInput, StrokeModel, StrokeStep, propel,
};

const DT: f32 = 1.0 / 60.0;

fn sample(
    model: &mut StrokeModel,
    hands: [Option<StrokeHand>; 2],
    rotation: Quat,
    underwater: bool,
) -> StrokeStep {
    model.step(&StrokeInput {
        hands: hands.map(|hand| {
            hand.map(|hand| StrokeHand {
                position: rotation * hand.position,
                palm: rotation * hand.palm,
            })
        }),
        head: Vec3::ZERO,
        forward: rotation * Vec3::NEG_Z,
        underwater,
        dt: DT,
    })
}

fn breaststroke(t: f32, side: f32) -> StrokeHand {
    let (position, palm) = if t < 0.3 {
        let a = t / 0.3;
        (
            Vec3::new(side * (0.12 + 0.4 * a), -0.4, -0.65 + 0.04 * a),
            Vec3::new(side * 0.55, -0.8, 0.25).normalize(),
        )
    } else if t < 0.6 {
        let a = (t - 0.3) / 0.3;
        (
            Vec3::new(side * (0.52 - 0.34 * a), -0.4, -0.61 + 0.56 * a),
            Vec3::new(side * 0.1, -0.8, 0.6).normalize(),
        )
    } else {
        let a = ((t - 0.6) / 0.6).min(1.0);
        (
            Vec3::new(side * (0.18 - 0.06 * a), -0.4, -0.05 - 0.6 * a),
            Vec3::NEG_Z,
        )
    };
    StrokeHand { position, palm }
}

fn breaststroke_cycle(model: &mut StrokeModel, rotation: Quat) -> Vec<StrokeStep> {
    (0..=72)
        .map(|tick| {
            sample(
                model,
                [-1.0, 1.0].map(|side| Some(breaststroke(tick as f32 * DT, side))),
                rotation,
                false,
            )
        })
        .collect()
}

#[test]
fn broad_breaststroke_catches_on_the_outsweep_and_glides_through_recovery() {
    let mut model = StrokeModel::new();
    for _ in 0..3 {
        let steps = breaststroke_cycle(&mut model, Quat::IDENTITY);
        for hand in 0..2 {
            let catches: Vec<_> = steps
                .iter()
                .enumerate()
                .filter(|(_, step)| step.caught[hand].is_some())
                .collect();
            assert_eq!(catches.len(), 1);
            assert!(catches[0].0 < 18);
        }
        assert!(steps[17].speed > steps[0].speed);
        assert!(steps.iter().map(|step| step.turn_right).sum::<f32>().abs() < 1e-5);
        assert!(
            steps[45..]
                .iter()
                .all(|step| step.push == [glam::Vec2::ZERO; 2])
        );
        assert!(steps.last().unwrap().speed > 0.0);
        assert!(
            steps[45..]
                .windows(2)
                .all(|pair| pair[1].speed < pair[0].speed)
        );
    }
}

#[test]
fn stroke_recognition_is_not_fixed_to_cockpit_forward() {
    let straight = breaststroke_cycle(&mut StrokeModel::new(), Quat::IDENTITY);
    let turned = breaststroke_cycle(&mut StrokeModel::new(), Quat::from_rotation_y(1.2));
    for (a, b) in straight.iter().zip(&turned) {
        assert!((a.speed - b.speed).abs() < 1e-5);
        assert!((a.turn_right - b.turn_right).abs() < 1e-5);
        assert_eq!(a.caught.map(|c| c.is_some()), b.caught.map(|c| c.is_some()));
    }
}

#[test]
fn alternating_crawl_keeps_independent_catches_and_forward_drive() {
    let mut model = StrokeModel::new();
    let mut catches = [0; 2];
    let mut turn = 0.0;
    let mut last = StrokeStep::NONE;
    for tick in 0..360 {
        let hands = [0, 1].map(|i| {
            let phase = (tick as f32 * DT + i as f32 * 0.6) % 1.2;
            let side = if i == 0 { -1.0 } else { 1.0 };
            let (z, palm) = if phase < 0.5 {
                (-0.65 + phase * 1.4, Vec3::Z)
            } else {
                (0.05 - (phase - 0.5), Vec3::NEG_Z)
            };
            Some(StrokeHand {
                position: Vec3::new(side * 0.3, -0.4, z),
                palm,
            })
        });
        last = sample(&mut model, hands, Quat::IDENTITY, false);
        for (i, caught) in last.caught.iter().enumerate() {
            catches[i] += usize::from(caught.is_some());
        }
        turn += last.turn_right;
    }
    assert_eq!(catches, [5, 5]);
    assert!(last.speed > 0.0);
    assert!(turn.abs() < 0.3, "{turn}");
}

#[test]
fn unqualified_gestures_and_tracking_reentry_do_not_catch() {
    for gesture in 0..4 {
        let mut model = StrokeModel::new();
        for tick in 0..120 {
            let t = tick as f32 * DT;
            let hands = [-1.0, 1.0].map(|side| {
                let (position, palm) = match gesture {
                    0 => (
                        Vec3::new(side * (0.3 + 0.1 * (t * 12.0).sin()), -0.4, 0.0),
                        Vec3::X * side,
                    ),
                    1 => (
                        Vec3::new(side * 0.3, -0.4 + 0.2 * (t * 6.0).sin(), -0.5),
                        Vec3::NEG_Y,
                    ),
                    2 => (Vec3::new(side * 0.3, -0.4, -0.5 + 0.5 * t), Vec3::NEG_Z),
                    _ => (
                        Vec3::new(side * 0.3, -0.4, -0.5 + 0.003 * (t * 30.0).sin()),
                        Vec3::Z,
                    ),
                };
                Some(StrokeHand { position, palm })
            });
            let step = sample(&mut model, hands, Quat::IDENTITY, false);
            assert_eq!(step.speed, 0.0, "gesture {gesture}");
            assert_eq!(step.caught, [None; 2]);
        }
    }
    let mut model = StrokeModel::new();
    for tick in 0..60 {
        let hands = if tick % 2 == 0 {
            [None; 2]
        } else {
            [-1.0, 1.0].map(|side| Some(breaststroke(0.5, side)))
        };
        let step = sample(&mut model, hands, Quat::IDENTITY, false);
        assert_eq!(step.speed, 0.0);
        assert_eq!(step.caught, [None; 2]);
    }
}

#[test]
fn dry_hands_do_not_stroke_but_underwater_hands_do_and_preserve_pitched_motion() {
    for underwater in [false, true] {
        let mut model = StrokeModel::new();
        let steps: Vec<_> = (0..36)
            .map(|tick| {
                let hands = [-1.0, 1.0].map(|side| {
                    let mut hand = breaststroke(tick as f32 * DT, side);
                    hand.position.y = 0.1;
                    Some(hand)
                });
                sample(&mut model, hands, Quat::IDENTITY, underwater)
            })
            .collect();
        let speed = steps.last().unwrap().speed;
        if underwater {
            assert!(speed > 0.0);
            let facing = Vec3::new(0.0, 0.6, -0.8);
            assert!(propel(Vec3::ZERO, facing, speed).abs_diff_eq(facing * speed, 1e-5));
        } else {
            assert!(
                steps
                    .iter()
                    .all(|step| step.speed == 0.0 && step.caught == [None; 2])
            );
        }
    }
    let facing = Vec3::NEG_Z;
    let core = Vec3::new(0.3, 0.2, 0.0);
    let out = propel(core, facing, 2.0);
    assert_eq!(out.x, core.x);
    assert_eq!(out.y, core.y);
    assert_eq!(propel(facing * 4.0, facing, 2.0), facing * 4.0);
}

/// Both hands pulling back and to the left push the swimmer forward and to the right, and the
/// heading turns right toward the push: by about the push's angle over a stroke, plus the turn the
/// hands' uneven offsets make, which a straight stroke from the same hands shows alone.
#[test]
fn a_stroke_toward_one_side_turns_the_heading_toward_it() {
    let stroke = |lateral: f32| {
        let mut model = StrokeModel::new();
        let ticks = 24;
        let mut turn = 0.0;
        for tick in 0..=ticks + 30 {
            let a = (tick as f32 / ticks as f32).min(1.0);
            let motion = Vec3::new(lateral, 0.0, 0.5);
            let hands = [-1.0, 1.0].map(|side| {
                // The straight stroke runs along the diagonal one's midline, so both carry the
                // same offsets.
                let midline = Vec3::X * (-0.35 - lateral) * 0.5;
                Some(StrokeHand {
                    position: Vec3::new(side * 0.2, -0.4, -0.6) + midline + motion * a,
                    palm: motion.normalize(),
                })
            });
            turn += sample(&mut model, hands, Quat::IDENTITY, false).turn_right;
        }
        turn
    };
    let turn = stroke(-0.35) - stroke(0.0);
    let angle = 0.35f32.atan2(0.5);
    assert!(
        turn > 0.5 * angle && turn < 1.5 * angle,
        "{turn} against {angle}"
    );
}
