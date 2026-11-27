//! The physical controls' tuning: where the controls of the driver's seat sit and how they move,
//! the wheel's steering, the fit of the vehicle's own wheel and handlebars, and the controls'
//! haptic cues.

use glam::Vec3;
use jc3vr_core::config::{Invalid, nonnegative, positive, range, vec3};
use serde::Deserialize;

use crate::vehicles::{
    config::{PhysicalControlsConfig, WheelConfig, WheelTracking},
    tuning::VEHICLES,
};

/// The physical controls as the tuning file holds them.
pub type PhysicalTuning = PhysicalControls<SteeringWheelTuning>;

/// The physical controls with the wheel settings the configuration sets, as the controls use them.
pub type PhysicalControlsTuning = PhysicalControls<WheelSpec>;

/// Where the physical controls of the driver's seat sit and how they move (see
/// [`crate::vehicles::physical`]), over the wheel's description `W`.
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
    pub(crate) centre_m: Vec3,
    pub radius_m: f32,
    /// [`WheelSpec::tilt_deg`].
    pub tilt_deg: f32,
    /// [`WheelSpec::response`].
    pub response: f32,
    /// [`WheelSpec::deadzone_deg`].
    pub(crate) deadzone_deg: f32,
    /// [`WheelSpec::return_deg_per_s`].
    pub(crate) return_deg_per_s: f32,
}

/// A steering wheel: a rim in a plane facing the driver, tilted back with its column. The hub,
/// radius, and tilt place the wheel where the vehicle's own is not found; the lock, the response,
/// the tracking, and the return apply to every wheel (see
/// [`crate::vehicles::steering`]).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct WheelSpec {
    /// The wheel's hub.
    pub(crate) centre_m: Vec3,
    pub radius_m: f32,
    /// How far the wheel's face tilts up from vertical toward the driver, degrees.
    pub tilt_deg: f32,
    /// [`WheelConfig::lock_deg`].
    pub lock_deg: f32,
    /// The exponent the share of the lock is raised to: one is linear, above one steers more gently
    /// around centre and as fully at the lock.
    pub response: f32,
    /// Wheel angles within this of centre steer nothing, degrees.
    pub(crate) deadzone_deg: f32,
    /// [`WheelConfig::turned_by`].
    pub turned_by: WheelTracking,
    /// How fast the released wheel returns to centre, degrees per second.
    pub(crate) return_deg_per_s: f32,
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
/// [`crate::vehicles::physical`]). The middle and half width place them where the
/// vehicle's own are not found; the lock, the response, and the reach apply to all.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandlebarsSpec {
    /// The middle of the bars.
    pub(crate) centre_m: Vec3,
    /// From the middle to each grip, metres.
    pub half_width_m: f32,
    /// The bars' turn that steers fully, degrees. The bars stop there.
    pub(crate) max_steer_deg: f32,
    /// Bar turns within this of centre steer nothing, degrees, so that a resting hand's tremor and
    /// the small turns a wrist makes do not weave the bike.
    pub(crate) deadzone_deg: f32,
    /// The exponent the share of the lock past the deadzone is raised to: one is linear, above one
    /// steers more gently around centre and as fully at the lock.
    pub response: f32,
    /// How far from the hand's half of the bars its grip may be for a grip press to take them,
    /// metres: wider than the other controls', since the bars are where a reaching hand expects
    /// them less exactly than a wheel's rim.
    pub grab_radius_m: f32,
    /// The share of the half width from the middle where a hand's half of the bars starts to grab.
    pub(crate) grab_from_middle: f32,
    /// The share of the half width from the middle to where a hand's half of the bars stops
    /// grabbing.
    pub(crate) grab_past_end: f32,
}

/// A joystick whose top moves in the horizontal plane. Let go, a spring returns it to centre.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StickSpec {
    /// The stick's top at rest, given on the dominant side.
    pub top_m: Vec3,
    /// How far the top moves for a full deflection, metres.
    pub(crate) max_deflection_m: f32,
    /// The share of the full deflection around centre that outputs nothing, `0..1`, so that a
    /// resting hand's tremor does not fly the vehicle.
    pub deadzone: f32,
    /// The released stick's centring spring: the time constant it returns to centre with, seconds.
    /// Zero snaps it back.
    pub return_s: f32,
}

/// A plane's control wheel on its column (see [`crate::vehicles::physical::yoke`]):
/// turned for the roll and pushed or pulled along the column for the pitch. Its place comes from
/// the cockpit's model.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct YokeSpec {
    /// The turn either way that rolls fully, degrees. The yoke stops there.
    pub(crate) max_roll_deg: f32,
    /// How far the yoke moves along its column either way for a full pitch, metres.
    pub travel_m: f32,
    /// The share of each axis's range around centre that outputs nothing, `0..1`.
    pub deadzone: f32,
    /// The released yoke's centring spring: the time constant it returns to centre with, seconds.
    pub return_s: f32,
}

