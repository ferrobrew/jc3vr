//! The payload's vehicles layer: vehicles, mounted guns, the wingsuit, and the parachute under the
//! controller scheme, built on the body, weapons, and physical layers and naming nothing beside or
//! above them.
//!
//! The crate sits above the core, input, view, body, weapons, and physical crates and below the
//! gameplay crate, beside the grapple crate with no edge between them (`docs/architecture.md`,
//! "Crate layering"), and owns:
//!
//! - [`controllers::vehicles`]: the physical controls of the driver's seat and the cockpit's
//!   buttons, the driver's view, the hand-aimed mounted guns and driver weapons, the passenger's
//!   seated aim, the wingsuit's arm steering, evade, and gestures, the parachute's handles and the
//!   ripcord, the take-off from the ground, and the vehicle rail, with their settings group, its
//!   slot, and its checks;
//! - [`hooks::equipment`]: the weapon update detours and the native start-using guard the
//!   physically gripped mounted guns use.
//!
//! # The public surface
//!
//! The layers above reach the modules by path, so an item is `pub` exactly when a crate above names
//! it: the bindings, which run the vehicle handling's two halves on the input tick, the summary,
//! the controllers' register files and hand stages, the look detour, the controller settings'
//! checks, the HUD's reticle, the debug UI's panels, status rows, and controls page, the
//! diagnostics, and the payload's composition and settings aggregate. Everything else keeps its
//! crate-private visibility.
//!
//! The gameplay crate composes the registrations and the controllers' hook library, and the
//! payload the equipment detours:
//!
//! - [`controllers::vehicles::register_body::register`] fills the body and physical crates'
//!   registrations the vehicle handling contributes to, and
//!   [`controllers::vehicles::register_mounted::register`] the body crate's mounted-gun fact; the
//!   gameplay crate's install calls both.
//! - [`controllers::vehicles::hook_library`], which the gameplay crate enables inside its
//!   controllers library in its established place (the order is load-bearing).
//! - [`hooks::equipment::hook_library`]: the weapon update detours and the native start-using
//!   guard the physically gripped mounted guns use, which the payload's composition enables in its
//!   established place (the order is load-bearing).
//! - The settings: [`controllers::vehicles::config::VEHICLES_CONFIG`] declares the slot for the
//!   group beside its type, and the payload binds it to its aggregate. The gameplay crate's
//!   controller settings checks run the group's own
//!   ([`VehiclesConfig::validate`](controllers::vehicles::config::VehiclesConfig::validate)).
//!
//! The crate's own tests register a configuration root of their own (`test_config`), since the
//! payload's aggregate is not there, and use the body crate's `test-support` fixtures.

pub mod controllers;
pub mod hooks;

#[cfg(test)]
mod test_config;
