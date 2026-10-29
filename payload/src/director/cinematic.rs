//! Cinematic mode: one switch that hides every overlay the mod draws over the scene, without
//! touching the settings that configure them, so flipping it back restores exactly what was there.
//!
//! The overlays are the floating HUD quad (`crate::hud`), the interactive egui panel
//! (`crate::hud::egui_panel`), the flat egui overlay and its desktop-mirror composite
//! (`crate::hud::mirror_overlay`, `crate::vr::mirror`), and the flat overlay pass in
//! `graphics_flip`. Each of those gates on [`active`]. The game's own HUD is already redirected into the floating quad's texture whenever the
//! quad is drawn, so hiding the quad hides the HUD; with the redirect off (flatscreen) the game's
//! HUD stays, and the debug tab says so.

use std::sync::atomic::{AtomicBool, Ordering};

/// Whether cinematic mode is on. Mirrored from `DirectorConfig::cinematic` every frame by
/// [`crate::director::update`]; an atomic so the draw-thread gates read it without the config lock.
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Whether the overlays are hidden. Read from the draw and mirror threads.
pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Publish the config's cinematic flag. Logs the edges so a recording's log shows when the
/// overlays went away.
pub(super) fn set(on: bool) {
    if ACTIVE.swap(on, Ordering::Relaxed) != on {
        tracing::info!("director: cinematic mode {}", if on { "ON" } else { "OFF" });
    }
}
