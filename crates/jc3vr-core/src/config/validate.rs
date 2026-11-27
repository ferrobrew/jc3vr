//! The error a settings check returns, which the tolerant load reads to reset the setting it names
//! ([`crate::config::load::reset_named`]), and the range checks a subsystem's own checks are built
//! from.

use std::fmt;

use glam::{Vec2, Vec3};

/// A setting that is well-typed but out of range or inconsistent with another. The message leads
/// with the setting's path, relative to the checked subtree, which the tolerant load resets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invalid(pub String);

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid setting: {}", self.0)
    }
}

impl std::error::Error for Invalid {}

/// Check that `value` is finite and in `[min, max]`.
pub fn range(name: &str, value: f32, min: f32, max: f32) -> Result<(), Invalid> {
    if !value.is_finite() || value < min || value > max {
        return Err(Invalid(format!(
            "{name} must be finite and in [{min}, {max}], got {value}"
        )));
    }
    Ok(())
}

/// Check that `value` is finite, above zero, and at most `max`.
pub fn positive(name: &str, value: f32, max: f32) -> Result<(), Invalid> {
    range(name, value, f32::EPSILON, max)
}

/// Check that `value` is finite and in `[0, max]`.
pub fn nonnegative(name: &str, value: f32, max: f32) -> Result<(), Invalid> {
    range(name, value, 0.0, max)
}

/// Check that each component of `value` is finite and in `[-max, max]`.
pub fn vec2(name: &str, value: Vec2, max: f32) -> Result<(), Invalid> {
    range(&format!("{name}.x"), value.x, -max, max)?;
    range(&format!("{name}.y"), value.y, -max, max)
}

/// Check that each component of `value` is finite and in `[-max, max]`.
pub fn vec3(name: &str, value: Vec3, max: f32) -> Result<(), Invalid> {
    range(&format!("{name}.x"), value.x, -max, max)?;
    range(&format!("{name}.y"), value.y, -max, max)?;
    range(&format!("{name}.z"), value.z, -max, max)
}
