//! The vehicles' tuning, read from the shipped `tuning/vehicles.toml` ([`VEHICLES`]): the physical
//! controls' placement and motion, with the wheel settings they take from the configuration
//! ([`crate::vehicles::config::PhysicalControlsConfig`]), the wheel and handlebars
//! fit, the steering, and the cues ([`physical`]); the wingsuit's arm steering, gestures, evade, and
//! take-off, the parachute's handles and rig, and the ripcord ([`airborne`]); the mounted guns, the
//! gunner's seat, the driving view, and the cockpit calibration ([`seats`]); and the helicopter's
//! skid.
//!
//! The physical controls are placed in the seated character's body frame (+X right, +Y up, +Z back,
//! metres) with the character's eyes at the origin: the cockpit frame, whose origin the head pose
//! pins to the character's animated head, which rides the seat. Offsets are given for a
//! right-handed layout, and the sticks' and levers' X is mirrored for a left-handed one, so the
//! stick that sits by the dominant knee moves with the dominant hand. The wheel and the handlebars
//! are centred on the vehicle and never mirrored; where the vehicle's own wheel or handlebars are
//! found, they are placed from it instead ([`crate::vehicles::wheel`]), and the specs
//! here are the fallback.

mod airborne;
mod physical;
mod seats;

use std::time::Duration;

use jc3vr_core::{
    config::{Invalid, nonnegative, positive, range},
    tuning::Tuning,
};
use serde::Deserialize;

pub use crate::vehicles::tuning::{
    airborne::{
        ArmSteeringTuning, ParachuteHandlesTuning, ParachuteRigTuning, RipcordTuning,
        TakeOffTuning, WingsuitEvadeTuning, WingsuitGesturesTuning,
    },
    physical::{
        CockpitButtonsTuning, ControlCuesTuning, ControlMotionTuning, FlightControlsTuning,
        HandlebarsSpec, LeverAxis, LeverSpec, PhysicalControls, PhysicalControlsTuning,
        PhysicalTuning, SteeringTuning, SteeringWheelTuning, StickSpec, WheelFitTuning, WheelSpec,
        YokeSpec,
    },
    seats::{
        CalibrationTuning, DrivingViewTuning, GunnerSeatTuning, MountedRecoilTuning, MountedTuning,
    },
};

jc3vr_core::tuning_file! {
    /// The vehicles' tuning file.
    pub static VEHICLES: VehiclesTuning = "vehicles.toml";
}

/// The vehicles' tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VehiclesTuning {
    /// The physical controls, short of the wheel settings the configuration holds.
    pub physical: PhysicalTuning,
    pub steering: SteeringTuning,
    pub controls: ControlMotionTuning,
    pub cues: ControlCuesTuning,
    pub(crate) wheel_fit: WheelFitTuning,
    pub arm_steering: ArmSteeringTuning,
    pub wingsuit_gestures: WingsuitGesturesTuning,
    pub wingsuit_evade: WingsuitEvadeTuning,
    pub take_off: TakeOffTuning,
    pub parachute_handles: ParachuteHandlesTuning,
    pub parachute_rig: ParachuteRigTuning,
    pub ripcord: RipcordTuning,
    pub rail: RailTuning,
    pub driving_view: DrivingViewTuning,
    pub calibration: CalibrationTuning,
    pub mounted: MountedTuning,
    pub(crate) gunner_seat: GunnerSeatTuning,
}

impl Tuning for VehiclesTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.physical.validate()?;
        self.steering.validate()?;
        self.controls.validate()?;
        self.cues.validate()?;
        self.wheel_fit.validate()?;
        self.arm_steering.validate()?;
        self.wingsuit_gestures.validate()?;
        self.wingsuit_evade.validate()?;
        self.take_off.validate()?;
        self.parachute_handles.validate()?;
        self.parachute_rig.validate()?;
        self.ripcord.validate()?;
        self.rail.validate()?;
        self.driving_view.validate()?;
        self.calibration.validate()?;
        self.mounted.validate()?;
        self.gunner_seat.validate()
    }
}

/// The helicopter skid (see [`crate::vehicles::rail`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RailTuning {
    /// How far the skid's line reaches either way from where Rico lands on it, metres.
    pub half_length_m: f32,
    /// How far off the line, level, the feet go before Rico has stepped off, metres.
    pub(crate) step_off_m: f32,
    /// The sticky pull's time constant: the offset off the line closes at this rate, seconds.
    /// Longer is less sticky.
    pub(crate) pull_s: f32,
    /// The time constant that holds the feet at the skid's height, seconds.
    pub(crate) height_s: f32,
    /// The fastest walk the rail passes on, metres per second.
    pub(crate) max_walk_mps: f32,
    /// How far below the skid's line the feet go before the rail lets them go, metres.
    pub max_drop_m: f32,
    /// How long after its arrival a noted airframe stands for the door side's, milliseconds.
    pub(crate) airframe_fresh_ms: u64,
    /// How long after the rail takes Rico the input tick's mode is trusted, milliseconds: the mode
    /// reads the fall the graph's exit starts until the first carried step lands him.
    pub(crate) mode_grace_ms: u64,
}

impl RailTuning {
    /// [`Self::airframe_fresh_ms`].
    pub(crate) fn airframe_fresh(&self) -> Duration {
        Duration::from_millis(self.airframe_fresh_ms)
    }

    /// [`Self::mode_grace_ms`].
    pub(crate) fn mode_grace(&self) -> Duration {
        Duration::from_millis(self.mode_grace_ms)
    }

    fn validate(&self) -> Result<(), Invalid> {
        positive("rail.half_length_m", self.half_length_m, 10.0)?;
        positive("rail.step_off_m", self.step_off_m, 5.0)?;
        nonnegative("rail.pull_s", self.pull_s, 10.0)?;
        nonnegative("rail.height_s", self.height_s, 10.0)?;
        positive("rail.max_walk_mps", self.max_walk_mps, 50.0)?;
        positive("rail.max_drop_m", self.max_drop_m, 10.0)?;
        millis("rail.airframe_fresh_ms", self.airframe_fresh_ms, 600_000)?;
        millis("rail.mode_grace_ms", self.mode_grace_ms, 60_000)
    }
}

/// Check that a duration in milliseconds is at most `max`.
fn millis(name: &str, value: u64, max: u64) -> Result<(), Invalid> {
    range(name, value as f32, 0.0, max as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        let tuning = VEHICLES.get();
        assert_eq!(tuning.validate(), Ok(()));
    }
}
