//! The held wheel's motion: the holding hands turn it, and let go, it returns to centre.

use glam::Quat;

use crate::controllers::{
    frame::Hand,
    vehicles::{
        physical::{ControlState, Holding, Placement},
        steering::{self, WheelCue, WheelGrip, WheelTravel},
    },
};

/// The wheel's steering this tick, `None` while it neither is held nor returns, with the cue it
/// gives the holding hands. `stick_steers` says the stick's turn axis is out of its deadzone: a
/// released wheel still returning to centre then snaps there, so that the stick steers at once.
pub(super) fn step_wheel(
    state: &mut ControlState,
    placement: &Placement,
    holding: Holding,
    (orientations, stick_steers): ([Option<Quat>; 2], bool),
    dt: f32,
) -> (Option<f32>, Option<WheelCue>) {
    let spec = &placement.config.wheel;
    if holding.iter().all(Option::is_none) {
        state.wheel_grips = [None; 2];
        state.wheel_past_lock = 0.0;
        if state.wheel_angle == 0.0 {
            return (None, None);
        }
        if stick_steers {
            state.wheel_angle = 0.0;
            state.wheel_cues = steering::CueArming::new();
            return (None, None);
        }
        // The returning wheel steers on its way back, so the vehicle straightens with it.
        state.wheel_angle = steering::return_step(state.wheel_angle, spec, dt);
        return (
            Some(steering::steer_for_angle(state.wheel_angle, spec)),
            None,
        );
    }
    let angle = state.wheel_angle;
    let turns = Hand::BOTH.map(|hand| {
        let i = hand.index();
        let (_, point) = holding[i]?;
        let grip = state.wheel_grips[i]
            .get_or_insert_with(|| WheelGrip::new(&placement.wheel, point, orientations[i], angle));
        grip.turn(spec.turned_by, point, orientations[i])
    });
    let mut cue = None;
    if let Some(turn) = steering::combine(turns) {
        let travel = steering::advance(
            WheelTravel {
                angle,
                past_lock: state.wheel_past_lock,
            },
            turn,
            spec,
        );
        state.wheel_angle = travel.angle;
        state.wheel_past_lock = travel.past_lock;
        cue = steering::cue(&mut state.wheel_cues, angle, travel.angle, spec);
    }
    (
        Some(steering::steer_for_angle(state.wheel_angle, spec)),
        cue,
    )
}
