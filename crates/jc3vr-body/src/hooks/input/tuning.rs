//! The input detours' tuning, read from the shipped `tuning/input.toml` ([`INPUT_HOOKS`]): the
//! carried air speed, the on-foot heading pin, the parachute's yaw alignment, and the swim heading
//! and dive.

use jc3vr_core::{
    config::engine::{Invalid, positive, range},
    tuning::Tuning,
};
use serde::Deserialize;

jc3vr_core::tuning_file! {
    /// The input detours' tuning file.
    pub static INPUT_HOOKS: InputHooksTuning = "input.toml";
}

/// The input detours' tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputHooksTuning {
    pub air: AirTuning,
    pub locomotion: LocomotionTuning,
    pub parachute: ParachuteTuning,
    pub swim: SwimTuning,
}

impl Tuning for InputHooksTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("air.carry_s", self.air.carry_s, 30.0)?;
        let l = &self.locomotion;
        positive("locomotion.turn_step_deg", l.turn_step_deg, 180.0)?;
        range(
            "locomotion.move_dir_idle_threshold",
            l.move_dir_idle_threshold,
            0.0,
            1.0,
        )?;
        range("locomotion.input_deadzone", l.input_deadzone, 0.0, 1.0)?;
        let p = &self.parachute;
        range("parachute.align_hold_s", p.align_hold_s, 0.0, 10.0)?;
        range("parachute.align_min_mps", p.align_min_mps, 0.0, 100.0)?;
        range(
            "parachute.align_reversed_rad",
            p.align_reversed_rad,
            0.0,
            std::f32::consts::PI,
        )?;
        let s = &self.swim;
        positive("swim.turn_rate_deg_s", s.turn_rate_deg_s, 3_600.0)?;
        range("swim.pitch_deadzone_deg", s.pitch_deadzone_deg, 0.0, 90.0)?;
        range("swim.pitch_limit_deg", s.pitch_limit_deg, 0.0, 89.0)?;
        positive("swim.pitch_rate_deg_s", s.pitch_rate_deg_s, 3_600.0)
    }
}

/// The air physics' carried speed.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AirTuning {
    /// How long a carried speed takes to ease back to the game's cap, seconds.
    pub carry_s: f32,
}

/// The on-foot heading pin and the movement input's thresholds.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocomotionTuning {
    /// The tracking turn rate: the maximum yaw step, in degrees per orientation update (one per
    /// frame), passed to the orientation executor while the body is pinned to the camera or the
    /// headpose's heading. Positive, since the executor divides by it.
    pub turn_step_deg: f32,
    /// Move-direction magnitudes below this are treated as no input (idle), where the pin always
    /// applies so that turning the camera turns the body.
    pub move_dir_idle_threshold: f32,
    /// Movement-input magnitudes below this are treated as no input.
    pub input_deadzone: f32,
}

/// When the canopy's yaw alignment stays off.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParachuteTuning {
    /// How long after the canopy opens its yaw alignment stays off, seconds.
    pub align_hold_s: f32,
    /// The horizontal speed below which the canopy's yaw alignment stays off, metres per second.
    pub align_min_mps: f32,
    /// How far from the facing the velocity may point before the canopy's yaw alignment stays off,
    /// radians.
    pub align_reversed_rad: f32,
}

/// The swim heading and the underwater dive.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwimTuning {
    /// The rate the body is turned toward the swim heading, degrees per second. It stays above the
    /// look input's own rate, or the body trails the target and the turn continues after the stick
    /// is released; the game's own swim turn rates are far below it. Capped per frame by the angle
    /// `DoRotate` eases out over, so it cannot overshoot.
    pub turn_rate_deg_s: f32,
    /// The head pitch, in degrees, within which the underwater dive control does nothing, so a
    /// resting head does not slowly dive or surface the swimmer.
    pub pitch_deadzone_deg: f32,
    /// The furthest the underwater dive control will pitch the body, in degrees. Kept short of
    /// vertical, where a heading is undefined.
    pub pitch_limit_deg: f32,
    /// How fast, in degrees per second, the underwater dive pitch follows the head. Slower than the
    /// yaw rate on purpose: a dive that snaps as fast as a turn reads as the world tipping over.
    pub pitch_rate_deg_s: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(INPUT_HOOKS.get().validate(), Ok(()));
    }
}
