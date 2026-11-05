//! The vehicle, mounted-gun, wingsuit, and parachute settings, held in
//! [`ControllersConfig::vehicles`](crate::controllers::config::ControllersConfig::vehicles). The
//! physical controls' fixed placement and tuning are [`crate::controllers::vehicles::tuning`]'s.

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
    /// In a passenger or stunt seat, turn the seated body toward the gun hand's aim point instead
    /// of the gaze's, so the body leans and points the gun where the hand does. Off, the body turns
    /// toward the gaze as the game's does, while the gun still aims by the hand.
    pub passenger_body_by_hand: bool,
    /// Mounted guns and gunner seats aimed by the hands on the handles.
    pub mounted: MountedConfig,
    /// The physical controls of the driver's seat.
    pub physical: PhysicalControlsConfig,
    /// The arm-pose steering of the wingsuit.
    pub arm_steering: ArmSteeringConfig,
    /// Fresh-pose wingsuit deployment, bilateral withdrawal, and palms-forward braking.
    pub wingsuit_gestures: WingsuitGesturesConfig,
    /// The parachute's handles, grabbed and pulled to steer and brake.
    pub parachute_handles: ParachuteHandlesConfig,
    /// The ripcord at the hip, grabbed and pulled to open the parachute from a fall or the
    /// wingsuit.
    pub ripcord: RipcordConfig,
    /// The driver's view moved per vehicle to a standard place behind the wheel.
    pub driving_view: DrivingViewConfig,
    /// Turn the body to the head's heading when the boost wingsuit takes off from the ground, so the
    /// suit launches where the player looks
    /// ([`take_off`](crate::controllers::vehicles::take_off)).
    pub take_off_to_head: bool,
}

impl VehiclesConfig {
    pub const fn new() -> Self {
        Self {
            driver_weapons_by_hand: true,
            passenger_body_by_hand: true,
            mounted: MountedConfig::new(),
            physical: PhysicalControlsConfig::new(),
            arm_steering: ArmSteeringConfig::new(),
            wingsuit_gestures: WingsuitGesturesConfig::new(),
            parachute_handles: ParachuteHandlesConfig::new(),
            ripcord: RipcordConfig::new(),
            driving_view: DrivingViewConfig::new(),
            take_off_to_head: true,
        }
    }
}

impl Default for VehiclesConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The driving view's settings, in [`VehiclesConfig::driving_view`] (see
/// [`crate::controllers::vehicles::driving_view`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DrivingViewConfig {
    /// Move a car's view per vehicle so the eyes sit at the standard offset from the wheel's hub.
    pub enabled: bool,
}

impl DrivingViewConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
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
    /// Put the rendered hands on the gun's handles (the anchored tier), aim the mounted gun from
    /// their positions there, and kick the handles, and the hands on them, with each shot (see
    /// [`crate::controllers::vehicles::mounted_recoil`]). Off, the gun aims along the gaze and the
    /// mounted-gun state yields the hands to the animation, which also holds the handles.
    pub aim_by_hands: bool,
    /// Take the handles with the grip (see [`crate::controllers::vehicles::mounted_grip`]): only a
    /// hand holding its handle is drawn on it and aims the gun, and with both let go the gun
    /// stays where they left it. Off, both hands sit on the handles and aim the gun whenever the
    /// character mans it.
    pub grip_handles: bool,
}

