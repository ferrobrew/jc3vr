//! The payload's physical layer: the physical hands and breaking window glass, built on the body
//! layer and naming nothing above it.
//!
//! The crate sits above the core, input, view, and body crates (`docs/architecture.md`, "Crate
//! layering"), beside the weapons crate, which it does not depend on, and owns [`controllers`]:
//!
//! - [`controllers::physical_hands`]: each empty hand's palm colliding with the world: pushing the
//!   body off surfaces, holding the hand at them, striking characters and objects, taking hold of
//!   loose objects, characters, and doors, climbing and vaulting, and the hand bodies with the
//!   physics step that carries what the hands hold;
//! - [`controllers::glass`]: breaking window glass with a palm or a held gun, as bullets crack it,
//!   and bringing a pane down with a hard enough strike.
//!
//! What the weapon handling knows about a held weapon reaches the hands through the body crate's
//! held-item facts and registrations ([`jc3vr_body::controllers::held`]), which the gameplay crate
//! connects above both.
//!
//! # The public surface
//!
//! The layers above reach these modules by path, so an item is `pub` exactly when a crate above
//! names it: mostly the gameplay crate's registrations and hand stages, the vehicles and the
//! grapple, the debug UI's panels and status rows, and the directing tools. Everything else keeps
//! its crate-private visibility.
//!
//! The gameplay crate composes the registrations and the hook libraries:
//!
//! - [`contributions`]: what the crate reads from the crates above it, one record the vehicle
//!   handling registers, which the install's registration check requires.
//! - [`controllers::physical_hands::register_body::register`] fills the physical hands' fields of
//!   the body's contributions record, and the gameplay crate's install calls it; the hand stage
//!   and the aim hook the physical hands provide sit in the gameplay crate's ordered tables.
//! - The hook libraries, which the gameplay crate and the payload enable in their established
//!   order with the other layers':
//!   [`controllers::physical_hands::impacts::hook_library`],
//!   [`controllers::physical_hands::lifetime::hook_library`], and
//!   [`controllers::glass::hook_library`] inside the gameplay crate's controllers library, and
//!   [`controllers::physical_hands::human_hull::hook_library`] in the payload's composition.
//! - The settings: [`controllers::physical_hands::config::PHYSICAL_HANDS_CONFIG`] and
//!   [`controllers::glass::config::GLASS_CONFIG`] declare the slots for the two groups beside
//!   their types, and the payload binds them to its aggregate.
//!
//! The crate's own tests register a configuration root of their own (`test_config`), since the
//! payload's aggregate is not there, and use the body crate's `test-support` fixtures.

pub mod contributions;
pub mod controllers;

#[cfg(test)]
mod test_config;
