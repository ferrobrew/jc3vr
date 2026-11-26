//! The vehicles' tuning, read from the shipped `tuning/vehicles.toml` ([`VEHICLES`]): the physical
//! controls' placement and motion, with the wheel settings they take from the configuration
//! ([`PhysicalControlsConfig`]), the wingsuit's arm steering, gestures, and evade, the parachute's
//! handles and ripcord, and the helicopter's skid.
//!
//! The physical controls are placed in the seated character's body frame (+X right, +Y up, +Z back,
//! metres) with the character's eyes at the origin: the cockpit frame, whose origin the head pose
//! pins to the character's animated head, which rides the seat. Offsets are given for a
//! right-handed layout, and the sticks' and levers' X is mirrored for a left-handed one, so the
//! stick that sits by the dominant knee moves with the dominant hand. The wheel and the handlebars
//! are centred on the vehicle and never mirrored; where the vehicle's own wheel or handlebars are
//! found, they are placed from it instead ([`crate::controllers::vehicles::wheel`]), and the specs
//! here are the fallback.

use glam::Vec3;
use jc3vr_core::{
    config::engine::{Invalid, nonnegative, positive, range, vec3},
    tuning::Tuning,
};
use serde::Deserialize;

use crate::controllers::vehicles::config::{PhysicalControlsConfig, WheelConfig, WheelTracking};

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
    pub arm_steering: ArmSteeringTuning,
    pub wingsuit_gestures: WingsuitGesturesTuning,
    pub wingsuit_evade: WingsuitEvadeTuning,
    pub parachute_handles: ParachuteHandlesTuning,
    pub ripcord: RipcordTuning,
    pub rail: RailTuning,
}

impl Tuning for VehiclesTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.physical.validate()?;
        self.arm_steering.validate()?;
        self.wingsuit_gestures.validate()?;
        self.wingsuit_evade.validate()?;
        self.parachute_handles.validate()?;
        positive("ripcord.pull_m", self.ripcord.pull_m, 2.0)?;
        self.rail.validate()
    }
}

/// The physical controls as the tuning file holds them.
pub type PhysicalTuning = PhysicalControls<SteeringWheelTuning>;

/// The physical controls with the wheel settings the configuration sets, as the controls use them.
pub type PhysicalControlsTuning = PhysicalControls<WheelSpec>;

/// Where the physical controls of the driver's seat sit and how they move (see
/// [`crate::controllers::vehicles::physical`]), over the wheel's description `W`.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalControls<W> {
    /// The car's and the boat's wheel.
    pub wheel: W,
    /// The handlebars of the motorcycles, the ATV, the jetski, and the duck.
    pub handlebars: HandlebarsSpec,
    /// The helicopter's cyclic, by the dominant knee.
    pub cyclic: StickSpec,
    /// The helicopter's collective, by the off-hand hip.
    pub collective: LeverSpec,
    /// The plane's flight stick, by the dominant knee.
    pub flight_stick: StickSpec,
    /// The plane's throttle, by the off-hand hip.
    pub throttle: LeverSpec,
    /// The control wheel of a plane whose cockpit models one in place of a stick.
    pub yoke: YokeSpec,
    /// What the plane's flight stick and throttle add to a plain stick and lever: the stick's
    /// twist for the plane's rudder and the helicopter's yaw, and the throttle's afterburner
    /// gate.
    pub flight: FlightControlsTuning,
    /// The horn and the nitrous worked by hand.
    pub buttons: CockpitButtonsTuning,
    /// How far from a control a hand's grip may be for a grip press to grab it, metres.
    pub grab_radius_m: f32,
}

impl PhysicalControlsTuning {
    /// The shipped tuning with the wheel settings `config` sets.
    pub fn of(config: &PhysicalControlsConfig) -> Self {
        let tuning = &VEHICLES.get().physical;
        Self {
            wheel: WheelSpec::of(&tuning.wheel, &config.wheel),
            handlebars: tuning.handlebars,
            cyclic: tuning.cyclic,
            collective: tuning.collective,
            flight_stick: tuning.flight_stick,
            throttle: tuning.throttle,
            yoke: tuning.yoke,
            flight: tuning.flight,
            buttons: tuning.buttons,
            grab_radius_m: tuning.grab_radius_m,
        }
    }

