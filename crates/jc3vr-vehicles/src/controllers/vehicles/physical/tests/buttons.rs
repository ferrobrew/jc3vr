//! Tests for the cockpit's buttons and the collective's twist grip.

use jc3vr_body::controllers::grab::cockpit::Button;

use super::*;
use crate::controllers::vehicles::{
    physical::buttons::{ButtonEvent, PushButton, PushPhase, grip_twist, push_step},
    write_outputs,
};

fn button() -> PushButton {
    PushButton {
        centre: Vec3::ZERO,
        normal: Vec3::Z,
        radius: 0.05,
        depth: 0.02,
    }
}

#[test]
fn a_push_from_the_front_presses_and_coming_back_out_lets_go() {
    let b = button();
    let mut phase = PushPhase::Out;
    phase = push_step(phase, &b, Some(Vec3::new(0.0, 0.0, 0.08)));
    assert_eq!(phase, PushPhase::Armed);
    phase = push_step(phase, &b, Some(Vec3::new(0.01, 0.0, -0.01)));
    assert_eq!(phase, PushPhase::Out, "short of the press depth");
    phase = push_step(PushPhase::Armed, &b, Some(Vec3::new(0.0, 0.0, -0.03)));
    assert_eq!(phase, PushPhase::Pressed);
    // It gives a little before it lets go.
    phase = push_step(phase, &b, Some(Vec3::new(0.0, 0.0, -0.015)));
    assert_eq!(phase, PushPhase::Pressed);
    phase = push_step(phase, &b, Some(Vec3::new(0.0, 0.0, 0.02)));
    assert_eq!(phase, PushPhase::Armed);
    assert_eq!(push_step(PushPhase::Pressed, &b, None), PushPhase::Out);
}

#[test]
fn a_hand_reaching_the_button_from_behind_or_the_side_presses_nothing() {
    let b = button();
    // From behind the face, as through the dash.
    let phase = push_step(PushPhase::Out, &b, Some(Vec3::new(0.0, 0.0, -0.03)));
    assert_eq!(phase, PushPhase::Out);
    // From the side, outside the radius in front, then slid in behind the face.
    let phase = push_step(PushPhase::Out, &b, Some(Vec3::new(0.2, 0.0, 0.05)));
    assert_eq!(phase, PushPhase::Out);
    assert_eq!(
        push_step(phase, &b, Some(Vec3::new(0.0, 0.0, -0.03))),
        PushPhase::Out
    );
}

#[test]
fn the_free_hand_sounds_the_horn_on_the_hub_while_the_other_holds_the_wheel() {
    let p = placement();
    let wheel = p.wheel;
    let kinds = [ControlKind::Wheel];
    let mut state = ControlState::default();
    let rim = wheel.rim_point(-FRAC_PI_2);
    let front = wheel.centre + wheel.normal() * 0.06;
    let (out, _) = step_with(
        &mut state,
        &p,
        &kinds,
        [Some(rim), Some(front)],
        [Some(ControlKind::Wheel), None],
    );
    assert!(!out.horn);
    let pushed = wheel.centre - wheel.normal() * 0.03;
    let (out, events) = step_with(&mut state, &p, &kinds, [Some(rim), Some(pushed)], [None; 2]);
    assert!(out.horn);
    assert!(events.contains(&ControlEvent::Button(ButtonEvent::Pressed(
        Hand::Right,
        Button::Horn
    ))));
    let mut desired = Vec::new();
    write_outputs(&out, &mut desired);
    // The horn also toggles a vehicle's magnet, as the gamepad's button does.
    for action in [
        jc3gi::input::input_action_map::Action::SOUND_HORN_SIREN,
        jc3gi::input::input_action_map::Action::TOGGLE_MAGNET_1,
    ] {
        assert!(
            desired.iter().any(|d| d.target.action == action),
            "{action:?}"
        );
    }

    // The hand holding the wheel presses nothing, even pushed into the hub.
    let mut state = ControlState::default();
    step_with(
        &mut state,
        &p,
        &kinds,
        [Some(rim), None],
        [Some(ControlKind::Wheel), None],
    );
    let (out, _) = step_with(&mut state, &p, &kinds, [Some(front), None], [None; 2]);
    assert!(!out.horn);
}

#[test]
fn the_nitrous_button_sits_beside_the_wheel_and_holds_the_vehicle_mod() {
    let p = placement();
    let config = p.config.buttons;
    let nitrous = PushButton::nitrous(&p.wheel, &config);
    let offset = nitrous.centre - p.wheel.centre;
    assert!(offset.dot(p.wheel.right) > p.wheel.radius, "{offset}");
    let kinds = [ControlKind::Wheel];
    let mut state = ControlState::default();
    step_with(
        &mut state,
        &p,
        &kinds,
        [None, Some(nitrous.centre + nitrous.normal * 0.05)],
        [None; 2],
    );
    let (out, _) = step_with(
        &mut state,
        &p,
        &kinds,
        [None, Some(nitrous.centre - nitrous.normal * 0.03)],
        [None; 2],
    );
    assert!(out.nitrous && !out.horn);

    let mut off = placement();
    off.nitrous = false;
    let mut state = ControlState::default();
    step_with(
        &mut state,
        &off,
        &kinds,
        [None, Some(nitrous.centre + nitrous.normal * 0.05)],
        [None; 2],
    );
    let (out, _) = step_with(
        &mut state,
        &off,
        &kinds,
        [None, Some(nitrous.centre - nitrous.normal * 0.03)],
        [None; 2],
    );
    assert!(!out.nitrous);
}

#[test]
fn the_collective_twists_either_way() {
    let grab = Quat::from_rotation_y(0.2);
    let either = Quat::from_rotation_z(-0.7) * grab;
    assert!(grip_twist(grab, either) > 0.6);
    let other = Quat::from_rotation_z(0.7) * grab;
    assert!(grip_twist(grab, other) > 0.6);
}
