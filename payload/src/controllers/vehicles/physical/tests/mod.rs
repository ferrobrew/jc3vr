//! Tests for the physical controls: the wheel, the handlebars, and the common grab and release here,
//! the plane's flight stick and throttle in [`flight`], and the cockpit's buttons in [`buttons`].

mod buttons;
mod flight;

use std::f32::consts::FRAC_PI_2;

use super::*;
use crate::controllers::vehicles::{
    config::PhysicalControlsConfig,
    physical::{
        handlebars::{bar_grab_point, bar_turn, steer_for_turn},
        levers::detent,
        sticks,
    },
    steering,
    tuning::{HandlebarsSpec, LeverAxis, PHYSICAL, PhysicalControlsTuning},
};

const DT: f32 = 1.0 / 30.0;

fn placement(set: impl FnOnce(&mut PhysicalControlsConfig)) -> Placement {
    let mut config = PhysicalControlsConfig::new();
    set(&mut config);
    Placement::new(PhysicalControlsTuning::of(&config), Hand::Right)
}

/// A tick with every tracked hand's grip held.
fn step_with(
    state: &mut ControlState,
    placement: &Placement,
    kinds: &[ControlKind],
    grips: [Option<Vec3>; 2],
    grabs: [Option<ControlKind>; 2],
) -> (Outputs, Vec<ControlEvent>) {
    step_held(
        state,
        placement,
        kinds,
        grips,
        grabs,
        grips.map(|g| g.is_some()),
    )
}

fn step_held(
    state: &mut ControlState,
    placement: &Placement,
    kinds: &[ControlKind],
    grips: [Option<Vec3>; 2],
    grabs: [Option<ControlKind>; 2],
    held: [bool; 2],
) -> (Outputs, Vec<ControlEvent>) {
    let mut events = Vec::new();
    let out = step(
        state,
        placement,
        &StepInputs {
            kinds,
            grips,
            orientations: [None; 2],
            held,
            grabs,
            stick_steers: false,
            stick_moves: false,
            lever_moves: false,
            dt: DT,
        },
        &mut events,
    );
    (out, events)
}

#[test]
fn turning_the_wheel_clockwise_steers_right_and_it_returns_when_let_go() {
    let p = placement(|c| c.wheel.enabled = true);
    let spec = p.config.wheel;
    let wheel = p.wheel;
    let kinds = [ControlKind::Wheel];
    let mut state = ControlState::default();
    let left = wheel.rim_point(-FRAC_PI_2);
    let right = wheel.rim_point(FRAC_PI_2);
    let grabs = [Some(ControlKind::Wheel); 2];
    step_with(&mut state, &p, &kinds, [Some(left), Some(right)], grabs);
    assert!(state.holds.iter().all(Option::is_some));

    let turn = 0.5;
    let left = wheel.rim_point(-FRAC_PI_2 + turn);
    let right = wheel.rim_point(FRAC_PI_2 + turn);
    let (out, _) = step_with(&mut state, &p, &kinds, [Some(left), Some(right)], [None; 2]);
    let expected = steering::steer_for_angle(turn, &spec);
    assert!(expected > 0.0);
    assert!((out.steer.unwrap() - expected).abs() < 1e-4, "{out:?}");

    let grips = [Some(left), Some(right)];
    let (out, events) = step_held(&mut state, &p, &kinds, grips, [None; 2], [false; 2]);
    assert_eq!(events.len(), 2);
    assert!(state.holds.iter().all(Option::is_none));
    let returning = out.steer.unwrap();
    assert!(returning > 0.0 && returning < expected, "{out:?}");
    for _ in 0..60 {
        step_held(&mut state, &p, &kinds, grips, [None; 2], [false; 2]);
    }
    assert_eq!(state.wheel_angle, 0.0);
    let (out, _) = step_held(&mut state, &p, &kinds, grips, [None; 2], [false; 2]);
    assert_eq!(out.steer, None);
}

