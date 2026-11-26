//! The world-space overlay layer: the plan of record's "The world-space overlay layer"
//! (`docs/mod/input/controllers-and-roomscale.md`). Sprites and lines drawn into the scene per eye,
//! tested against the scene's depth, for readouts that need their own depths: the grapple's
//! highlight and beam, the guns' dot or laser, the grab-volume highlights, the ghosts of yielded
//! hands, and the controller gizmos.
//!
//! - [`primitive`]: what a producer submits;
//! - [`store`]: the retained per-source slots, replaced by each submission and expiring on their
//!   own, which any thread may submit to ([`store::submit`], [`store::clear`]);
//! - [`producers`]: the overlay's own producers, run once per rendered frame;
//! - [`resolve`]: anchors placed and primitives packed for the frame, relative to its origin;
//! - [`view`]: each eye's camera-relative view, in two-pass stereo and under the collapse;
//! - [`renderer`]: the D3D11 pipeline;
//! - [`pass`]: the draw, which reads the overlay's settings and the hands' render-time poses
//!   through the inputs the controllers register (`crate::controllers::register_render`).
//!
//! The game's own HUD reticles, hidden while the hands aim, are the HUD's
//! (`crate::hud::game_reticle`).
//!
//! The overlay draws at the end of `RenderEngine::PostDraw`, onto the eye's final back buffer
//! before the HUD panel, after the post chain and the upscaler: the readouts keep their colours
//! through exposure changes and do not smear under temporal reconstruction. The frame's
//! primitives are snapshotted and uploaded once, on the first eye's draw, so both eyes draw the same
//! geometry.

pub mod pass;
pub mod primitive;
pub mod producers;
pub mod renderer;
pub mod resolve;
pub mod store;
pub mod view;

pub use crate::controllers::overlay::pass::{OverlayStatus, status};
pub(crate) use crate::controllers::overlay::pass::{draw, release_on_eject};
use crate::{
    config::Config,
    controllers::{
        hands::late,
        overlay::{pass::DrawSettings, resolve::HandPoses},
        scheme,
    },
    hooks,
};

/// Run the overlay's producers. Called once per render dispatch at the end of
/// `RenderEngine::PostDraw`, before the engine's context lock is taken; it acts only on the
/// frame's first eye.
pub(crate) fn tick(eye: usize) {
    if !crate::stereo::dispatch::first_ui_eye(eye) || crate::stereo::far_phase() {
        return;
    }
    let active = Config::lock_query(|c| c.controllers.enabled && c.controllers.overlay.enabled)
        && scheme::is_controllers()
        && hooks::in_gameplay();
    producers::tick(active, &render_poses());
}

/// Each hand's world poses as it is drawn at the camera's interpolation fraction, so what is
/// anchored to a hand stays on the hand the late update and the presentation layer draw.
pub(crate) fn render_poses() -> [Option<HandPoses>; 2] {
    late::drawn_hands(hooks::camera::last_dtf()).map(|hand| {
        hand.map(|h| HandPoses {
            wrist: h.wrist,
            grip: h.grip,
            aim: h.aim,
        })
    })
}

/// How the overlay draws, or `None` while it or the controllers are off.
pub(crate) fn draw_settings() -> Option<DrawSettings> {
    Config::lock_query(|c| {
        let overlay = &c.controllers.overlay;
        (c.controllers.enabled && overlay.enabled).then_some(DrawSettings {
            min_line_px: overlay.min_line_px,
            depth_tolerance_m: overlay.depth_tolerance_m,
            depth_tolerance_rel: overlay.depth_tolerance_rel,
        })
    })
}
