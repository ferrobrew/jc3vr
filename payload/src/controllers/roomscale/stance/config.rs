//! The standing stance's settings, under `controllers.roomscale.stance`.

use serde::{Deserialize, Serialize};

/// Keeping the character's body in step with the player's. The unarmed body acts on foot whenever
/// the controllers drive the hands; the relaxed idle and the squaring act only while the character
/// stands still in the base idle state and is not aiming, holding a grapple tether, or crouched. All
/// of them act only under the controller scheme with the headset driving the view.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StanceConfig {
    /// The weapon-agnostic body: which of the game's weapon animations reach the body on foot.
    pub unarmed: UnarmedBodyConfig,
    /// Stage one: while standing still, keep the wielded weapon's animation modifiers off the body's
    /// rule system, so the idle plays its unarmed relaxed clip instead of the weapon's held stance.
    /// The upper-body graphs keep the modifiers. Off by default: the relaxed clip's arms and hand
    /// pose take no account of the weapon, which the controller hands cover only while tracked. The
    /// unarmed body ([`UnarmedBodyConfig`]) does the same on every on-foot tick, so this only
    /// matters with the weapon animations set to [`WeaponAnimations::Native`].
    pub relaxed_idle: bool,
    /// Stage two: while standing still, square the stance with HumanIK: the ankles placed side by
    /// side across the body's forward, the pelvis and chest turned to face it, and the feet turned
    /// to point along it. Off by default: it admits the legs to the body solve on every standing
    /// tick, and it has not been checked in the headset.
    pub square: bool,
    /// How long the character must have stood still before the squaring starts, seconds, so that a
    /// stop's settling animation is not fought.
    pub settle_s: f32,
    /// How long the squaring takes to blend in, seconds.
    pub blend_in_s: f32,
    /// How long the squaring takes to blend out when the character starts moving or another
    /// condition ends it, seconds.
    pub blend_out_s: f32,
    /// The horizontal speed below which the character counts as standing still, metres per second.
    /// The roomscale chase moves the body without leaving the idle state.
    pub still_speed_mps: f32,
    /// The distance between the ankles across the body, metres at the character's scale. Zero keeps
    /// the animated spacing and removes only the stagger.
    pub feet_width_m: f32,
    /// How much of the pelvis's turn away from the body's forward is removed, `0..=1`.
    pub pelvis_share: f32,
    /// How much of the chest's turn away from the body's forward is removed, `0..=1`. Carried by
    /// the body IK's torso rotation, so it needs the body IK on.
    pub chest_share: f32,
    /// How much of each foot's turn away from the body's forward is removed, `0..=1`.
    pub feet_yaw_share: f32,
    /// The resistance written for the ankles while their rotation is held, `0..=1`, so that a reach
    /// elsewhere in the body does not drag the planted feet.
    pub ankle_resist: f32,
}

impl StanceConfig {
    pub const fn new() -> Self {
        Self {
            unarmed: UnarmedBodyConfig::new(),
            relaxed_idle: false,
            square: false,
            settle_s: 0.4,
            blend_in_s: 0.5,
            blend_out_s: 0.15,
            still_speed_mps: 0.15,
            feet_width_m: 0.28,
            pelvis_share: 1.0,
            chest_share: 1.0,
            feet_yaw_share: 0.8,
            ankle_resist: 0.8,
        }
    }
}

impl Default for StanceConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Which of the game's weapon animations reach the character's body on foot.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum WeaponAnimations {
    /// The game's own: the weapon's stances, the upper-body actions, and the additive layers.
    Native,
    /// The body plays what it plays unarmed, whatever is held: the weapon's animation modifiers stay
    /// off every layer but the upper-body actions, and the layers set in [`UnarmedBodyConfig`] are
    /// left out of the pose while still running, so reloads, draws, and holsters keep their timing
    /// and their animation events.
    #[default]
    Unarmed,
}

impl WeaponAnimations {
    pub const ALL: [WeaponAnimations; 2] = [WeaponAnimations::Native, WeaponAnimations::Unarmed];

    pub const fn label(self) -> &'static str {
        match self {
            WeaponAnimations::Native => "native",
            WeaponAnimations::Unarmed => "unarmed",
        }
    }
}

/// The weapon-agnostic body (see [`crate::controllers::roomscale::stance::unarmed`]). Acts only
/// under the controller scheme with the headset driving the view, in gameplay, while the hand tiers
/// classify the character as on foot (including a reload, draw, or holster), so it defaults on.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UnarmedBodyConfig {
    pub weapon_animations: WeaponAnimations,
    /// Leave the upper-body action layer (draw, holster, switch, reload, pump, bolt, pickup, and
    /// throws) out of the pose.
    pub mask_actions: bool,
    /// Leave the synchronised upper-body layer (the turns toward the aim target, and the locomotion
    /// partials) out of the pose.
    pub mask_synced_partial: bool,
    /// Leave the synchronised additive layer (leans, landings, and the grapple, grenade, and launcher
    /// fire additives) out of the pose.
    pub mask_additive: bool,
    /// Switch the game's off-hand weapon grip off for both hands, driven or not, so that only the
    /// foregrip grab puts a hand on a two-handed weapon.
    pub release_weapon_grip: bool,
}

impl UnarmedBodyConfig {
    pub const fn new() -> Self {
        Self {
            weapon_animations: WeaponAnimations::Unarmed,
            mask_actions: true,
            mask_synced_partial: true,
            mask_additive: true,
            release_weapon_grip: true,
        }
    }
}

impl Default for UnarmedBodyConfig {
    fn default() -> Self {
        Self::new()
    }
}