impl MountedConfig {
    pub const fn new() -> Self {
        Self {
            aim_by_hands: true,
            grip_handles: true,
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
/// grip presses that would fire the vehicle's weapons. Where they sit and how they move is
/// [`PhysicalControlsTuning`](crate::controllers::vehicles::tuning::PhysicalControlsTuning)'s.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhysicalControlsConfig {
    /// The car's and the boat's wheel, with the horn on its hub.
    pub wheel: WheelConfig,
    /// The handlebars of the motorcycles, the ATV, the jetski, and the duck.
    pub handlebars: bool,
    /// The helicopter's cyclic and the plane's flight stick, by the dominant knee.
    pub sticks: bool,
    /// The helicopter's collective, twisted for the nitrous, and the plane's throttle with its
    /// afterburner gate, by the off-hand hip.
    pub levers: bool,
    /// A nitrous button on the dash beside the wheel, pushed with a free hand.
    pub nitrous_button: bool,
    /// Open the throttle by twisting the handlebars' right grip back, as a motorcycle's throttle
    /// is twisted. The trigger still accelerates; the larger of the two wins.
    pub twist_throttle: bool,
    /// Twist the held flight stick about its shaft to work the rudder: turning the top to the
    /// right yaws right.
    pub twist_rudder: bool,
}

impl PhysicalControlsConfig {
    pub const fn new() -> Self {
        Self {
            wheel: WheelConfig::new(),
            handlebars: true,
            sticks: true,
            levers: true,
            nitrous_button: true,
            twist_throttle: true,
            twist_rudder: true,
        }
    }

    /// Whether any control is switched on.
    pub const fn any(&self) -> bool {
        self.wheel.enabled || self.handlebars || self.sticks || self.levers
    }
}

impl Default for PhysicalControlsConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The steering wheel's switch and the settings that apply to every wheel, whether placed from the
/// vehicle's own or from the fallback (see [`crate::controllers::vehicles::steering`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WheelConfig {
    pub enabled: bool,
    /// The wheel angle that steers fully, degrees either way: half the lock-to-lock turn. The wheel
    /// stops there. A road car's is several times this; a game's arcade steering wants full lock in
    /// reach without letting go.
    pub lock_deg: f32,
    /// The exponent the share of the lock is raised to: one is linear, above one steers more gently
    /// around centre and as fully at the lock.
    pub response: f32,
    /// What a holding hand turns the wheel by. Named apart from the `tracking` key that saved the
    /// earlier twist default, so that a saved file takes the travel default once.
    pub turned_by: WheelTracking,
}

impl WheelConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            lock_deg: 135.0,
            response: 1.0,
            turned_by: WheelTracking::Position,
        }
    }
}

impl Default for WheelConfig {
    fn default() -> Self {
        Self::new()
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

/// The bilateral wingsuit gestures. Distances are shares of the calibrated arm span.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WingsuitGesturesConfig {
    pub enabled: bool,
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
    /// The lateral margin that keeps an active brake from flickering near its spread threshold.
    pub pose_hysteresis_span: f32,
    /// Each wrist's largest roll from palm-flat, either way, for a flight pose.
    pub flight_roll_deg: f32,
    /// Each wrist's smallest roll toward palm-forward to enter braking.
    pub brake_roll_deg: f32,
    /// Each wrist's largest roll toward palm-forward to enter braking, short of palm-up.
    pub brake_roll_max_deg: f32,
    /// How far past either end of the brake's roll band a held brake may go.
    pub brake_hysteresis_deg: f32,
    /// Both wrists' rolls within this of palm-flat release reserved pitch after braking.
    pub pitch_release_roll_deg: f32,
    pub neutral_dwell_s: f32,
    pub flight_dwell_s: f32,
    pub close_dwell_s: f32,
    pub brake_dwell_s: f32,
    /// A larger pose-time gap invalidates all pending gestures.
    pub max_sample_gap_s: f32,
}

impl WingsuitGesturesConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            flight_lateral_span: 0.23,
            flight_angle_deg: 45.0,
            withdraw_lateral_span: 0.10,
            withdraw_lowered_span: 0.18,
            withdraw_reach_span: 0.24,
            pose_hysteresis_span: 0.03,
            flight_roll_deg: 40.0,
            brake_roll_deg: 60.0,
            brake_roll_max_deg: 150.0,
            brake_hysteresis_deg: 15.0,
            pitch_release_roll_deg: 30.0,
            neutral_dwell_s: 0.12,
            flight_dwell_s: 0.20,
            close_dwell_s: 0.30,
            brake_dwell_s: 0.12,
            max_sample_gap_s: 0.20,
        }
    }

    /// Whether the tuning is finite, bounded, keeps withdrawal outside the flight deadzone, keeps the
    /// flight pose's roll short of a held brake's, and keeps the brake's band clear of palm-up, where
    /// the roll wraps.
    /// Invalid saved tuning disables gesture output rather than clamping a toggle threshold.
    pub fn valid(&self) -> bool {
        let bounded = |value: f32, low, high| value.is_finite() && (low..=high).contains(&value);
        bounded(self.flight_lateral_span, 0.15, 0.40)
            && bounded(self.flight_angle_deg, 15.0, 60.0)
            && bounded(self.withdraw_lateral_span, 0.0, 0.18)
            && bounded(self.withdraw_lowered_span, 0.10, 0.35)
            && bounded(self.withdraw_reach_span, 0.10, 0.35)
            && bounded(self.pose_hysteresis_span, 0.01, 0.08)
            && self.withdraw_lateral_span + self.pose_hysteresis_span < self.flight_lateral_span
            && bounded(self.flight_roll_deg, 15.0, 60.0)
            && bounded(self.brake_roll_deg, 40.0, 85.0)
            && bounded(self.brake_roll_max_deg, 100.0, 165.0)
            && bounded(self.brake_hysteresis_deg, 2.0, 20.0)
            && self.flight_roll_deg < self.brake_roll_deg - self.brake_hysteresis_deg
            && self.brake_roll_max_deg + self.brake_hysteresis_deg < 180.0
            && bounded(self.pitch_release_roll_deg, 0.0, 60.0)
            && self.pitch_release_roll_deg <= self.flight_roll_deg
            && bounded(self.neutral_dwell_s, 0.05, 0.50)
            && bounded(self.flight_dwell_s, 0.10, 1.0)
            && bounded(self.close_dwell_s, 0.15, 1.0)
            && bounded(self.brake_dwell_s, 0.05, 0.50)
            && bounded(self.max_sample_gap_s, 0.05, 0.50)
    }
}

