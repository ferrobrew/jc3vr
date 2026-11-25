//! Hiding the game's own context button hints under the controller scheme
//! ([`SummaryConfig::hide_game_hints`](crate::controllers::summary::SummaryConfig::hide_game_hints)).
//!
//! The HUD shows the hints it raises in context (release the tethers, reel in, the gunner seat, a
//! mission's prompts) with a gamepad glyph per action
//! (`docs/engine/button-hints-and-localisation.md`). Under the controller scheme the mod pins the
//! game to its gamepad rules, so those glyphs name gamepad buttons for actions the controllers
//! drive with other inputs. The two hint stages of the HUD
//! movie are held hidden through [`crate::hud::scaleform`] while the scheme is in effect; the
//! game's hint queue keeps running, so its timing and cooldowns behave as normal. The controls
//! summary stands in for them.

use crate::{config::Config, controllers::scheme};

/// Whether to hide the hints: the toggle, with the controllers' master switch, and the controller
/// scheme.
pub const fn decide(enabled: bool, controllers_scheme: bool) -> bool {
    enabled && controllers_scheme
}

/// Hold the game's hint stages hidden under the controller scheme, and restore them otherwise.
///
/// # Safety
///
/// Must be called from the [`MovieImpl::Capture`](jc3gi::ui::scaleform::MovieImpl) detour, as
/// [`crate::hud::scaleform::apply_hint_suppression`] requires.
pub(crate) unsafe fn hide_game_hints() {
    let enabled =
        Config::lock_query(|c| c.controllers.enabled && c.controllers.summary.hide_game_hints);
    let hidden = decide(enabled, scheme::is_controllers());
    // SAFETY: forwarded from the caller.
    unsafe { crate::hud::scaleform::apply_hint_suppression(hidden) };
}
