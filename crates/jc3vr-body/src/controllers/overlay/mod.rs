//! The world-space overlay layer: the plan of record's "The world-space overlay layer"
//! (`docs/mod/input/controllers-and-roomscale.md`). Sprites and lines drawn into the scene per eye,
//! tested against the scene's depth, for readouts that need their own depths: the grapple's
//! highlight and beam, the guns' dot or laser, the grab-volume highlights, the ghosts of yielded
//! hands, and the controller gizmos.
//!
//! The overlay's data model lives in [`jc3vr_view::controllers::overlay`], and its render half in
//! the render crate's `controllers::overlay`; this module holds the producers and the inputs the
//! draw reads.
//!
//! - [`primitive`]: what a producer submits;
//! - [`store`]: the retained per-source slots, replaced by each submission and expiring on their
//!   own, which any thread may submit to ([`store::submit`], [`store::clear`]);
//! - [`producers`]: the overlay's own producers, run once per rendered frame;
//! - [`resolve`]: anchors placed and primitives packed for the frame, relative to its origin;
//! - the render crate's `view`: each eye's camera-relative view, in two-pass stereo and under the
//!   collapse;
//! - the render crate's `renderer`: the D3D11 pipeline;
//! - the render crate's `pass`: the draw, which reads the overlay's settings and the hands'
//!   render-time poses through the [`inputs`] the controllers register
//!   (the gameplay crate's `controllers::register_render`).
//!
//! [`primitive`]: jc3vr_view::controllers::overlay::primitive
//! [`store`]: jc3vr_view::controllers::overlay::store
//! [`store::submit`]: jc3vr_view::controllers::overlay::store::submit
//! [`store::clear`]: jc3vr_view::controllers::overlay::store::clear
//! [`resolve`]: jc3vr_view::controllers::overlay::resolve
//! [`inputs`]: jc3vr_view::controllers::overlay::inputs
//!
//! The game's own HUD reticles, hidden while the hands aim, are the HUD's
//! (`hud::game_reticle`).
//!
//! The overlay draws at the end of `RenderEngine::PostDraw`, onto the eye's final back buffer
//! before the HUD panel, after the post chain and the upscaler: the readouts keep their colours
//! through exposure changes and do not smear under temporal reconstruction. The frame's
//! primitives are snapshotted and uploaded once, on the first eye's draw, so both eyes draw the same
//! geometry.

pub mod config;
pub mod producers;

use jc3vr_view::controllers::overlay::{inputs::DrawSettings, resolve::HandPoses};

use crate::controllers::{hands::late, overlay::config::OVERLAY_CONFIG, scheme};

/// Run the overlay's producers. Called once per render dispatch at the end of
/// `RenderEngine::PostDraw`, before the engine's context lock is taken; it acts only on the
/// frame's first eye.
pub fn tick(eye: usize) {
    if !jc3vr_view::stereo::dispatch::first_ui_eye(eye) {
        return;
    }
    let active = scheme::enabled()
        && OVERLAY_CONFIG.query(|c| c.enabled)
        && scheme::is_controllers()
        && jc3vr_core::hooks::in_gameplay();
    producers::tick(active, &render_poses());
}

/// Each hand's world poses as it is drawn at the camera's interpolation fraction, so what is
/// anchored to a hand stays on the hand the late update and the presentation layer draw.
pub fn render_poses() -> [Option<HandPoses>; 2] {
    late::drawn_hands(jc3vr_view::hooks::camera::last_dtf()).map(|hand| {
        hand.map(|h| HandPoses {
            wrist: h.wrist,
            grip: h.grip,
            aim: h.aim,
        })
    })
}

/// How the overlay draws, or `None` while it or the controllers are off.
pub fn draw_settings() -> Option<DrawSettings> {
    let enabled = scheme::enabled();
    OVERLAY_CONFIG.query(|overlay| {
        (enabled && overlay.enabled).then_some(DrawSettings {
            min_line_px: overlay.min_line_px,
            depth_tolerance_m: overlay.depth_tolerance_m,
            depth_tolerance_rel: overlay.depth_tolerance_rel,
        })
    })
}
