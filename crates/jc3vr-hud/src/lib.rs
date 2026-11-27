//! The payload's HUD layer: everything the player reads off the floating panel and the hands, and
//! the egui renderer that draws the debug UI, built on the gameplay layer's state and the view
//! layer's runtime, and naming nothing above it.
//!
//! The crate sits on the gameplay crate and every crate below it, beside the tooling crate
//! (`docs/architecture.md`, "Crate layering"), and owns:
//!
//! - [`hud`]: the floating panel and the layers drawn with it: the anchored hand readouts, the
//!   world markers and their labels, the grab hints, the reticles, the pointer routing for the
//!   game's UI and the egui panel, the controls summary panel, and the mirror overlay.
//! - [`controllers`]: the controller pieces that belong to the HUD: the controls map
//!   ([`controllers::map`]) and the summary's presentation (the grid, the glance, the page clock,
//!   and the hiding of the game's hints, in [`controllers::summary`]). The controller
//!   subsystems and the summary's model live in the gameplay crate.
//! - [`hooks`]: the UI detours ([`hooks::ui`]), which route the pointer into the game's Scaleform
//!   movies and draw the HUD around the game's own.
//! - [`egui_impl`]: the egui renderer and the debug UI's input capture.
//!
//! # The public surface
//!
//! The layers above reach these modules by path, so an item is `pub` exactly when a crate above
//! names it: mostly the debug UI's panels, the status rows, the window procedure, and the payload's
//! frame driver and composition. Everything else keeps its crate-private visibility.
//!
//! The crate's registrations with the layers below it (the post-draw's panels and redirect, the
//! desktop mirror, the flip, the input capture, and the controller pointers) are gathered in
//! [`register()`], which the payload's composition calls once before the hooks are installed. The
//! payload enables [`hooks::ui::hook_library`] and [`controllers::map::hook_library`] in its order
//! with the other layers'. The HUD's settings live in [`hud::config::HUD_CONFIG`], which the
//! payload binds to its aggregate.

pub mod controllers;
pub mod egui_impl;
pub mod hooks;
pub mod hud;
mod register;

pub use crate::register::register;
