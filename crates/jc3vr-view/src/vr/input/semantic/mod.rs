//! The semantic actions: OpenXR actions named for what they do in the game (`jump`, `reload`,
//! `accelerate`) rather than for the physical input they read, so that the runtime's binding
//! interface shows the player game meanings.
//!
//! There is one action set per control context plus `global`. `global` is always active; of the
//! others exactly one is, chosen from the mode ([`ActionSetKind`]): `on_foot` (on foot, swimming,
//! falling, and the parachute), `wingsuit`, `land_vehicle` (cars, motorcycles, boats, and
//! unresolved vehicles), `helicopter`, `plane`, and `menu` (the game's menus and the native map).
//! Every set has priority 0, the same as the always-active `hands` set that holds the raw inputs,
//! because a runtime ignores a lower-priority set's binding on an input a higher-priority set also
//! binds, and the raw and semantic actions share their inputs.
//!
//! Every action is created with both hands as subaction paths and read per hand, so the hand a
//! press came from stays known after the player remaps it.
//!
//! - [`jc3vr_input::semantic::vocabulary`] declares each set's actions, their types, and their
//!   localized names.
//! - [`defaults`] holds the suggested bindings on the Index and Touch profiles, written in terms
//!   of the dominant hand and the off-hand, and the left-handed table mirrored from the
//!   right-handed one.
//! - [`frame`] publishes each frame's per-hand state of the active set's and `global`'s actions.
//!
//! The binding tables read the semantic state ([`crate::controllers::bindings::intents`]); the
//! gestures, grabs, chords, and pointers read the raw inputs of the `hands` set.
//!
//! - [`bound`] reduces the runtime's bound sources to the physical inputs each action reads on
//!   each hand, which the bindings' masks act on.

pub mod admission;
pub mod bound;
pub mod defaults;
pub mod frame;
pub mod history;

#[cfg(test)]
mod tests;

/// The version of the action schema: the sets, their actions, and the actions' types. It is the
/// OpenXR application version, so a runtime that keys saved bindings on it can tell the schemas
/// apart. A change to the schema bumps it and adds an entry to the changelog in
/// `docs/mod/input/controllers.md` ("Action schema").
pub const SCHEMA_VERSION: u32 = 6;
