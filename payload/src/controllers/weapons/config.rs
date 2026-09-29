//! The weapon-handling settings (the holsters, the foregrip, the recoil, the haptics, and the
//! dual-wield split), held in [`WeaponsConfig`](crate::controllers::config::WeaponsConfig) beside
//! the other weapon-handling subsystems' toggles.

use glam::Vec3;
use serde::{Deserialize, Serialize};

/// The body holsters and the grenade pouch (see [`crate::controllers::weapons`]). They act only
/// under the controller scheme, so they default on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HolstersConfig {
    /// Draw a weapon with a grip press at its holster, and holster the held weapon with a grip
    /// press at its own holster.
    pub enabled: bool,
    /// Keep the move stick's click cycling to the next weapon while the holsters are on, as a
    /// fallback for players who cannot reach them. Off the holsters, the click always cycles.
    pub stick_click_cycles: bool,
    /// The pistol holsters on both hips: the dual-wield slot.
    pub hip: VolumeSpec,
    /// The holsters over the shoulders: the two-handed slot over the dominant shoulder and the
    /// special slot over the other.
    pub shoulder: VolumeSpec,
    /// The explosives holster on the chest.
    pub chest: VolumeSpec,
    /// Offer the grenade pouch as a grab volume for the dominant hand. Off by default until the
    /// physical throw exists: a grab there does nothing yet, and it would swallow a grip press
    /// there.
    pub grenade_pouch: bool,
    /// The grenade pouch at the front of the belt on the dominant side.
    pub pouch: VolumeSpec,
    /// How far the head may drop below its standing height before the holsters drop with it, metres.
    /// Looking down at a holster lowers the head a little without the body crouching, so a small
    /// drop leaves the volumes where they are; a crouch carries them down.
    pub crouch_deadband_m: f32,
}

impl HolstersConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            stick_click_cycles: false,
            hip: VolumeSpec {
                offset_m: Vec3::new(0.04, 0.02, 0.0),
                radius_m: 0.10,
            },
            shoulder: VolumeSpec {
                offset_m: Vec3::new(0.0, 0.10, 0.10),
                radius_m: 0.13,
            },
            chest: VolumeSpec {
                offset_m: Vec3::new(0.0, -0.18, -0.14),
                radius_m: 0.10,
            },
            grenade_pouch: false,
            pouch: VolumeSpec {
                offset_m: Vec3::new(-0.10, 0.04, -0.12),
                radius_m: 0.08,
            },
            crouch_deadband_m: 0.15,
        }
    }
}

impl Default for HolstersConfig {
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
    /// line between them is too short to aim along, and the weapon stays on the dominant hand.
    pub min_span_m: f32,
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

/// Controller vibration (see [`crate::controllers::weapons::haptics`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HapticsConfig {
    /// Off, every queued pulse is dropped.
    pub enabled: bool,
    /// Scales every pulse's amplitude, `0..=1`.
    pub intensity: f32,
    /// Play the game's own rumbles (weapon fire, damage, impacts, and the rest) on the
    /// controllers, each on the hand that caused it.
    pub game_rumble: bool,
    /// How much a shot's recoil kick adds to its pulse's amplitude, per radian of kick.
    pub kick_amplitude_per_rad: f32,
    /// The longest pulse a game rumble plays for, seconds: the game's rumbles can last long enough
    /// to blur into one another on a controller.
    pub max_pulse_s: f32,
    /// Pulse the grapple hand when the grapple fires and when its hook attaches.
    pub grapple: bool,
    pub grapple_fire: PulseSpec,
    pub grapple_attach: PulseSpec,
    /// Pulse the grapple hand when a yank is detected.
    pub yank: PulseSpec,
}

impl HapticsConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            intensity: 1.0,
            game_rumble: true,
            kick_amplitude_per_rad: 6.0,
            max_pulse_s: 0.4,
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
pub struct DualWieldConfig {
    /// Fire each pistol with its own hand's trigger and aim it with its own hand's ray. While it
    /// acts, the off-hand's trigger fires the off-hand pistol and the off-hand's stick click
    /// retracts the tethers instead.
    pub split: bool,
    /// Put the first pistol in the dominant hand and the second in the other, moving the pistols'
    /// attach bones when the game's layout has them the other way round.
    pub place_by_handedness: bool,
}

impl DualWieldConfig {
    pub const fn new() -> Self {
        Self {
            split: true,
            place_by_handedness: true,
        }
    }
}

impl Default for DualWieldConfig {
    fn default() -> Self {
        Self::new()
    }
}
