use super::*;
use crate::controllers::vehicles::tuning::VEHICLES;

fn config() -> &'static WingsuitEvadeTuning {
    &VEHICLES.get().wingsuit_evade
}

const DT: f32 = 1.0 / 90.0;

fn input(bank_deg: f32) -> EvadeInput {
    EvadeInput {
        bank_deg: Some(bank_deg),
        pitch: 0.0,
        gate: None,
        dt: DT,
    }
}

/// Swing the bank linearly from `from` to `to` over `seconds`, returning each tick's outcome.
fn swing(recognizer: &mut EvadeRecognizer, from: f32, to: f32, seconds: f32) -> Vec<Outcome> {
    let ticks = (seconds / DT).round().max(1.0) as usize;
    (1..=ticks)
        .map(|tick| {
            let bank = from + (to - from) * tick as f32 / ticks as f32;
            recognizer.step(input(bank), config())
        })
        .collect()
}

fn throws(outcomes: &[Outcome]) -> Vec<Throw> {
    outcomes
        .iter()
        .filter_map(|outcome| match outcome {
            Outcome::Throw(throw) => Some(*throw),
            _ => None,
        })
        .collect()
}

#[test]
fn a_fast_steep_bank_throws_once_toward_the_lower_hand() {
    let mut recognizer = EvadeRecognizer::default();
    swing(&mut recognizer, 0.0, 0.0, 0.2);
    let outcomes = swing(&mut recognizer, 0.0, 70.0, 0.15);
    let thrown = throws(&outcomes);
    assert_eq!(thrown.len(), 1);
    assert_eq!(
        thrown[0].direction,
        EvadeDirection {
            side: Side::Right,
            pitch: None
        }
    );
    assert!(thrown[0].bank_deg >= config().throw_bank_deg);

    let mut recognizer = EvadeRecognizer::default();
    let thrown = throws(&swing(&mut recognizer, 5.0, -70.0, 0.15));
    assert_eq!(thrown.len(), 1);
    assert_eq!(thrown[0].direction.side, Side::Left);
}

#[test]
fn a_slow_bank_into_the_same_angle_never_throws() {
    let mut recognizer = EvadeRecognizer::default();
    assert!(throws(&swing(&mut recognizer, 0.0, 80.0, 2.0)).is_empty());
}

#[test]
fn a_fast_bank_that_stays_shallow_never_throws() {
    let mut recognizer = EvadeRecognizer::default();
    let outcomes = swing(&mut recognizer, -30.0, 30.0, 0.1);
    assert!(throws(&outcomes).is_empty());
    assert!(outcomes.iter().all(|outcome| *outcome == Outcome::Idle));
}

#[test]
fn a_throw_holds_its_direction_then_lets_go() {
    let mut recognizer = EvadeRecognizer::default();
    let mut held = swing(&mut recognizer, 0.0, 70.0, 0.1);
    held.extend(swing(&mut recognizer, 70.0, 0.0, DIRECTION_HOLD_S + 0.1));
    let holding = held
        .iter()
        .skip_while(|outcome| !matches!(outcome, Outcome::Throw(_)))
        .filter(|outcome| matches!(outcome, Outcome::Holding(_)))
        .count();
    let expected = (DIRECTION_HOLD_S / DT) as usize;
    assert!(holding.abs_diff(expected) <= 1, "{holding} vs {expected}");
    assert_eq!(held.last(), Some(&Outcome::Idle));
}

#[test]
fn a_second_throw_needs_the_arms_back_and_the_cooldown_passed() {
    let mut recognizer = EvadeRecognizer::default();
    assert_eq!(throws(&swing(&mut recognizer, 0.0, 70.0, 0.1)).len(), 1);
    // Holding the steep bank, or wobbling within it, throws nothing more.
    assert!(throws(&swing(&mut recognizer, 70.0, 60.0, 0.3)).is_empty());
    assert!(throws(&swing(&mut recognizer, 60.0, 80.0, 0.1)).is_empty());
    // Back to level and thrown again inside the cooldown: refused, and it re-arms nothing.
    swing(&mut recognizer, 80.0, 0.0, 0.2);
    let refused = swing(&mut recognizer, 0.0, -70.0, 0.1);
    assert!(throws(&refused).is_empty());
    assert!(
        refused
            .iter()
            .any(|outcome| matches!(outcome, Outcome::Refused(_, Blocked::Cooldown)))
    );
    // After the cooldown, a fresh throw from level counts.
    swing(&mut recognizer, -70.0, 0.0, config().cooldown_s);
    assert_eq!(throws(&swing(&mut recognizer, 0.0, -70.0, 0.1)).len(), 1);
}

