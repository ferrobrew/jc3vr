//! Joint position/rate viability and explicit recovery of inconsistent stored rates.

use glam::{Quat, Vec3};

use crate::controllers::body::rig::estimate::{
    Anatomy, EstimateError,
    fit::{Context, MAX_ACCELERATION, MAX_SPEED, PARAMETERS, Parameters, difference},
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum TemporalStatus {
    Normal,
    /// A nonviable incoming rate is discarded explicitly; anatomy and future viability still hold.
    RateRecovery,
    /// A carried feasible pose is held after the bounded dynamic search fails.
    /// The acceleration discontinuity and frozen swing clock reduce authority explicitly.
    ConstraintRecovery,
}

pub(crate) fn prepare(context: &mut Context<'_>) -> TemporalStatus {
    if !context.continuity {
        return TemporalStatus::Normal;
    }
    let local_rate = Quat::from_rotation_y(-context.root_yaw)
        * Vec3::new(context.rates[0], 0.0, context.rates[1]);
    let invalid = (0..PARAMETERS).any(|axis| {
        let rate = match axis {
            0 => local_rate.x,
            1 => local_rate.z,
            _ => context.rates[axis],
        };
        !viable(context.previous.0[axis], rate, axis, context.anatomy)
            || interval(axis, context).is_err()
    });
    if invalid {
        context.rates = [0.0; PARAMETERS];
        TemporalStatus::RateRecovery
    } else {
        TemporalStatus::Normal
    }
}

pub(crate) fn constrain(
    parameters: &mut Parameters,
    context: &Context<'_>,
) -> Result<(), EstimateError> {
    let rotation = Quat::from_rotation_y(context.root_yaw);
    let mut planar = rotation.inverse()
        * Vec3::new(
            parameters.0[0] - context.previous.0[0],
            0.0,
            parameters.0[1] - context.previous.0[1],
        );
    let (minimum_x, maximum_x) = interval(0, context)?;
    let (minimum_z, maximum_z) = interval(1, context)?;
    planar.x = planar.x.clamp(minimum_x, maximum_x);
    planar.z = planar.z.clamp(minimum_z, maximum_z);
    let world = rotation * planar;
    parameters.0[0] = context.previous.0[0] + world.x;
    parameters.0[1] = context.previous.0[1] + world.z;
    for axis in 2..PARAMETERS {
        let (minimum, maximum) = interval(axis, context)?;
        let value = context.previous.0[axis]
            + difference(parameters.0[axis], context.previous.0[axis], axis);
        parameters.0[axis] = value.clamp(minimum, maximum);
    }
    parameters.0[2] = crate::controllers::body::rig::torso::wrap(parameters.0[2]);
    Ok(())
}

pub(crate) fn accepts(parameters: Parameters, context: &Context<'_>) -> bool {
    let planar = Quat::from_rotation_y(-context.root_yaw)
        * Vec3::new(
            parameters.0[0] - context.previous.0[0],
            0.0,
            parameters.0[1] - context.previous.0[1],
        );
    (0..PARAMETERS).all(|axis| {
        let previous = if axis < 2 {
            0.0
        } else {
            context.previous.0[axis]
        };
        let value = match axis {
            0 => planar.x,
            1 => planar.z,
            _ => previous + difference(parameters.0[axis], previous, axis),
        };
        let Ok((minimum, maximum)) = interval(axis, context) else {
            return false;
        };
        let rate = if context.continuity {
            (value - previous) / context.dt
        } else {
            0.0
        };
        value >= minimum - 1.0e-6
            && value <= maximum + 1.0e-6
            && viable(value, rate, axis, context.anatomy)
    })
}

/// Planar axes return root-frame displacement intervals; other axes return coordinate intervals.
pub(crate) fn interval(axis: usize, context: &Context<'_>) -> Result<(f32, f32), EstimateError> {
    if axis < 2 {
        if !context.continuity {
            return Ok((f32::NEG_INFINITY, f32::INFINITY));
        }
        let local = Quat::from_rotation_y(-context.root_yaw)
            * Vec3::new(context.rates[0], 0.0, context.rates[1]);
        let rate = if axis == 0 { local.x } else { local.z };
        let change = MAX_ACCELERATION[axis] * context.dt;
        let minimum = (rate - change).max(-MAX_SPEED[axis]) * context.dt;
        let maximum = (rate + change).min(MAX_SPEED[axis]) * context.dt;
        return if minimum.is_finite() && maximum.is_finite() && minimum <= maximum {
            Ok((minimum, maximum))
        } else {
            Err(EstimateError::EmptyTemporalIntersection { axis })
        };
    }
    let (mut minimum, mut maximum) = bounds(axis, context.anatomy);
    if context.continuity {
        let previous = context.previous.0[axis];
        let acceleration = MAX_ACCELERATION[axis];
        let dt = context.dt;
        let rate = context.rates[axis];
        let change = acceleration * dt;
        minimum = minimum.max(previous + (rate - change).max(-MAX_SPEED[axis]) * dt);
        maximum = maximum.min(previous + (rate + change).min(MAX_SPEED[axis]) * dt);
        let (lower, upper) = bounds(axis, context.anatomy);
        // At the endpoint, v^2/(2a) must fit before the anatomical boundary.
        if lower.is_finite() {
            minimum =
                minimum.max(previous - viable_displacement(previous - lower, acceleration, dt));
        }
        if upper.is_finite() {
            maximum =
                maximum.min(previous + viable_displacement(upper - previous, acceleration, dt));
        }
    }
    if !minimum.is_nan() && !maximum.is_nan() && minimum <= maximum {
        Ok((minimum, maximum))
    } else {
        Err(EstimateError::EmptyTemporalIntersection { axis })
    }
}

fn viable_displacement(room: f32, acceleration: f32, dt: f32) -> f32 {
    let room = room.max(0.0);
    2.0 * room / ((1.0 + 2.0 * room / (acceleration * dt * dt)).sqrt() + 1.0)
}

pub(crate) fn viable(position: f32, rate: f32, axis: usize, anatomy: &Anatomy) -> bool {
    let (lower, upper) = bounds(axis, anatomy);
    if !position.is_finite() || !rate.is_finite() || rate.abs() > MAX_SPEED[axis] + 1.0e-4 {
        return false;
    }
    let stopping = rate * rate / (2.0 * MAX_ACCELERATION[axis]);
    position >= lower - 1.0e-6
        && position <= upper + 1.0e-6
        && (rate <= 0.0 || !upper.is_finite() || stopping <= upper - position + 1.0e-6)
        && (rate >= 0.0 || !lower.is_finite() || stopping <= position - lower + 1.0e-6)
}

pub(crate) fn bounds(axis: usize, anatomy: &Anatomy) -> (f32, f32) {
    match axis {
        3 => (-0.45, 0.45),
        4 => (-0.40, 0.65),
        5 => (
            anatomy.character.leg(0) * 0.12,
            anatomy.character.leg(0) * 0.998,
        ),
        6 => (-0.15, 1.75),
        7 => (-0.35, 0.75),
        8 => (-0.70, 0.70),
        _ => (f32::NEG_INFINITY, f32::INFINITY),
    }
}
