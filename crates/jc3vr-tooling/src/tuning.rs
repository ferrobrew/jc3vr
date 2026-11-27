//! The tooling's tuning, read from the shipped `tuning/tooling.toml` ([`TOOLING`]): the directing
//! tools' teleport reach and time-scale ladder, the tracking recording's body and body-state
//! samplers, and the visual stage's arrival dwell.

use std::fmt::Display;

use jc3vr_core::{
    config::{Invalid, positive, range},
    tuning::Tuning,
};
use serde::Deserialize;

jc3vr_core::tuning_file! {
    /// The tooling's tuning file.
    pub static TOOLING: ToolingTuning = "tooling.toml";
}

/// The tooling's tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolingTuning {
    pub director: DirectorTuning,
    pub bodies: BodiesTuning,
    pub body_state: BodyStateTuning,
    pub stage: StageTuning,
}

impl Tuning for ToolingTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.director.validate()?;
        self.bodies.validate()?;
        positive(
            "body_state.repeat_after_s",
            self.body_state.repeat_after_s,
            600.0,
        )?;
        count("stage.arrived_frames", self.stage.arrived_frames, 1, 600)
    }
}

/// The directing tools ([`crate::director`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectorTuning {
    /// The farthest an automatic teleport jumps instantly before it takes the streamed path,
    /// metres. It stays short of where the engine warns about a teleport without the load state
    /// ([`ENGINE_INSTANT_TELEPORT_WARNING_M`] from the camera), leaving a margin for the character
    /// standing away from the camera.
    pub(crate) instant_teleport_range_m: f32,
    /// The factor one press of the time-scale keys multiplies (up) or divides (down) the time
    /// scale by, so repeated presses walk a geometric ladder.
    pub(crate) time_scale_step: f32,
}

impl DirectorTuning {
    fn validate(&self) -> Result<(), Invalid> {
        range(
            "director.instant_teleport_range_m",
            self.instant_teleport_range_m,
            f32::EPSILON,
            ENGINE_INSTANT_TELEPORT_WARNING_M,
        )?;
        range("director.time_scale_step", self.time_scale_step, 1.01, 10.0)
    }
}

/// The physics bodies around Rico as recording events ([`crate::diagnostics`]'s bodies sampler).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodiesTuning {
    /// Within this of Rico's root, or [`hand_radius_m`](Self::hand_radius_m) of a hand, a dynamic
    /// body is near, metres.
    pub(crate) rico_radius_m: f32,
    pub(crate) hand_radius_m: f32,
    /// The most bodies kept, held ones first and then the nearest.
    pub max_bodies: usize,
    /// Each step walks this share of the body slots (one over it), and at least
    /// [`min_slice`](Self::min_slice) of them: a body is found within this many steps of coming
    /// near.
    pub(crate) walk_steps: u32,
    pub(crate) min_slice: u32,
    /// A kept body is introduced again after this long, so a stretch of a recording names its
    /// bodies, seconds.
    pub(crate) intro_repeat_s: f32,
    /// Below this squared speed, in either velocity, a body is at rest.
    pub(crate) rest_speed_sq: f32,
}

impl BodiesTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("bodies.rico_radius_m", self.rico_radius_m, 100.0)?;
        positive("bodies.hand_radius_m", self.hand_radius_m, 100.0)?;
        count("bodies.max_bodies", self.max_bodies, 1, MAX_EVENT_BODIES)?;
        count("bodies.walk_steps", self.walk_steps, 1, 1024)?;
        count("bodies.min_slice", self.min_slice, 1, 1 << 20)?;
        positive("bodies.intro_repeat_s", self.intro_repeat_s, 600.0)?;
        range("bodies.rest_speed_sq", self.rest_speed_sq, 0.0, 1.0)
    }
}

/// The body's animation state as recording events ([`crate::diagnostics`]'s body-state sampler).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyStateTuning {
    /// How long an unchanged state goes before a note repeats it, seconds.
    pub(crate) repeat_after_s: f32,
}

/// The visual stage (`crate::testing::stage`).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageTuning {
    /// How many consecutive frames the player must stand in gameplay with no teleport in flight
    /// before the spawns are submitted: long enough for the landing to settle.
    pub(crate) arrived_frames: u32,
}

/// The distance from the camera past which the engine warns about a teleport without the load
/// state, metres.
const ENGINE_INSTANT_TELEPORT_WARNING_M: f32 = 500.0;

/// The most bodies one bodies event encodes without truncation (the tracking format's list bound).
const MAX_EVENT_BODIES: usize = 256;

/// Check that the count `value` is in `[min, max]`.
fn count<T: PartialOrd + Display>(name: &str, value: T, min: T, max: T) -> Result<(), Invalid> {
    if value < min || value > max {
        return Err(Invalid(format!(
            "{name} must be in [{min}, {max}], got {value}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(TOOLING.get().validate(), Ok(()));
    }
}