    /// The shipped tuning with the default wheel settings.
    #[cfg(test)]
    pub(crate) fn shipped() -> Self {
        Self::of(&PhysicalControlsConfig::new())
    }
}

/// The steering wheel's tuning: where the fallback wheel sits, and how every wheel steers short of
/// the settings ([`WheelConfig`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SteeringWheelTuning {
    /// [`WheelSpec::centre_m`].
    pub centre_m: Vec3,
    pub radius_m: f32,
    /// [`WheelSpec::tilt_deg`].
    pub tilt_deg: f32,
    /// [`WheelSpec::response`].
    pub response: f32,
    /// [`WheelSpec::deadzone_deg`].
    pub deadzone_deg: f32,
    /// [`WheelSpec::return_deg_per_s`].
    pub return_deg_per_s: f32,
}

/// A steering wheel: a rim in a plane facing the driver, tilted back with its column. The hub,
/// radius, and tilt place the wheel where the vehicle's own is not found; the lock, the response,
/// the tracking, and the return apply to every wheel (see
/// [`crate::controllers::vehicles::steering`]).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct WheelSpec {
    /// The wheel's hub.
    pub centre_m: Vec3,
    pub radius_m: f32,
    /// How far the wheel's face tilts up from vertical toward the driver, degrees.
    pub tilt_deg: f32,
    /// [`WheelConfig::lock_deg`].
    pub lock_deg: f32,
    /// The exponent the share of the lock is raised to: one is linear, above one steers more gently
    /// around centre and as fully at the lock.
    pub response: f32,
    /// Wheel angles within this of centre steer nothing, degrees.
    pub deadzone_deg: f32,
    /// [`WheelConfig::turned_by`].
    pub turned_by: WheelTracking,
    /// How fast the released wheel returns to centre, degrees per second.
    pub return_deg_per_s: f32,
}

impl WheelSpec {
    const fn of(tuning: &SteeringWheelTuning, config: &WheelConfig) -> Self {
        Self {
            centre_m: tuning.centre_m,
            radius_m: tuning.radius_m,
            tilt_deg: tuning.tilt_deg,
            lock_deg: config.lock_deg,
            response: tuning.response,
            deadzone_deg: tuning.deadzone_deg,
            turned_by: config.turned_by,
            return_deg_per_s: tuning.return_deg_per_s,
        }
    }
}

/// Handlebars, turning about their centre (see
/// [`crate::controllers::vehicles::physical`]). The middle and half width place them where the
/// vehicle's own are not found; the lock, the response, and the reach apply to all.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandlebarsSpec {
    /// The middle of the bars.
    pub centre_m: Vec3,
    /// From the middle to each grip, metres.
    pub half_width_m: f32,
    /// The bars' turn that steers fully, degrees. The bars stop there.
    pub max_steer_deg: f32,
    /// Bar turns within this of centre steer nothing, degrees, so that a resting hand's tremor and
    /// the small turns a wrist makes do not weave the bike.
    pub deadzone_deg: f32,
    /// The exponent the share of the lock past the deadzone is raised to: one is linear, above one
    /// steers more gently around centre and as fully at the lock.
    pub response: f32,
    /// How far from the hand's half of the bars its grip may be for a grip press to take them,
    /// metres: wider than the other controls', since the bars are where a reaching hand expects
    /// them less exactly than a wheel's rim.
    pub grab_radius_m: f32,
}

