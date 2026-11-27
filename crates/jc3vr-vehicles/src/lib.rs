//! The payload's vehicles layer: vehicles, mounted guns, the wingsuit, and the parachute under the
//! controller scheme, built on the body, weapons, and physical layers and naming nothing beside or
//! above them.
//!
//! The crate sits above the core, input, view, body, weapons, and physical crates and below the
//! gameplay crate, beside the grapple crate with no edge between them (`docs/architecture.md`,
//! "Crate layering"), and owns:
//!
//! - [`vehicles`]: the physical controls of the driver's seat and the cockpit's
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
//! the controllers' hand stages and tables, the look detour, the controller settings'
//! checks, the HUD's reticle, the debug UI's panels, status rows, and controls page, the
//! diagnostics, and the payload's composition and settings aggregate. Everything else keeps its
//! crate-private visibility.
//!
//! The payload's composition registers the crate and enables its hook libraries:
//!
//! - [`register()`]: what the vehicle handling contributes to the physical crate and its fields of
//!   the body's contributions (the take-off and the mounted-gun fact), which the composition calls
//!   before any hook is installed. It returns [`Registered`], the only way to reach the equipment
//!   detours ([`hooks::equipment`]), which the composition enables in their place in its order.
//! - [`vehicles::hook_library`], which the composition enables in its place in the
//!   order.
//! - [`vehicles::mounted::recoil::on_shot`], the handles' kick by each shot of the
//!   mounted gun the hands aim, which the composition hands to the weapon handling's
//!   contributions.
//! - The settings: [`vehicles::config::VEHICLES_CONFIG`] declares the slot for the
//!   group beside its type, and the payload binds it to its aggregate. The gameplay crate's
//!   controller settings checks run the group's own
//!   ([`VehiclesConfig::validate`](vehicles::config::VehiclesConfig::validate)).
//!
//! The crate's own tests register a configuration root of their own (`test_config`), since the
//! payload's aggregate is not there, and use the body crate's `test-support` fixtures.

pub mod hooks;
mod register;
pub mod vehicles;

pub use register::{Registered, register};

#[cfg(test)]
mod test_config;
