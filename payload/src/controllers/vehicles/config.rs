//! The vehicle, mounted-gun, wingsuit, and parachute settings, held in
//! [`ControllersConfig::vehicles`](crate::controllers::config::ControllersConfig::vehicles).
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
    /// The physical controls of the driver's seat.
    pub physical: PhysicalControlsConfig,
    /// The arm-pose steering of the wingsuit.
    pub arm_steering: ArmSteeringConfig,
    /// The parachute's handles, grabbed and pulled to steer and brake.
    pub parachute_handles: ParachuteHandlesConfig,
    /// The driver's view moved per vehicle to a standard place behind the wheel.
    pub driving_view: DrivingViewConfig,
}

impl VehiclesConfig {
    pub const fn new() -> Self {
        Self {
            driver_weapons_by_hand: true,
            mounted: MountedConfig::new(),
            physical: PhysicalControlsConfig::new(),
            arm_steering: ArmSteeringConfig::new(),
            parachute_handles: ParachuteHandlesConfig::new(),
            driving_view: DrivingViewConfig::new(),
        }
    }
}

impl Default for VehiclesConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The driving view's settings, in
/// [`VehiclesConfig::driving_view`] (see [`crate::controllers::vehicles::driving_view`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DrivingViewConfig {
    /// Move the view per vehicle so the eyes sit at the standard offset from the wheel's hub.
    pub enabled: bool,
    /// Where the hub sits from the eyes in the standard driving position, cockpit frame (+X right,
    /// +Y up, +Z back), metres.
    pub hub_from_eyes_m: Vec3,
    /// The longest correction applied, metres; a longer one is shortened to this along its
    /// direction.
    pub max_m: f32,
    /// How long the view takes to ease to a new correction, seconds.
    pub blend_s: f32,
    /// Correct boats too. Off, only cars are corrected.
    pub boats: bool,
}

impl DrivingViewConfig {
    /// The standard is the middle of a road car's ergonomic range: the wheel's centre about a
    /// quarter to a third of a metre below the driver's eyes and about half a metre ahead of them.
    /// It is a little nearer than a road car's, because a VR rim holds no weight and a long reach
    /// tires the arms.
    pub const fn new() -> Self {
        Self {
            enabled: true,
            hub_from_eyes_m: Vec3::new(0.0, -0.30, -0.45),
            max_m: 0.25,
            blend_s: 1.0,
            boats: false,
        }
    }
}

impl Default for DrivingViewConfig {
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
    pub flight: FlightControlsConfig,
    /// How far from a control a hand's grip may be for a grip press to grab it, metres.
    pub grab_radius_m: f32,
    /// How far a holding hand may move from the control before it lets go, metres.
    pub release_distance_m: f32,
    /// Place the wheel and the handlebars where the vehicle's own are. Off, or where the vehicle
    /// has none, the specs place them.
    pub from_vehicle: bool,
    /// Turn the vehicle's rendered steering wheel with the steering: by the held wheel's angle, or
    /// otherwise by the vehicle's own steering, so the stick turns it too.
    pub animate_wheel: bool,
    /// Draw the wheel's rim and axis, for tuning.
    pub show_wheel: bool,
}

