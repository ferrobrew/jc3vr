//! The weapons layer's controller subsystems: the weapon handling and the loadout table it keeps,
//! following the plan of record in `docs/mod/input/controllers-and-roomscale.md`.
//!
//! - [`weapons`]: physical weapon handling: the holsters and grab volumes, the two-handed
//!   foregrip, the weapon actions that run with the hands on the controllers, recoil, and the
//!   dual-wield split;
//! - [`loadout`]: the loadout, the one source of truth for the local player's items: the one place
//!   each item is (a hand, a socket of the body, or the world near the player), and the moves
//!   between them;
//! - [`spawn`]: the spawn system's requests and the shipped spawn definition table, for the
//!   inventory's gives and the director's spawner.

pub mod loadout;
pub mod spawn;
pub mod weapons;
