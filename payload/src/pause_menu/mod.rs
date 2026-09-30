//! The VR menu: VR settings pages inside the game's own pause menu, drawn by the pause movie's own
//! settings screen and widgets and driven from Rust through the Scaleform value API. No Flash of the
//! mod's own is involved. See `docs/mod/pause-menu.md` for the route and the hooks, and
//! `docs/engine/pause-menu.md` for the movie.
//!
//! - [`model`]: the pure logic: the page model ([`model::page`]), row ids and row encoding, slider
//!   mapping, flow steps, the live-value diff, the menu state machine, and command routing;
//! - [`gfx`]: the capture-thread token and the owned AS3 value;
//! - [`runtime`]: the detours and the movie access they use;
//! - [`pages`]: the registry of pages.
//!
//! To add a page, describe it as a [`model::page::PageDef`] beside the settings it edits and list
//! it in [`pages::PAGES`].

mod config;
pub(crate) mod gfx;
pub(crate) mod model;
pub(crate) mod pages;
mod runtime;

use re_utilities::hook_library::HookLibrary;

pub use crate::pause_menu::config::PauseMenuConfig;
use crate::pause_menu::{model::state::MenuState, runtime::SESSION};

/// The pause-menu detours.
pub(crate) fn hook_library() -> HookLibrary {
    runtime::hook_library()
}

/// The menu state, for the debug UI.
pub(crate) fn state() -> MenuState {
    SESSION.lock().state
}
