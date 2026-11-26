//! The payload's gameplay layer: the controller subsystems built on the body layer, which change
//! how the game plays in the headset, and naming nothing above it.
//!
//! The crate sits above the core, input, view, body, weapons, and physical crates
//! (`docs/architecture.md`, "Crate layering") and owns:
//!
//! - [`controllers`]: the controller subsystems built on the body: the bindings, the vehicles, the
//!   snatch, the scope state, and the summary model, with the controllers' settings aggregate,
//!   their install, and their hook library. The body-bound subsystems they build on (the body,
//!   hands, aim, grip, locomotion, roomscale, the mode detector, the haptics, and the shared
//!   vocabulary and facts) live in the body crate, under [`jc3vr_body::controllers`], the weapon
//!   handling with the loadout in the weapons crate, under [`jc3vr_weapons::controllers`], and the
//!   physical hands and the glass in the physical crate, under [`jc3vr_physical::controllers`]. The data the render halves of the overlay and
//!   the scope read, and the cockpit transform, live in the view crate, under
//!   [`jc3vr_view::controllers`]; the passes that draw them live in the render crate's
//!   `controllers`, which this crate does not depend on.
//! - [`grapple`]: the grapple's device, its physical rope and swing, and its seats.
//! - [`hooks`]: the gameplay detours (the input tick's look detour, the equipment, and the sim
//!   step) and the stance height the view crate's camera adjust applies.
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
//! - [`install`] registers the controller subsystems' head-pose, VR-frame, render, and lifecycle
//!   participants, and what the weapon handling, the physical hands, and the vehicles contribute
//!   to the body crate's registrations, and must run before the hooks are installed.
//! - The hook libraries, which the payload enables in its own interleaved order with the other
//!   layers' (the order is load-bearing): [`hooks::sim::hook_library`],
//!   [`hooks::equipment::hook_library`], the body crate's
//!   [`jc3vr_body::hooks::character::hook_library`] and
//!   [`jc3vr_body::hooks::animation::hook_library`], [`hooks::input::hook_library`] (which
//!   composes the body crate's input detours around the look detour), [`controllers::hook_library`]
//!   (which composes the body-bound subsystems' and the physical crate's libraries with these and
//!   takes the HUD's controls map library to enable in its place), the physical crate's
//!   [`jc3vr_physical::controllers::physical_hands::human_hull::hook_library`],
//!   [`grapple::physical::hook_library`], and [`grapple::device::hook_library`].
//! - The settings: each subsystem declares the [`ConfigSlot`](jc3vr_core::config::engine::ConfigSlot)
//!   for its group beside the group's type (one per controllers group, such as
//!   [`jc3vr_weapons::controllers::weapons::config::WEAPONS_CONFIG`]), and the payload binds them
//!   to its aggregate. A subsystem reads only its own group and the groups of the subsystems it builds on,
//!   through their slots; the whole `controllers` table
//!   ([`controllers::config::CONTROLLERS_CONFIG`]) is read only by the bindings, the summary, and
//!   the layers above.
//!
//! The `profiler` feature adds the sim step's profiler scopes. The crate's own tests register a
//! configuration root of their own (`test_config`), since the payload's aggregate is not there,
//! and use the body crate's `test-support` fixtures.

pub mod controllers;
pub mod grapple;
pub mod hooks;

pub use crate::controllers::install;

#[cfg(test)]
mod test_config;
