//! Tests for the cockpit's buttons and twist grips.

use super::*;
use crate::controllers::vehicles::{
    config::CockpitButtonsConfig,
    physical::buttons::{
        Button, ButtonEvent, PushButton, PushPhase, TwistGrip, grip_twist, push_step,
    },
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
    let p = placement(|c| c.wheel.enabled = true);
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
    assert!(
        desired
            .iter()
            .any(|d| d.target.action == jc3gi::input::input_action_map::Action::SOUND_HORN_SIREN)
    );

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
    let p = placement(|c| c.wheel.enabled = true);
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

    let off = placement(|c| {
        c.wheel.enabled = true;
        c.buttons.nitrous_button = false;
    });
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
fn twisting_the_right_grip_back_burns_the_nitrous_on_the_bars() {
    let config = CockpitButtonsConfig::new();
    let grab = Quat::from_rotation_y(0.2);
    // Rolling the grip's top back toward the rider is a positive turn about the bars' line.
    let back = Quat::from_rotation_x((config.twist_deg + 5.0).to_radians()) * grab;
    assert!(grip_twist(TwistGrip::Bars, grab, back, Vec3::X) > config.twist_deg.to_radians());
    let forward = Quat::from_rotation_x(-0.6) * grab;
    assert!(grip_twist(TwistGrip::Bars, grab, forward, Vec3::X) < 0.0);
    // The collective's grip works either way.
    let either = Quat::from_rotation_z(-0.7) * grab;
    assert!(grip_twist(TwistGrip::Collective, grab, either, Vec3::X) > 0.6);

    let p = placement(|c| c.handlebars.enabled = true);
    let kinds = [ControlKind::Handlebars];
    let ends = bar_ends(p.bars.centre, p.bars.half_width, Vec2::ZERO);
    let mut state = ControlState::default();
    let mut events = Vec::new();
    let mut inputs = StepInputs {
        kinds: &kinds,
        grips: [Some(ends[0]), Some(ends[1])],
        orientations: [Some(Quat::IDENTITY), Some(grab)],
        held: [true; 2],
        grabs: [Some(ControlKind::Handlebars); 2],
        stick_steers: false,
        stick_moves: false,
        lever_moves: false,
        dt: DT,
    };
    assert!(!step(&mut state, &p, &inputs, &mut events).nitrous);
    inputs.grabs = [None; 2];
    inputs.orientations = [Some(back), Some(grab)];
    assert!(
        !step(&mut state, &p, &inputs, &mut events).nitrous,
        "the left grip does not twist"
    );
    inputs.orientations = [Some(Quat::IDENTITY), Some(back)];
    let out = step(&mut state, &p, &inputs, &mut events);
    assert!(out.nitrous);
    assert!(events.contains(&ControlEvent::Button(ButtonEvent::Twisted(
        Hand::Right,
        TwistGrip::Bars
    ))));
    inputs.orientations = [Some(Quat::IDENTITY), Some(grab)];
    assert!(!step(&mut state, &p, &inputs, &mut events).nitrous);
}
