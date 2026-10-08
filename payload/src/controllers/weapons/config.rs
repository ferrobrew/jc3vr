//! The weapon-handling settings (the holsters, the pouches, the foregrip, the recoil, the weight,
//! the haptics, the dual-wield split, the grenade throw, the C4 throw, and the scopes), held in
//! [`WeaponsConfig`](crate::controllers::config::WeaponsConfig) beside the other weapon-handling
//! subsystems' toggles. The reload's are in [`crate::controllers::weapons::reload::config`].

use glam::Vec3;
use serde::{Deserialize, Serialize};

/// The body holsters (see [`crate::controllers::weapons`]). They act only under the controller
/// scheme, so they default on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HolstersConfig {
    /// Draw a weapon with a grip press at its holster, and holster the held weapon with a grip
    /// press at its own holster.
    pub enabled: bool,
    /// Keep the move stick's click cycling to the next weapon while the holsters are on, as a
    /// fallback for players who cannot reach them. Off the holsters, the click always cycles.
    /// Under the dual-wield split the click retracts the tethers instead.
    pub stick_click_cycles: bool,
    /// The pistol holsters on both hips: the dual-wield slot.
    pub hip: VolumeSpec,
    /// The holsters over the shoulders: the two-handed slot over the dominant shoulder and the
    /// special slot over the other.
    pub shoulder: VolumeSpec,
    /// The explosives holster on the chest.
    pub chest: VolumeSpec,
    /// How far the head may drop below its standing height before the holsters drop with it, metres.
    /// Looking down at a holster lowers the head a little without the body crouching, so a small
    /// drop leaves the volumes where they are; a crouch carries them down.
    pub crouch_deadband_m: f32,
    /// Draw every body volume, the holsters as well as the pouches, through the world-space overlay
    /// at all times, so that their placement can be seen and tuned. Needs the overlay on.
    pub show_volumes: bool,
    /// Which body the holsters and pouches ride.
    pub anchor: HolsterAnchor,
}

/// Which body the holsters and pouches ride (see [`crate::controllers::weapons::volumes`]).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HolsterAnchor {
    /// The player's own body, estimated from the player's body dimensions and turned to the player's
    /// estimated torso, and reached by the player's own hands, before the body-scale remap.
    #[default]
    Body,
    /// The character's skeleton: his hip and shoulder joints, so that the volumes sit on the body
    /// the player sees. Falls back to the player's body where the skeleton is unavailable or not
    /// upright: in a vehicle, in a ragdoll, or in a skydive.
    Rico,
}

impl HolsterAnchor {
    pub const ALL: [HolsterAnchor; 2] = [HolsterAnchor::Body, HolsterAnchor::Rico];

    pub const fn label(self) -> &'static str {
        match self {
            HolsterAnchor::Body => "the player's body",
            HolsterAnchor::Rico => "Rico's skeleton",
        }
    }
}

impl HolstersConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            stick_click_cycles: true,
            hip: VolumeSpec {
                offset_m: Vec3::new(0.04, 0.04, 0.0),
                radius_m: 0.12,
            },
            shoulder: VolumeSpec {
                offset_m: Vec3::new(0.0, 0.10, 0.10),
                radius_m: 0.17,
            },
            chest: VolumeSpec {
                offset_m: Vec3::new(0.0, -0.16, -0.14),
                radius_m: 0.09,
            },
            crouch_deadband_m: 0.15,
            show_volumes: false,
            anchor: HolsterAnchor::Body,
        }
    }
}