/// A joystick whose top moves in the horizontal plane. Let go, a spring returns it to centre.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StickSpec {
    /// The stick's top at rest, given on the dominant side.
    pub top_m: Vec3,
    /// How far the top moves for a full deflection, metres.
    pub max_deflection_m: f32,
    /// The share of the full deflection around centre that outputs nothing, `0..1`, so that a
    /// resting hand's tremor does not fly the vehicle.
    pub deadzone: f32,
    /// The released stick's centring spring: the time constant it returns to centre with, seconds.
    /// Zero snaps it back.
    pub return_s: f32,
}

/// A plane's control wheel on its column (see [`crate::controllers::vehicles::physical::yoke`]):
/// turned for the roll and pushed or pulled along the column for the pitch. Its place comes from
/// the cockpit's model.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct YokeSpec {
    /// The turn either way that rolls fully, degrees. The yoke stops there.
    pub max_roll_deg: f32,
    /// How far the yoke moves along its column either way for a full pitch, metres.
    pub travel_m: f32,
    /// The share of each axis's range around centre that outputs nothing, `0..1`.
    pub deadzone: f32,
    /// The released yoke's centring spring: the time constant it returns to centre with, seconds.
    pub return_s: f32,
}

/// The horn and the nitrous worked by hand (see
/// [`crate::controllers::vehicles::physical::buttons`]): the horn pressed with a free hand on the
/// wheel's hub, a nitrous button on the dash beside the wheel (on a vehicle with nitrous), and the
/// collective's grip twisted either way for the nitrous. The plane's afterburner is its throttle's
/// gate ([`FlightControlsTuning::gate_at`]). The buttons stay alongside the controller's.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CockpitButtonsTuning {
    /// The hub's share of the wheel's radius that presses the horn.
    pub hub_share: f32,
    /// The nitrous button from the wheel's rim on its right: along the wheel's right, its up, and
    /// its axis toward the driver, metres.
    pub nitrous_from_rim_m: Vec3,
    pub nitrous_radius_m: f32,
    /// How far past a button's face a hand pushes to press it, metres.
    pub press_depth_m: f32,
    /// How far the collective's grip is twisted from where the hand took it to burn the nitrous,
    /// degrees: well past the roll a wrist makes while it raises and lowers the lever.
    pub twist_deg: f32,
}

/// The plane's flight stick and throttle beyond a plain stick and lever (see
/// [`crate::controllers::vehicles::physical`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlightControlsTuning {
    /// Twists within this of where the hand took the stick work nothing, degrees.
    pub twist_deadzone_deg: f32,
    /// The twist that gives a full rudder, degrees.
    pub twist_full_deg: f32,
    /// Where the throttle's gate at the top of its travel starts, as a share of its forward travel,
    /// `0..1`. Below the gate the throttle stops short of the thrust that lights the afterburner,
    /// and pushed through it, the throttle is full and the afterburner burns where the game allows
    /// it.
    pub gate_at: f32,
    /// The share of the throttle's whole travel, at its back, that is the reverse section behind
    /// the zero gate, `0..0.9`. The rest, in front of the gate, is forward thrust from nothing to
    /// full.
    pub reverse_share: f32,
    /// How far past zero the hand pulls (or pushes) the throttle to take it through the zero gate
    /// into the other section, as a share of the whole travel. Short of it, the throttle stays at
    /// zero.
    pub zero_pull: f32,
    /// The share of either section next to zero that gives nothing, and in which a released
    /// throttle snaps to zero.
    pub zero_snap: f32,
    /// The share of the forward section between the ticks the held throttle gives its hand.
    pub thrust_step: f32,
}

/// A lever that slides along one axis.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeverSpec {
    /// The handle's neutral position, given on the off-hand side.
    pub rest_m: Vec3,
    pub axis: LeverAxis,
    /// The handle's whole travel, from full negative to full positive, metres.
    pub travel_m: f32,
    /// The share of the travel around neutral that outputs nothing, `0..1`.
    pub detent: f32,
    /// Keep the lever where it was let go, as a throttle does, rather than returning it to neutral.
    pub hold_on_release: bool,
}

/// The direction a lever's handle travels in for a positive output.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize)]
pub enum LeverAxis {
    /// Up.
    Vertical,
    /// Forward.
    ForeAft,
}

