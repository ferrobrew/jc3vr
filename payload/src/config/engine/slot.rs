//! Typed views of the erased root: a [`ConfigSlot`] names one part of the configuration (a
//! subsystem's settings), and its guards lock the whole root and deref to that part.
//!
//! A slot is declared where its type is known and bound once, before anything reads it, by the
//! code that knows the root's type: binding gives the slot its [`Projection`], the functions that
//! find its part in the root ([`projection!`](crate::config::engine::projection)). Every slot
//! shares the store's one lock, so the [nesting rule](crate::config::engine::store#lock-nesting)
//! covers them all.

use std::{
    ops::{Deref, DerefMut},
    sync::OnceLock,
};

use crate::config::engine::{
    automatic::{self, Observed},
    persist,
    store::{self, Held, MappedGuard, Root, RootGuard},
};

/// One part of the configuration, of type `T`, at `path` in the settings file.
pub(crate) struct ConfigSlot<T: 'static> {
    path: &'static str,
    projection: OnceLock<Projection<T>>,
}

impl<T: 'static> ConfigSlot<T> {
    /// A slot for the part at `path` (`hud`, `controllers`), not yet bound.
    pub(crate) const fn new(path: &'static str) -> Self {
        Self {
            path,
            projection: OnceLock::new(),
        }
    }

    /// The part's path in the settings file; empty for the whole root.
    #[cfg(test)]
    pub(crate) fn path(&self) -> &'static str {
        self.path
    }

    /// Give the slot its projection. Binding a slot twice is a programming error and panics.
    pub(crate) fn bind(&self, projection: Projection<T>) {
        assert!(
            self.projection.set(projection).is_ok(),
            "config: the slot `{}` is bound twice",
            self.path
        );
    }

    /// Lock the configuration to read the part, or to change it on the payload's own account (a
    /// fallback, a value discovered at runtime), which is not a reason to save. The guard derefs
    /// to the part; see the [nesting rule](crate::config::engine::store#lock-nesting).
    pub(crate) fn lock(&self) -> SlotGuard<T> {
        let (guard, held) = store::lock_mapped(self.projection().exclusive);
        SlotGuard { guard, _held: held }
    }

    /// Lock the configuration, run `f` against the part, and return the result. The lock is held
    /// only for `f`, so the read can sit inside a larger expression that locks the configuration
    /// again (`HUD.query(|h| h.quad) && VR.query(|v| v.mirror)`), which two [`lock`](Self::lock)
    /// temporaries in one statement cannot.
    pub(crate) fn query<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        f(&self.lock())
    }

    /// Lock the configuration for a change the user meant. The guard derefs to the part; when it
    /// drops, the configuration is marked changed and a save follows once the changes settle.
    pub(crate) fn edit(&self) -> EditGuard<T> {
        let projection = *self.projection();
        let mut root = store::lock_root();
        let before = automatic::observe(&mut *root);
        EditGuard {
            root,
            projection,
            before,
        }
    }

    /// The part in `root`, which must be of the registered schema's type.
    pub(crate) fn project<'a>(&self, root: &'a Root) -> &'a T {
        (self.projection().shared)(root)
    }

    /// The part's default: its value in the registered schema's defaults.
    pub(crate) fn default_value(&self) -> T
    where
        T: Clone,
    {
        self.project(store::schema().defaults().as_ref()).clone()
    }

    /// The part in `root`, which must be of the registered schema's type.
    pub(crate) fn project_mut<'a>(&self, root: &'a mut Root) -> &'a mut T {
        (self.projection().exclusive)(root)
    }

    fn projection(&self) -> &Projection<T> {
        self.projection.get().unwrap_or_else(|| {
            panic!(
                "config: the slot `{}` is not bound; the payload binds every slot before \
                 reading the configuration",
                self.path
            )
        })
    }
}

/// The functions that find a slot's part in the root. Build one with
/// [`projection!`](crate::config::engine::projection).
pub(crate) struct Projection<T: 'static> {
    shared: for<'a> fn(&'a Root) -> &'a T,
    exclusive: for<'a> fn(&'a mut Root) -> &'a mut T,
}

impl<T: 'static> Projection<T> {
    pub(crate) const fn new(
        shared: for<'a> fn(&'a Root) -> &'a T,
        exclusive: for<'a> fn(&'a mut Root) -> &'a mut T,
    ) -> Self {
        Self { shared, exclusive }
    }
}

impl<T: 'static> Clone for Projection<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> Copy for Projection<T> {}

/// The [`Projection`] of the field path `$field` (`controllers.input`) in a root of type `$root`,
/// or of the whole root when no path is given.
macro_rules! projection {
    ($root:ty $(, $($field:ident).+)?) => {
        $crate::config::engine::Projection::new(
            |root| &(*$crate::config::engine::downcast_ref::<$root>(root))$($(.$field)+)?,
            |root| &mut (*$crate::config::engine::downcast_mut::<$root>(root))$($(.$field)+)?,
        )
    };
}
pub(crate) use projection;

/// The guard [`ConfigSlot::lock`] returns.
pub(crate) struct SlotGuard<T: 'static> {
    guard: MappedGuard<T>,
    _held: Held,
}

impl<T: 'static> Deref for SlotGuard<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.guard
    }
}

impl<T: 'static> DerefMut for SlotGuard<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.guard
    }
}

/// The guard [`ConfigSlot::edit`] returns. Holding it through a frame of debug UI without changing
/// anything is harmless: the save compares the serialized text with the file's and writes nothing
/// when they match.
pub(crate) struct EditGuard<T: 'static> {
    root: RootGuard,
    projection: Projection<T>,
    before: Observed,
}

impl<T: 'static> Deref for EditGuard<T> {
    type Target = T;

    fn deref(&self) -> &T {
        (self.projection.shared)(&*self.root)
    }
}

impl<T: 'static> DerefMut for EditGuard<T> {
    fn deref_mut(&mut self) -> &mut T {
        (self.projection.exclusive)(&mut *self.root)
    }
}

impl<T: 'static> Drop for EditGuard<T> {
    fn drop(&mut self) {
        automatic::settle(&mut *self.root, &self.before);
        persist::note_edit();
    }
}
