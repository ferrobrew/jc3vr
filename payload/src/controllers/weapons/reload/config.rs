//! The true reload's settings (see [`crate::controllers::weapons::reload`]), held in
//! [`WeaponsConfig::reload`](crate::controllers::config::WeaponsConfig::reload).

use glam::Vec3;
use serde::{Deserialize, Serialize};

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
    /// Ejected magazines that stay in the world with their rounds.
    pub loose: LooseMagazineConfig,
}

impl ReloadConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            button: ReloadButton::Eject,
            tap_s: 0.3,
            flick_eject: false,
            flick_speed_dps: 900.0,
            flick_cooldown_s: 0.6,
            well_offset_m: Vec3::new(0.0, -0.08, -0.06),
            held_top_m: 0.07,
            capture_radius_m: 0.05,
            capture_angle_deg: 50.0,
            seat_depth_m: 0.025,
            timeout_s: 8.0,
            show_well: false,
            belt: BeltConfig::new(),
            chamber: true,
            manual_action: true,
            action: ActionConfig::new(),
            dual_split: true,
            loose: LooseMagazineConfig::new(),
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

/// Magazines in the world (see [`crate::controllers::weapons::reload::loose`]). Acts only under the
/// controller scheme with the reload on, so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LooseMagazineConfig {
    /// An ejected magazine keeps the rounds it held as an object in the world, and a dropped
    /// magazine that fits the held gun can be picked up and seated with its rounds.
    pub enabled: bool,
    /// The radius of a dropped magazine's grab volume, metres.
    pub radius_m: f32,
    /// How far from the player a dropped magazine may be to be picked up, metres.
    pub range_m: f32,
    /// How far behind its top a held magazine's centre is, metres: the hand carries the magazine's
    /// body there.
    pub half_length_m: f32,
    /// The fastest a held magazine is moved toward the hand, metres per second.
    pub max_carry_mps: f32,
    /// How long a dropped magazine is remembered, seconds.
    pub keep_s: f32,
}

impl LooseMagazineConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            radius_m: 0.1,
            range_m: 2.0,
            half_length_m: 0.06,
            max_carry_mps: 20.0,
            keep_s: 300.0,
        }
    }
}

impl Default for LooseMagazineConfig {
    fn default() -> Self {
        Self::new()
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
    /// The action is a part that follows the hand through its travel: a slide or a charging handle
    /// runs home on its spring when let go, a slide catches at the rear on an empty magazine, a
    /// pump stays where it is left, and a bolt turns up before it slides. Off, the action is a
    /// gesture: a pull to the pull share and a release or a push home.
    pub physical: bool,
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
    /// How long a released slide or charging handle takes to run home from the rear on its
    /// spring, seconds.
    pub spring_s: f32,
    /// A slide catches at the rear when the magazine in the gun is empty: after the last shot, or
    /// racked over an empty magazine. It stays back until it is racked again.
    pub lock_back: bool,
    /// How far a bolt's handle turns up to unlock it, degrees.
    pub bolt_lift_deg: f32,
    /// How far a bolt's handle is from the bolt's axis, metres.
    pub bolt_radius_m: f32,
    /// The share of the bolt's turn that counts as turned all the way, `0.5..=1`; the turn down
    /// counts once it is within the same share of the bottom.
    pub lift_fraction: f32,
}

impl ActionConfig {
    pub const fn new() -> Self {
        Self {
            grab_radius_m: 0.07,
            pull_fraction: 0.8,
            return_fraction: 0.3,
            release_distance_m: 0.15,
            default_travel_m: 0.06,
            physical: true,
            spring_s: 0.05,
            lock_back: true,
            bolt_lift_deg: 60.0,
            bolt_radius_m: 0.03,
            lift_fraction: 0.85,
        }
    }
}

impl Default for ActionConfig {
    fn default() -> Self {
        Self::new()
    }
}
