//! The payload's body layer: the player's body and everything built directly on it, which the
//! weapon handling, the physical hands, the vehicles, the grapple, and the bindings above it share.
//!
//! The crate sits above the core, input, and view crates and below the gameplay crate
//! (`docs/architecture.md`, "Crate layering"), and owns:
//!
//! - [`controllers`]: the body-bound controller subsystems: the body, the hands, fingers, and
//!   presentation, the grip model, the aim and the aim-state pin, locomotion and roomscale, the mode
//!   detector and the scheme, the clock, the haptics, the overlay producers, the spawn table, the
//!   telemetry notes, and the shared modules the subsystems above meet in (the input vocabulary in
//!   `intent`, the held-item facts in `held`, the physics substrate in `physics`, and the published
//!   facts in `facts`). Each settings group these subsystems own has its slot beside its type.
//! - [`hooks`]: the character, animation, and input detours, without the look detour, which lands
//!   the bindings' writes and stays in the gameplay crate.
//!
//! # The public surface
//!
//! The layers above reach these modules by path, so an item is `pub` exactly when a crate above
//! names it. The subsystems above contribute to the body through registrations and facts rather
//! than being named from here: the hands' stage table and late observers, the aim's weapon hooks,
//! the climb's readers, the palms' push, the body mirror's attachment sampler, and the character
//! hooks' blockers and consumers. The gameplay crate fills them at install and composes the hook
//! libraries in their established order. The crate's own tests register a configuration root of
//! their own (`test_config`) holding the groups the body-bound subsystems read, and share the
//! fixtures in `test_support`.

pub mod controllers;
pub mod hooks;

#[cfg(test)]
mod test_config;
#[cfg(test)]
mod test_support;
