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
//! - [`live`]: the one live table of the local player's items, dual-written beside the old weapon
//!   handling and published each input tick;
//! - [`sandbox`]: a table of the debug UI's own, driven by hand with made-up objects.
//!
//! Only the readouts and the pins of the weapons lying in the world read the live table yet: the
//! old weapon handling ([`crate::controllers::weapons`]) still moves the weapons, and moves onto
//! this core step by step in phase 3b.

mod binding;
mod event;
mod item;
pub mod live;
mod projection;
mod reset;
pub mod sandbox;
mod socket;
mod table;
mod transition;

#[cfg(test)]
mod tests;

pub use binding::ObjectAddress;
pub use event::Event;
pub use item::{ItemId, ItemKind, WeaponType};
pub use projection::{SlotContent, StoreProjection};
pub use reset::{CacheKind, ResetKind, SlotObject};
pub use socket::{BodyLayout, Socket};
pub use table::{LoadoutSnapshot, Place};
pub use transition::{
    Give, GiveSource, LostWhy, Release, Removal, RemovalWhy, Restored, Take, TakeSource,
};
