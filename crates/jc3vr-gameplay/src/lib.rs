//! The payload's gameplay layer: the bindings that drive the game from the controllers, the
//! controls summary's model, and the gameplay detours, built on every controller crate below it and
//! naming nothing above it.
//!
//! The crate sits above the core, input, view, body, weapons, physical, vehicles, and grapple
//! crates (`docs/architecture.md`, "Crate layering") and owns:
//!
//! - [`controllers`]: the bindings, the controls summary's model, the camera height readout, the
//!   controllers' settings aggregate, and the registrations that declare the order of what the
//!   crates below contribute. The subsystems the bindings drive live in the crates below: the body,
//!   hands, aim, grip, locomotion, roomscale, mode detector, haptics, and shared vocabulary and
//!   facts under [`jc3vr_body::controllers`], the weapon handling with the loadout under
//!   [`jc3vr_weapons::controllers`], the physical hands and the glass under
//!   [`jc3vr_physical::controllers`], the vehicles, mounted guns, wingsuit, and parachute under
//!   [`jc3vr_vehicles::vehicles`], and the grapple with the snatch under [`jc3vr_grapple`]. The
//!   data the render halves of the overlay and the scope read, and the cockpit transform, live in
//!   the view crate, under [`jc3vr_view::controllers`]; the passes that draw them live in the
//!   render crate, which this crate does not depend on.
//! - [`hooks`]: the gameplay detours: the input tick's look detour, the sim step, and the online
//!   managers the sim tick runs.
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
//! - The settings: each subsystem declares the [`ConfigSlot`](jc3vr_core::config::ConfigSlot)
//!   for its group beside the group's type (one per controllers group, such as
//!   [`jc3vr_weapons::controllers::weapons::config::WEAPONS_CONFIG`]), and the `controllers` slot
//!   binds them wherever it is bound. A subsystem reads only its own group and the groups of the
//!   subsystems it builds on, through their slots; the whole `controllers` table
//!   ([`controllers::config::CONTROLLERS_CONFIG`]) is read only by the bindings, the summary, and
//!   the layers above.
//!
//! The `profiler` feature adds the sim step's profiler scopes. The crate's own tests register a
//! configuration root of their own (`test_config`), since the payload's aggregate is not there,
//! and serialize over the view crate's input history through `test_support`.

pub mod controllers;
pub mod hooks;

pub use crate::controllers::register;

#[cfg(test)]
mod test_config;
#[cfg(test)]
mod test_support;
