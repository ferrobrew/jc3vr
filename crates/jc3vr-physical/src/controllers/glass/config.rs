//! Breaking glass's settings, `controllers.glass`, and its fixed tuning.

use jc3vr_core::config::engine::ConfigSlot;
use serde::{Deserialize, Serialize};

/// The glass breaking's settings, at `controllers.glass` in the settings file; the payload binds
/// the slot at startup.
pub static GLASS_CONFIG: ConfigSlot<GlassConfig> = ConfigSlot::new("controllers.glass");

/// Breaking glass by hand (see [`crate::controllers::glass`]). It acts only under the controller
/// scheme, and its tuning is fixed at [`GLASS`].
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GlassConfig {
    /// Let bullets pass cleanly through a pane brought down, rather than meeting the glass's shapes
    /// that hiding it leaves ([`bullets`](crate::controllers::glass::bullets)).
    pub bullets_pass: bool,
}

impl GlassConfig {
    pub const fn new() -> Self {
        Self { bullets_pass: true }
    }
}

impl Default for GlassConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// How strikes on glass are measured and what they do, fixed at [`GLASS`].
#[derive(Copy, Clone, Debug, PartialEq)]
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

/// The glass strikes' tuning.
pub const GLASS: GlassTuning = GlassTuning {
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
};
