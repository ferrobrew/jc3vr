//! The vehicle, mounted-gun, wingsuit, and parachute settings, held in
//! [`ControllersConfig::vehicles`](crate::controllers::config::ControllersConfig::vehicles).
//!
//! The physical controls are placed in the seated character's body frame (+X right, +Y up, +Z back,
//! metres) with the character's eyes at the origin: the cockpit frame, whose origin the head pose
//! pins to the character's animated head, which rides the seat. Offsets are given for a
//! right-handed layout, and every control's X is mirrored for a left-handed one, so the stick that
//! sits by the dominant knee moves with the dominant hand.

use glam::Vec3;
use serde::{Deserialize, Serialize};

/// Vehicles, mounted guns, the wingsuit, and the parachute. Stick bindings always work; the
/// physical controls, the arm steering, and the parachute handles are additional and, as part of
/// the full gameplay preset, on by default too.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VehiclesConfig {
    /// Aim the driver's vehicle weapons at the dominant hand's aim point instead of the gaze. Off,
    /// they aim along the gaze as the game does.
    pub driver_weapons_by_hand: bool,
    /// Mounted guns and gunner seats aimed by the hands on the handles.
    pub mounted: MountedConfig,
    /// The opt-in physical controls of the driver's seat.
    pub physical: PhysicalControlsConfig,
    /// The arm-pose steering of the wingsuit.
    pub arm_steering: ArmSteeringConfig,
    /// The parachute's handles, grabbed and pulled to steer and brake.
    pub parachute_handles: ParachuteHandlesConfig,
}

impl VehiclesConfig {
    pub const fn new() -> Self {
        Self {
            driver_weapons_by_hand: true,
            mounted: MountedConfig::new(),
            physical: PhysicalControlsConfig::new(),
            arm_steering: ArmSteeringConfig::new(),
            parachute_handles: ParachuteHandlesConfig::new(),
        }
    }
}

impl Default for VehiclesConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Mounted guns and gunner seats (see [`crate::controllers::vehicles::mounted`]). Acts only under
/// the controller scheme, so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MountedConfig {
    /// Aim the mounted gun from the hands' positions on its handles. Off, it aims along the gaze.
    pub aim_by_hands: bool,
    /// Put the rendered hands on the gun's handles (the anchored tier). Off, the mounted-gun state
    /// yields the hands to the animation, which also holds the handles.
    pub anchor_hands: bool,
    /// The shortest lever from the handles to the gun's pivot, metres. A gun whose handles the
    /// game does not place, or places at the pivot, swings about a pivot this far ahead of them.
    pub min_lever_m: f32,
}

impl MountedConfig {
    pub const fn new() -> Self {
        Self {
            aim_by_hands: true,
            anchor_hands: true,
            min_lever_m: 0.4,
        }
    }
}

impl Default for MountedConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The physical controls of the driver's seat (see [`crate::controllers::vehicles::physical`]).
/// Each is on by default, as part of the full gameplay preset; a control in reach swallows the
/// grip presses that would fire the vehicle's weapons.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhysicalControlsConfig {
    /// The car's and the boat's wheel.
    pub wheel: WheelSpec,
    /// The motorcycle's handlebars.
    pub handlebars: HandlebarsSpec,
    /// The helicopter's cyclic, by the dominant knee.
    pub cyclic: StickSpec,
    /// The helicopter's collective, by the off-hand hip.
    pub collective: LeverSpec,
    /// The plane's flight stick, by the dominant knee.
    pub flight_stick: StickSpec,
    /// The plane's throttle, by the off-hand hip.
    pub throttle: LeverSpec,
    /// How far from a control a hand's grip may be for a grip press to grab it, metres.
    pub grab_radius_m: f32,
    /// How far a holding hand may move from the control before it lets go, metres.
    pub release_distance_m: f32,
}

impl PhysicalControlsConfig {
    pub const fn new() -> Self {
        Self {
            wheel: WheelSpec {
                enabled: true,
                centre_m: Vec3::new(0.0, -0.42, -0.40),
                radius_m: 0.19,
                tilt_deg: 25.0,
                max_angle_deg: 180.0,
                return_deg_per_s: 360.0,
            },
            handlebars: HandlebarsSpec {
                enabled: true,
                centre_m: Vec3::new(0.0, -0.50, -0.45),
                half_width_m: 0.34,
                max_steer_deg: 30.0,
                max_lean_deg: 25.0,
            },
            cyclic: StickSpec {
                enabled: true,
                top_m: Vec3::new(0.12, -0.78, -0.30),
                max_deflection_m: 0.12,
            },
            collective: LeverSpec {
                enabled: true,
                rest_m: Vec3::new(-0.28, -0.80, -0.10),
                axis: LeverAxis::Vertical,
                travel_m: 0.30,
                detent: 0.1,
                hold_on_release: false,
            },
            flight_stick: StickSpec {
                enabled: true,
                top_m: Vec3::new(0.10, -0.75, -0.32),
                max_deflection_m: 0.12,
            },
            throttle: LeverSpec {
                enabled: true,
                rest_m: Vec3::new(-0.28, -0.70, -0.20),
                axis: LeverAxis::ForeAft,
                travel_m: 0.24,
                detent: 0.1,
                hold_on_release: true,
            },
            grab_radius_m: 0.09,
            release_distance_m: 0.25,
        }
    }

