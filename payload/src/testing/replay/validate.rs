//! Numeric admission for data consumed by native replay publication.

use std::path::Path;

use glam::{Mat4, Quat, Vec3};

use crate::{recorder::tracking::format::Recording, testing::replay::ReplayError};

pub(super) fn recording(
    path: &Path,
    recording: &Recording,
    world_scale: f32,
) -> Result<(), ReplayError> {
    for (index, frame) in recording.frames.iter().enumerate() {
        let check = |valid, field: String| require(path, Some(index), field, valid);
        let position = |value| position(value) && scaled_position(value, world_scale);
        if let Some(head) = frame.head {
            for (eye, sample) in head.eyes.iter().enumerate() {
                check(
                    position(sample.pose.position) && quaternion(sample.pose.orientation),
                    format!("head.eyes[{eye}].pose"),
                )?;
                let [left, right, up, down] = sample.fov;
                let in_range =
                    |angle: f32| angle.is_finite() && angle.abs() < std::f32::consts::FRAC_PI_2;
                check(
                    sample.fov.into_iter().all(in_range) && left < right && down < up,
                    format!("head.eyes[{eye}].fov"),
                )?;
            }
            let [left, right] = head.eyes.map(|eye| eye.pose);
            let center = 0.5 * (left.position + right.position);
            check(position(center), "head.center.position".to_owned())?;
            check(
                quaternion(left.orientation.slerp(right.orientation, 0.5)),
                "head.center.orientation".to_owned(),
            )?;
            for (eye, sample) in head.eyes.iter().enumerate() {
                check(
                    position(sample.pose.position - center),
                    format!("head.eyes[{eye}].offset"),
                )?;
            }
            check(
                (right.position - left.position).length().is_finite(),
                "head.inter_eye_distance".to_owned(),
            )?;
        }
        if let Some(controllers) = &frame.controllers {
            for (hand, sample) in controllers.hands.iter().enumerate() {
                if sample.active {
                    let inputs = sample.inputs;
                    for (name, value) in [
                        ("trigger", inputs.trigger),
                        ("squeeze", inputs.squeeze),
                        ("squeeze_force", inputs.squeeze_force),
                        ("trackpad_force", inputs.trackpad_force),
                    ] {
                        check(
                            value.is_finite() && (0.0..=1.0).contains(&value),
                            format!("controllers.hands[{hand}].inputs.{name}"),
                        )?;
                    }
                    for (name, value) in [
                        ("thumbstick", inputs.thumbstick),
                        ("trackpad", inputs.trackpad),
                    ] {
                        check(
                            value.is_finite() && value.abs().max_element() <= 1.0,
                            format!("controllers.hands[{hand}].inputs.{name}"),
                        )?;
                    }
                }
                for (name, tracked) in [("grip", sample.grip), ("aim", sample.aim)] {
                    check(
                        !tracked.position_valid || position(tracked.pose.position),
                        format!("controllers.hands[{hand}].{name}.position"),
                    )?;
                    check(
                        !tracked.orientation_valid || quaternion(tracked.pose.orientation),
                        format!("controllers.hands[{hand}].{name}.orientation"),
                    )?;
                    check(
                        tracked.linear_velocity.is_none_or(|v| v.is_finite()),
                        format!("controllers.hands[{hand}].{name}.linear_velocity"),
                    )?;
                    check(
                        tracked.angular_velocity.is_none_or(|v| v.is_finite()),
                        format!("controllers.hands[{hand}].{name}.angular_velocity"),
                    )?;
                }
                if let Some(joints) = &sample.joints {
                    for (joint, sample) in joints.iter().enumerate() {
                        check(
                            !sample.position_valid || position(sample.pose.position),
                            format!("controllers.hands[{hand}].joints[{joint}].position"),
                        )?;
                        check(
                            !sample.orientation_valid || quaternion(sample.pose.orientation),
                            format!("controllers.hands[{hand}].joints[{joint}].orientation"),
                        )?;
                        check(
                            !sample.position_valid
                                || (sample.radius.is_finite() && sample.radius >= 0.0),
                            format!("controllers.hands[{hand}].joints[{joint}].radius"),
                        )?;
                    }
                }
            }
        }
        check(
            frame.stage.head_height.is_none_or(f32::is_finite),
            "stage.head_height".to_owned(),
        )?;
        check(
            frame
                .stage
                .bounds
                .is_none_or(|v| v.is_finite() && v.min_element() > 0.0),
            "stage.bounds".to_owned(),
        )?;
        for (event, recenter) in frame.recenters.iter().enumerate() {
            check(
                quaternion(recenter.turn),
                format!("recenters[{event}].turn"),
            )?;
        }
    }
    Ok(())
}

pub(super) fn transform(
    path: &Path,
    field: &str,
    transform: &[f32; 16],
) -> Result<(), ReplayError> {
    let matrix = Mat4::from_cols_array(transform);
    let determinant = matrix.determinant();
    require(
        path,
        None,
        field.to_owned(),
        matrix.is_finite()
            && determinant.is_finite()
            && determinant.abs() > 1e-8
            && transform[3].abs() < 1e-5
            && transform[7].abs() < 1e-5
            && transform[11].abs() < 1e-5
            && (transform[15] - 1.0).abs() < 1e-5,
    )
}

// Cockpit coordinates describe a physical tracking space, not world travel. A 100 m per-axis
// allowance exceeds a room-scale session while keeping midpoint, distances, and scaled rotations
// far from f32 overflow. World start-marker translations do not use this bound.
pub(super) const MAX_COCKPIT_POSITION_M: f32 = 100.0;

fn position(value: Vec3) -> bool {
    value.is_finite() && value.abs().max_element() <= MAX_COCKPIT_POSITION_M
}

fn scaled_position(value: Vec3, world_scale: f32) -> bool {
    let scaled = value * world_scale;
    // The squared-length test reserves headroom for rotation, subtraction, and native distances.
    world_scale.is_finite()
        && world_scale > 0.0
        && scaled.is_finite()
        && scaled.length_squared().is_finite()
        && scaled.length_squared() <= f32::MAX / 256.0
}

fn quaternion(value: Quat) -> bool {
    let length = value.length_squared();
    value.is_finite() && length.is_finite() && (length - 1.0).abs() <= 0.01
}

fn require(
    path: &Path,
    frame: Option<usize>,
    field: String,
    valid: bool,
) -> Result<(), ReplayError> {
    if valid {
        Ok(())
    } else {
        Err(ReplayError::InvalidSample {
            path: path.to_owned(),
            frame,
            field,
        })
    }
}