impl PhysicalControlsConfig {
    pub const fn new() -> Self {
        Self {
            wheel: WheelSpec {
                enabled: true,
                centre_m: Vec3::new(0.0, -0.42, -0.40),
                radius_m: 0.19,
                tilt_deg: 25.0,
                lock_deg: 135.0,
                response: 1.0,
                deadzone_deg: 2.0,
                turned_by: WheelTracking::Position,
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
                deadzone: 0.05,
                return_s: 0.12,
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
                max_deflection_m: 0.10,
                deadzone: 0.05,
                return_s: 0.12,
            },
            throttle: LeverSpec {
                enabled: true,
                rest_m: Vec3::new(-0.28, -0.70, -0.20),
                axis: LeverAxis::ForeAft,
                travel_m: 0.24,
                detent: 0.1,
                hold_on_release: true,
            },
            flight: FlightControlsConfig::new(),
            grab_radius_m: 0.09,
            release_distance_m: 0.25,
            from_vehicle: true,
            animate_wheel: true,
            show_wheel: false,
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

/// A steering wheel: a rim in a plane facing the driver, tilted back with its column. The hub,
/// radius, and tilt place the wheel where the vehicle's own is not found; the lock, the response,
/// the tracking, and the return apply to every wheel (see
/// [`crate::controllers::vehicles::steering`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WheelSpec {
    pub enabled: bool,
    /// The wheel's hub.
    pub centre_m: Vec3,
    pub radius_m: f32,
    /// How far the wheel's face tilts up from vertical toward the driver, degrees.
    pub tilt_deg: f32,
    /// The wheel angle that steers fully, degrees either way: half the lock-to-lock turn. The wheel
    /// stops there. A road car's is several times this; a game's arcade steering wants full lock in
    /// reach without letting go.
    pub lock_deg: f32,
    /// The exponent the share of the lock is raised to: one is linear, above one steers more gently
    /// around centre and as fully at the lock.
    pub response: f32,
    /// Wheel angles within this of centre steer nothing, degrees.
    pub deadzone_deg: f32,
    /// What a holding hand turns the wheel by. Named apart from the `tracking` key that saved the
    /// earlier twist default, so that a saved file takes the travel default once.
    pub turned_by: WheelTracking,
    /// How fast the released wheel returns to centre, degrees per second.
    pub return_deg_per_s: f32,
}

impl Default for WheelSpec {
    fn default() -> Self {
        PhysicalControlsConfig::new().wheel
    }
}

/// What a hand holding the wheel turns it by.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WheelTracking {
    /// The hand's travel around the hub in the wheel's plane, with the hand's twist standing in
    /// near the hub. The default: it follows the arms carrying the hands round the rim.
    Position,
    /// The hand's twist about the wheel's axis alone, as a hand clamped to the rim turns.
    /// Independent of where the hand is, so a wheel fitted slightly off still turns truly, but it
    /// reads only the wrist's roll.
    Twist,
}

impl WheelTracking {
    pub const fn label(self) -> &'static str {
        match self {
            WheelTracking::Position => "the hand's travel around the hub",
            WheelTracking::Twist => "the hand's twist",
        }
    }
}

/// Handlebars, pivoting about their centre. The middle and half width place them where the
/// vehicle's own are not found.
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

/// A joystick whose top moves in the horizontal plane. Let go, a spring returns it to centre.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StickSpec {
    pub enabled: bool,
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

/// The plane's flight stick and throttle beyond a plain stick and lever (see
/// [`crate::controllers::vehicles::physical`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FlightControlsConfig {
    /// Twist the held flight stick about its shaft to work the rudder: turning the top to the
    /// right yaws right.
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

impl FlightControlsConfig {
    pub const fn new() -> Self {
        Self {
            twist_rudder: true,
            twist_deadzone_deg: 6.0,
            twist_full_deg: 35.0,
            afterburner_gate: true,
            gate_at: 0.85,
        }
    }
}

impl Default for FlightControlsConfig {
    fn default() -> Self {
        Self::new()
    }
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
/// with the arms spread, the difference in the hands' heights banks, and the wrists' tilt, the
/// hands' height, or the arms' sweep pitches ([`ArmPitch`]). On by default, as part of the full
/// gameplay preset, alongside the stick steering it adds to.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ArmSteeringConfig {
    pub wingsuit: bool,
    /// How far apart the hands must be, as a share of the arm span, for the arms to count as
    /// spread. Closer together, the arms steer nothing and the stick steers alone.
    pub min_spread: f32,
    /// Height differences and offsets below this steer nothing, metres. The bank, and the height
    /// and sweep pitches, share it.
    pub deadzone_m: f32,
    /// The height difference between the hands that banks fully, metres.
    pub full_bank_m: f32,
    /// What pitches the wingsuit.
    pub pitch_by: ArmPitch,
    /// How far both hands must be above or below the shoulders to pitch fully, metres, under
    /// [`ArmPitch::Height`].
    pub full_pitch_m: f32,
    /// How far the wrists must tilt from the neutral to pitch fully, degrees, under
    /// [`ArmPitch::WristTilt`].
    pub full_tilt_deg: f32,
    /// Wrist tilts within this of the neutral pitch nothing, degrees.
    pub tilt_deadzone_deg: f32,
    /// The wrist tilt that pitches nothing, degrees, positive with the leading edges down: zero is
    /// the palms held flat.
    pub tilt_neutral_deg: f32,
    /// How far both hands must be swept ahead of or behind the shoulders to pitch fully, metres,
    /// under [`ArmPitch::Sweep`].
    pub full_sweep_m: f32,
    /// Reverse the pitch: by default, the leading edges tilted down, the hands lowered, or the arms
    /// swept back dive.
    pub invert_pitch: bool,
    /// Ring each hand in the wingsuit, lit while the arms steer, with a line between them.
    pub show_guide: bool,
    /// Show a panel below the view in the wingsuit with what the arm steering reads and writes:
    /// the spread, whether the arms steer, the bank and pitch inputs, the arms' stick vector, the
    /// player's stick, and the sum written. A diagnostic, off by default.
    pub show_readout: bool,
}