/// The horn and the nitrous worked by hand (see
/// [`crate::vehicles::physical::buttons`]): the horn pressed with a free hand on the
/// wheel's hub, a nitrous button on the dash beside the wheel (on a vehicle with nitrous), and the
/// collective's grip twisted either way for the nitrous. The plane's afterburner is its throttle's
/// gate ([`FlightControlsTuning::gate_at`]). The buttons stay alongside the controller's.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CockpitButtonsTuning {
    /// The hub's share of the wheel's radius that presses the horn.
    pub(crate) hub_share: f32,
    /// The nitrous button from the wheel's rim on its right: along the wheel's right, its up, and
    /// its axis toward the driver, metres.
    pub(crate) nitrous_from_rim_m: Vec3,
    pub(crate) nitrous_radius_m: f32,
    /// How far past a button's face a hand pushes to press it, metres.
    pub(crate) press_depth_m: f32,
    /// The share of the press depth a pressed button gives before it lets go, so a hand resting on
    /// it does not chatter.
    pub(crate) release_depth: f32,
    /// How far, as a multiple of its radius, a hand may slide across a pressed button and keep it
    /// pressed.
    pub(crate) held_radius: f32,
    /// How far in front of a push button's face a hand may be to arm it, metres.
    pub(crate) arm_reach_m: f32,
    /// How far past a push button's face a hand may be and still press it, metres: further is a
    /// hand through the dash.
    pub(crate) max_push_m: f32,
    /// How far the collective's grip is twisted from where the hand took it to burn the nitrous,
    /// degrees: well past the roll a wrist makes while it raises and lowers the lever.
    pub twist_deg: f32,
    /// How far a twist grip comes back from its engaging twist before it lets go, degrees.
    pub(crate) twist_hysteresis_deg: f32,
}

/// The plane's flight stick and throttle beyond a plain stick and lever (see
/// [`crate::vehicles::physical`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlightControlsTuning {
    /// Twists within this of where the hand took the stick work nothing, degrees.
    pub(crate) twist_deadzone_deg: f32,
    /// The twist that gives a full rudder, degrees.
    pub(crate) twist_full_deg: f32,
    /// Where the throttle's gate at the top of its travel starts, as a share of its forward travel,
    /// `0..1`. Below the gate the throttle stops short of the thrust that lights the afterburner,
    /// and pushed through it, the throttle is full and the afterburner burns where the game allows
    /// it.
    pub(crate) gate_at: f32,
    /// The share of the throttle's whole travel, at its back, that is the reverse section behind
    /// the zero gate, `0..0.9`. The rest, in front of the gate, is forward thrust from nothing to
    /// full.
    pub(crate) reverse_share: f32,
    /// How far past zero the hand pulls (or pushes) the throttle to take it through the zero gate
    /// into the other section, as a share of the whole travel. Short of it, the throttle stays at
    /// zero.
    pub(crate) zero_pull: f32,
    /// The share of either section next to zero that gives nothing, and in which a released
    /// throttle snaps to zero.
    pub(crate) zero_snap: f32,
    /// The share of the forward section between the ticks the held throttle gives its hand.
    pub(crate) thrust_step: f32,
}

/// A lever that slides along one axis.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeverSpec {
    /// The handle's neutral position, given on the off-hand side.
    pub(crate) rest_m: Vec3,
    pub axis: LeverAxis,
    /// The handle's whole travel, from full negative to full positive, metres.
    pub travel_m: f32,
    /// The share of the travel around neutral that outputs nothing, `0..1`.
    pub detent: f32,
    /// Keep the lever where it was let go, as a throttle does, rather than returning it to neutral.
    pub(crate) hold_on_release: bool,
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

/// How the wheel turns and returns (see [`crate::vehicles::steering`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SteeringTuning {
    /// Over this last stretch of its return, the released wheel slows, degrees.
    pub(crate) return_ease_deg: f32,
    /// The slowest the easing lets the return go, as a share of the return rate.
    pub(crate) return_ease_floor: f32,
    /// A returning wheel this close to centre is at centre, degrees.
    pub settled_deg: f32,
    /// How far the wheel must leave its stop or centre before that cue may fire again, degrees.
    pub(crate) cue_rearm_deg: f32,
    /// A hand closer to the hub than this share of the radius turns nothing by its travel; one past
    /// twice it counts fully.
    pub(crate) hub_share: f32,
}

/// How the sticks, the yoke, and the handlebars move beyond their specs (see
/// [`crate::vehicles::physical`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlMotionTuning {
    /// How long the handlebars stay in a hand whose grip has opened, seconds: long enough for
    /// fingers lifted off the handle a moment to close again.
    pub(crate) bars_grip_grace_s: f32,
    /// A returning stick closer to centre than this, metres, is at centre.
    pub(crate) stick_settled_m: f32,
    /// A hand nearer the yoke's column than this, metres, has no angle about it to turn the yoke
    /// by.
    pub(crate) yoke_min_radius_m: f32,
    /// A returning yoke within this turn of centre, radians, and [`Self::yoke_settled_m`] along its
    /// column, is at centre.
    pub(crate) yoke_settled_rad: f32,
    pub(crate) yoke_settled_m: f32,
    /// Where a helicopter's missing collective sits from its cyclic's grip, in the cockpit part's
    /// own space (+X to the pilot's right, +Y up, +Z back), metres: by the pilot's left thigh, left
    /// of the cyclic, below it, and further back. A placement, not a measurement.
    pub(crate) collective_from_cyclic_m: Vec3,
}