impl Default for HolstersConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The pouches on the front of the body (see [`crate::controllers::weapons::volumes`]): the
/// magazine pouch at the front of the belt on the free hand's side, and the grenade and C4 pouches on
/// either side of the chest. Each is placed only while its feature is on: the grenade pouch with
/// the grenade throw, the C4 pouch with the C4 throw, and the magazine pouch while the true reload
/// reloads the held weapon. Offsets are given for the right side and mirrored for the left.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PouchesConfig {
    /// Draw each placed pouch faintly through the world-space overlay at all times, and brighter
    /// while a hand is within reach of it, so that their places can be learned. Needs the overlay
    /// on.
    pub show: bool,
    /// Which side of the chest the grenade pouch sits on. The C4 pouch takes the other side.
    pub grenade_side: PouchSide,
    /// The grenade pouch, offset from the chest's front on its side.
    pub grenade: VolumeSpec,
    /// The C4 pouch, offset from the chest's front on its side.
    pub c4: VolumeSpec,
    /// The universal reload pouch, offset from the belt's front on the free hand's side. It hands out
    /// what the held weapon takes: a magazine, a shell, a rocket, or a pod.
    pub magazine: VolumeSpec,
    /// How far outside a pouch's surface a grip press still takes from it, metres, when the hand
    /// is in no other volume: a hand feeling for a pouch lands a little off it, and a press there
    /// must not fire the grapple instead.
    pub reach_margin_m: f32,
}

/// Which side of the chest a pouch sits on, by the player's hands: by the hand the guns ride
/// ([`crate::controllers::weapons::volumes::gun_side`]), which is the dominant hand unless a gun
/// rides the other.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PouchSide {
    /// The gun hand's side.
    #[default]
    Dominant,
    /// The free hand's side.
    OffHand,
}

impl PouchSide {
    pub const ALL: [PouchSide; 2] = [PouchSide::Dominant, PouchSide::OffHand];

    pub const fn label(self) -> &'static str {
        match self {
            PouchSide::Dominant => "gun hand's side",
            PouchSide::OffHand => "free hand's side",
        }
    }
}

impl PouchesConfig {
    pub const fn new() -> Self {
        // Each pouch's centre is a few centimetres out from the body's surface, where a hand laid
        // flat on the body closes.
        let pouch = VolumeSpec {
            offset_m: Vec3::new(0.0, 0.0, -0.04),
            radius_m: 0.075,
        };
        Self {
            show: true,
            grenade_side: PouchSide::Dominant,
            grenade: pouch,
            c4: pouch,
            magazine: pouch,
            reach_margin_m: 0.05,
        }
    }
}

impl Default for PouchesConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// A spherical grab volume placed relative to a body landmark (see
/// [`crate::controllers::weapons::volumes`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VolumeSpec {
    /// The centre's offset from the landmark in the body frame (+X right, +Y up, +Z back), metres,
    /// given for the right side and mirrored across the body's midline for the left.
    pub offset_m: Vec3,
    pub radius_m: f32,
}

/// The two-handed weapon's foregrip (see [`crate::controllers::weapons::foregrip`]). Acts only under
/// the controller scheme, so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ForegripConfig {
    /// Let the off-hand grab a two-handed weapon's foregrip, which then sets the weapon's direction.
    pub enabled: bool,
    /// The foregrip volume's radius around the weapon's grip point, metres.
    pub radius_m: f32,
    /// How far the off-hand may move from the foregrip before it lets go, metres.
    pub release_distance_m: f32,
    /// How long the weapon takes to turn onto the line between the hands after a grab, and back
    /// onto the dominant hand after a release, seconds.
    pub blend_s: f32,
    /// Where the foregrip sits along the barrel from the dominant wrist, metres, for a weapon with
    /// no grip point of its own for the off-hand.
    pub fallback_distance_m: f32,
    /// The shortest distance between the hands that still sets a direction, metres. Closer, the
    /// line between them is too short to aim along, and the weapon stays on the dominant hand; the
    /// two-handed pose fades out over the next `min_span_m` as the hands close in, so the weapon
    /// does not jump between the two.
    pub min_span_m: f32,
    /// The furthest the line between the hands may turn the weapon away from where the holding
    /// hand points it, degrees. Past it the weapon stops turning, the off-hand falls away from
    /// the foregrip, and the hold lets go once that passes the release distance, so an off-hand
    /// that crosses behind the holding hand cannot spin the weapon round.
    pub max_turn_deg: f32,
    /// Hold the foregrip only while the off-hand's grip is held, letting go when it opens, as a
    /// real foregrip is held. Off, the grip is a toggle: a press grabs and the next press lets go.
    pub hold: bool,
}

