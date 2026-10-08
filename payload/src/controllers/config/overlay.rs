//! The world-space overlay layer: its readouts and the renderer's tuning.

use serde::{Deserialize, Serialize};

/// The world-space overlay layer: its readouts and the renderer's tuning. Acts only under the
/// controller scheme.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayConfig {
    /// Master switch for the layer. Off, nothing is drawn, whoever submits.
    pub enabled: bool,
    /// What the overlay draws for the guns' aim.
    pub gun_readout: GunReadout,
    /// Highlight the surface or object the grapple would attach to, while the target is valid and
    /// in range.
    pub grapple_highlight: bool,
    /// Draw a beam from the grapple wrist to the grapple's target point.
    pub grapple_beam: bool,
    /// Hide the game's own weapon reticle wherever the guns aim by hand (on foot whether or not a
    /// gun is out, and in a vehicle while its weapons aim by hand), and its grapple reticle
    /// everywhere under the controller scheme. Both mark the gaze's aim point (see
    /// [`crate::controllers::overlay::game_reticle`]). Independent of [`Self::enabled`].
    pub hide_game_reticle: bool,
    /// Draw a translucent ghost at the controller of a hand the animation owns.
    pub ghost_hands: bool,
    /// Highlight the grab volume each hand is in (a holster, the foregrip, the grenade pouch).
    pub grab_volumes: bool,
    /// Draw axes at the controllers' grip and aim poses, for debugging.
    pub debug_gizmos: bool,
    /// Draw Rico's solved skeleton reflected in a mirror ahead of him, with the targets it was
    /// posed toward, for checking the body's alignment ([`crate::controllers::body::mirror`]). A
    /// diagnostic, off by default.
    pub body_mirror: bool,
    /// How far ahead of Rico the body mirror stands, metres.
    pub body_mirror_distance_m: f32,
    /// How far behind the scene's depth, in metres, a primitive still counts as visible, so that a
    /// marker on a surface is not hidden by that surface.
    pub depth_tolerance_m: f32,
    /// The same tolerance as a fraction of the view distance, where it exceeds the absolute one: the
    /// depth buffer's precision falls with distance.
    pub depth_tolerance_rel: f32,
    /// The thinnest a line is drawn, in pixels. A line thinner than this fades instead.
    pub min_line_px: f32,
    /// The alpha scale of a ghost hand where the scene is in front of it.
    pub xray_alpha: f32,
}

impl OverlayConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            gun_readout: GunReadout::None,
            grapple_highlight: true,
            grapple_beam: true,
            hide_game_reticle: true,
            ghost_hands: false,
            grab_volumes: false,
            debug_gizmos: false,
            body_mirror: false,
            body_mirror_distance_m: 1.2,
            depth_tolerance_m: 0.05,
            depth_tolerance_rel: 0.01,
            min_line_px: 1.5,
            xray_alpha: 0.25,
        }
    }
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// What the overlay draws for a gun's aim.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GunReadout {
    /// Nothing: the gun's own sights are the only aid.
    #[default]
    None,
    /// A dot at the hit point, at its true depth.
    Dot,
    /// A line from the muzzle to the hit point.
    Laser,
}
