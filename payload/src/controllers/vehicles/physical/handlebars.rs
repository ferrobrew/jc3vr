//! The handlebars' motion: they pivot about their middle with the holding grips.
//!
//! The grips measure the bars' turn and roll ([`bar_angles`]), but a hand rarely takes the bars
//! exactly where they rest: it grabs a few centimetres in front of or below its end, and one hand
//! alone measures the bars as if the other end mirrored it through the middle. So the measure is
//! latched when the hands that hold the bars change, and the bars move from where they were by
//! the change the grips measure since ([`BarsLatch`]). A grab, a second hand joining, or one
//! letting go therefore never jumps the bars or biases them.

use std::f32::consts::{PI, TAU};

use glam::{Quat, Vec2, Vec3};

use crate::controllers::vehicles::{
    config::HandlebarsSpec,
    physical::{ControlState, Holding, Placement},
};

/// The handlebars as the holding hands took them.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct BarsLatch {
    /// Which hands held the bars when the latch was taken, indexed by
    /// [`Hand::index`](crate::controllers::frame::Hand::index).
    holders: [bool; 2],
    /// The bars' turn and roll then.
    bars: Vec2,
    /// What the holding grips measured then.
    measured: Vec2,
}

impl BarsLatch {
    /// The bars for the grips measuring `measured` now, within their limits.
    fn bars(&self, measured: Vec2, spec: &HandlebarsSpec) -> Vec2 {
        let change = measured - self.measured;
        clamp_bars(self.bars + Vec2::new(wrap_angle(change.x), change.y), spec)
    }
}

pub(super) fn step_bars(
    state: &mut ControlState,
    placement: &Placement,
    holding: Holding,
) -> Option<(f32, f32)> {
    let spec = &placement.config.handlebars;
    let grips = holding.map(|h| h.map(|(_, point)| point));
    if grips.iter().all(Option::is_none) {
        state.bars = Vec2::ZERO;
        state.bars_latch = None;
        return None;
    }
    // Grips the bars' angles cannot be measured from (a hand on the pivot) leave the bars where
    // they are.
    if let Some(measured) = bar_angles(placement.bars.centre, grips) {
        let holders = grips.map(|g| g.is_some());
        let latch = match state.bars_latch {
            Some(latch) if latch.holders == holders => latch,
            _ => BarsLatch {
                holders,
                bars: state.bars,
                measured,
            },
        };
        state.bars_latch = Some(latch);
        state.bars = latch.bars(measured, spec);
    }
    let steer = state.bars.x / spec.max_steer_deg.to_radians().max(1e-3);
    let lean = state.bars.y / spec.max_lean_deg.to_radians().max(1e-3);
    Some((steer, lean))
}

/// The bars for the holding `grips` as they are now, which may be fresher than the last tick's:
/// moved from the latch by the change the grips measure, or where the last tick left them when the
/// latch is for other hands or the grips measure nothing.
pub(super) fn bars_now(
    state: &ControlState,
    placement: &Placement,
    grips: [Option<Vec3>; 2],
) -> Vec2 {
    let holders = grips.map(|g| g.is_some());
    state
        .bars_latch
        .filter(|latch| latch.holders == holders)
        .zip(bar_angles(placement.bars.centre, grips))
        .map_or(state.bars, |(latch, measured)| {
            latch.bars(measured, &placement.config.handlebars)
        })
}

/// The handlebars' turn (positive with the right grip pulled back) and roll (positive with the
/// right grip lower), from the holding grips. With one grip the other end mirrors it through the
/// middle. `None` with no grip.
pub(super) fn bar_angles(centre: Vec3, grips: [Option<Vec3>; 2]) -> Option<Vec2> {
    let across = match grips {
        [Some(left), Some(right)] => right - left,
        [None, Some(right)] => (right - centre) * 2.0,
        [Some(left), None] => (centre - left) * 2.0,
        [None, None] => return None,
    };
    let flat = Vec2::new(across.x, across.z).length();
    if flat < 1e-4 || !across.is_finite() {
        return None;
    }
    Some(Vec2::new(across.z.atan2(across.x), (-across.y).atan2(flat)))
}

fn clamp_bars(angles: Vec2, spec: &HandlebarsSpec) -> Vec2 {
    let steer = spec.max_steer_deg.to_radians().abs();
    let lean = spec.max_lean_deg.to_radians().abs();
    Vec2::new(angles.x.clamp(-steer, steer), angles.y.clamp(-lean, lean))
}

/// The handlebars' grips, indexed by [`Hand::index`](crate::controllers::frame::Hand::index), for
/// the bars turned and rolled by `bars`.
pub fn bar_ends(centre: Vec3, half_width: f32, bars: Vec2) -> [Vec3; 2] {
    let across = Quat::from_rotation_y(-bars.x) * Quat::from_rotation_z(-bars.y) * Vec3::X;
    [centre - across * half_width, centre + across * half_width]
}

/// Wrap an angle difference into `-π..=π`.
fn wrap_angle(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}
