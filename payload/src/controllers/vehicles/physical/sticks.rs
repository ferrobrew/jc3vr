//! The sticks' and levers' motion: a stick's top and a lever's handle follow the holding hand
//! within their travel.

use glam::{Vec2, Vec3};

use crate::controllers::vehicles::{
    config::{LeverSpec, StickSpec},
    physical::{ControlKind, ControlState, Holding, Placement},
};

pub(super) fn step_stick(
    state: &mut ControlState,
    placement: &Placement,
    kind: ControlKind,
    holding: Holding,
) -> Option<Vec2> {
    let spec = placement.stick(kind)?;
    let Some((hold, point)) = holding.into_iter().flatten().next() else {
        state.stick = Vec2::ZERO;
        return None;
    };
    let top = placement.mirror(spec.top_m);
    state.stick = stick_deflection(&spec, top, point - hold.offset);
    let max = spec.max_deflection_m.max(1e-3);
    Some(Vec2::new(state.stick.x / max, -state.stick.y / max))
}

pub(super) fn step_lever(
    state: &mut ControlState,
    placement: &Placement,
    kind: ControlKind,
    holding: Holding,
) -> Option<f32> {
    let (index, spec) = placement.lever(kind)?;
    match holding.into_iter().flatten().next() {
        Some((hold, point)) => {
            state.levers[index] =
                lever_value(&spec, placement.mirror(spec.rest_m), point - hold.offset);
        }
        None if !spec.hold_on_release => {
            state.levers[index] = 0.0;
            return None;
        }
        None if state.levers[index] == 0.0 => return None,
        None => {}
    }
    Some(detent(state.levers[index], spec.detent))
}

/// A stick's deflection (+X right, +Y back) for its top pulled to `top_now`, limited to the full
/// deflection.
pub fn stick_deflection(spec: &StickSpec, top: Vec3, top_now: Vec3) -> Vec2 {
    let d = top_now - top;
    let flat = Vec2::new(d.x, d.z);
    if !flat.is_finite() {
        return Vec2::ZERO;
    }
    flat.clamp_length_max(spec.max_deflection_m.max(0.0))
}

pub(super) fn deflection3(d: Vec2) -> Vec3 {
    Vec3::new(d.x, 0.0, d.y)
}

/// A lever's position, `-1..=1`, for its handle pulled to `handle_now`.
pub fn lever_value(spec: &LeverSpec, rest: Vec3, handle_now: Vec3) -> f32 {
    let half = (spec.travel_m * 0.5).max(1e-3);
    let along = (handle_now - rest).dot(spec.axis.direction()) / half;
    if along.is_finite() {
        along.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// A lever's handle at position `value`.
pub fn lever_handle(spec: &LeverSpec, rest: Vec3, value: f32) -> Vec3 {
    rest + spec.axis.direction() * (spec.travel_m * 0.5 * value)
}

/// `value` with a detent around zero, rescaled so that it still reaches one.
pub fn detent(value: f32, detent: f32) -> f32 {
    let detent = detent.clamp(0.0, 0.95);
    let magnitude = value.abs();
    if magnitude <= detent {
        return 0.0;
    }
    value.signum() * ((magnitude - detent) / (1.0 - detent)).min(1.0)
}