impl Default for WingsuitGesturesConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The arm-pose steering of the wingsuit (see [`crate::controllers::vehicles::arm_steering`]):
/// with the arms spread, the difference in the hands' heights banks, and the wrists' tilt, the
/// hands' height, or the arms' sweep pitches ([`ArmPitch`]). On by default, as part of the full
/// gameplay preset, alongside the stick steering it adds to. With the overlay on, the overlay
/// rings each hand in the wingsuit, lit while the arms steer.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ArmSteeringConfig {
    pub wingsuit: bool,
    /// Height differences and offsets below this steer nothing, metres. The bank, and the height
    /// and sweep pitches, share it.
    pub deadzone_m: f32,
    /// The height difference between the hands that banks fully, metres.
    pub full_bank_m: f32,
    /// What pitches the wingsuit.
    pub pitch_by: ArmPitch,
    /// The wrist tilt that pitches nothing, degrees, positive with the leading edges down: zero is
    /// the palms held flat.
    pub tilt_neutral_deg: f32,
    /// Reverse the pitch: by default, the leading edges tilted down, the hands lowered, or the arms
    /// swept back dive.
    pub invert_pitch: bool,
}

impl ArmSteeringConfig {
    pub const fn new() -> Self {
        Self {
            wingsuit: true,
            deadzone_m: 0.04,
            full_bank_m: 0.45,
            pitch_by: ArmPitch::WristTilt,
            tilt_neutral_deg: 0.0,
            invert_pitch: false,
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
    /// How far down from where it was taken a handle is pulled to act fully, metres.
    pub full_pull_m: f32,
    /// How far both handles must be pulled, as a share of a full pull, before they brake. Below it,
    /// both pulled together only bank by their difference.
    pub brake_threshold: f32,
}

impl ParachuteHandlesConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            full_pull_m: 0.30,
            brake_threshold: 0.2,
        }
    }
}

impl Default for ParachuteHandlesConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The ripcord (see [`crate::controllers::vehicles::ripcord`]): a handle at the hip on the dominant
/// side, taken with the grip and pulled out to open the parachute while falling or in the wingsuit.
/// The canopy's button still opens it.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RipcordConfig {
    pub enabled: bool,
    /// How far the holding hand must come from where it took the ripcord to pull it, metres.
    pub pull_m: f32,
}

impl RipcordConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            pull_m: 0.15,
        }
    }
}

impl Default for RipcordConfig {
    fn default() -> Self {
        Self::new()
    }
}
