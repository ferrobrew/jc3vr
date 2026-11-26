//! The payload's UI layer: the debug UI and the pause menu's VR pages, the surfaces through which a
//! player or a developer reads and changes the mod's state, built on every layer below and named
//! only by the payload.
//!
//! The crate sits above the core, input, render, gameplay, HUD, and tooling crates
//! (`docs/architecture.md`, "Crate layering"), and owns:
//!
//! - [`ui`]: the debug UI: the dockable tab tree and its persisted layout, the feature catalog and
//!   the per-feature panels, the overview, the navigation, the previews, the session checklist,
//!   and the startup and restart banners.
//! - [`status`]: the status rows, the read-only summary of whether each system is live and
//!   healthy, which the debug UI's Status tab shows.
//! - [`pause_menu`]: the `VR` entry in the game's pause menu, its detours, and the page model the
//!   VR pages are built from.
//! - [`controllers`]: the VR pages themselves ([`controllers::menu`]), their presets
//!   ([`controllers::presets`]), and the body calibration page ([`controllers::body::menu`]). The
//!   controller subsystems they edit live in the gameplay crate.
//! - [`profiler`]: the Performance tab's profiler panel, behind the `profiler` feature.
//!
//! # The public surface
//!
//! Only the payload names this crate, so an item is `pub` exactly when the payload's frame driver,
//! shutdown, or composition names it: the debug window and the banners ([`ui::egui_debug_window`],
//! [`ui::startup_banner`], and [`ui::restart_banner`]), the layout writer's shutdown
//! ([`ui::layout::shutdown`]), and the pause menu's detours
//! ([`pause_menu::hook_library`]), which the payload enables in its own order with the other
//! layers'. Everything else keeps its crate-private visibility.
//!
//! The panels, on the other hand, reach deep into the layers below: they read and edit the
//! settings of every subsystem through its slot and show its live state, so most of the public
//! surface of the gameplay, render, HUD, and tooling crates exists for this crate. The crate owns
//! no settings of its own, and its build script embeds the newest session's checklist
//! (`docs/sessions/<stamp>/next-session.md`) for [`ui`]'s session checklist.

pub mod controllers;
pub mod pause_menu;
#[cfg(feature = "profiler")]
pub mod profiler;
pub mod status;
pub mod ui;

#[cfg(test)]
mod test_config;
