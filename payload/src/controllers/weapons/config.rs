//! The weapon-handling settings (the holsters, the foregrip, the recoil, the haptics, the
//! dual-wield split, the grenade throw, melee strikes, and the magazine gesture, and the scopes), held in [`WeaponsConfig`](crate::controllers::config::WeaponsConfig) beside
//! the other weapon-handling subsystems' toggles.

use glam::Vec3;
use serde::{Deserialize, Serialize};

/// The body holsters and the grenade pouch (see [`crate::controllers::weapons`]). They act only
/// under the controller scheme, so they default on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HolstersConfig {
    /// Draw a weapon with a grip press at its holster, and holster the held weapon with a grip
    /// press at its own holster.
    pub enabled: bool,
    /// Keep the move stick's click cycling to the next weapon while the holsters are on, as a
    /// fallback for players who cannot reach them (seated, the estimated hip height can put the
    /// hip holsters out of reach). Off the holsters, the click always cycles. Under the
    /// dual-wield split the click retracts the tethers instead.
    pub stick_click_cycles: bool,
    /// The pistol holsters on both hips: the dual-wield slot.
    pub hip: VolumeSpec,
    /// The holsters over the shoulders: the two-handed slot over the dominant shoulder and the
    /// special slot over the other.
    pub shoulder: VolumeSpec,
    /// The explosives holster on the chest.
    pub chest: VolumeSpec,
    /// The grenade pouch at the front of the belt on the dominant side, a grab volume for the
    /// dominant hand while the physical grenade throw
    /// ([`WeaponsConfig::grenade_throw`](crate::controllers::config::WeaponsConfig::grenade_throw))
    /// is on.
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
            stick_click_cycles: true,
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

/// Controller vibration (see [`crate::controllers::weapons::haptics`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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
#[serde(default)]
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
    /// Aim the game's own throw (the stick-click fallback) at the point the dominant hand's ray
    /// reaches rather than at the gaze.
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
        }
    }
}

impl Default for GrenadeConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The strike detector for physical melee (see [`crate::controllers::weapons::melee`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MeleeStrikeConfig {
    /// How fast a hand must move to strike, metres per second.
    pub strike_speed_mps: f32,
    /// How slow the hand must get before it can strike again, metres per second.
    pub rearm_speed_mps: f32,
    /// The least share of a strike's speed that must point away from the shoulder, `-1..=1`, so
    /// that pulling a hand back toward the body (a yank) is not a strike.
    pub min_outward: f32,
    /// The shortest time between two strikes of one hand, seconds.
    pub cooldown_s: f32,
    /// For how many input ticks after a strike the melee target the strike chose is held in the
    /// aim control's melee slot, so that the game's melee reads it whichever tick it runs on.
    pub target_hold_ticks: u32,
    /// Also consider the target the striking hand points at, not only the one it moves toward.
    pub pointing_targets: bool,
    /// A pulse on the striking hand when a strike starts a melee attack.
    pub pulse: PulseSpec,
}

impl MeleeStrikeConfig {
    pub const fn new() -> Self {
        Self {
            strike_speed_mps: 3.0,
            rearm_speed_mps: 1.2,
            min_outward: 0.3,
            cooldown_s: 0.6,
            target_hold_ticks: 4,
            pointing_targets: true,
            pulse: PulseSpec {
                amplitude: 0.6,
                duration_s: 0.05,
            },
        }
    }
}

impl Default for MeleeStrikeConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The magazine reload gesture (see [`crate::controllers::weapons::magazine`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MagazineConfig {
    /// The magazine pouch at the front of the belt on the off-hand's side, placed like the grenade
    /// pouch, a grab volume for the off-hand. Given for the right side and mirrored for the left.
    pub pouch: VolumeSpec,
    /// The magazine well's centre in the holding hand's wrist frame (-Z toward the fingers, +Y out
    /// of the back of the hand), metres: under the gun, just ahead of the grip. Given for the right
    /// hand and mirrored for the left.
    pub well_offset_m: Vec3,
    /// How close the off-hand's grip must come to the well to insert the magazine, metres.
    pub well_radius_m: f32,
    /// How long a magazine taken from the pouch stays in the hand before it is dropped, seconds.
    pub timeout_s: f32,
    /// A pulse on the off-hand when it takes a magazine and when it inserts one.
    pub pulse: PulseSpec,
}

impl MagazineConfig {
    pub const fn new() -> Self {
        Self {
            pouch: VolumeSpec {
                offset_m: Vec3::new(0.02, 0.04, -0.12),
                radius_m: 0.08,
            },
            well_offset_m: Vec3::new(0.0, -0.08, -0.06),
            well_radius_m: 0.08,
            timeout_s: 4.0,
            pulse: PulseSpec {
                amplitude: 0.35,
                duration_s: 0.04,
            },
        }
    }
}

impl Default for MagazineConfig {
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
