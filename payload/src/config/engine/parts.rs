//! Several slots' parts of one configuration root, read or changed together without naming the
//! root's type: the live configuration under one lock ([`lock_parts`], [`edit_parts`]), or a copy
//! of it ([`Snapshot`]) that is changed on the side and put back whole, or serialized into a
//! capture's manifest as the root type itself would serialize.
//!
//! A single slot's guard is the usual access. These are for the code that works on the whole
//! configuration at once: a capture harness that pins a profile across subsystems and restores the
//! settings afterwards, or a debug UI tab whose controls span several groups. Like every guard,
//! [`lock_parts`] and [`edit_parts`] hold the one configuration lock, so the
//! [nesting rule](crate::config::engine::store#lock-nesting) applies to them.

use serde::{Serialize, Serializer};

use crate::config::engine::{
    automatic::{self, Observed},
    persist,
    slot::ConfigSlot,
    store::{self, Root, RootGuard},
};

/// Access to slots' parts of one configuration root.
pub(crate) trait Parts {
    fn root(&self) -> &Root;

    fn root_mut(&mut self) -> &mut Root;

    /// The part `slot` names.
    fn part<T>(&self, slot: &'static ConfigSlot<T>) -> &T {
        slot.project(self.root())
    }

    /// The part `slot` names, to change.
    fn part_mut<T>(&mut self, slot: &'static ConfigSlot<T>) -> &mut T {
        slot.project_mut(self.root_mut())
    }
}

/// Lock the whole configuration to read several parts, or to change them on the payload's own
/// account, which is not a reason to save.
pub(crate) fn lock_parts() -> PartsGuard {
    PartsGuard {
        root: store::lock_root(),
    }
}

/// Lock the whole configuration for a change the user meant. When the guard drops, the
/// configuration is marked changed and a save follows once the changes settle.
pub(crate) fn edit_parts() -> PartsEdit {
    let mut root = store::lock_root();
    let before = automatic::observe(&mut *root);
    PartsEdit { root, before }
}

/// The guard [`lock_parts`] returns.
pub(crate) struct PartsGuard {
    root: RootGuard,
}

impl PartsGuard {
    /// Replace the whole configuration with `snapshot`.
    pub(crate) fn restore(&mut self, snapshot: &Snapshot) {
        self.root
            .replace(store::schema().clone_root(snapshot.root.as_ref()));
    }
}

impl Parts for PartsGuard {
    fn root(&self) -> &Root {
        &*self.root
    }

    fn root_mut(&mut self) -> &mut Root {
        &mut *self.root
    }
}

/// The guard [`edit_parts`] returns.
pub(crate) struct PartsEdit {
    root: RootGuard,
    before: Observed,
}

impl Parts for PartsEdit {
    fn root(&self) -> &Root {
        &*self.root
    }

    fn root_mut(&mut self) -> &mut Root {
        &mut *self.root
    }
}

impl Drop for PartsEdit {
    fn drop(&mut self) {
        automatic::settle(&mut *self.root, &self.before);
        persist::note_edit();
    }
}

/// A copy of the whole configuration.
pub(crate) struct Snapshot {
    root: Box<Root>,
}

impl Snapshot {
    /// Copy the live configuration.
    pub(crate) fn take() -> Self {
        Self::of(&lock_parts())
    }

    /// Copy the configuration `parts` holds.
    pub(crate) fn of(parts: &impl Parts) -> Self {
        Self {
            root: store::schema().clone_root(parts.root()),
        }
    }

    /// The configuration's defaults.
    #[cfg(test)]
    pub(crate) fn defaults() -> Self {
        Self {
            root: store::schema().defaults(),
        }
    }

    /// Every setting as TOML text, for a capture's manifest or a comparison.
    pub(crate) fn to_toml(&self) -> Result<String, toml::ser::Error> {
        store::schema().serialize(self.root.as_ref())
    }

    /// Replace the live configuration with this copy, on the payload's own account.
    pub(crate) fn restore(&self) {
        lock_parts().restore(self);
    }
}

impl Clone for Snapshot {
    fn clone(&self) -> Self {
        Self {
            root: store::schema().clone_root(self.root.as_ref()),
        }
    }
}

/// Serializes as the registered root type does, through the same serializer calls, so a
/// snapshot nested in a larger document (a trace's manifest, a capture's metadata) emits the same
/// text as the typed configuration would, in any format and at any indentation.
impl Serialize for Snapshot {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        erased_serde::serialize(store::schema().as_serialize(self.root.as_ref()), serializer)
    }
}

impl Parts for Snapshot {
    fn root(&self) -> &Root {
        self.root.as_ref()
    }

    fn root_mut(&mut self) -> &mut Root {
        self.root.as_mut()
    }
}
