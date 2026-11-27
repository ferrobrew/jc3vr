//! The payload's grapple layer: the grapple and the grapple snatch, built on the body, weapons,
//! and physical layers and naming nothing beside or above them.
//!
//! The crate sits above the core, input, view, body, weapons, and physical crates and below the
//! gameplay crate, beside the vehicles crate with no edge between them (`docs/architecture.md`,
//! "Crate layering"), and owns:
//!
//! - [`grapple`]: the grapple's comfort filter on the head pose, its device, its physical rope,
//!   pull, swing, and lock, the free hand's hold on the line, and the seat grapple;
//! - [`controllers::grapple_button`] and [`controllers::yank`]: the grapple button's gestures and
//!   the yank gesture, which the bindings read for the grapple and the snatch;
//! - [`controllers::snatch`]: the grapple snatch, which pulls a weapon, a loose object, or a car
//!   door out of reach into the empty grapple hand on the game's hook;
//! - [`register()`]: what the grapple contributes to the layers below it, registered in one place.
//!
//! # The public surface
//!
//! The layers above reach these modules by path, so an item is `pub` exactly when a crate above
//! names it: the bindings, which run the grapple's and the snatch's ticks in their game stage, the
//! gameplay crate's register files, the debug UI's panels and status rows, the diagnostics, and the
//! payload's composition and tuning loader. Everything else keeps its crate-private visibility.
//!
//! The payload's composition enables the hook libraries, [`controllers::snatch::hook_library`],
//! [`grapple::physical::hook_library`], and [`grapple::device::hook_library`], in its order with
//! the other layers', and calls [`register()`] once before the hooks are installed. The grapple has
//! no settings group: its tuning is read from the crate's tuning files, `tuning/grapple.toml`
//! ([`grapple::tuning::GRAPPLE`]) and `tuning/snatch.toml`
//! ([`controllers::snatch::config::SNATCH`]).
//!
//! The crate's own tests register a configuration root of their own (`test_config`), since the
//! payload's aggregate is not there.

pub mod controllers;
pub mod grapple;
mod register;

pub use register::register;

#[cfg(test)]
mod test_config;
