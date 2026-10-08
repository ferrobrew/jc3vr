//! Writing a held control's or a steered stick's values over the input tables' analog writes,
//! replacing whichever half-axes the write touches.

use glam::Vec2;
use jc3gi::input::input_action_map::Action::{self, *};

use crate::controllers::{
    bindings::hold::{Desired, MapKind, Target, Want},
    vehicles::physical::Outputs,
};

/// Write the held controls' axes over the tables' writes for the same actions.
pub fn write_outputs(out: &Outputs, desired: &mut Vec<Desired>) {
    if let Some(steer) = out.steer {
        replace_axis(desired, steer, TURN_LEFT, TURN_RIGHT);
    }
    if let Some(lean) = out.lean {
        replace_axis(desired, lean, BIKE_LEAN_LEFT, BIKE_LEAN_RIGHT);
    }
    if let Some(cyclic) = out.cyclic {
        replace_axis(desired, cyclic.x, HELI_ROLL_LEFT, HELI_ROLL_RIGHT);
        replace_axis(desired, cyclic.y, HELI_BACKWARD, HELI_FORWARD);
    }
    if let Some(collective) = out.collective {
        replace_axis(desired, collective, HELI_DEC_ALTITUDE, HELI_INC_ALTITUDE);
    }
    if let Some(stick) = out.flight_stick {
        replace_axis(desired, stick.x, PLANE_ROLL_LEFT, PLANE_ROLL_RIGHT);
        // Pushing the stick forward pitches the nose down.
        replace_axis(desired, stick.y, PLANE_PITCH_UP, PLANE_PITCH_DOWN);
    }
    if let Some(rudder) = out.rudder {
        replace_axis(desired, rudder, PLANE_TURN_LEFT, PLANE_TURN_RIGHT);
    }
    if let Some(throttle) = out.throttle {
        replace_axis(desired, throttle, PLANE_DEC_TRUST, PLANE_INC_TRUST);
    }
    // The buttons hold their actions alongside the controller's buttons, which merge with them.
    for (pressed, action) in [(out.horn, SOUND_HORN_SIREN), (out.nitrous, USE_VEHICLE_MOD)] {
        if pressed {
            desired.push(Desired {
                target: Target {
                    map: MapKind::Player,
                    action,
                },
                want: Want::Hold(1.0),
            });
        }
    }
}

/// Whether the tables wrote either half of the axis `negative`/`positive` this tick: their stick
/// is out of its deadzone.
pub(super) fn axis_wanted(desired: &[Desired], negative: Action, positive: Action) -> bool {
    desired.iter().any(|d| {
        d.target.map == MapKind::Player
            && (d.target.action == negative || d.target.action == positive)
            && match d.want {
                Want::Analog(v) | Want::Hold(v) => v > 0.0,
                Want::Click => true,
            }
    })
}

/// Whether the tables wrote the axes of the cyclic or the flight stick this tick.
pub(super) fn stick_axes_wanted(desired: &[Desired]) -> bool {
    [
        (HELI_ROLL_LEFT, HELI_ROLL_RIGHT),
        (HELI_BACKWARD, HELI_FORWARD),
        (PLANE_ROLL_LEFT, PLANE_ROLL_RIGHT),
        (PLANE_PITCH_UP, PLANE_PITCH_DOWN),
    ]
    .into_iter()
    .any(|(negative, positive)| axis_wanted(desired, negative, positive))
}

/// Whether the tables wrote the axis of the collective or the throttle this tick.
pub(super) fn lever_axes_wanted(desired: &[Desired]) -> bool {
    axis_wanted(desired, HELI_DEC_ALTITUDE, HELI_INC_ALTITUDE)
        || axis_wanted(desired, PLANE_DEC_TRUST, PLANE_INC_TRUST)
}

/// Write `stick` (+X right, +Y forward) over the tables' move half-axes and their length.
pub(super) fn write_move(desired: &mut Vec<Desired>, stick: Vec2) {
    replace_axis(desired, stick.y, MOVE_BACKWARD, MOVE_FORWARD);
    replace_axis(desired, stick.x, MOVE_LEFT, MOVE_RIGHT);
    replace_axis(desired, stick.length().min(1.0), MOVE_ALL, MOVE_ALL);
}

/// Replace every player-map want on `negative` and `positive` with the signed `value` split into
/// the two non-negative half-axes, as the tables write an axis.
fn replace_axis(desired: &mut Vec<Desired>, value: f32, negative: Action, positive: Action) {
    desired.retain(|d| {
        !(d.target.map == MapKind::Player
            && (d.target.action == negative || d.target.action == positive))
    });
    let value = if value.is_finite() { value } else { 0.0 };
    for (action, half) in [(negative, -value), (positive, value)] {
        if half > 0.0 {
            desired.push(Desired {
                target: Target {
                    map: MapKind::Player,
                    action,
                },
                want: Want::Analog(half.min(1.0)),
            });
        }
    }
}
