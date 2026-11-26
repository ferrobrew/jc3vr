//! The clean view: one switch that takes the mod's overlays out of the picture, at a chosen scope,
//! without touching the settings that configure them, so switching it off restores exactly what was
//! there.
//!
//! At [`CleanViewScope::Desktop`] it hides the flat egui debug overlay from the desktop window: the
//! direct back-buffer pass in `graphics_flip` and the desktop-mirror composite
//! (`crate::hud::mirror_overlay`). The headset image, the VR floating panel, and the in-world
//! overlays stay. At [`CleanViewScope::Everything`] it also hides every overlay the mod draws over
//! the scene: the floating HUD quad (`crate::hud`), the interactive egui panel
//! (`crate::hud::egui_panel`) and its desktop-mirror composite (`crate::vr::mirror`), and the
//! in-world overlays. The game's own HUD is redirected into the floating quad's texture whenever the
//! quad is drawn, so hiding the quad hides the HUD; with the redirect off (flatscreen) the game's HUD
//! stays, and the debug tab says so.
//!
//! The desktop gates read [`hides_desktop`] and the rest read [`hides_everything`]. F4 toggles the
//! switch ([`toggle`]); the scope is picked in the director tab.

use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::config::{self, Config};

/// The clean view's settings: the switch and its scope. Neither is saved: the switch is on at every
/// launch, which keeps the flat debug overlay off the desktop window until F4 shows it, so a session
/// that showed the overlay does not leave it showing at the next launch; and the scope is a choice
/// for the shot at hand, so a launch never starts with the HUD hidden.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct CleanViewConfig {
    /// Hide the mod's overlays at [`scope`](Self::scope).
    pub on: bool,
    /// How much [`on`](Self::on) hides.
    pub scope: CleanViewScope,
}

impl CleanViewConfig {
    pub const fn new() -> Self {
        Self {
            on: true,
            scope: CleanViewScope::Desktop,
        }
    }
}

impl Default for CleanViewConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// How much the clean view hides.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
pub enum CleanViewScope {
    /// The flat debug overlay on the desktop window only.
    #[default]
    Desktop,
    /// Every overlay the mod draws, in the headset and on the desktop.
    Everything,
}

/// Whether the flat debug overlay is hidden from the desktop window: the clean view is on, at
/// either scope. Read from the draw and mirror threads.
pub fn hides_desktop() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Whether every overlay is hidden: the clean view is on at [`CleanViewScope::Everything`]. Read
/// from the draw and mirror threads.
pub fn hides_everything() -> bool {
    hides_desktop() && EVERYTHING.load(Ordering::Relaxed)
}

/// Flip the clean view at its current scope, returning whether it is now on. Bound to F4.
pub fn toggle() -> bool {
    let mut cfg = config::edit();
    let clean_view = settings(&mut cfg);
    clean_view.on = !clean_view.on;
    publish(*clean_view);
    clean_view.on
}

/// Switch the clean view on at its current scope: the debug UI's Hide.
pub(crate) fn hide() {
    let mut cfg = config::edit();
    let clean_view = settings(&mut cfg);
    clean_view.on = true;
    publish(*clean_view);
}

/// Publish the config's clean view, once per frame from [`crate::director::update`], so a change
/// made in the debug UI reaches the gates. Logs the edges so a recording's log shows when the
/// overlays went away.
pub(super) fn update(cfg: &CleanViewConfig) {
    publish(*cfg);
}

/// The clean view's part of the settings.
fn settings(cfg: &mut Config) -> &mut CleanViewConfig {
    &mut cfg.director.clean_view
}

/// Whether the clean view is on. Mirrored from [`CleanViewConfig::on`]; an atomic so the
/// draw-thread gates read it without the config lock. Starts on, as the config does.
static ON: AtomicBool = AtomicBool::new(true);
/// Whether the clean view's scope is [`CleanViewScope::Everything`].
static EVERYTHING: AtomicBool = AtomicBool::new(false);

fn publish(CleanViewConfig { on, scope }: CleanViewConfig) {
    let everything = scope == CleanViewScope::Everything;
    let was_on = ON.swap(on, Ordering::Relaxed);
    let was_everything = EVERYTHING.swap(everything, Ordering::Relaxed);
    if was_on != on || (on && was_everything != everything) {
        tracing::info!(
            "director: clean view {}",
            match (on, scope) {
                (false, _) => "OFF",
                (true, CleanViewScope::Desktop) => "ON (desktop)",
                (true, CleanViewScope::Everything) => "ON (everything)",
            }
        );
    }
}