#[test]
fn a_hand_in_the_wheel_with_its_grip_open_takes_nothing() {
    let p = placement(|c| c.wheel.enabled = true);
    let kinds = [ControlKind::Wheel];
    let mut state = ControlState::default();
    let on = p.wheel.rim_point(0.3);
    // No press edge: the hand rests on the rim, held or not, and takes nothing.
    let (out, events) = step_held(
        &mut state,
        &p,
        &kinds,
        [None, Some(on)],
        [None; 2],
        [false, true],
    );
    assert!(events.is_empty() && state.holds[1].is_none());
    assert_eq!(out.steer, None);
    // A press takes it, and it stays taken only while the grip is held.
    step_with(
        &mut state,
        &p,
        &kinds,
        [None, Some(on)],
        [None, Some(ControlKind::Wheel)],
    );
    step_with(&mut state, &p, &kinds, [None, Some(on)], [None; 2]);
    assert!(state.holds[1].is_some());
    let (_, events) = step_held(
        &mut state,
        &p,
        &kinds,
        [None, Some(on)],
        [None; 2],
        [false; 2],
    );
    assert_eq!(
        events,
        vec![ControlEvent::Released(
            Hand::Right,
            ControlKind::Wheel,
            Release::GripReleased
        )]
    );
}

#[test]
fn grabbing_the_wheel_does_not_jump_it_and_a_hand_over_carries_on() {
    let p = placement(|c| c.wheel.enabled = true);
    let wheel = p.wheel;
    let kinds = [ControlKind::Wheel];
    let mut state = ControlState::default();
    let take = |state: &mut ControlState, hand: Hand, at: Vec3| {
        let mut grips = [None; 2];
        grips[hand.index()] = Some(at);
        let mut grabs = [None; 2];
        grabs[hand.index()] = Some(ControlKind::Wheel);
        step_with(state, &p, &kinds, grips, grabs)
    };
    // The right hand takes the wheel off its top and turns it a third of a turn.
    take(&mut state, Hand::Right, wheel.rim_point(0.3));
    assert_eq!(state.wheel_angle, 0.0);
    step_with(
        &mut state,
        &p,
        &kinds,
        [None, Some(wheel.rim_point(0.3 + 2.0))],
        [None; 2],
    );
    let turned = state.wheel_angle;
    assert!((turned - 2.0).abs() < 1e-4, "{turned}");
    // The left hand takes it at the top, and the right lets go: the wheel stays put.
    let top = wheel.rim_point(0.0);
    let (_, _) = step_with(
        &mut state,
        &p,
        &kinds,
        [Some(top), Some(wheel.rim_point(2.3))],
        [Some(ControlKind::Wheel), None],
    );
    step_held(
        &mut state,
        &p,
        &kinds,
        [Some(top), Some(wheel.rim_point(2.3))],
        [None; 2],
        [true, false],
    );
    assert!((state.wheel_angle - turned).abs() < 1e-4);
    // The left hand carries on turning, up to the lock.
    step_held(
        &mut state,
        &p,
        &kinds,
        [Some(wheel.rim_point(0.2)), None],
        [None; 2],
        [true, false],
    );
    let lock = p.config.wheel.lock_deg.to_radians();
    assert!((state.wheel_angle - (turned + 0.2).min(lock)).abs() < 1e-4);
}

#[test]
fn a_hand_that_leaves_the_wheel_lets_go() {
    let p = placement(|c| c.wheel.enabled = true);
    let kinds = [ControlKind::Wheel];
    let mut state = ControlState::default();
    let on = p.wheel.rim_point(0.3);
    step_with(
        &mut state,
        &p,
        &kinds,
        [None, Some(on)],
        [None, Some(ControlKind::Wheel)],
    );
    let away = on + Vec3::new(0.0, 0.0, 0.5);
    let (_, events) = step_with(&mut state, &p, &kinds, [None, Some(away)], [None; 2]);
    assert_eq!(
        events,
        vec![ControlEvent::Released(
            Hand::Right,
            ControlKind::Wheel,
            Release::MovedAway
        )]
    );
}