/// When the sticks' and levers' haptic cues fire (see
/// [`crate::vehicles::physical::cues`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlCuesTuning {
    /// How far a control must leave a stop, a detent, or the gate, as a share of its travel, before
    /// the cue may fire again.
    pub rearm: f32,
    /// The share of the travel that counts as at the stop.
    pub(crate) at_stop: f32,
    /// The share of the travel from centre that counts as at centre.
    pub(crate) at_centre: f32,
}

/// What the box fit of a vehicle's own wheel and handlebars accepts and where it puts the grips
/// (see [`crate::vehicles::wheel`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WheelFitTuning {
    /// The steepest tilt the box fit gives, degrees: a bus's wheel lies about this flat.
    pub max_tilt_deg: f32,
    /// The smallest wheel the box fit accepts, metres.
    pub(crate) min_wheel_radius_m: f32,
    /// How far in from the bars' ends and rear the grips sit, metres.
    pub(crate) grip_inset_m: f32,
    /// The narrowest handlebars the box fit accepts, metres from the middle to a grip.
    pub(crate) min_bars_half_width_m: f32,
}

/// The largest offset from the eyes any control may sit at, metres.
const MAX_OFFSET_M: f32 = 2.0;

impl PhysicalTuning {
    pub(super) fn validate(&self) -> Result<(), Invalid> {
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
        range(
            "physical.handlebars.grab_from_middle",
            bars.grab_from_middle,
            0.0,
            1.0,
        )?;
        range(
            "physical.handlebars.grab_past_end",
            bars.grab_past_end,
            1.0,
            2.0,
        )?;

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
        range(
            "physical.buttons.release_depth",
            buttons.release_depth,
            0.0,
            1.0,
        )?;
        range(
            "physical.buttons.held_radius",
            buttons.held_radius,
            1.0,
            5.0,
        )?;
        positive("physical.buttons.arm_reach_m", buttons.arm_reach_m, 1.0)?;
        range(
            "physical.buttons.max_push_m",
            buttons.max_push_m,
            buttons.press_depth_m,
            1.0,
        )?;
        positive("physical.buttons.twist_deg", buttons.twist_deg, 180.0)?;
        range(
            "physical.buttons.twist_hysteresis_deg",
            buttons.twist_hysteresis_deg,
            0.0,
            buttons.twist_deg,
        )?;

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

impl SteeringTuning {
    pub(super) fn validate(&self) -> Result<(), Invalid> {
        nonnegative("steering.return_ease_deg", self.return_ease_deg, 180.0)?;
        range(
            "steering.return_ease_floor",
            self.return_ease_floor,
            0.0,
            1.0,
        )?;
        nonnegative("steering.settled_deg", self.settled_deg, 10.0)?;
        nonnegative("steering.cue_rearm_deg", self.cue_rearm_deg, 90.0)?;
        range("steering.hub_share", self.hub_share, 0.0, 0.5)
    }
}

impl ControlMotionTuning {
    pub(super) fn validate(&self) -> Result<(), Invalid> {
        nonnegative("controls.bars_grip_grace_s", self.bars_grip_grace_s, 5.0)?;
        nonnegative("controls.stick_settled_m", self.stick_settled_m, 0.1)?;
        nonnegative("controls.yoke_min_radius_m", self.yoke_min_radius_m, 0.5)?;
        nonnegative("controls.yoke_settled_rad", self.yoke_settled_rad, 0.5)?;
        nonnegative("controls.yoke_settled_m", self.yoke_settled_m, 0.1)?;
        vec3(
            "controls.collective_from_cyclic_m",
            self.collective_from_cyclic_m,
            MAX_OFFSET_M,
        )
    }
}

impl ControlCuesTuning {
    pub(super) fn validate(&self) -> Result<(), Invalid> {
        range("cues.rearm", self.rearm, 0.0, 0.5)?;
        range("cues.at_stop", self.at_stop, 0.5, 1.0)?;
        range("cues.at_centre", self.at_centre, 0.0, 0.5)
    }
}

impl WheelFitTuning {
    pub(super) fn validate(&self) -> Result<(), Invalid> {
        range("wheel_fit.max_tilt_deg", self.max_tilt_deg, 0.0, 90.0)?;
        positive("wheel_fit.min_wheel_radius_m", self.min_wheel_radius_m, 1.0)?;
        nonnegative("wheel_fit.grip_inset_m", self.grip_inset_m, 0.5)?;
        positive(
            "wheel_fit.min_bars_half_width_m",
            self.min_bars_half_width_m,
            1.0,
        )
    }
}
