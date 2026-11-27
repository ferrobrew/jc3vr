//! The vehicle settings a player sets, held at `controllers.vehicles` ([`VEHICLES_CONFIG`]), and
//! the snapshot of them a vehicle tick reads ([`VehicleSettings`]). Everything else about the
//! vehicles, the mounted guns, the wingsuit, and the parachute is fixed in
//! [`crate::controllers::vehicles::tuning`].

use jc3vr_body::{
    controllers::{
        body::config::BODY_REMAP_CONFIG,
        roomscale::config::{DRIVING_VIEW_CONFIG, DrivingViewConfig},
        scheme,
    },
    hooks::input::wingsuit::{WINGSUIT_EVADE_CONFIG, WingsuitEvadeConfig},
};
use jc3vr_core::{
    config::{ConfigSlot, Invalid, SlotOptions, range},
    slot_children,
};
use jc3vr_input::frame::Hand;
use serde::{Deserialize, Serialize};

/// The vehicle, mounted gun, wingsuit, and parachute settings, at `controllers.vehicles` in the
/// settings file. The driver's view and the evade are the body's groups, which this one holds.
pub static VEHICLES_CONFIG: ConfigSlot<VehiclesConfig> = ConfigSlot::with(
    "controllers.vehicles",
    SlotOptions::new().with_children(slot_children!(VEHICLES_CONFIG {
        DRIVING_VIEW_CONFIG => driving_view,
        WINGSUIT_EVADE_CONFIG => wingsuit_evade,
    })),
);

/// The settings one vehicle handling tick reads: its own group and the few fields of lower groups
/// it consults, read one group at a time so that no two configuration guards are held at once.
#[derive(Copy, Clone)]
pub(crate) struct VehicleSettings {
    pub(crate) vehicles: VehiclesConfig,
    /// The hand the roles follow ([`scheme::acting_hand`]).
    pub(crate) dominant: Hand,
    /// Whether the hands are remapped onto the character's arms
    /// ([`BodyRemapConfig::remap_hands`](jc3vr_body::controllers::body::config::BodyRemapConfig::remap_hands)).
    pub(crate) remap_hands: bool,
}

impl VehicleSettings {
    /// The settings as they are now.
    pub(crate) fn read() -> Self {
        let vehicles = *VEHICLES_CONFIG.lock();
        let dominant = scheme::acting_hand();
        let remap_hands = BODY_REMAP_CONFIG.query(|c| c.remap_hands);
        Self {
            vehicles,
            dominant,
            remap_hands,
        }
    }
}

/// The vehicle settings a player sets: the wheel's lock and how it turns, the wingsuit's arm
/// steering, the boost wingsuit's evade, the parachute's handles, and the driver's view. Stick
/// bindings always work; the physical controls, the arm steering, and the parachute handles are
/// additional and on by default.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VehiclesConfig {
    /// The physical controls of the driver's seat.
    pub physical: PhysicalControlsConfig,
    /// The arm-pose steering of the wingsuit.
    pub arm_steering: ArmSteeringConfig,
    /// The boost wingsuit's barrel-roll evade, thrown by banking the spread arms hard and fast
    /// ([`wingsuit_evade`](crate::controllers::vehicles::wingsuit_evade)).
    pub wingsuit_evade: WingsuitEvadeConfig,
    /// The parachute's handles, grabbed and pulled to steer and brake.
    pub parachute_handles: ParachuteHandlesConfig,
    /// The driver's view moved per vehicle to a standard place behind the wheel.
    pub driving_view: DrivingViewConfig,
}

impl VehiclesConfig {
    pub const fn new() -> Self {
        Self {
            physical: PhysicalControlsConfig::new(),
            arm_steering: ArmSteeringConfig::new(),
            wingsuit_evade: WingsuitEvadeConfig::new(),
            parachute_handles: ParachuteHandlesConfig::new(),
            driving_view: DrivingViewConfig::new(),
        }
    }

    /// Check the settings whose types admit values the vehicle handling cannot use: the wheel's
    /// lock and the wingsuit's neutral wrist tilt. Paths are relative to `controllers`.
    pub fn validate(&self) -> Result<(), Invalid> {
        range(
            "vehicles.physical.wheel.lock_deg",
            self.physical.wheel.lock_deg,
            1.0,
            1080.0,
        )?;
        range(
            "vehicles.arm_steering.tilt_neutral_deg",
            self.arm_steering.tilt_neutral_deg,
            -90.0,
            90.0,
        )
    }
}

impl Default for VehiclesConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The physical controls' settings (see [`crate::controllers::vehicles::physical`]). Every control
/// is on; where they sit and how they move is
/// [`PhysicalControlsTuning`](crate::controllers::vehicles::tuning::PhysicalControlsTuning)'s.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhysicalControlsConfig {
    /// The car's and the boat's wheel.
    pub wheel: WheelConfig,
}

impl PhysicalControlsConfig {
    pub const fn new() -> Self {
        Self {
            wheel: WheelConfig::new(),
        }
    }
}

impl Default for PhysicalControlsConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The settings that apply to every steering wheel, whether placed from the vehicle's own or from
/// the fallback (see [`crate::controllers::vehicles::steering`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WheelConfig {
    /// The wheel angle that steers fully, degrees either way: half the lock-to-lock turn. The wheel
    /// stops there. A road car's is several times this; a game's arcade steering wants full lock in
    /// reach without letting go.
    pub lock_deg: f32,
    /// What a holding hand turns the wheel by. Named apart from the `tracking` key that saved the
    /// earlier twist default, so that a saved file takes the travel default once.
    pub turned_by: WheelTracking,
}

impl WheelConfig {
    pub const fn new() -> Self {
        Self {
            lock_deg: 135.0,
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

/// The arm-pose steering of the wingsuit (see [`crate::controllers::vehicles::arm_steering`]):
/// with the arms spread, the difference in the hands' heights banks, and the wrists' tilt, the
/// hands' height, or the arms' sweep pitches ([`ArmPitch`]). On by default, alongside the stick
/// steering it adds to. With the overlay on, the overlay rings each hand in the wingsuit, lit while
/// the arms steer. Its deadzone and full bank are tuning
/// ([`ArmSteeringTuning`](crate::controllers::vehicles::tuning::ArmSteeringTuning)).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ArmSteeringConfig {
    pub wingsuit: bool,
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
/// it down from where it was taken banks to that side; both pulled down brake. On by default; the
/// stick still steers and dives alongside them. The pulls are tuning
/// ([`ParachuteHandlesTuning`](crate::controllers::vehicles::tuning::ParachuteHandlesTuning)).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParachuteHandlesConfig {
    pub enabled: bool,
}

impl ParachuteHandlesConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for ParachuteHandlesConfig {
    fn default() -> Self {
        Self::new()
    }
}
