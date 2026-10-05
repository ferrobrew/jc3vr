//! The true reload's settings (see [`crate::controllers::weapons::reload`]), held in
//! [`WeaponsConfig::reload`](crate::controllers::config::WeaponsConfig::reload).

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::controllers::weapons::config::{PulseSpec, VolumeSpec};

/// The true reload. Acts only under the controller scheme, so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReloadConfig {
    /// Reload by hand: the game's own reloads of the weapons the mod reloads are stopped, and a
    /// magazine is ejected, taken from the belt pouch, seated in the well, and chambered by hand.
    /// Off, the game's timed reload plays from the reload button as it does on a gamepad.
    pub enabled: bool,
    /// What a tap of the reload button does.
    pub button: ReloadButton,
    /// The longest press of the reload button that counts as a tap, seconds. The game holsters on
    /// a longer hold of the same button, so a hold never ejects.
    pub tap_s: f32,
    /// Eject a gun's magazine with a sharp flick of the hand holding it.
    pub flick_eject: bool,
    /// The angular speed of the gun hand that counts as a flick, degrees per second.
    pub flick_speed_dps: f32,
    /// The shortest time between two flick ejects, seconds.
    pub flick_cooldown_s: f32,
    /// The universal pouch at the front of the belt on the dominant side. It hands out what the
    /// held weapon takes: a magazine, a shell, a rocket, or a pod. Given for the right side and
    /// mirrored for the left.
    pub pouch: VolumeSpec,
    /// The magazine well of a weapon the wells table has no entry for, in the holding hand's wrist
    /// frame (-Z toward the fingers, +Y out of the back of the hand), metres: under the gun, just
    /// ahead of the grip. Given for the right hand and mirrored for the left.
    pub well_offset_m: Vec3,
    /// How far ahead of the hand's grip the top of a held magazine sits, metres, along the
    /// direction the closed hand points.
    pub held_top_m: f32,
    /// How close the top of a held magazine must come to the well's mouth to catch it, metres.
    pub capture_radius_m: f32,
    /// How far a held magazine may be turned from the well's axis and still catch, degrees.
    pub capture_angle_deg: f32,
    /// How far past the mouth a caught magazine travels to seat, metres.
    pub seat_depth_m: f32,
    /// How long a magazine taken from the pouch stays in the hand before it is dropped, seconds.
    pub timeout_s: f32,
    /// Draw a ring at the well's mouth while a magazine is in the hand.
    pub show_well: bool,
    /// The one-handed belt reload: push the gun's well down onto the pouch.
    pub belt: BeltConfig,
    /// Require the action to be worked by hand after the gun runs dry: the slide, the charging
    /// handle, the bolt, or the pump. A tactical reload (an eject with rounds left) skips it, and
    /// the belt reload works the action itself.
    pub chamber: bool,
    /// Require the bolt of a bolt-action rifle and the pump of a pump-action shotgun to be worked
    /// by hand after every shot, in place of the game's own cycling.
    pub manual_action: bool,
    /// The slide, charging handle, bolt, and pump.
    pub action: ActionConfig,
    /// Give each dual-wielded pistol its own magazine: half the weapon's, run dry, ejected, and
    /// reloaded on its own.
    pub dual_split: bool,
    pub haptics: ReloadHaptics,
}

impl ReloadConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            button: ReloadButton::Eject,
            tap_s: 0.3,
            flick_eject: true,
            flick_speed_dps: 900.0,
            flick_cooldown_s: 0.6,
            pouch: VolumeSpec {
                offset_m: Vec3::ZERO,
                radius_m: 0.09,
            },
            well_offset_m: Vec3::new(0.0, -0.08, -0.06),
            held_top_m: 0.07,
            capture_radius_m: 0.05,
            capture_angle_deg: 50.0,
            seat_depth_m: 0.025,
            timeout_s: 8.0,
            show_well: true,
            belt: BeltConfig::new(),
            chamber: true,
            manual_action: true,
            action: ActionConfig::new(),
            dual_split: true,
            haptics: ReloadHaptics::new(),
        }
    }
}