#[test]
fn the_handlebars_turn_with_a_pulled_back_grip_and_not_with_a_lowered_one() {
    let c = Vec3::ZERO;
    let turn = bar_turn(
        c,
        [
            Some(Vec3::new(-0.3, 0.0, -0.1)),
            Some(Vec3::new(0.3, 0.0, 0.1)),
        ],
    )
    .unwrap();
    assert!(turn > 0.0);
    let lowered = bar_turn(
        c,
        [
            Some(Vec3::new(-0.3, 0.1, 0.0)),
            Some(Vec3::new(0.3, -0.1, 0.0)),
        ],
    )
    .unwrap();
    assert!(lowered.abs() < 1e-5, "{lowered}");
    let one = bar_turn(c, [None, Some(Vec3::new(0.3, 0.0, 0.1))]).unwrap();
    assert!((one - turn).abs() < 1e-5);
    let ends = bar_ends(c, 0.3, turn);
    assert!(ends[1].z > 0.0 && ends[0].z < 0.0);
    assert!(ends.iter().all(|e| e.y.abs() < 1e-6));
}

#[test]
fn the_handlebars_steer_past_a_deadzone_on_a_curve_and_lean_as_they_steer() {
    let spec = PHYSICAL.handlebars;
    let lock = spec.max_steer_deg.to_radians();
    let deadzone = spec.deadzone_deg.to_radians();
    assert!(deadzone > 0.0 && spec.response > 1.0);
    assert_eq!(steer_for_turn(deadzone * 0.9, &spec), 0.0);
    assert_eq!(steer_for_turn(lock, &spec), 1.0);
    assert_eq!(steer_for_turn(-2.0 * lock, &spec), -1.0);
    assert_eq!(steer_for_turn(f32::NAN, &spec), 0.0);
    let halfway = deadzone + (lock - deadzone) * 0.5;
    let half = steer_for_turn(halfway, &spec);
    assert!(half > 0.0 && half < 0.5, "{half}");
    assert!((steer_for_turn(-halfway, &spec) + half).abs() < 1e-6);
    let linear = HandlebarsSpec {
        response: 1.0,
        ..spec
    };
    assert!((steer_for_turn(halfway, &linear) - 0.5).abs() < 1e-5);

    // Held and turned, the bars write the same value to the steering and the lean, whatever the
    // grips' heights.
    let p = placement(|c| c.handlebars = true);
    let kinds = [ControlKind::Handlebars];
    let ends = bar_ends(p.bars.centre, p.bars.half_width, 0.0);
    let mut state = ControlState::default();
    step_with(
        &mut state,
        &p,
        &kinds,
        [Some(ends[0]), Some(ends[1])],
        [Some(ControlKind::Handlebars); 2],
    );
    let turned = bar_ends(p.bars.centre, p.bars.half_width, halfway);
    let (out, _) = step_with(
        &mut state,
        &p,
        &kinds,
        [
            Some(turned[0] + Vec3::new(0.0, 0.06, 0.0)),
            Some(turned[1] - Vec3::new(0.0, 0.06, 0.0)),
        ],
        [None; 2],
    );
    let steer = out.steer.unwrap();
    assert!((steer - half).abs() < 1e-3, "{steer} {half}");
    assert_eq!(out.lean, out.steer);
}

#[test]
fn a_hand_takes_the_handlebars_anywhere_along_its_half() {
    let p = placement(|c| c.handlebars = true);
    let state = ControlState::default();
    let ends = bar_ends(p.bars.centre, p.bars.half_width, 0.0);
    let radius = p.config.handlebars.grab_radius_m;
    assert!(radius > p.config.grab_radius_m);
    // Inboard of the right grip and a little above it, the bars are in reach at the point below.
    let inboard = p.bars.centre.lerp(ends[1], 0.6) + Vec3::new(0.0, 0.1, 0.0);
    let candidates = candidates(
        &p,
        &state,
        &[ControlKind::Handlebars],
        [None, Some(inboard)],
    );
    assert_eq!(candidates.len(), 1);
    assert!(candidates[0].reach(inboard).is_some());
    assert!(candidates[0].centre.distance(inboard) < 0.1 + 1e-4);
    // The middle of the bars and the other half are not this hand's.
    let middle = p.bars.centre;
    let grab = p
        .grab_point(&state, ControlKind::Handlebars, Hand::Right, middle)
        .unwrap();
    assert!(grab.x > p.bars.centre.x + p.bars.half_width * 0.3);
    // Well past the end, the reach runs out.
    let past = ends[1] + Vec3::new(radius + 0.12, 0.0, 0.0);
    let far = candidates_for(&p, &state, past);
    assert!(far.reach(past).is_none());
}