impl LeverAxis {
    pub const fn direction(self) -> Vec3 {
        match self {
            LeverAxis::Vertical => Vec3::Y,
            LeverAxis::ForeAft => Vec3::NEG_Z,
        }
    }
}

/// The arm steering's thresholds (see [`crate::controllers::vehicles::arm_steering`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArmSteeringTuning {
    /// Height differences and offsets below this steer nothing, metres. The bank, and the height
    /// and sweep pitches, share it.
    pub deadzone_m: f32,
    /// The height difference between the hands that banks fully, metres.
    pub full_bank_m: f32,
}

/// The bilateral wingsuit gestures: fresh-pose deployment, bilateral withdrawal, and palms-forward
/// braking (see [`crate::controllers::vehicles::wingsuit_gestures`]). Distances are shares of the
/// calibrated arm span.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WingsuitGesturesTuning {
    /// Each wrist's minimum outward distance from its own shoulder for a flight pose.
    pub flight_lateral_span: f32,
    /// Maximum arm angle away from the outward horizontal direction for a fresh flight pose.
    pub flight_angle_deg: f32,
    /// Each wrist must be this close laterally before withdrawal qualifies.
    pub withdraw_lateral_span: f32,
    /// Each wrist qualifies as withdrawn when lowered this far below its shoulder.
    pub withdraw_lowered_span: f32,
    /// A tucked wrist also qualifies within this shoulder distance, without lowering the arm.
    pub withdraw_reach_span: f32,
    /// How far inside the flight pose's spread a held brake's arms may come before it releases.
    pub pose_hysteresis_span: f32,
    /// Each wrist's largest roll from palm-flat, either way, for a flight pose. Both palms back
    /// within it release a held brake.
    pub flight_roll_deg: f32,
    /// Each wrist's smallest roll toward palm-forward to enter braking.
    pub brake_roll_deg: f32,
    /// Each wrist's largest roll toward palm-forward to enter braking, short of palm-up. A held
    /// brake ignores it.
    pub brake_roll_max_deg: f32,
    pub neutral_dwell_s: f32,
    pub flight_dwell_s: f32,
    pub close_dwell_s: f32,
    pub brake_dwell_s: f32,
    /// A larger pose-time gap invalidates all pending gestures.
    pub max_sample_gap_s: f32,
}

/// The boost wingsuit's barrel-roll evade's throw (see
/// [`crate::controllers::vehicles::wingsuit_evade`]). The bank is the angle of the line between
/// the hands from level, positive with the left hand above the right.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WingsuitEvadeTuning {
    /// The bank, either way, a throw must reach, degrees.
    pub throw_bank_deg: f32,
    /// How far the bank must swing toward the throw's side within [`Self::throw_window_s`],
    /// degrees.
    pub throw_sweep_deg: f32,
    /// The time the swing must happen in, seconds.
    pub throw_window_s: f32,
    /// The bank, either way, the arms must come back within before another throw counts, degrees.
    pub rearm_bank_deg: f32,
    /// The time after a throw before another is taken, seconds.
    pub cooldown_s: f32,
    /// The arms' pitch, as a share of a full pitch, that turns the throw's side roll into its
    /// diagonal: below it the evade rolls straight to the side.
    pub diagonal_pitch: f32,
}

/// How the parachute's handles pull (see [`crate::controllers::vehicles::parachute`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParachuteHandlesTuning {
    /// How far down from where it was taken a handle is pulled to act fully, metres.
    pub full_pull_m: f32,
    /// How far both handles must be pulled, as a share of a full pull, before they brake. Below it,
    /// both pulled together only bank by their difference.
    pub brake_threshold: f32,
}

/// The ripcord (see [`crate::controllers::vehicles::ripcord`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RipcordTuning {
    /// How far the holding hand must come from where it took the ripcord to pull it, metres.
    pub pull_m: f32,
}

