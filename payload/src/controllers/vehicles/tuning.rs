//! The physical controls' fixed placement and tuning, and the switches they take from the
//! configuration ([`PhysicalControlsConfig`]).
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

use crate::controllers::vehicles::config::{
    BarsSteeringConfig, PhysicalControlsConfig, WheelConfig, WheelTracking,
};

/// The physical controls' tuning with the default switches.
pub const PHYSICAL: PhysicalControlsTuning = PhysicalControlsTuning::new();

/// Where the physical controls of the driver's seat sit and how they move (see
/// [`crate::controllers::vehicles::physical`]).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PhysicalControlsTuning {
    /// The car's and the boat's wheel.
    pub wheel: WheelSpec,
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
    /// What the plane's flight stick and throttle add to a plain stick and lever: the stick's
    /// twist for the rudder and the throttle's afterburner gate.
    pub flight: FlightControlsTuning,
    /// The horn and the nitrous worked by hand.
    pub buttons: CockpitButtonsTuning,
    /// How far from a control a hand's grip may be for a grip press to grab it, metres.
    pub grab_radius_m: f32,
    /// How far a holding hand may move from the control before it lets go, metres.
    pub release_distance_m: f32,
}

impl PhysicalControlsTuning {
    const fn new() -> Self {
        Self {
            wheel: WheelSpec::of(&WheelConfig::new()),
            handlebars: HandlebarsSpec {
                centre_m: Vec3::new(0.0, -0.50, -0.45),
                half_width_m: 0.34,
                max_steer_deg: 30.0,
                deadzone_deg: BarsSteeringConfig::new().deadzone_deg,
                response: BarsSteeringConfig::new().response,
                grab_radius_m: 0.16,
            },
            cyclic: StickSpec {
                top_m: Vec3::new(0.12, -0.78, -0.30),
                max_deflection_m: 0.12,
                deadzone: 0.05,
                return_s: 0.12,
            },
            collective: LeverSpec {
                rest_m: Vec3::new(-0.28, -0.80, -0.10),
                axis: LeverAxis::Vertical,
                travel_m: 0.30,
                detent: 0.1,
                hold_on_release: false,
            },
            flight_stick: StickSpec {
                top_m: Vec3::new(0.10, -0.75, -0.32),
                max_deflection_m: 0.10,
                deadzone: 0.05,
                return_s: 0.12,
            },
            throttle: LeverSpec {
                rest_m: Vec3::new(-0.28, -0.70, -0.20),
                axis: LeverAxis::ForeAft,
                travel_m: 0.24,
                detent: 0.1,
                hold_on_release: true,
            },
            flight: FlightControlsTuning {
                twist_rudder: true,
                twist_deadzone_deg: 6.0,
                twist_full_deg: 35.0,
                afterburner_gate: true,
                gate_at: 0.85,
            },
            buttons: CockpitButtonsTuning {
                horn: true,
                hub_share: 0.35,
                nitrous_button: true,
                nitrous_from_rim_m: Vec3::new(0.12, 0.02, 0.0),
                nitrous_radius_m: 0.045,
                press_depth_m: 0.02,
                twist_nitrous: true,
                twist_deg: 40.0,
                bars_twist: true,
                throttle_twist_deg: 30.0,
            },
            grab_radius_m: 0.09,
            release_distance_m: 0.25,
        }
    }

    /// The fixed tuning with the settings `config` sets.
    pub const fn of(config: &PhysicalControlsConfig) -> Self {
        Self {
            wheel: WheelSpec::of(&config.wheel),
            flight: FlightControlsTuning {
                twist_rudder: config.twist_rudder,
                ..PHYSICAL.flight
            },
            handlebars: HandlebarsSpec {
                deadzone_deg: config.bars_steering.deadzone_deg,
                response: config.bars_steering.response,
                ..PHYSICAL.handlebars
            },
            buttons: CockpitButtonsTuning {
                nitrous_button: config.nitrous_button,
                bars_twist: config.bars_twist,
                ..PHYSICAL.buttons
            },
            ..PHYSICAL
        }
    }
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
    /// [`WheelConfig::response`].
    pub response: f32,
    /// Wheel angles within this of centre steer nothing, degrees.
    pub deadzone_deg: f32,
    /// [`WheelConfig::turned_by`].
    pub turned_by: WheelTracking,
    /// How fast the released wheel returns to centre, degrees per second.
    pub return_deg_per_s: f32,
}

