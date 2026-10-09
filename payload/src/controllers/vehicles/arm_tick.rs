//! The wingsuit's arm-pose steering: adding the arms' stick vector to the move stick while the
//! wingsuit's arm steering is switched on.

use glam::Vec2;

use crate::controllers::{
    bindings::{controls::Controls, hold::Desired},
    config::ControllersConfig,
    frame::Hand,
    hands::ActiveHands,
    mode::{AirborneVariant, Mode},
    vehicles::{
        arm_steering::{self as arm, ArmBody, ArmHand},
        axis_output::write_move,
        begin::body_dimensions,
        types::ArmReadout,
    },
};

/// Whether the arm steering acts in `mode`: in the wingsuit, with it switched on.
pub(super) fn arms_act(mode: Mode, config: &ControllersConfig) -> bool {
    config.vehicles.arm_steering.wingsuit && mode == Mode::Airborne(AirborneVariant::Wingsuit)
}

/// Add the arm pose to the move stick in the wingsuit, when its arm steering is on. `last` is the
/// arms' vector on the last tick (`Some(None)` when they did not steer, `None` with no last tick),
/// and `aiming` whether the grapple's button is held: while it is, the arms keep the vector they
/// had when it was pressed ([`arm::hold_while_aiming`]).
pub(super) fn arm_tick(
    mode: Mode,
    controls: &Controls,
    active: &ActiveHands,
    desired: &mut Vec<Desired>,
    last: Option<Option<Vec2>>,
    aiming: bool,
) -> Option<ArmReadout> {
    let config = &active.config.vehicles.arm_steering;
    if !arms_act(mode, &active.config) {
        return None;
    }
    let hand = |hand: Hand| {
        active.hands[hand.index()].map(|h| ArmHand {
            position: h.grip.position,
            wrist: h.wrist.orientation,
        })
    };
    let hands = [hand(Hand::Left)?, hand(Hand::Right)?];
    let dims = body_dimensions(&active.config);
    let body = ArmBody {
        head: active.head.position,
        eyes_above_shoulders: dims.eye_height - dims.shoulder_height,
        arm_span: dims.arm_span,
    };
    let pose = arm::arm_pose(hands, &body);
    let held = last.flatten();
    let engaged = held.is_some();
    let arms = arm::hold_while_aiming(arm::steer(&pose, config, engaged), held, aiming);
    let stick = controls.off().stick;
    let written = arm::combine(stick, arms);
    write_move(desired, written);
    Some(ArmReadout {
        pose,
        needed_spread: arm::needed_spread(config, engaged),
        shoulder_y: body.head.y - body.eyes_above_shoulders,
        pitch: arm::pitch_input(&pose, config),
        arms,
        held_for_grapple: aiming,
        stick,
        written,
    })
}
