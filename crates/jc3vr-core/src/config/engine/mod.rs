//! The configuration engine: the store and its typed slots, the tolerant load, the save, and the
//! automatic switches, written against a registered [`Schema`] rather than any subsystem's
//! settings, so that it names nothing above it.
//!
//! The configuration is one root value behind one lock ([`store`]). The code that knows the
//! root's type registers its [`Schema`] (its defaults, serialization, migration, and validation,
//! from a [`ConfigRoot`] impl) and binds each [`ConfigSlot`] to the part of the root it names,
//! before anything reads the configuration. A slot's guards lock the whole root, so a guard is a
//! consistent snapshot, and the [nesting rule](store#lock-nesting) applies to every slot at once.
//!
//! The settings file is loaded once at startup ([`init`], [`load`]), saved from the game thread's
//! frame once edits settle ([`tick`], [`persist`]), and flushed at [`shutdown`]. A save writes the
//! settings that differ from their defaults ([`format`]), with the user's choice for each switch
//! the payload turned off on its own ([`automatic`]).

pub(crate) mod automatic;
pub mod format;
pub mod load;
mod parts;
pub mod paths;
pub mod persist;
mod slot;
pub mod store;
#[cfg(any(test, feature = "test-support"))]
pub mod test_root;
mod validate;
pub mod write;

pub use crate::config::engine::{
    automatic::{Switch, disable_automatically},
    parts::{Parts, Snapshot, edit_parts, lock_parts},
    persist::{SaveStatus, init, shutdown, status, tick},
    slot::{
        ConfigSlot, EditGuard, Projection, SlotGuard, SlotOptions, SlotValue, Validate, bind_child,
        projection,
    },
    store::{ConfigRoot, Schema, downcast_mut, downcast_ref, register},
    validate::{Invalid, nonnegative, positive, range, vec2, vec3},
};
#[cfg(any(test, feature = "test-support"))]
pub use crate::config::engine::{
    persist::{TestFixture, test_fixture},
    slot::mismatched_defaults,
};

#[cfg(test)]
mod tests;
