//! The payload's gameplay layer: everything that changes how the game plays in the headset, built
//! on the render layer's runtime and the input layer's controller frame, and naming nothing above
//! it.
//!
//! The crate sits above the core, input, and render crates (`docs/architecture.md`, "Crate
//! layering") and owns:
//!
//! - [`controllers`]: the controller subsystems: the bindings and the mode detector, the
//!   configuration and the clock, the haptics, the weapons with the loadout and the glass, the
//!   physical hands, the body, the vehicles, the hands, fingers, presentation, grab, and aim,
//!   locomotion and roomscale, the snatch, the overlay producers, the scope state, the summary
//!   model, the telemetry notes, and the spawn table. The render halves of the overlay and the
//!   scope (the passes) and the cockpit transform live in the render crate, under
//!   [`jc3vr_render::controllers`], and are named there.
//! - [`grapple`]: the grapple's device, its physical rope and swing, and its seats.
//! - [`hooks`]: the gameplay detours (the character and its pose, the animation graph, the input
//!   tick, the equipment, and the sim step) and the stance height the render crate's camera adjust
//!   applies.
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
//!   participants, and must run before the hooks are installed.
//! - The hook libraries, which the payload enables in its own interleaved order with the other
//!   layers' (the order is load-bearing): [`hooks::sim::hook_library`],
//!   [`hooks::equipment::hook_library`], [`hooks::character::hook_library`],
//!   [`hooks::animation::hook_library`], [`hooks::input::hook_library`],
//!   [`controllers::hook_library`] (which takes the HUD's controls map library to enable in its
//!   place), [`controllers::physical_hands::human_hull::hook_library`],
//!   [`grapple::physical::hook_library`], and [`grapple::device::hook_library`].
//! - The settings: each subsystem declares the [`ConfigSlot`](jc3vr_core::config::engine::ConfigSlot)
//!   for its group beside the group's type ([`controllers::body::BODY_CONFIG`],
//!   [`hooks::input::config::MOVEMENT_CONFIG`], [`hooks::character::config::BODY_IK_CONFIG`], and
//!   one per controllers group, such as [`controllers::weapons::config::WEAPONS_CONFIG`]), and the
//!   payload binds them to its aggregate. A subsystem reads only its own group and the groups of
//!   the subsystems it builds on, through their slots or a field slot onto one setting
//!   ([`controllers::hands::config::MOUNTED_AIM_BY_HANDS`]); the whole `controllers` table
//!   ([`controllers::config::CONTROLLERS_CONFIG`]) is read only by the bindings, the summary, and
//!   the layers above.
//!
//! The `profiler` feature adds the sim step's profiler scopes. The crate's own tests register a
//! configuration root of their own (`test_config`), since the payload's aggregate is not there, and
//! the body-bound subsystems' tests share the fixtures in `test_support`.

pub mod controllers;
pub mod grapple;
pub mod hooks;

pub use crate::controllers::install;

#[cfg(test)]
mod test_config;
#[cfg(test)]
mod test_support;