fn candidates_for(p: &Placement, state: &ControlState, point: Vec3) -> Candidate {
    candidates(p, state, &[ControlKind::Handlebars], [None, Some(point)])[0]
}

#[test]
fn grabbing_the_handlebars_off_their_ends_neither_jumps_nor_biases_them() {
    let p = placement(|c| c.handlebars = true);
    let kinds = [ControlKind::Handlebars];
    let ends = bar_ends(p.bars.centre, p.bars.half_width, 0.0);
    let mut state = ControlState::default();
    let close = |a: f32, b: f32| (a - b).abs() < 1e-4;

    // The right hand takes its end a few centimetres in front of it and a little low.
    let right = ends[1] + Vec3::new(0.0, -0.03, -0.05);
    let (out, _) = step_with(
        &mut state,
        &p,
        &kinds,
        [None, Some(right)],
        [None, Some(ControlKind::Handlebars)],
    );
    assert!(close(state.bars, 0.0), "{}", state.bars);
    assert_eq!(out.steer, Some(0.0));

    // Pulling it back turns the bars by the change alone.
    let pulled = right + Vec3::new(0.0, 0.0, 0.04);
    let (out, _) = step_with(&mut state, &p, &kinds, [None, Some(pulled)], [None; 2]);
    let turned = state.bars;
    let measured = |grips| bar_turn(p.bars.centre, grips).unwrap();
    let change = measured([None, Some(pulled)]) - measured([None, Some(right)]);
    assert!(out.steer.unwrap() > 0.0);
    assert!(close(turned, change), "{turned} {change}");

    // The left hand joins off its end too, and the bars stay where they were.
    let left = ends[0] + Vec3::new(0.0, 0.02, 0.06);
    step_with(
        &mut state,
        &p,
        &kinds,
        [Some(left), Some(pulled)],
        [Some(ControlKind::Handlebars), None],
    );
    assert!(close(state.bars, turned), "{}", state.bars);
    let anchored = anchor_point(
        &state,
        &p,
        state.holds[0].unwrap(),
        Hand::Left,
        [Some(left), Some(pulled)],
    )
    .unwrap();
    let drawn = bar_grab_point(&p.bars, turned, Hand::Left, left);
    assert!(anchored.distance(drawn) < 1e-4);
    // The drawn hand sits on the turned bars' left half.
    let end = bar_ends(p.bars.centre, p.bars.half_width, turned)[0];
    let on_bar = (drawn - p.bars.centre).cross(end - p.bars.centre).length();
    assert!(on_bar < 1e-5, "{on_bar}");

    // The right hand lets go, and the bars still stay.
    step_held(
        &mut state,
        &p,
        &kinds,
        [Some(left), Some(pulled)],
        [None; 2],
        [true, false],
    );
    assert!(close(state.bars, turned), "{}", state.bars);
}

