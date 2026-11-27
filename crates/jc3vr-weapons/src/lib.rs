//! The payload's weapons layer: physical weapon handling and the loadout table, built on the body
//! layer and naming nothing beside or above it.
//!
//! The crate sits above the core, input, view, and body crates and below the gameplay crate, beside
//! the physical crate with no edge between them (`docs/architecture.md`, "Crate layering"), and
//! owns [`controllers`]: the weapon handling ([`controllers::weapons`]: the holsters and grab
//! volumes, the foregrip and the weight, the reloads, the grenades and C4, recoil, the dual-wield
//! split, the scope, and the weapon wheel, with their settings group and slot) and the loadout
//! ([`controllers::loadout`]: the one place each of the local player's items is).
//!
//! # The public surface
//!
//! The layers above reach these modules by path, so an item is `pub` exactly when a crate above
//! names it: the bindings, the snatch, the vehicles' mounted guns, the controllers' register files,
//! the HUD, the debug UI, and the payload's composition and settings aggregate. What the weapon
//! handling reads from the crates above it is declared in one place, [`contributions`], which the
//! gameplay crate's install fills. The weapon handling meets the physical hands only through the
//! body crate's held-item seam and registrations (`jc3vr_body::controllers::held`), which the
//! weapon handling fills in one place (`register_body`), called from the gameplay crate's install. The gameplay crate composes
//! [`controllers::weapons::hook_library`] into its controllers library in its established place.
//!
//! The crate's own tests register a configuration root of their own (`test_config`) holding the
//! weapons group and the body-bound groups the weapon handling reads, and use the body crate's
//! `test-support` fixtures.

pub mod contributions;
pub mod controllers;

#[cfg(test)]
mod test_config;
