//! The handlebars' motion: they pivot about their middle with the holding grips.

use glam::{Quat, Vec2, Vec3};

use crate::controllers::vehicles::{
    config::HandlebarsSpec,
    physical::{ControlState, Holding, Placement},
};

pub(super) fn step_bars(
    state: &mut ControlState,
    placement: &Placement,
    holding: Holding,
) -> Option<(f32, f32)> {
    let spec = &placement.config.handlebars;
    let grips = holding.map(|h| h.map(|(_, point)| point));
    let Some(angles) = bar_angles(placement.bars.centre, grips) else {
        state.bars = Vec2::ZERO;
        return None;
    };
    state.bars = clamp_bars(angles, spec);
    let steer = state.bars.x / spec.max_steer_deg.to_radians().max(1e-3);
    let lean = state.bars.y / spec.max_lean_deg.to_radians().max(1e-3);
    Some((steer, lean))
}

/// The handlebars' turn (positive with the right grip pulled back) and roll (positive with the
/// right grip lower), from the holding grips. With one grip the other end mirrors it through the
/// middle. `None` with no grip.
pub fn bar_angles(centre: Vec3, grips: [Option<Vec3>; 2]) -> Option<Vec2> {
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

pub(super) fn clamp_bars(angles: Vec2, spec: &HandlebarsSpec) -> Vec2 {
    let steer = spec.max_steer_deg.to_radians().abs();
    let lean = spec.max_lean_deg.to_radians().abs();
    Vec2::new(angles.x.clamp(-steer, steer), angles.y.clamp(-lean, lean))
}

/// The handlebars' grips, indexed by [`Hand::index`], for the bars turned and rolled by `bars`.
pub fn bar_ends(centre: Vec3, half_width: f32, bars: Vec2) -> [Vec3; 2] {
    let across = Quat::from_rotation_y(-bars.x) * Quat::from_rotation_z(-bars.y) * Vec3::X;
    [centre - across * half_width, centre + across * half_width]
}
