//! The payload's gameplay layer: the controller subsystems built on the body layer, which change
//! how the game plays in the headset, and naming nothing above it.
//!
//! The crate sits above the core, input, view, body, weapons, physical, and grapple crates
//! (`docs/architecture.md`, "Crate layering") and owns:
//!
//! - [`controllers`]: the controller subsystems built on the body: the bindings and the summary
//!   model, with the controllers' settings aggregate and their registration. The body-bound subsystems they build on (the body,
//!   hands, aim, grip, locomotion, roomscale, the mode detector, the haptics, and the shared
//!   vocabulary and facts) live in the body crate, under [`jc3vr_body::controllers`], the weapon
//!   handling with the loadout in the weapons crate, under [`jc3vr_weapons::controllers`], and the
//!   physical hands and the glass in the physical crate, under [`jc3vr_physical::controllers`], and the
//!   grapple with the snatch in the grapple crate, under [`jc3vr_grapple`]. The data the render halves of the overlay and
//!   the scope read, and the cockpit transform, live in the view crate, under
//!   [`jc3vr_view::controllers`]; the passes that draw them live in the render crate's
//!   `controllers`, which this crate does not depend on.
//! - [`hooks`]: the gameplay detours (the input tick's look detour, the sim step, and the online
//!   managers the sim tick runs) and the stance height the view crate's camera adjust applies.
//!
//! # The public surface
//!
//! The layers above reach these modules by path, so an item is `pub` exactly when a crate above
//! names it: mostly the HUD, the debug UI's controller panels and status rows, the directing tools,
//! and the payload's composition and settings aggregate. Everything else keeps its crate-private
//! visibility.
//!
//! The payload composes the hooks and registrations:
//!
//! - [`register`] registers the controller subsystems' head-pose, VR-frame, and render
//!   participants, and fills the body's contributions whose order this crate declares (the aim's
//!   hooks, the hands' stage and observer tables, the grip sources' order, and the character
//!   hooks' refusals). The payload's composition calls it before the hooks are installed.
//! - The hook libraries [`hooks::sim::hook_library`], [`hooks::online::hook_library`], and
//!   [`hooks::input::hook_library`] (which composes the body crate's input detours around the look
//!   detour), which the composition enables in its order with the other layers'.
//! - The settings: each subsystem declares the [`ConfigSlot`](jc3vr_core::config::engine::ConfigSlot)
//!   for its group beside the group's type (one per controllers group, such as
//!   [`jc3vr_weapons::controllers::weapons::config::WEAPONS_CONFIG`]), and the `controllers` slot
//!   binds them wherever it is bound. A subsystem reads only its own group and the groups of the subsystems it builds on,
//!   through their slots; the whole `controllers` table
//!   ([`controllers::config::CONTROLLERS_CONFIG`]) is read only by the bindings, the summary, and
//!   the layers above.
//!
//! The `profiler` feature adds the sim step's profiler scopes. The crate's own tests register a
//! configuration root of their own (`test_config`), since the payload's aggregate is not there,
//! and use the body crate's `test-support` fixtures.

pub mod controllers;
pub mod hooks;

pub use crate::controllers::register;

#[cfg(test)]
mod test_config;
