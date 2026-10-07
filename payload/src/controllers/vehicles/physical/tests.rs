//! Tests for the physical controls.

use std::f32::consts::FRAC_PI_2;

use super::*;
use crate::controllers::vehicles::{
    config::LeverAxis,
    physical::{handlebars::bar_angles, sticks::detent},
    steering,
};

const DT: f32 = 1.0 / 30.0;

fn placement(set: impl FnOnce(&mut PhysicalControlsConfig)) -> Placement {
    let mut config = PhysicalControlsConfig::new();
    set(&mut config);
    Placement::new(config, Hand::Right)
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
fn the_handlebars_steer_with_a_pulled_back_grip_and_lean_with_a_lowered_one() {
    let c = Vec3::ZERO;
    let steer = bar_angles(
        c,
        [
            Some(Vec3::new(-0.3, 0.0, -0.1)),
            Some(Vec3::new(0.3, 0.0, 0.1)),
        ],
    )
    .unwrap();
    assert!(steer.x > 0.0 && steer.y.abs() < 1e-5);
    let lean = bar_angles(
        c,
        [
            Some(Vec3::new(-0.3, 0.1, 0.0)),
            Some(Vec3::new(0.3, -0.1, 0.0)),
        ],
    )
    .unwrap();
    assert!(lean.y > 0.0 && lean.x.abs() < 1e-5);
    let one = bar_angles(c, [None, Some(Vec3::new(0.3, 0.0, 0.1))]).unwrap();
    assert!((one.x - steer.x).abs() < 1e-5);
    let ends = bar_ends(c, 0.3, Vec2::new(steer.x, 0.0));
    assert!(ends[1].z > 0.0 && ends[0].z < 0.0);
    let rolled = bar_ends(c, 0.3, Vec2::new(0.0, lean.y));
    assert!(rolled[1].y < 0.0);
}

#[test]
fn grabbing_the_handlebars_off_their_ends_neither_jumps_nor_biases_them() {
    let p = placement(|c| c.handlebars.enabled = true);
    let kinds = [ControlKind::Handlebars];
    let ends = bar_ends(p.bars.centre, p.bars.half_width, Vec2::ZERO);
    let mut state = ControlState::default();
    let close = |a: Vec2, b: Vec2| (a - b).length() < 1e-4;

    // The right hand takes its end a few centimetres in front of it and a little low.
    let right = ends[1] + Vec3::new(0.0, -0.03, -0.05);
    let (out, _) = step_with(
        &mut state,
        &p,
        &kinds,
        [None, Some(right)],
        [None, Some(ControlKind::Handlebars)],
    );
    assert!(close(state.bars, Vec2::ZERO), "{}", state.bars);
    assert_eq!(out.steer, Some(0.0));

    // Pulling it back turns the bars by the change alone.
    let pulled = right + Vec3::new(0.0, 0.0, 0.04);
    let (out, _) = step_with(&mut state, &p, &kinds, [None, Some(pulled)], [None; 2]);
    let turned = state.bars;
    let measured = |grips| bar_angles(p.bars.centre, grips).unwrap();
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
    let drawn = bar_ends(p.bars.centre, p.bars.half_width, turned)[0];
    assert!(anchored.distance(drawn) < 1e-4);

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
    let p = placement(|c| c.cyclic.enabled = true);
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
    assert!(
        (cyclic.y - 0.5).abs() < 1e-4 && cyclic.x.abs() < 1e-5,
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
    assert_eq!(out.cyclic, None);
    assert_eq!(state.stick, Vec2::ZERO);
}

#[test]
fn a_throttle_stays_where_it_is_let_go_and_a_collective_returns() {
    let p = placement(|c| {
        c.throttle.enabled = true;
        c.collective.enabled = true;
    });
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
    let p = placement(|c| c.cyclic.enabled = true);
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
    let mut config = PhysicalControlsConfig::new();
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