impl ForegripConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            radius_m: 0.10,
            release_distance_m: 0.25,
            blend_s: 0.12,
            fallback_distance_m: 0.32,
            min_span_m: 0.08,
            max_turn_deg: 70.0,
            hold: true,
        }
    }
}

impl Default for ForegripConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The recoil offset (see [`crate::controllers::weapons::recoil`]): each shot kicks the gun hand
/// away from the controller pose, and a damped spring returns it. Acts only under the controller
/// scheme, so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecoilConfig {
    pub enabled: bool,
    /// Scales the kick taken from the game's per-shot recoil.
    pub kick_scale: f32,
    /// How far the gun moves back along the grip per radian of pitch kick, metres.
    pub kick_back_m_per_rad: f32,
    /// The largest rotation the offset reaches, degrees, however fast the shots come.
    pub max_angle_deg: f32,
    /// The return spring's stiffness, per second squared.
    pub spring_stiffness: f32,
    /// The return spring's damping ratio: `1` is critically damped, below `1` overshoots.
    pub spring_damping_ratio: f32,
    /// Multiplies the stiffness while the off-hand holds the foregrip.
    pub two_hand_stiffness_scale: f32,
    /// Multiplies the kick while the off-hand holds the foregrip.
    pub two_hand_kick_scale: f32,
    /// Keep the game's weapon camera modifier from turning the camera by the recoil (and from its
    /// snap-aim assist) while the guns aim by hand: the camera belongs to the headset.
    pub suppress_camera_kick: bool,
}

impl RecoilConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            kick_scale: 1.0,
            kick_back_m_per_rad: 0.5,
            max_angle_deg: 25.0,
            spring_stiffness: 400.0,
            spring_damping_ratio: 0.8,
            two_hand_stiffness_scale: 2.0,
            two_hand_kick_scale: 0.5,
            suppress_camera_kick: true,
        }
    }
}

impl Default for RecoilConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The weight of a long weapon (see [`crate::controllers::weapons::heft`]): held in one hand, a
/// long gun sags, trails the hand on a spring, and sways, its kick grows, and its spread widens,
/// all scaled by the weapon's class; braced on the foregrip, it keeps a lighter weight of its own
/// ([`BracedHeftConfig`]). Acts only under the controller scheme, so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HeftConfig {
    /// The one-handed weight.
    pub enabled: bool,
    /// Widen the game's own shot scatter while the weapon is held in one hand.
    pub spread: bool,
    /// The hand's distance from the head at which the reach sag starts, metres.
    pub reach_near_m: f32,
    /// The hand's distance from the head at which the reach sag is full, metres.
    pub reach_far_m: f32,
    /// The sway's frequency, hertz; the two axes run at incommensurate multiples of it.
    pub sway_hz: f32,
    /// The furthest the weapon may trail or sag away from the controller, degrees.
    pub max_angle_deg: f32,
    /// Pistols, revolvers, and SMGs.
    pub sidearm: HeftTuning,
    /// Assault rifles, shotguns, and sniper rifles.
    pub rifle: HeftTuning,
    /// Rocket and grenade launchers, miniguns, and the other heavy and special weapons.
    pub heavy: HeftTuning,
    /// The weight of a gun braced in both hands.
    pub braced: BracedHeftConfig,
}

impl HeftConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            spread: true,
            reach_near_m: 0.3,
            reach_far_m: 0.65,
            sway_hz: 0.35,
            max_angle_deg: 35.0,
            sidearm: HeftTuning::NONE,
            rifle: HeftTuning {
                sag_deg: 5.0,
                reach_sag_deg: 7.0,
                lag_stiffness: 260.0,
                lag_damping_ratio: 0.75,
                sway_deg: 0.6,
                kick_scale: 1.3,
                spread_scale: 1.6,
            },
            heavy: HeftTuning {
                sag_deg: 14.0,
                reach_sag_deg: 14.0,
                lag_stiffness: 70.0,
                lag_damping_ratio: 0.65,
                sway_deg: 1.5,
                kick_scale: 1.8,
                spread_scale: 2.5,
            },
            braced: BracedHeftConfig::new(),
        }
    }
}

