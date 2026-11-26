//! The vehicles' fixed tuning: the physical controls' placement and motion, with the wheel settings
//! they take from the configuration ([`PhysicalControlsConfig`]), the wingsuit's arm steering,
//! gestures, and evade, the parachute's handles and ripcord, and the helicopter's skid.
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

use crate::controllers::vehicles::config::{PhysicalControlsConfig, WheelConfig, WheelTracking};

/// The physical controls' tuning with the default wheel settings.
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
    const fn new() -> Self {
        Self {
            wheel: WheelSpec::of(&WheelConfig::new()),
            handlebars: HandlebarsSpec {
                centre_m: Vec3::new(0.0, -0.50, -0.45),
                half_width_m: 0.34,
                max_steer_deg: 30.0,
                deadzone_deg: 3.0,
                response: 1.6,
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
            yoke: YokeSpec {
                max_roll_deg: 75.0,
                travel_m: 0.08,
                deadzone: 0.05,
                return_s: 0.12,
            },
            flight: FlightControlsTuning {
                twist_deadzone_deg: 6.0,
                twist_full_deg: 35.0,
                afterburner_gate: true,
                gate_at: 0.85,
                reverse_share: 1.0 / 3.0,
                zero_pull: 0.12,
                zero_snap: 0.06,
                thrust_step: 0.1,
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
            },
            grab_radius_m: 0.09,
        }
    }

    /// The fixed tuning with the wheel settings `config` sets.
    pub const fn of(config: &PhysicalControlsConfig) -> Self {
        Self {
            wheel: WheelSpec::of(&config.wheel),
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
    const fn of(config: &WheelConfig) -> Self {
        Self {
            centre_m: Vec3::new(0.0, -0.42, -0.40),
            radius_m: 0.19,
            tilt_deg: 25.0,
            lock_deg: config.lock_deg,
            response: 1.0,
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

/// A plane's control wheel on its column (see [`crate::controllers::vehicles::physical::yoke`]):
/// turned for the roll and pushed or pulled along the column for the pitch. Its place comes from
/// the cockpit's model.
#[derive(Copy, Clone, Debug, PartialEq)]
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
/// [`crate::controllers::vehicles::physical::buttons`]): the horn pressed on the wheel's hub, a
/// nitrous button on the dash beside the wheel, and the collective twisted for the nitrous. The plane's
/// afterburner is its throttle's gate
/// ([`FlightControlsTuning::afterburner_gate`]). The buttons stay alongside the controller's.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct CockpitButtonsTuning {
    /// Sound the horn by pushing a free hand into the wheel's hub.
    pub horn: bool,
    /// The hub's share of the wheel's radius that presses the horn.
    pub hub_share: f32,
    /// A nitrous button on the dash beside the wheel, pushed with a free hand. Off on a vehicle
    /// without nitrous.
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
}

impl CockpitButtonsTuning {
    /// Whether any button is switched on.
    pub const fn any(&self) -> bool {
        self.horn || self.nitrous_button || self.twist_nitrous
    }
}

/// The plane's flight stick and throttle beyond a plain stick and lever (see
/// [`crate::controllers::vehicles::physical`]).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FlightControlsTuning {
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

/// The wingsuit's arm steering's fixed tuning.
pub const ARM_STEERING: ArmSteeringTuning = ArmSteeringTuning {
    deadzone_m: 0.04,
    full_bank_m: 0.45,
};

/// The arm steering's thresholds (see [`crate::controllers::vehicles::arm_steering`]).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ArmSteeringTuning {
    /// Height differences and offsets below this steer nothing, metres. The bank, and the height
    /// and sweep pitches, share it.
    pub deadzone_m: f32,
    /// The height difference between the hands that banks fully, metres.
    pub full_bank_m: f32,
}

/// The bilateral wingsuit gestures' fixed tuning.
pub const WINGSUIT_GESTURES: WingsuitGesturesTuning = WingsuitGesturesTuning::new();

impl WingsuitGesturesTuning {
    pub const fn new() -> Self {
        Self {
            flight_lateral_span: 0.23,
            flight_angle_deg: 45.0,
            withdraw_lateral_span: 0.10,
            withdraw_lowered_span: 0.18,
            withdraw_reach_span: 0.24,
            pose_hysteresis_span: 0.03,
            flight_roll_deg: 40.0,
            brake_roll_deg: 60.0,
            brake_roll_max_deg: 150.0,
            neutral_dwell_s: 0.12,
            flight_dwell_s: 0.20,
            close_dwell_s: 0.30,
            brake_dwell_s: 0.12,
            max_sample_gap_s: 0.20,
        }
    }
}

impl Default for WingsuitGesturesTuning {
    fn default() -> Self {
        Self::new()
    }
}

/// The bilateral wingsuit gestures: fresh-pose deployment, bilateral withdrawal, and palms-forward
/// braking (see [`crate::controllers::vehicles::wingsuit_gestures`]). Distances are shares of the
/// calibrated arm span.
#[derive(Copy, Clone, Debug, PartialEq)]
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

/// The boost wingsuit's evade's fixed tuning.
pub const WINGSUIT_EVADE: WingsuitEvadeTuning = WingsuitEvadeTuning::new();

impl WingsuitEvadeTuning {
    pub const fn new() -> Self {
        Self {
            throw_bank_deg: 50.0,
            throw_sweep_deg: 45.0,
            throw_window_s: 0.3,
            rearm_bank_deg: 25.0,
            cooldown_s: 1.5,
            diagonal_pitch: 0.6,
        }
    }
}

impl Default for WingsuitEvadeTuning {
    fn default() -> Self {
        Self::new()
    }
}

/// The boost wingsuit's barrel-roll evade's throw (see
/// [`crate::controllers::vehicles::wingsuit_evade`]). The bank is the angle of the line between
/// the hands from level, positive with the left hand above the right.
#[derive(Copy, Clone, Debug, PartialEq)]
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

/// The parachute's handles' fixed tuning.
pub const PARACHUTE_HANDLES: ParachuteHandlesTuning = ParachuteHandlesTuning {
    full_pull_m: 0.30,
    brake_threshold: 0.2,
};

/// How the parachute's handles pull (see [`crate::controllers::vehicles::parachute`]).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ParachuteHandlesTuning {
    /// How far down from where it was taken a handle is pulled to act fully, metres.
    pub full_pull_m: f32,
    /// How far both handles must be pulled, as a share of a full pull, before they brake. Below it,
    /// both pulled together only bank by their difference.
    pub brake_threshold: f32,
}

/// How far the holding hand must come from where it took the ripcord to pull it, metres (see
/// [`crate::controllers::vehicles::ripcord`]).
pub const RIPCORD_PULL_M: f32 = 0.15;

/// The helicopter skid's fixed tuning.
pub const RAIL: RailTuning = RailTuning {
    half_length_m: 1.0,
    step_off_m: 0.35,
    pull_s: 0.25,
};

/// The helicopter skid (see [`crate::controllers::vehicles::rail`]).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RailTuning {
    /// How far the skid's line reaches either way from where Rico lands on it, metres.
    pub half_length_m: f32,
    /// How far off the line, level, the feet go before Rico has stepped off, metres.
    pub step_off_m: f32,
    /// The sticky pull's time constant: the offset off the line closes at this rate, seconds.
    /// Longer is less sticky.
    pub pull_s: f32,
}