#[test]
fn a_stick_follows_the_hand_within_its_travel_and_springs_back() {
    let p = placement(|c| c.sticks = true);
    let kinds = [ControlKind::Cyclic];
    let top = p.config.cyclic.top_m;
    let mut state = ControlState::default();
    let grip = top + Vec3::new(0.0, 0.05, 0.0);
    step_with(
        &mut state,
        &p,
        &kinds,
        [None, Some(grip)],
        [None, Some(ControlKind::Cyclic)],
    );
    let pushed = grip + Vec3::new(0.0, 0.0, -0.06);
    let (out, _) = step_with(&mut state, &p, &kinds, [None, Some(pushed)], [None; 2]);
    let cyclic = out.cyclic.unwrap();
    let expected = sticks::stick_value(&p.config.cyclic, Vec2::new(0.0, -0.06)).y;
    assert!(expected > 0.4 && expected < 0.5, "{expected}");
    assert!(
        (cyclic.y - expected).abs() < 1e-4 && cyclic.x.abs() < 1e-5,
        "{cyclic}"
    );
    let far = grip + Vec3::new(0.2, 0.0, 0.0);
    let anchor = anchor_point(
        &state,
        &p,
        state.holds[1].unwrap(),
        Hand::Right,
        [None, Some(far)],
    )
    .unwrap();
    assert!((anchor.x - grip.x - p.config.cyclic.max_deflection_m).abs() < 1e-4);
    let (out, _) = step_held(
        &mut state,
        &p,
        &kinds,
        [None, Some(pushed)],
        [None; 2],
        [false; 2],
    );
    // Let go, the spring brings the stick back, still flying the vehicle on its way.
    let returning = out.cyclic.unwrap();
    assert!(returning.y > 0.0 && returning.y < cyclic.y, "{returning}");
    for _ in 0..60 {
        step_held(&mut state, &p, &kinds, [None; 2], [None; 2], [false; 2]);
    }
    assert_eq!(state.stick, Vec2::ZERO);
    let (out, _) = step_held(&mut state, &p, &kinds, [None; 2], [None; 2], [false; 2]);
    assert_eq!(out.cyclic, None);
}

#[test]
fn a_throttle_stays_where_it_is_let_go_and_a_collective_returns() {
    let p = placement(|c| c.levers = true);
    for (kind, holds) in [
        (ControlKind::Throttle, true),
        (ControlKind::Collective, false),
    ] {
        let kinds = [kind];
        let (rest, axis) = match kind {
            ControlKind::Throttle => (p.config.throttle.rest_m, LeverAxis::ForeAft),
            _ => (p.config.collective.rest_m, LeverAxis::Vertical),
        };
        let mut state = ControlState::default();
        step_with(
            &mut state,
            &p,
            &kinds,
            [Some(rest), None],
            [Some(kind), None],
        );
        let moved = rest + axis.direction() * 0.12;
        let (out, _) = step_with(&mut state, &p, &kinds, [Some(moved), None], [None; 2]);
        let value = match kind {
            ControlKind::Throttle => out.throttle,
            _ => out.collective,
        };
        assert!(value.unwrap() > 0.5, "{kind:?} {out:?}");
        let (out, _) = step_held(
            &mut state,
            &p,
            &kinds,
            [Some(moved), None],
            [None; 2],
            [false; 2],
        );
        let value = match kind {
            ControlKind::Throttle => out.throttle,
            _ => out.collective,
        };
        assert_eq!(value.is_some(), holds, "{kind:?}");
    }
}

#[test]
fn a_one_handed_control_passes_to_the_hand_that_takes_it() {
    let p = placement(|c| c.sticks = true);
    let kinds = [ControlKind::Cyclic];
    let top = p.config.cyclic.top_m;
    let mut state = ControlState::default();
    step_with(
        &mut state,
        &p,
        &kinds,
        [None, Some(top)],
        [None, Some(ControlKind::Cyclic)],
    );
    let (_, events) = step_with(
        &mut state,
        &p,
        &kinds,
        [Some(top), Some(top)],
        [Some(ControlKind::Cyclic), None],
    );
    assert!(events.contains(&ControlEvent::Released(
        Hand::Right,
        ControlKind::Cyclic,
        Release::TakenOver
    )));
    assert!(state.holds[0].is_some() && state.holds[1].is_none());
}

#[test]
fn a_left_handed_layout_mirrors_the_sticks_but_not_the_wheel() {
    let mut config = PhysicalControlsTuning::of(&PhysicalControlsConfig::new());
    config.wheel.centre_m.x = -0.3;
    let p = Placement::new(config, Hand::Left);
    let state = ControlState::default();
    let stick = p
        .grab_point(&state, ControlKind::Cyclic, Hand::Left, Vec3::ZERO)
        .unwrap();
    assert!(stick.x < 0.0);
    let lever = p
        .grab_point(&state, ControlKind::Collective, Hand::Right, Vec3::ZERO)
        .unwrap();
    assert!(lever.x > 0.0);
    assert_eq!(p.wheel.centre, config.wheel.centre_m);
    let rim = p
        .grab_point(
            &state,
            ControlKind::Wheel,
            Hand::Left,
            Vec3::new(-0.3, 0.0, 0.0),
        )
        .unwrap();
    assert!(rim.x < -0.25, "{rim}");
}