#[test]
fn a_gate_refuses_the_throw_without_starting_the_cooldown() {
    let mut recognizer = EvadeRecognizer::default();
    let gated = (1..=10)
        .map(|tick| {
            recognizer.step(
                EvadeInput {
                    gate: Some(Blocked::Braking),
                    ..input(7.0 * tick as f32)
                },
                config(),
            )
        })
        .collect::<Vec<_>>();
    assert!(
        gated
            .iter()
            .any(|outcome| matches!(outcome, Outcome::Refused(_, Blocked::Braking)))
    );
    swing(&mut recognizer, 70.0, 0.0, 0.2);
    assert_eq!(throws(&swing(&mut recognizer, 0.0, 70.0, 0.1)).len(), 1);
}

#[test]
fn losing_the_arms_forgets_the_swing_and_the_held_direction() {
    let mut recognizer = EvadeRecognizer::default();
    swing(&mut recognizer, 0.0, 40.0, 0.1);
    let lost = EvadeInput {
        bank_deg: None,
        ..input(0.0)
    };
    assert_eq!(recognizer.step(lost, config()), Outcome::Idle);
    // The swing before the loss does not count toward this one.
    assert!(throws(&swing(&mut recognizer, 45.0, 60.0, 0.05)).is_empty());
}

#[test]
fn the_arms_pitch_picks_the_diagonal_past_the_threshold() {
    for (pitch, expected) in [
        (0.0, None),
        (0.5, None),
        (0.8, Some(Pitch::Forward)),
        (-0.8, Some(Pitch::Back)),
        (f32::NAN, None),
    ] {
        let mut recognizer = EvadeRecognizer::default();
        let outcomes = (1..=10)
            .map(|tick| {
                recognizer.step(
                    EvadeInput {
                        pitch,
                        ..input(-7.0 * tick as f32)
                    },
                    config(),
                )
            })
            .collect::<Vec<_>>();
        let thrown = throws(&outcomes);
        assert_eq!(thrown.len(), 1, "pitch {pitch}");
        assert_eq!(thrown[0].direction.pitch, expected, "pitch {pitch}");
        assert_eq!(thrown[0].direction.side, Side::Left);
    }
}

#[test]
fn each_direction_writes_a_unit_stick_in_its_own_sector() {
    for side in [Side::Left, Side::Right] {
        for pitch in [None, Some(Pitch::Forward), Some(Pitch::Back)] {
            let stick = EvadeDirection { side, pitch }.stick();
            assert!((stick.length() - 1.0).abs() < 1e-5);
            assert_eq!(stick.x > 0.0, side == Side::Right);
            let angle = stick.y.atan2(stick.x.abs()).to_degrees();
            let expected = match pitch {
                None => 0.0,
                Some(Pitch::Forward) => 45.0,
                Some(Pitch::Back) => -45.0,
            };
            assert!((angle - expected).abs() < 1e-3);
        }
    }
}

#[test]
fn the_bank_reads_the_hand_line_and_needs_the_arms_spread() {
    let span = 1.8;
    assert_eq!(bank_deg(0.0, 0.9, span), Some(0.0));
    let steep = bank_deg(0.9, 0.5, span).unwrap();
    assert!((steep - 45.0).abs() < 1e-3);
    assert!(bank_deg(-0.9, 0.5, span).unwrap() < 0.0);
    // A vertical hand line, both arms straight up and down, still reads as a bank.
    assert!((bank_deg(1.5, 0.0, span).unwrap() - 90.0).abs() < 1e-3);
    // Hands drawn in read no bank, nor do broken measures.
    assert_eq!(bank_deg(0.2, 0.2, span), None);
    assert_eq!(bank_deg(f32::NAN, 0.9, span), None);
    assert_eq!(bank_deg(0.1, f32::NAN, span), None);
}