/// The helicopter skid (see [`crate::controllers::vehicles::rail`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RailTuning {
    /// How far the skid's line reaches either way from where Rico lands on it, metres.
    pub half_length_m: f32,
    /// How far off the line, level, the feet go before Rico has stepped off, metres.
    pub step_off_m: f32,
    /// The sticky pull's time constant: the offset off the line closes at this rate, seconds.
    /// Longer is less sticky.
    pub pull_s: f32,
}

/// The largest offset from the eyes any control may sit at, metres.
const MAX_OFFSET_M: f32 = 2.0;

impl PhysicalTuning {
    fn validate(&self) -> Result<(), Invalid> {
        let wheel = &self.wheel;
        vec3("physical.wheel.centre_m", wheel.centre_m, MAX_OFFSET_M)?;
        positive("physical.wheel.radius_m", wheel.radius_m, 1.0)?;
        range("physical.wheel.tilt_deg", wheel.tilt_deg, 0.0, 90.0)?;
        positive("physical.wheel.response", wheel.response, 10.0)?;
        nonnegative("physical.wheel.deadzone_deg", wheel.deadzone_deg, 45.0)?;
        nonnegative(
            "physical.wheel.return_deg_per_s",
            wheel.return_deg_per_s,
            10_000.0,
        )?;

        let bars = &self.handlebars;
        vec3("physical.handlebars.centre_m", bars.centre_m, MAX_OFFSET_M)?;
        positive("physical.handlebars.half_width_m", bars.half_width_m, 1.0)?;
        positive(
            "physical.handlebars.max_steer_deg",
            bars.max_steer_deg,
            90.0,
        )?;
        range(
            "physical.handlebars.deadzone_deg",
            bars.deadzone_deg,
            0.0,
            bars.max_steer_deg,
        )?;
        positive("physical.handlebars.response", bars.response, 10.0)?;
        positive("physical.handlebars.grab_radius_m", bars.grab_radius_m, 1.0)?;

        self.cyclic.validate("physical.cyclic")?;
        self.flight_stick.validate("physical.flight_stick")?;
        self.collective.validate("physical.collective")?;
        self.throttle.validate("physical.throttle")?;

        let yoke = &self.yoke;
        positive("physical.yoke.max_roll_deg", yoke.max_roll_deg, 180.0)?;
        positive("physical.yoke.travel_m", yoke.travel_m, 1.0)?;
        range("physical.yoke.deadzone", yoke.deadzone, 0.0, 0.9)?;
        nonnegative("physical.yoke.return_s", yoke.return_s, 10.0)?;

        let flight = &self.flight;
        positive(
            "physical.flight.twist_full_deg",
            flight.twist_full_deg,
            180.0,
        )?;
        range(
            "physical.flight.twist_deadzone_deg",
            flight.twist_deadzone_deg,
            0.0,
            flight.twist_full_deg,
        )?;
        range("physical.flight.gate_at", flight.gate_at, 0.0, 1.0)?;
        range(
            "physical.flight.reverse_share",
            flight.reverse_share,
            0.0,
            0.9,
        )?;
        range("physical.flight.zero_pull", flight.zero_pull, 0.0, 1.0)?;
        range("physical.flight.zero_snap", flight.zero_snap, 0.0, 1.0)?;
        positive("physical.flight.thrust_step", flight.thrust_step, 1.0)?;

        let buttons = &self.buttons;
        range("physical.buttons.hub_share", buttons.hub_share, 0.0, 1.0)?;
        vec3(
            "physical.buttons.nitrous_from_rim_m",
            buttons.nitrous_from_rim_m,
            1.0,
        )?;
        positive(
            "physical.buttons.nitrous_radius_m",
            buttons.nitrous_radius_m,
            1.0,
        )?;
        positive("physical.buttons.press_depth_m", buttons.press_depth_m, 1.0)?;
        positive("physical.buttons.twist_deg", buttons.twist_deg, 180.0)?;

        positive("physical.grab_radius_m", self.grab_radius_m, 1.0)
    }
}