impl Default for HeftConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The weight of a long gun braced in both hands, on the foregrip (see
/// [`crate::controllers::weapons::heft`]): the gun follows where the hands turn it on a critically
/// damped spring, so it lags a quick swing a little and settles on the aim without overshooting,
/// and it sags a little. The hands are carried with the gun, so they stay on it. Pistols and
/// revolvers carry none. Acts only under the controller scheme, so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BracedHeftConfig {
    pub enabled: bool,
    /// How heavy a braced gun feels: one is the classes' tuning, zero carries no weight, and two
    /// lags twice as long and sags twice as far.
    pub strength: f32,
    /// The furthest a braced gun may lag or sag from where the hands turn it, degrees, so that the
    /// hands, carried with it, never stray far from the controllers.
    pub max_angle_deg: f32,
    /// SMGs, the lightest.
    pub smg: BracedTuning,
    /// Assault rifles, shotguns, and sniper rifles.
    pub rifle: BracedTuning,
    /// Launchers, miniguns, and the other heavy and special weapons, the heaviest.
    pub heavy: BracedTuning,
}

impl BracedHeftConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            strength: 1.0,
            max_angle_deg: 8.0,
            smg: BracedTuning {
                sag_deg: 0.5,
                stiffness: 5000.0,
            },
            rifle: BracedTuning {
                sag_deg: 1.5,
                stiffness: 1600.0,
            },
            heavy: BracedTuning {
                sag_deg: 3.0,
                stiffness: 500.0,
            },
        }
    }
}

impl Default for BracedHeftConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// One weapon class's braced weight.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BracedTuning {
    /// How far the barrel pitches down, degrees.
    pub sag_deg: f32,
    /// The stiffness of the critically damped spring the gun follows the hands on, per second
    /// squared. The higher, the less it lags: a swing at a steady speed trails by about twice the
    /// speed over the stiffness's square root. Zero follows rigidly.
    pub stiffness: f32,
}

impl BracedTuning {
    /// No weight at all: the braced gun follows the hands as they are.
    pub const NONE: Self = Self {
        sag_deg: 0.0,
        stiffness: 0.0,
    };

    /// This tuning at `strength`: the sag scaled by it, and the stiffness by its inverse square,
    /// so that the lag scales with it. Zero or less carries no weight.
    pub fn at_strength(self, strength: f32) -> Self {
        if !(strength.is_finite() && strength > 0.0) {
            return Self::NONE;
        }
        Self {
            sag_deg: self.sag_deg * strength,
            stiffness: self.stiffness / (strength * strength),
        }
    }
}

impl Default for BracedTuning {
    fn default() -> Self {
        Self::NONE
    }
}

/// One weapon class's one-handed weight.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HeftTuning {
    /// How far the weapon pitches down with the hand close to the body, degrees.
    pub sag_deg: f32,
    /// How much further it pitches down with the arm extended, degrees.
    pub reach_sag_deg: f32,
    /// The stiffness of the spring the weapon follows the hand's rotation on, per second squared.
    /// Zero follows the hand rigidly.
    pub lag_stiffness: f32,
    /// The spring's damping ratio: `1` is critically damped, below `1` overshoots.
    pub lag_damping_ratio: f32,
    /// The sway's amplitude, degrees.
    pub sway_deg: f32,
    /// Multiplies the recoil kick while the weapon is held in one hand.
    pub kick_scale: f32,
    /// Multiplies the game's shot scatter while the weapon is held in one hand.
    pub spread_scale: f32,
}

impl HeftTuning {
    /// No weight at all: the weapon follows the hand as it is.
    pub const NONE: Self = Self {
        sag_deg: 0.0,
        reach_sag_deg: 0.0,
        lag_stiffness: 0.0,
        lag_damping_ratio: 1.0,
        sway_deg: 0.0,
        kick_scale: 1.0,
        spread_scale: 1.0,
    };
}

