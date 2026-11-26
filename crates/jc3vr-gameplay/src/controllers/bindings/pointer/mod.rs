//! The controller pointers' input side: each input tick turns the controller frame into the pointer
//! state the HUD delivers to the game UI and the egui debug panel.
//!
//! - [`controller`] turns the dominant controller's aim ray and trigger into a game-UI pointer
//!   sample;
//! - [`aim_hold`] pins the cursor on a light trigger pull and clicks only on a full one, for every
//!   controller pointer;
//! - [`egui_controller`] turns either controller's aim ray, trigger, upper face button, and
//!   thumbstick into egui pointer updates, and moves the panel while a hand grabs it;
//! - [`egui_grab`] decides which hand's grip holds the panel.
//!
//! The rays are intersected with the panels the renderer last drew
//! ([`jc3vr_view::hud::pointer::panel`]). The HUD reads the results: it routes the game UI's mouse from
//! [`selected_controller`] and drains the egui updates ([`take_updates`]). What the pointers need
//! from the HUD (its settings, the desktop mouse's capture, and the debug panel's placement) comes
//! through the [`PointerHud`] table the HUD registers at install.

mod aim_hold;
mod controller;
mod egui_controller;
mod egui_grab;

use std::sync::OnceLock;

use jc3vr_input::frame::{Hand, Pose};

pub use crate::controllers::bindings::pointer::{
    controller::{
        ControllerPointer, clear_controller, owns_menu_trigger, selected_controller,
        update_controller,
    },
    egui_controller::{
        ControllerUpdate, reset_egui_controller, take_updates, update_egui_controller,
    },
};

/// What the controller pointers read from and do to the HUD, registered by the HUD at install
/// ([`register_hud`]). Each entry is called from the input tick.
pub struct PointerHud {
    /// Whether the game-UI pointer is enabled: the HUD is redirected onto the floating panel.
    pub native_pointer_enabled: fn() -> bool,
    /// Whether a desktop press currently owns the game UI, so a controller press must not start
    /// mid-drag.
    pub desktop_capture: fn() -> bool,
    /// The egui panel pointer's settings.
    pub egui_pointer: fn() -> EguiPointerSettings,
    /// Whether the egui panel may accept controller pointer, scroll, and grab input.
    pub panel_accepts_edit_input: fn() -> bool,
    /// Whether the egui panel has a placement a grab can take hold of.
    pub panel_is_placed: fn() -> bool,
    /// Begin a hand's grab of the egui panel with the hand at a cockpit-frame aim pose. Returns
    /// whether the grab began.
    pub begin_grab: fn(Hand, Pose) -> bool,
    /// Hold a hand's grab for a tick: the aim pose when known, the push rate along the aim ray,
    /// and the tick's length in seconds.
    pub hold_grab: fn(Hand, Option<Pose>, f32, f32),
    /// End the grab, leaving the panel where it was last carried to.
    pub end_grab: fn(),
    /// Show or hide the egui panel. Returns whether it is now shown.
    pub toggle_panel: fn() -> bool,
}

/// The egui panel pointer's settings, read together.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct EguiPointerSettings {
    /// Whether the controllers point at the panel.
    pub enabled: bool,
    /// The panel's width over its height.
    pub aspect: f32,
}

/// Register the HUD's side of the pointers. Only one table is held: the first registration wins
/// and a later one is refused with a warning.
pub fn register_hud(hud: &'static PointerHud) {
    if HUD.set(hud).is_err() {
        tracing::warn!("controller pointer: a HUD is already registered; the new one is ignored");
    }
}

/// Show or hide the egui panel, returning whether it is now shown. Without a registered HUD there
/// is no panel to show.
pub(crate) fn toggle_panel() -> bool {
    registered_hud().is_some_and(|hud| (hud.toggle_panel)())
}

/// The registered HUD. Without one, the pointers behave as with no panel shown.
fn registered_hud() -> Option<&'static PointerHud> {
    HUD.get().copied()
}

static HUD: OnceLock<&'static PointerHud> = OnceLock::new();
