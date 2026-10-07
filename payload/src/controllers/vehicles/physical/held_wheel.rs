//! The held wheel's motion: the holding hands turn it, and let go, it returns to centre.

use glam::Quat;

use crate::controllers::{
    frame::Hand,
    vehicles::{
        physical::{ControlState, Holding, Placement},
        steering::{self, WheelGrip},
    },
};

pub(super) fn step_wheel(
    state: &mut ControlState,
    placement: &Placement,
    holding: Holding,
    orientations: [Option<Quat>; 2],
    dt: f32,
) -> Option<f32> {
    let spec = &placement.config.wheel;
    if holding.iter().all(Option::is_none) {
        state.wheel_grips = [None; 2];
        if state.wheel_angle == 0.0 {
            return None;
        }
        // The returning wheel steers on its way back, so the vehicle straightens with it.
        state.wheel_angle = steering::return_step(state.wheel_angle, spec, dt);
        return Some(steering::steer_for_angle(state.wheel_angle, spec));
    }
    let turns = Hand::BOTH.map(|hand| {
        let i = hand.index();
        let (_, point) = holding[i]?;
        let grip = state.wheel_grips[i]
            .get_or_insert_with(|| WheelGrip::new(&placement.wheel, point, orientations[i]));
        grip.turn(&placement.wheel, spec.turned_by, point, orientations[i])
    });
    if let Some(turn) = steering::combine(turns) {
        state.wheel_angle = steering::advance(state.wheel_angle, turn, spec);
    }
    Some(steering::steer_for_angle(state.wheel_angle, spec))
}