impl Default for HeftTuning {
    fn default() -> Self {
        Self::NONE
    }
}

/// Controller vibration (see [`crate::controllers::weapons::haptics`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HapticsConfig {
    /// Off, every queued pulse is dropped.
    pub enabled: bool,
    /// Scales every pulse's amplitude, `0..=1`.
    pub intensity: f32,
    /// Forward every game rumble (weapon fire, damage, impacts, landings, vehicles, and the rest)
    /// to the controllers: a weapon's own fire rumble goes to the hand holding that gun, and the
    /// game's continuous left and right motor strength otherwise goes to the hands by side.
    pub game_rumble: bool,
    /// Also let the physical gamepad rumble under the controller scheme, instead of silencing it
    /// so only the controllers vibrate.
    pub gamepad_rumble: bool,
    /// How much a shot's recoil kick adds to its pulse's amplitude, per radian of kick.
    pub kick_amplitude_per_rad: f32,
    /// The longest pulse a one-off game rumble plays for, seconds: the game's rumbles can last long
    /// enough to blur into one another on a controller.
    pub max_pulse_s: f32,
    /// How long each continuous rumble's forwarded pulse plays before the next frame refreshes it,
    /// seconds: long enough to bridge a frame hitch without a gap, short enough to stop quickly once
    /// the game's rumble does.
    pub continuous_pulse_s: f32,
    /// Pulse the grapple hand when the grapple fires and when its hook attaches.
    pub grapple: bool,
    pub grapple_fire: PulseSpec,
    pub grapple_attach: PulseSpec,
    /// Pulse the grapple hand when a yank is detected.
    pub yank: PulseSpec,
    /// Pulse the hand whose gun is pulled while it is empty and cannot reload, once per pull.
    pub dry_fire: PulseSpec,
    /// Cue the grab volumes on the hands: a tick as a hand enters a holster or a pouch, and a
    /// firmer pulse on a draw, a holster, and a foregrip grab or release.
    pub volume_cues: bool,
    /// The tick as a hand enters a holster or a pouch.
    pub volume_enter: PulseSpec,
    /// The pulse on a draw, a holster, and a foregrip grab or release.
    pub grab: PulseSpec,
}

impl HapticsConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            intensity: 1.0,
            game_rumble: true,
            gamepad_rumble: false,
            kick_amplitude_per_rad: 6.0,
            max_pulse_s: 0.4,
            continuous_pulse_s: 0.1,
            grapple: true,
            grapple_fire: PulseSpec {
                amplitude: 0.35,
                duration_s: 0.04,
            },
            grapple_attach: PulseSpec {
                amplitude: 0.7,
                duration_s: 0.06,
            },
            yank: PulseSpec {
                amplitude: 0.5,
                duration_s: 0.05,
            },
            dry_fire: PulseSpec {
                amplitude: 0.3,
                duration_s: 0.015,
            },
            volume_cues: true,
            volume_enter: PulseSpec {
                amplitude: 0.12,
                duration_s: 0.008,
            },
            grab: PulseSpec {
                amplitude: 0.35,
                duration_s: 0.025,
            },
        }
    }
}

impl Default for HapticsConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// One fixed haptic pulse.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PulseSpec {
    /// `0..=1`.
    pub amplitude: f32,
    pub duration_s: f32,
}

/// Dual-wielded pistols (see [`crate::controllers::weapons::dual_wield`]). Acts only under the
/// controller scheme, so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DualWieldConfig {
    /// Fire each pistol with its own hand's trigger and aim it with its own hand's ray. While it
    /// acts, the off-hand's trigger fires the off-hand pistol and the off-hand's stick click
    /// retracts the tethers instead.
    pub split: bool,
    /// Put the first pistol in the dominant hand and the second in the other, moving the pistols'
    /// attach bones when the game's layout has them the other way round. Needs `split`.
    pub place_by_handedness: bool,
    /// Draw each pistol with its own hand: a hand grabbing a hip holster draws or holsters only its
    /// own pistol, and a pistol not drawn waits on the hip on its hand's side. Needs `split`, which
    /// keeps an undrawn pistol from firing.
    pub independent_draw: bool,
}

