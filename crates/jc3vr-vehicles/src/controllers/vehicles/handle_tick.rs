//! Steering with the parachute's held handles: their combined stick vector added to the move
//! stick under the parachute.

use jc3vr_body::controllers::{
    intent::{controls::Controls, hold::Desired},
    locomotion::axis_output::write_move,
    mode::{AirborneVariant, Mode},
};

use crate::controllers::vehicles::{
    HandlesReadout, arm_steering as arm, config::VehicleSettings, parachute,
    parachute::HandleState, tuning::ParachuteHandlesTuning,
};

/// Whether the parachute handles act in `mode`.
pub(super) fn handles_act(mode: Mode, config: &VehicleSettings) -> bool {
    config.vehicles.parachute_handles.enabled && mode == Mode::Airborne(AirborneVariant::Parachute)
}

/// Steer with the held handles under the parachute: their vector added to the move stick, written
/// to the move half-axes the parachute task reads, as the stick's. With neither handle held, the
/// tables' stick writes stand.
pub(super) fn handles_tick(
    state: &HandleState,
    pulls: [f32; 2],
    controls: &Controls,
    desired: &mut Vec<Desired>,
    config: &ParachuteHandlesTuning,
) -> HandlesReadout {
    let held = state.holds.map(|h| h.is_some());
    let handles = held
        .contains(&true)
        .then(|| parachute::steer(pulls, config));
    let written = handles.map(|h| arm::combine(controls.off().stick, Some(h)));
    if let Some(written) = written {
        write_move(desired, written);
    }
    HandlesReadout {
        held,
        pulls,
        handles,
        written,
    }
}
