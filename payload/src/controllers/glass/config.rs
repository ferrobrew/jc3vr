//! Breaking glass's settings, `controllers.glass`.

use serde::{Deserialize, Serialize};

/// Breaking glass by hand (see [`crate::controllers::glass`]). It acts only under the controller
/// scheme, so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GlassConfig {
    /// Master switch: a palm or a held gun that strikes a window damages it.
    pub enabled: bool,
    /// The muzzle end of a held gun strikes, as well as an empty hand's palm.
    pub weapons: bool,
    /// A hard enough strike, or enough damage, brings the whole pane down.
    pub shatter: bool,
    /// Play the game's melee impact against glass where a strike lands.
    pub sound: bool,
    /// Let bullets pass cleanly through a pane brought down, rather than meeting the glass's shapes
    /// that hiding it leaves ([`bullets`](crate::controllers::glass::bullets)).
    pub bullets_pass: bool,
    pub tuning: GlassTuning,
}

impl GlassConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            weapons: true,
            shatter: true,
            sound: true,
            bullets_pass: true,
            tuning: GlassTuning::new(),
        }
    }
}

impl Default for GlassConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// How strikes on glass are measured and what they do.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GlassTuning {
    /// The slowest strike that damages glass, world metres per second: a hand resting against a
    /// window, or pushed slowly through it, does nothing.
    pub min_speed_mps: f32,
    /// The speed at and past which a strike does [`max_damage`](Self::max_damage).
    pub full_speed_mps: f32,
    /// The damage of the slowest strike that counts, in the game's damage-point strength (the
    /// strength a bullet adds is on `MetaWindow::AddWindowDamagePoint`).
    pub min_damage: f32,
    /// The damage of a strike at [`full_speed_mps`](Self::full_speed_mps).
    pub max_damage: f32,
    /// One strike at this speed or faster brings the whole pane down.
    pub shatter_speed_mps: f32,
    /// A pane whose damage reaches this brings itself down: the total of its damage points, each
    /// counted up to one, plus what blows on a point already at full strength added past it.
    pub shatter_total: f32,
    /// How long a striker waits before it strikes again, seconds, so that one swing strikes once.
    pub cooldown_s: f32,
    /// How far past the palm its sweep reaches, metres.
    pub palm_lead_m: f32,
    /// How far past the muzzle a gun's sweep reaches, metres.
    pub tip_lead_m: f32,
    /// How far ahead of the hand's aim pose a gun without measured geometry (a pistol) has its
    /// muzzle, metres.
    pub pistol_reach_m: f32,
    /// The longest time between two samples of a striker that still makes a swing, seconds.
    pub max_gap_s: f32,
}

impl GlassTuning {
    pub const fn new() -> Self {
        Self {
            min_speed_mps: 2.0,
            full_speed_mps: 7.0,
            min_damage: 0.15,
            max_damage: 0.6,
            shatter_speed_mps: 8.5,
            shatter_total: 2.5,
            cooldown_s: 0.2,
            palm_lead_m: 0.05,
            tip_lead_m: 0.03,
            pistol_reach_m: 0.18,
            max_gap_s: 0.1,
        }
    }
}

impl Default for GlassTuning {
    fn default() -> Self {
        Self::new()
    }
}