impl DualWieldConfig {
    pub const fn new() -> Self {
        Self {
            split: true,
            place_by_handedness: true,
            independent_draw: true,
        }
    }
}

impl Default for DualWieldConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Which hand holds the weapon (see [`crate::controllers::weapons::hold`]). Acts only under the
/// controller scheme.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HoldConfig {
    /// Hold the wielded weapon in the hand that drew it, picked it up, or was handed it, rather
    /// than always in the right hand. Only that hand's trigger fires it.
    pub either_hand: bool,
    /// Pass the held gun to the free hand with a grip press at the gun's grip. Needs
    /// `either_hand`.
    pub handover: bool,
    /// The radius of the gun's grip volume the free hand grabs to take the gun, metres.
    pub handle_radius_m: f32,
    /// Take a weapon off the ground straight into the grabbing hand, as the game picks a weapon
    /// up while swimming, with its magazine as it was and no pickup animation. Off, a grab clicks
    /// the game's use-item action and the game picks the weapon up its own way.
    pub direct_pickup: bool,
}

impl HoldConfig {
    pub const fn new() -> Self {
        Self {
            either_hand: true,
            handover: true,
            handle_radius_m: 0.07,
            direct_pickup: true,
        }
    }
}

impl Default for HoldConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The physical grenade throw (see [`crate::controllers::weapons::grenade`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GrenadeConfig {
    /// Where the held grenade sits in the throwing controller's grip frame, metres, given for the
    /// right hand and mirrored for the left. The grip's origin is the centre of the closed fist and
    /// its -Z runs out of the front of the fist.
    pub hold_offset_m: Vec3,
    /// How far back from the grip's release the throw's velocity is looked for, seconds.
    pub release_window_s: f32,
    /// The span around the fastest moment of the throw whose velocities are averaged, seconds on
    /// each side.
    pub release_peak_span_s: f32,
    /// Scales the hand's release velocity: `1` throws exactly as fast as the hand moved.
    pub velocity_scale: f32,
    /// The fastest a grenade may leave the hand, metres per second, against tracking glitches.
    pub max_speed_mps: f32,
    /// Aim the game's own throw at the point the dominant hand's ray reaches rather than at the
    /// gaze. No controller input binds the game's throw (grenades come from the pouch), so this
    /// matters only to a throw the game starts by other means.
    pub hand_aimed_fallback: bool,
    /// How long before the fuse runs out the held grenade starts to warn, seconds.
    pub warning_s: f32,
    /// Draw a ring at the throwing hand while a grenade is held, turning from green to red as the
    /// fuse runs down.
    pub warning_overlay: bool,
    /// Pulse the throwing hand as the fuse runs down, faster toward the end.
    pub warning_haptics: bool,
    pub warning_pulse: PulseSpec,
    /// The gap between warning pulses when the warning starts, and when the fuse is about to run
    /// out, seconds.
    pub warning_interval_s: [f32; 2],
    /// A pulse on the throwing hand when the grenade is primed in it.
    pub prime_pulse: PulseSpec,
    /// A buzz on the hand that grabbed the pouch when the game refuses it a grenade: none are
    /// left, grenades are not allowed now, or the game's own throw holds one.
    pub refused_pulse: PulseSpec,
}

impl GrenadeConfig {
    pub const fn new() -> Self {
        Self {
            hold_offset_m: Vec3::ZERO,
            release_window_s: 0.1,
            release_peak_span_s: 0.015,
            velocity_scale: 1.0,
            max_speed_mps: 30.0,
            hand_aimed_fallback: true,
            warning_s: 2.0,
            warning_overlay: true,
            warning_haptics: true,
            warning_pulse: PulseSpec {
                amplitude: 0.45,
                duration_s: 0.03,
            },
            warning_interval_s: [0.5, 0.08],
            prime_pulse: PulseSpec {
                amplitude: 0.3,
                duration_s: 0.04,
            },
            refused_pulse: PulseSpec {
                amplitude: 0.5,
                duration_s: 0.15,
            },
        }
    }
}

impl Default for GrenadeConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The throw of a charge from the C4 pouch (see [`crate::controllers::weapons::c4`]); the pouch
/// itself is placed with the others ([`PouchesConfig`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct C4Config {
    /// Where the held charge sits in the throwing controller's grip frame, metres, given for the
    /// right hand and mirrored for the left.
    pub hold_offset_m: Vec3,
    /// How far back from the grip's release the throw's velocity is looked for, seconds.
    pub release_window_s: f32,
    /// The span around the fastest moment of the throw whose velocities are averaged, seconds on
    /// each side.
    pub release_peak_span_s: f32,
    /// Scales the hand's release velocity: `1` throws exactly as fast as the hand moved.
    pub velocity_scale: f32,
    /// The fastest a charge may leave the hand, metres per second, against tracking glitches.
    pub max_speed_mps: f32,
    /// A pulse on the hand a charge is primed in.
    pub prime_pulse: PulseSpec,
    /// A pulse on the hand that throws a charge, as it leaves the hand.
    pub throw_pulse: PulseSpec,
    /// A buzz on the hand that grabbed the pouch when the game refuses it a charge: as many are
    /// placed as may be, or planting is not allowed now.
    pub refused_pulse: PulseSpec,
    /// A pulse on both hands when the placed charges detonate, from the wrist button or the stick
    /// click alike.
    pub detonate_pulse: PulseSpec,
    /// The detonator button on the off-hand's wrist.
    pub detonator: WristDetonatorConfig,
}

impl C4Config {
    pub const fn new() -> Self {
        Self {
            hold_offset_m: Vec3::ZERO,
            release_window_s: 0.1,
            release_peak_span_s: 0.015,
            velocity_scale: 1.0,
            max_speed_mps: 25.0,
            prime_pulse: PulseSpec {
                amplitude: 0.3,
                duration_s: 0.04,
            },
            throw_pulse: PulseSpec {
                amplitude: 0.4,
                duration_s: 0.03,
            },
            refused_pulse: PulseSpec {
                amplitude: 0.5,
                duration_s: 0.15,
            },
            detonate_pulse: PulseSpec {
                amplitude: 1.0,
                duration_s: 0.25,
            },
            detonator: WristDetonatorConfig::new(),
        }
    }
}

impl Default for C4Config {
    fn default() -> Self {
        Self::new()
    }
}

/// The detonator button on the inside of the off-hand's wrist, pressed by the dominant hand's
/// index fingertip (see [`crate::controllers::weapons::c4::press`]). The button's frame is the
/// off-hand's grip frame, given for the left hand (the off-hand of a right-handed player) and
/// mirrored for the right.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WristDetonatorConfig {
    /// The centre of the button's face, metres. The grip's origin is the centre of the closed
    /// fist, -Z runs out of the front of the fist and +Z back toward the wrist, and +X on the left
    /// hand points out of the palm.
    pub offset_m: Vec3,
    /// The direction out of the button's face, which the button is drawn facing. Normalised on
    /// use.
    pub normal: Vec3,
    /// The drawn button's radius, metres.
    pub radius_m: f32,
    /// How close to the button's centre the fingertip must come to touch it, metres.
    pub touch_radius_m: f32,
    /// How far from the button's centre a fingertip must move away before the button presses
    /// again, metres. More than `touch_radius_m`, so that a fingertip resting on the button does
    /// not press it twice.
    pub release_radius_m: f32,
    /// How far behind the face's plane a fingertip may be and still touch, metres: further behind,
    /// it has come through the wrist, which a real finger cannot.
    pub max_behind_m: f32,
    /// How fast the fingertip must be closing on the button as it touches it, metres per second,
    /// for the touch to press at once.
    pub min_entry_speed_mps: f32,
    /// How long a slower fingertip must keep touching to press, seconds.
    pub dwell_s: f32,
    /// Where the index fingertip is in the dominant controller's aim frame while the finger points
    /// (off the trigger), metres, given for the right hand and mirrored for the left. Used when the
    /// runtime reports no tracked hand skeleton.
    pub fingertip_pointing_m: Vec3,
    /// Where the index fingertip is in the dominant controller's aim frame while it rests on the
    /// trigger, metres, given for the right hand and mirrored for the left.
    pub fingertip_resting_m: Vec3,
    /// Draw the button on the wrist, facing out of it: dim with no charges placed, lit with a pip
    /// beside it for each placed charge, and flashing on a press and on the detonation.
    pub overlay: bool,
    /// A click on both hands when the button presses with charges placed.
    pub pulse: PulseSpec,
    /// A buzz on the pressing hand when the button presses with no charges placed.
    pub empty_pulse: PulseSpec,
}

