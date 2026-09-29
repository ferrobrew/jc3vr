//! The holster and foregrip settings, held in
//! [`WeaponsConfig`](crate::controllers::config::WeaponsConfig) beside the other weapon-handling
//! subsystems' toggles.

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