impl StickSpec {
    fn validate(&self, path: &str) -> Result<(), Invalid> {
        vec3(&format!("{path}.top_m"), self.top_m, MAX_OFFSET_M)?;
        positive(
            &format!("{path}.max_deflection_m"),
            self.max_deflection_m,
            1.0,
        )?;
        range(&format!("{path}.deadzone"), self.deadzone, 0.0, 0.9)?;
        nonnegative(&format!("{path}.return_s"), self.return_s, 10.0)
    }
}

impl LeverSpec {
    fn validate(&self, path: &str) -> Result<(), Invalid> {
        vec3(&format!("{path}.rest_m"), self.rest_m, MAX_OFFSET_M)?;
        positive(&format!("{path}.travel_m"), self.travel_m, 1.0)?;
        range(&format!("{path}.detent"), self.detent, 0.0, 0.9)
    }
}

impl ArmSteeringTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("arm_steering.full_bank_m", self.full_bank_m, 2.0)?;
        range(
            "arm_steering.deadzone_m",
            self.deadzone_m,
            0.0,
            self.full_bank_m,
        )
    }
}

impl WingsuitGesturesTuning {
    fn validate(&self) -> Result<(), Invalid> {
        for (name, span) in [
            ("flight_lateral_span", self.flight_lateral_span),
            ("withdraw_lateral_span", self.withdraw_lateral_span),
            ("withdraw_lowered_span", self.withdraw_lowered_span),
            ("withdraw_reach_span", self.withdraw_reach_span),
            ("pose_hysteresis_span", self.pose_hysteresis_span),
        ] {
            nonnegative(&format!("wingsuit_gestures.{name}"), span, 2.0)?;
        }
        for (name, angle) in [
            ("flight_angle_deg", self.flight_angle_deg),
            ("flight_roll_deg", self.flight_roll_deg),
            ("brake_roll_deg", self.brake_roll_deg),
        ] {
            nonnegative(&format!("wingsuit_gestures.{name}"), angle, 180.0)?;
        }
        range(
            "wingsuit_gestures.brake_roll_max_deg",
            self.brake_roll_max_deg,
            self.brake_roll_deg,
            180.0,
        )?;
        for (name, dwell) in [
            ("neutral_dwell_s", self.neutral_dwell_s),
            ("flight_dwell_s", self.flight_dwell_s),
            ("close_dwell_s", self.close_dwell_s),
            ("brake_dwell_s", self.brake_dwell_s),
        ] {
            nonnegative(&format!("wingsuit_gestures.{name}"), dwell, 5.0)?;
        }
        positive(
            "wingsuit_gestures.max_sample_gap_s",
            self.max_sample_gap_s,
            5.0,
        )
    }
}

impl WingsuitEvadeTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("wingsuit_evade.throw_bank_deg", self.throw_bank_deg, 180.0)?;
        positive(
            "wingsuit_evade.throw_sweep_deg",
            self.throw_sweep_deg,
            360.0,
        )?;
        positive("wingsuit_evade.throw_window_s", self.throw_window_s, 5.0)?;
        range(
            "wingsuit_evade.rearm_bank_deg",
            self.rearm_bank_deg,
            0.0,
            self.throw_bank_deg,
        )?;
        nonnegative("wingsuit_evade.cooldown_s", self.cooldown_s, 30.0)?;
        range(
            "wingsuit_evade.diagonal_pitch",
            self.diagonal_pitch,
            0.0,
            1.0,
        )
    }
}

impl ParachuteHandlesTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("parachute_handles.full_pull_m", self.full_pull_m, 2.0)?;
        range(
            "parachute_handles.brake_threshold",
            self.brake_threshold,
            0.0,
            1.0,
        )
    }
}

impl RailTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("rail.half_length_m", self.half_length_m, 10.0)?;
        positive("rail.step_off_m", self.step_off_m, 5.0)?;
        nonnegative("rail.pull_s", self.pull_s, 10.0)
    }
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