impl WristDetonatorConfig {
    pub const fn new() -> Self {
        Self {
            offset_m: Vec3::new(0.03, 0.0, 0.08),
            normal: Vec3::X,
            radius_m: 0.02,
            touch_radius_m: 0.03,
            release_radius_m: 0.05,
            max_behind_m: 0.015,
            min_entry_speed_mps: 0.25,
            dwell_s: 0.15,
            fingertip_pointing_m: Vec3::new(0.0, -0.02, -0.04),
            fingertip_resting_m: Vec3::new(0.0, -0.04, 0.02),
            overlay: true,
            pulse: PulseSpec {
                amplitude: 0.6,
                duration_s: 0.03,
            },
            empty_pulse: PulseSpec {
                amplitude: 0.25,
                duration_s: 0.12,
            },
        }
    }
}

impl Default for WristDetonatorConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Scoped weapons (see [`crate::controllers::weapons::scope`]): the game's precision aim suppressed,
/// and the lens drawn on the scope while it is at an eye.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScopeConfig {
    /// Refuse the game's precision aim under the controller scheme, and leave it when the scheme
    /// takes over: its camera zoom, its sniper overlay, and its hiding of the character (and so of
    /// the hands) all fight the headset. Independent of the lens.
    pub suppress_precision_aim: bool,
    /// How the lens image is made.
    pub path: LensPath,
    /// Which eye's image the resampled lens reads.
    pub source: LensSource,
    /// The lens's magnification.
    pub zoom: f32,
    /// The lens's radius, metres.
    pub lens_radius_m: f32,
    /// How far behind the sight line's origin the eyepiece sits along the sight line, metres. The
    /// sight line's origin is above the muzzle (with the weapon's sight-table entry), so this is
    /// roughly the distance from the muzzle back to the scope's rear lens.
    pub eyepiece_back_m: f32,
    /// How close an eye must be to the eyepiece for the lens to draw, metres.
    pub max_eye_distance_m: f32,
    /// Where the dark rim starts, as a fraction of the lens radius.
    pub vignette_start: f32,
    /// Draw a crosshair at the lens's centre, which is where the sight line points.
    pub reticle: bool,
}

impl ScopeConfig {
    pub const fn new() -> Self {
        Self {
            suppress_precision_aim: true,
            path: LensPath::Resample,
            source: LensSource::OtherEye,
            zoom: 4.0,
            lens_radius_m: 0.02,
            eyepiece_back_m: 0.8,
            max_eye_distance_m: 0.15,
            vignette_start: 0.7,
            reticle: true,
        }
    }
}

impl Default for ScopeConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// How the lens image is made.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LensPath {
    /// A magnified resample of an eye's own rendered frame, around the sight line's projection.
    #[default]
    Resample,
    /// A narrow-field-of-view scene render of its own. Not implemented: selecting it uses the
    /// resample (see `docs/mod/input/controllers.md`, "Scopes").
    SceneRender,
}

/// Which eye's image the resampled lens reads.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LensSource {
    /// The other eye's image, which the scope does not block. In two-pass stereo the second eye's
    /// image is the previous frame's when the first eye sights, reprojected by its own camera.
    #[default]
    OtherEye,
    /// The sighting eye's own image, of this frame. The scope itself sits in front of that eye
    /// along the sight line, so the magnified region shows the scope's own model.
    OwnEye,
}