impl Default for ReloadConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// What a tap of the reload button does under the true reload.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ReloadButton {
    /// Eject the magazine of the dominant gun, or of both dual-wielded pistols.
    #[default]
    Eject,
    /// The game's own timed reload, as on a gamepad: a fallback for players who cannot reload by
    /// hand. The pouch, the flick, and the belt still work.
    Native,
}

impl ReloadButton {
    pub const ALL: [ReloadButton; 2] = [ReloadButton::Eject, ReloadButton::Native];

    pub const fn label(self) -> &'static str {
        match self {
            ReloadButton::Eject => "eject the magazine",
            ReloadButton::Native => "the game's reload",
        }
    }
}

/// The one-handed belt reload (see [`crate::controllers::weapons::reload::belt`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BeltConfig {
    pub enabled: bool,
    /// How far the gun's insertion axis may lean from straight up, degrees: the well must face the
    /// belt.
    pub max_tilt_deg: f32,
    /// How fast the well must be moving down as it enters the pouch, metres per second: a push,
    /// not a hover.
    pub min_push_speed_mps: f32,
    /// How far into the pouch the well must be pushed, metres.
    pub depth_m: f32,
    /// How long after a draw the belt reload waits, seconds, so that drawing from a hip holster
    /// beside the pouch does not reload.
    pub after_draw_s: f32,
}

impl BeltConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            max_tilt_deg: 45.0,
            min_push_speed_mps: 0.3,
            depth_m: 0.03,
            after_draw_s: 0.8,
        }
    }
}

impl Default for BeltConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The slide, charging handle, bolt, and pump (see
/// [`crate::controllers::weapons::reload::chamber`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ActionConfig {
    /// How close the off-hand's grip must be to the handle to grab it, metres.
    pub grab_radius_m: f32,
    /// The share of the action's travel a pull must reach, `0..=1`.
    pub pull_fraction: f32,
    /// How close to its rest a pulled action must come back to count as returned, as a share of
    /// its travel.
    pub return_fraction: f32,
    /// How far the hand may stray from the gun's axis before the grab lets go, metres.
    pub release_distance_m: f32,
    /// The travel of an action the wells table gives none for, metres.
    pub default_travel_m: f32,
}

impl ActionConfig {
    pub const fn new() -> Self {
        Self {
            grab_radius_m: 0.07,
            pull_fraction: 0.8,
            return_fraction: 0.3,
            release_distance_m: 0.15,
            default_travel_m: 0.06,
        }
    }
}

impl Default for ActionConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The reload's pulses.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReloadHaptics {
    /// On the gun hand at an eject.
    pub eject: PulseSpec,
    /// On the hand that takes a magazine from the pouch.
    pub pouch_take: PulseSpec,
    /// Twice on a hand that reaches into the pouch with no ammunition left.
    pub pouch_empty: PulseSpec,
    /// On both hands when a magazine catches the well.
    pub capture: PulseSpec,
    /// On both hands when a magazine seats.
    pub seat: PulseSpec,
    /// On the gun hand when a belt push seats a magazine.
    pub belt_seat: PulseSpec,
    /// On the working hand when a pull reaches the action's travel.
    pub rack: PulseSpec,
    /// On the gun hand when the action closes on a round.
    pub chambered: PulseSpec,
}

impl ReloadHaptics {
    pub const fn new() -> Self {
        Self {
            eject: PulseSpec {
                amplitude: 0.4,
                duration_s: 0.02,
            },
            pouch_take: PulseSpec {
                amplitude: 0.25,
                duration_s: 0.02,
            },
            pouch_empty: PulseSpec {
                amplitude: 0.2,
                duration_s: 0.012,
            },
            capture: PulseSpec {
                amplitude: 0.3,
                duration_s: 0.015,
            },
            seat: PulseSpec {
                amplitude: 0.7,
                duration_s: 0.04,
            },
            belt_seat: PulseSpec {
                amplitude: 0.85,
                duration_s: 0.05,
            },
            rack: PulseSpec {
                amplitude: 0.5,
                duration_s: 0.025,
            },
            chambered: PulseSpec {
                amplitude: 0.35,
                duration_s: 0.02,
            },
        }
    }
}

impl Default for ReloadHaptics {
    fn default() -> Self {
        Self::new()
    }
}