impl WheelSpec {
    const fn of(config: &WheelConfig) -> Self {
        Self {
            centre_m: Vec3::new(0.0, -0.42, -0.40),
            radius_m: 0.19,
            tilt_deg: 25.0,
            lock_deg: config.lock_deg,
            response: config.response,
            deadzone_deg: 2.0,
            turned_by: config.turned_by,
            return_deg_per_s: 360.0,
        }
    }
}

/// Handlebars, turning about their centre (see
/// [`crate::controllers::vehicles::physical`]). The middle and half width place them where the
/// vehicle's own are not found; the lock, the response, and the reach apply to all.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct HandlebarsSpec {
    /// The middle of the bars.
    pub centre_m: Vec3,
    /// From the middle to each grip, metres.
    pub half_width_m: f32,
    /// The bars' turn that steers fully, degrees. The bars stop there.
    pub max_steer_deg: f32,
    /// [`BarsSteeringConfig::deadzone_deg`].
    pub deadzone_deg: f32,
    /// [`BarsSteeringConfig::response`].
    pub response: f32,
    /// How far from the hand's half of the bars its grip may be for a grip press to take them,
    /// metres: wider than the other controls', since the bars are where a reaching hand expects
    /// them less exactly than a wheel's rim.
    pub grab_radius_m: f32,
}

/// A joystick whose top moves in the horizontal plane. Let go, a spring returns it to centre.
#[derive(Copy, Clone, Debug, PartialEq)]
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

/// The horn, the nitrous, and the bars' twist grips worked by hand (see
/// [`crate::controllers::vehicles::physical::buttons`]): the horn pressed on the wheel's hub, a
/// nitrous button on the dash beside the wheel, the collective twisted for the nitrous, and the
/// handlebars' right grip twisted back for the throttle and the left for the brake. The plane's
/// afterburner is its throttle's gate
/// ([`FlightControlsTuning::afterburner_gate`]). The buttons stay alongside the controller's.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct CockpitButtonsTuning {
    /// Sound the horn by pushing a free hand into the wheel's hub.
    pub horn: bool,
    /// The hub's share of the wheel's radius that presses the horn.
    pub hub_share: f32,
    /// [`PhysicalControlsConfig::nitrous_button`].
    pub nitrous_button: bool,
    /// The nitrous button from the wheel's rim on its right: along the wheel's right, its up, and
    /// its axis toward the driver, metres.
    pub nitrous_from_rim_m: Vec3,
    pub nitrous_radius_m: f32,
    /// How far past a button's face a hand pushes to press it, metres.
    pub press_depth_m: f32,
    /// Burn the nitrous by twisting the collective's grip either way.
    pub twist_nitrous: bool,
    /// How far the collective's grip is twisted from where the hand took it to burn the nitrous,
    /// degrees: well past the roll a wrist makes while it raises and lowers the lever.
    pub twist_deg: f32,
    /// [`PhysicalControlsConfig::bars_twist`].
    pub bars_twist: bool,
    /// How far a handlebars' grip is twisted back for full throttle or full brake, degrees.
    pub throttle_twist_deg: f32,
}

impl CockpitButtonsTuning {
    /// Whether any button is switched on.
    pub const fn any(&self) -> bool {
        self.horn || self.nitrous_button || self.twist_nitrous || self.bars_twist
    }
}

/// The plane's flight stick and throttle beyond a plain stick and lever (see
/// [`crate::controllers::vehicles::physical`]).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FlightControlsTuning {
    /// [`PhysicalControlsConfig::twist_rudder`].
    pub twist_rudder: bool,
    /// Twists within this of where the hand took the stick work nothing, degrees.
    pub twist_deadzone_deg: f32,
    /// The twist that gives a full rudder, degrees.
    pub twist_full_deg: f32,
    /// Give the throttle a gate at the top of its travel: below it the throttle stops short of the
    /// thrust that lights the afterburner, and pushed through it, the throttle is full and the
    /// afterburner burns where the game allows it. Off, the throttle runs straight to full.
    pub afterburner_gate: bool,
    /// Where the gate starts, as a share of the throttle's forward travel, `0..1`.
    pub gate_at: f32,
}

/// A lever that slides along one axis.
#[derive(Copy, Clone, Debug, PartialEq)]
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
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
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