impl ArmSteeringConfig {
    pub const fn new() -> Self {
        Self {
            wingsuit: true,
            min_spread: 0.5,
            deadzone_m: 0.04,
            full_bank_m: 0.45,
            pitch_by: ArmPitch::WristTilt,
            full_pitch_m: 0.30,
            full_tilt_deg: 30.0,
            tilt_deadzone_deg: 5.0,
            tilt_neutral_deg: 0.0,
            full_sweep_m: 0.35,
            invert_pitch: false,
            show_guide: true,
            show_readout: false,
        }
    }
}

impl Default for ArmSteeringConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// What pitches the wingsuit under the arm steering.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArmPitch {
    /// Both wrists' tilt about the arms: the leading edges, the thumb side of a flat palm, tilted
    /// down dive and tilted up climb. The default: the wrist turns faster than the arms move.
    WristTilt,
    /// Both hands' height above or below the shoulders: lowered dive.
    Height,
    /// Both hands swept behind or ahead of the shoulders: swept back dive.
    Sweep,
}

impl ArmPitch {
    pub const ALL: [ArmPitch; 3] = [ArmPitch::WristTilt, ArmPitch::Height, ArmPitch::Sweep];

    pub const fn label(self) -> &'static str {
        match self {
            ArmPitch::WristTilt => "the wrists' tilt",
            ArmPitch::Height => "the hands' height",
            ArmPitch::Sweep => "the arms' sweep",
        }
    }
}

/// The parachute's handles (see [`crate::controllers::vehicles::parachute`]): one grab volume per
/// hand a little above, ahead of, and outside its shoulder, where a hand raised to a canopy's
/// toggle comes to rest standing or seated. A grip press inside one takes the handle, and pulling
/// it down from where it was taken banks to that side; both pulled down brake. On by default, as
/// part of the full gameplay preset; the stick still steers and dives alongside them.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParachuteHandlesConfig {
    pub enabled: bool,
    /// The right handle's centre from the right shoulder joint along the torso's axes (+X out, +Y
    /// up, +Z back), metres; the left handle mirrors it. Named apart from the `offset_m` key that
    /// saved the earlier place above eye level, so that a saved file takes this default once.
    pub from_shoulder_m: Vec3,
    /// How far from a handle's centre a hand's grip may be for a grip press to take it over every
    /// other volume, metres.
    pub radius_m: f32,
    /// How far from a handle's centre a grip press outside every other volume still takes it
    /// rather than firing the grapple, metres. At least the radius.
    pub reach_m: f32,
    /// How far down from where it was taken a handle is pulled to act fully, metres.
    pub full_pull_m: f32,
    /// Pulls shorter than this act on nothing, metres.
    pub deadzone_m: f32,
    /// How far both handles must be pulled, as a share of a full pull, before they brake. Below it,
    /// both pulled together only bank by their difference.
    pub brake_threshold: f32,
    /// How far a holding hand may move from its handle before it lets go, metres.
    pub release_distance_m: f32,
    /// Mark each handle under the parachute: faint while out of reach, brighter in reach, lit
    /// while held.
    pub show_handles: bool,
}

impl ParachuteHandlesConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            from_shoulder_m: Vec3::new(0.15, 0.12, -0.12),
            radius_m: 0.14,
            reach_m: 0.30,
            full_pull_m: 0.30,
            deadzone_m: 0.03,
            brake_threshold: 0.2,
            release_distance_m: 0.25,
            show_handles: true,
        }
    }
}

impl Default for ParachuteHandlesConfig {
    fn default() -> Self {
        Self::new()
    }
}
