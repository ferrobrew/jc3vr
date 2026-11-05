//! The loadout: the one source of truth for the local player's items, as the two-handed
//! inventory's "The architecture" describes it (`docs/mod/input/two-handed-inventory.md`).
//!
//! Every item the player has is in exactly one place: the left hand, the right hand, a socket of
//! the body, or the world near the player. The core is pure and tested without the game:
//!
//! - [`item`]: what an item is, and its identity for the session;
//! - [`binding`]: the live object or actor-free model identity an item is bound to;
//! - [`safe_items`]: checked actor-free grenade admission, movement, and retirement;
//! - [`socket`]: the body's sockets, the size classes they take, and where an item goes when the
//!   player does not choose;
//! - [`table`]: the table, whose invariants are in its types, its snapshot, and the primary hand;
//! - [`transition`]: the player's and the game's moves, each with its cause, refused whole with a
//!   typed error;
//! - [`reset`]: the spawn reset's re-keying, and the table's snapshots at the game's caches;
//! - [`projection`]: what the game's slots should hold, and what the sidecar records;
//! - [`event`]: what the hooks and the hands tell the table, folded into transitions;
//! - [`grab`]: what a hand does at a socket, decided against the table's snapshot;
//! - [`take`]: where a weapon taken from the world goes, decided against the table's snapshot;
//! - [`live`]: the one live table of the local player's items, fed by the game and the hands and
//!   published each input tick. It is the only writer of where the player's weapons are: the
//!   game's slots, the placement, and the pins follow it.

mod binding;
mod event;
pub mod grab;
mod item;
pub mod live;
mod projection;
mod reset;
pub(crate) mod restore;
pub mod safe_items;
mod socket;
mod table;
pub mod take;
mod transition;

#[cfg(test)]
mod tests;

pub use binding::{Binding, ObjectAddress};
pub use event::Event;
pub use item::{ItemId, ItemKind, WeaponType, slot_label};
pub use projection::{Projection, SLOTS, SlotContent, StoreProjection};
pub use reset::{CacheKind, ResetKind, SlotObject};
pub use socket::{BodyLayout, Socket};
#[cfg(test)]
pub(crate) use table::Loadout;
pub use table::{Departure, ItemView, LoadoutSnapshot, Place};
pub use transition::{GiveSource, LostWhy, Restored, TakeSource};
