//! The loadout: the one source of truth for the local player's items, as the two-handed
//! inventory's "The architecture" describes it (`docs/mod/input/two-handed-inventory.md`).
//!
//! Every item the player has is in exactly one place: the left hand, the right hand, a socket of
//! the body, or the world near the player. The core is pure and tested without the game:
//!
//! - [`item`]: what an item is, and its identity for the session;
//! - [`binding`]: the live object an item is bound to, the one place it meets the game;
//! - [`socket`]: the body's sockets, the size classes they take, and where an item goes when the
//!   player does not choose;
//! - [`table`]: the table, whose invariants are in its types, its snapshot, and the primary hand;
//! - [`transition`]: the player's and the game's moves, each with its cause, refused whole with a
//!   typed error;
//! - [`reset`]: the spawn reset's re-keying, and the table's snapshots at the game's caches;
//! - [`projection`]: what the game's slots should hold, and what the sidecar records;
//! - [`event`]: what the hooks and the hands tell the table, folded into transitions;
//! - [`sandbox`]: a table of the debug UI's own, driven by hand until the hooks feed the real one.
//!
//! Nothing is wired into the game yet: the old weapon handling
//! ([`crate::controllers::weapons`]) still runs, and moves onto this core in phase 3b.

mod binding;
mod event;
mod item;
mod projection;
mod reset;
pub mod sandbox;
mod socket;
mod table;
mod transition;

#[cfg(test)]
mod tests;

pub use event::Event;
pub use item::ItemKind;
pub use projection::SlotContent;
pub use reset::{CacheKind, ResetKind};
pub use socket::{BodyLayout, Socket};
pub use transition::{Give, GiveSource, LostWhy, Release, Removal, RemovalWhy, Take, TakeSource};