    /// Whether any control is switched on.
    pub const fn any(&self) -> bool {
        self.wheel.enabled
            || self.handlebars.enabled
            || self.cyclic.enabled
            || self.collective.enabled
            || self.flight_stick.enabled
            || self.throttle.enabled
    }
}

impl Default for PhysicalControlsConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// A steering wheel: a rim in a plane facing the driver, tilted back with its column.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WheelSpec {
    pub enabled: bool,
    /// The wheel's hub.
    pub centre_m: Vec3,
    pub radius_m: f32,
    /// How far the wheel's face tilts up from vertical toward the driver, degrees.
    pub tilt_deg: f32,
    /// The wheel angle that steers fully, degrees either way; the wheel stops there.
    pub max_angle_deg: f32,
    /// How fast the released wheel returns to centre, degrees per second.
    pub return_deg_per_s: f32,
}

/// Motorcycle handlebars, pivoting about their centre.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HandlebarsSpec {
    pub enabled: bool,
    /// The middle of the bars.
    pub centre_m: Vec3,
    /// From the middle to each grip, metres.
    pub half_width_m: f32,
    /// The bars' turn that steers fully, degrees.
    pub max_steer_deg: f32,
    /// The bars' roll that leans fully, degrees.
    pub max_lean_deg: f32,
}

/// A joystick whose top moves in the horizontal plane.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StickSpec {
    pub enabled: bool,
    /// The stick's top at rest, given on the dominant side.
    pub top_m: Vec3,
    /// How far the top moves for a full deflection, metres.
    pub max_deflection_m: f32,
}

/// A lever that slides along one axis.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LeverSpec {
    pub enabled: bool,
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
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// The arm-pose steering of the wingsuit (see [`crate::controllers::vehicles::arm_steering`]):
/// with the arms spread, the difference in the hands' heights banks, and both hands raised or
/// lowered pitches. On by default, as part of the full gameplay preset, alongside the stick
/// steering it adds to.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ArmSteeringConfig {
    pub wingsuit: bool,
    /// How far apart the hands must be, as a share of the arm span, for the arms to count as
    /// spread. Closer together, the arms steer nothing and the stick steers alone.
    pub min_spread: f32,
    /// Height differences and offsets below this steer nothing, metres.
    pub deadzone_m: f32,
    /// The height difference between the hands that banks fully, metres.
    pub full_bank_m: f32,
    /// How far both hands must be above or below the shoulders to pitch fully, metres.
    pub full_pitch_m: f32,
    /// Lowered hands pitch down (a dive) by default; this reverses it.
    pub invert_pitch: bool,
}

impl ArmSteeringConfig {
    pub const fn new() -> Self {
        Self {
            wingsuit: true,
            min_spread: 0.6,
            deadzone_m: 0.04,
            full_bank_m: 0.45,
            full_pitch_m: 0.30,
            invert_pitch: false,
        }
    }
}

impl Default for ArmSteeringConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The parachute's handles (see [`crate::controllers::vehicles::parachute`]): one grab volume per
/// hand above and outside its shoulder, where a canopy's toggle lines hang. A grip press inside one
/// takes the handle, and pulling it down from where it was taken banks to that side; both pulled
/// down brake. On by default, as part of the full gameplay preset; the stick still steers and
/// dives alongside them.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParachuteHandlesConfig {
    pub enabled: bool,
    /// The right handle's centre from the right shoulder joint (+X out, +Y up, +Z back), metres;
    /// the left handle mirrors it.
    pub offset_m: Vec3,
    /// How far from a handle's centre a hand's grip may be for a grip press to take it, metres.
    pub radius_m: f32,
    /// How far down from where it was taken a handle is pulled to act fully, metres.
    pub full_pull_m: f32,
    /// Pulls shorter than this act on nothing, metres.
    pub deadzone_m: f32,
    /// How far both handles must be pulled, as a share of a full pull, before they brake. Below it,
    /// both pulled together only bank by their difference.
    pub brake_threshold: f32,
    /// How far a holding hand may move from its handle before it lets go, metres.
    pub release_distance_m: f32,
}

impl ParachuteHandlesConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            offset_m: Vec3::new(0.18, 0.28, -0.05),
            radius_m: 0.12,
            full_pull_m: 0.30,
            deadzone_m: 0.03,
            brake_threshold: 0.2,
            release_distance_m: 0.25,
        }
    }
}

impl Default for ParachuteHandlesConfig {
    fn default() -> Self {
        Self::new()
    }
}
