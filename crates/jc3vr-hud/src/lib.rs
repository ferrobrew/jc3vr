//! The payload's HUD layer: everything the player reads off the floating panel and the hands, and
//! the egui renderer that draws the debug UI, built on the gameplay layer's state and the render
//! layer's runtime, and naming nothing above it.
//!
//! The crate sits above the core, input, render, and gameplay crates and beside the tooling crate
//! (`docs/architecture.md`, "Crate layering"), and owns:
//!
//! - [`hud`]: the floating panel and the layers drawn with it: the anchored hand readouts, the
//!   world markers and their labels, the grab hints, the reticles, the pointer routing for the
//!   game's UI and the egui panel, the controls summary panel, and the mirror overlay.
//! - [`controllers`]: the controller pieces that belong to the HUD: the controls map
//!   ([`controllers::map`]) and the summary's presentation (the glyphs, the grid, the glance, the
//!   page clock, and the game's hint glyphs, in [`controllers::summary`]). The controller
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
//! The payload composes the hooks and registrations: [`hud::register_render::register`] and the
//! pointer and egui registrations hook the HUD into the render layer, and the payload enables
//! [`hooks::ui::hook_library`] and passes [`controllers::map::hook_library`] to the gameplay
//! crate's controllers library in its own interleaved order with the other layers' (the order is
//! load-bearing). The HUD's settings live in [`hud::config::HUD_CONFIG`], which the payload binds
//! to its aggregate.

pub mod controllers;
pub mod egui_impl;
pub mod hooks;
pub mod hud;
