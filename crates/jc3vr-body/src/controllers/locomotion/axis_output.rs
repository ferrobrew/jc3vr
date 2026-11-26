//! Writing values over the input tables' analog writes on the player map: the move stick read and
//! written back as the tables write it, and any axis or analog action replaced or raised. The
//! grounded rig writes the stick it steers here, and the vehicle handling its held controls.

use glam::Vec2;
use jc3gi::input::input_action_map::Action::{self, *};

use crate::controllers::intent::hold::{Desired, MapKind, Target, Want};

/// The move stick the tables wrote this tick (+X right, +Y forward), from the move half-axes: each
/// half-axis at the largest of its wants, as the wants on one action merge.
pub(crate) fn read_move(desired: &[Desired]) -> Vec2 {
    let half = |action: Action| {
        desired
            .iter()
            .filter(|d| d.target.map == MapKind::Player && d.target.action == action)
            .map(|d| match d.want {
                Want::Analog(v) | Want::Hold(v) => v,
                Want::Click => 1.0,
            })
            .fold(0.0f32, f32::max)
    };
    Vec2::new(
        half(MOVE_RIGHT) - half(MOVE_LEFT),
        half(MOVE_FORWARD) - half(MOVE_BACKWARD),
    )
}

/// Write `stick` (+X right, +Y forward) over the tables' move half-axes and their length.
#[inline]
pub fn write_move(desired: &mut Vec<Desired>, stick: Vec2) {
    replace_axis(desired, stick.y, MOVE_BACKWARD, MOVE_FORWARD);
    replace_axis(desired, stick.x, MOVE_LEFT, MOVE_RIGHT);
    replace_axis(desired, stick.length().min(1.0), MOVE_ALL, MOVE_ALL);
}

/// Replace every player-map want on `negative` and `positive` with the signed `value` split into
/// the two non-negative half-axes, as the tables write an axis.
pub fn replace_axis(desired: &mut Vec<Desired>, value: f32, negative: Action, positive: Action) {
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

/// Raise the player-map want on `action` to at least `value`, `0..=1`, keeping a larger one the
/// tables wrote.
pub fn raise_analog(desired: &mut Vec<Desired>, value: f32, action: Action) {
    let value = if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let target = Target {
        map: MapKind::Player,
        action,
    };
    let existing = desired
        .iter()
        .filter(|d| d.target == target)
        .map(|d| match d.want {
            Want::Analog(v) | Want::Hold(v) => v,
            Want::Click => 0.0,
        })
        .fold(0.0, f32::max);
    desired.retain(|d| d.target != target);
    desired.push(Desired {
        target,
        want: Want::Analog(value.max(existing)),
    });
}

/// Whether the tables wrote either half of the axis `negative`/`positive` this tick: their stick
/// is out of its deadzone.
#[inline]
pub fn axis_wanted(desired: &[Desired], negative: Action, positive: Action) -> bool {
    desired.iter().any(|d| {
        d.target.map == MapKind::Player
            && (d.target.action == negative || d.target.action == positive)
            && match d.want {
                Want::Analog(v) | Want::Hold(v) => v > 0.0,
                Want::Click => true,
            }
    })
}