#[test]
fn the_detent_swallows_neutral_and_still_reaches_one() {
    assert_eq!(detent(0.05, 0.1), 0.0);
    assert_eq!(detent(1.0, 0.1), 1.0);
    assert!((detent(-0.55, 0.1) + 0.5).abs() < 1e-5);
}

#[test]
fn only_the_seat_class_s_controls_are_offered() {
    assert_eq!(
        ControlKind::for_seat(VehicleClass::Motorcycle, false),
        &[ControlKind::Handlebars]
    );
    assert_eq!(
        ControlKind::for_seat(VehicleClass::Car, false),
        &[ControlKind::Wheel]
    );
    for class in [VehicleClass::Car, VehicleClass::Sea] {
        assert_eq!(
            ControlKind::for_seat(class, true),
            &[ControlKind::Handlebars]
        );
    }
    let mut off = PhysicalControlsConfig::new();
    off.wheel.enabled = false;
    assert!(!ControlKind::Wheel.enabled(&off));
}

#[test]
fn at_the_lock_the_hand_stays_on_the_stopped_wheel_and_must_come_back() {
    let p = placement(|c| c.wheel.enabled = true);
    let wheel = p.wheel;
    let kinds = [ControlKind::Wheel];
    let lock = p.config.wheel.lock_deg.to_radians();
    let mut state = ControlState::default();
    let start = 0.3;
    step_with(
        &mut state,
        &p,
        &kinds,
        [None, Some(wheel.rim_point(start))],
        [None, Some(ControlKind::Wheel)],
    );
    // The hand runs on past the lock in small steps; the wheel stops there, with a bump once.
    let mut cues = Vec::new();
    let mut at = start;
    while at < start + lock + 0.3 {
        at += 0.05;
        let grips = [None, Some(wheel.rim_point(at))];
        let (out, _) = step_with(&mut state, &p, &kinds, grips, [None; 2]);
        cues.extend(out.wheel_cue);
    }
    assert_eq!(state.wheel_angle, lock);
    assert_eq!(cues, [steering::WheelCue::Stop]);
    let hold = state.holds[1].unwrap();
    let grips = [None, Some(wheel.rim_point(at))];
    let drawn = anchor_point(&state, &p, hold, Hand::Right, grips).unwrap();
    assert!(drawn.distance(wheel.rim_point(start + lock)) < 1e-4);
    // Coming back by less than the overshoot leaves the wheel at the stop.
    at -= 0.1;
    let grips = [None, Some(wheel.rim_point(at))];
    step_with(&mut state, &p, &kinds, grips, [None; 2]);
    assert_eq!(state.wheel_angle, lock);
}

#[test]
fn the_stick_takes_the_steering_back_from_a_returning_wheel() {
    let p = placement(|c| c.wheel.enabled = true);
    let kinds = [ControlKind::Wheel];
    let mut state = ControlState {
        wheel_angle: 1.0,
        ..ControlState::default()
    };
    let mut events = Vec::new();
    let mut inputs = StepInputs {
        kinds: &kinds,
        grips: [None; 2],
        orientations: [None; 2],
        held: [false; 2],
        grabs: [None; 2],
        stick_steers: false,
        stick_moves: false,
        lever_moves: false,
        dt: DT,
    };
    let out = step(&mut state, &p, &inputs, &mut events);
    assert!(out.steer.is_some_and(|s| s > 0.0));
    inputs.stick_steers = true;
    let out = step(&mut state, &p, &inputs, &mut events);
    assert_eq!(out.steer, None);
    assert_eq!(state.wheel_angle, 0.0);
}
