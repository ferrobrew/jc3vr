//! Typed views of the erased root: a [`ConfigSlot`] names one part of the configuration (a
//! subsystem's settings), and its guards lock the whole root and deref to that part.
//!
//! A slot is declared where its type is known and bound once, before anything reads it, by the
//! code that knows the root's type: binding gives the slot its [`Projection`], the functions that
//! find its part in the root ([`projection!`](crate::config::projection)). Every slot shares the
//! store's one lock, so the [nesting rule](crate::config::store#lock-nesting) covers them all.

use std::{
    ops::{Deref, DerefMut},
    sync::OnceLock,
};

use parking_lot::Mutex;
use serde::{Serialize, de::DeserializeOwned};
#[cfg(any(test, feature = "test-support"))]
use toml::Value;

#[cfg(any(test, feature = "test-support"))]
use crate::config::test_root;
use crate::config::{
    automatic::{self, Observed},
    load::Reset,
    persist,
    store::{self, Held, MappedGuard, Root, RootGuard},
};

/// One part of the configuration, of type `T`, at `path` in the settings file.
pub struct ConfigSlot<T: 'static> {
    path: &'static str,
    projection: OnceLock<Projection<T>>,
    options: SlotOptions<T>,
}

/// What a configuration root needs to know about a slot's part beyond its place: the default of a
/// single setting whose type's own default is not the setting's
/// ([`defaults_to`](Self::defaults_to)), the slots of the groups nested in the part, bound
/// wherever the slot is ([`with_children`](Self::with_children)), and the checks a load runs on
/// the part ([`validated`](Self::validated)).
pub struct SlotOptions<T: 'static> {
    /// Read only by a test root, which has no aggregate to default its parts from.
    #[cfg_attr(not(any(test, feature = "test-support")), allow(dead_code))]
    default: Option<fn() -> T>,
    children: Option<fn()>,
    validate: Option<Validate<T>>,
}

/// The checks a load runs on a slot's part, at the slot's path, pushing each reset.
pub type Validate<T> = fn(&mut T, &str, &mut Vec<Reset>);

/// What a slot's part must be for a root to default, copy, save, and load it.
pub trait SlotValue: Clone + Default + Serialize + DeserializeOwned + Send + 'static {}

impl<T: Clone + Default + Serialize + DeserializeOwned + Send + 'static> SlotValue for T {}

impl<T: 'static> SlotOptions<T> {
    pub const fn new() -> Self {
        Self {
            default: None,
            children: None,
            validate: None,
        }
    }

    /// `default` as the part's default, for a single setting whose type's default (`false`,
    /// `0.0`) is not the setting's. A group's default is its type's.
    pub const fn defaults_to(self, default: fn() -> T) -> Self {
        Self {
            default: Some(default),
            ..self
        }
    }

    /// `children`, which binds the slots of the groups nested in the part relative to the slot
    /// ([`slot_children!`](crate::slot_children)), run whenever the slot is bound.
    pub const fn with_children(self, children: fn()) -> Self {
        Self {
            children: Some(children),
            ..self
        }
    }

    /// `validate` as the checks a load runs on the part.
    pub const fn validated(self, validate: Validate<T>) -> Self {
        Self {
            validate: Some(validate),
            ..self
        }
    }
}

impl<T: 'static> Default for SlotOptions<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: 'static> ConfigSlot<T> {
    /// A slot for the part at `path` (`hud`, `controllers`), not yet bound.
    pub const fn new(path: &'static str) -> Self {
        Self::with(path, SlotOptions::new())
    }

    /// A slot for the part at `path`, with `options`, not yet bound.
    pub const fn with(path: &'static str, options: SlotOptions<T>) -> Self {
        Self {
            path,
            projection: OnceLock::new(),
            options,
        }
    }

    /// The part's path in the settings file; empty for the whole root.
    pub fn path(&self) -> &'static str {
        self.path
    }

    /// Give the slot its projection and bind its children through it. Binding a slot twice is a
    /// programming error and panics.
    pub fn bind(&'static self, projection: Projection<T>)
    where
        T: SlotValue,
    {
        assert!(
            self.projection.set(projection).is_ok(),
            "config: the slot `{}` is bound twice",
            self.path
        );
        BOUND.lock().push(self);
        if let Some(children) = self.options.children {
            children();
        }
    }

    /// Lock the configuration to read the part, or to change it on the payload's own account (a
    /// fallback, a value discovered at runtime), which is not a reason to save. The guard derefs
    /// to the part; see the [nesting rule](crate::config::store#lock-nesting).
    pub fn lock(&self) -> SlotGuard<T> {
        let (guard, held) = store::lock_mapped(self.projection().exclusive);
        SlotGuard { guard, _held: held }
    }

    /// Lock the configuration, run `f` against the part, and return the result. The lock is held
    /// only for `f`, so the read can sit inside a larger expression that locks the configuration
    /// again (`HUD.query(|h| h.quad) && VR.query(|v| v.mirror)`), which two [`lock`](Self::lock)
    /// temporaries in one statement cannot.
    pub fn query<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        f(&self.lock())
    }

    /// Lock the configuration for a change the user meant. The guard derefs to the part; when it
    /// drops, the configuration is marked changed and a save follows once the changes settle.
    pub fn edit(&self) -> EditGuard<T> {
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
    pub fn project<'a>(&self, root: &'a Root) -> &'a T {
        (self.projection().shared)(root)
    }

    /// The part's default: its value in the registered schema's defaults.
    pub fn default_value(&self) -> T
    where
        T: Clone,
    {
        self.project(store::schema().defaults().as_ref()).clone()
    }

    /// The part in `root`, which must be of the registered schema's type.
    pub fn project_mut<'a>(&self, root: &'a mut Root) -> &'a mut T {
        (self.projection().exclusive)(root)
    }

    fn projection(&self) -> &Projection<T> {
        self.projection.get().unwrap_or_else(|| unbound(self.path))
    }
}

/// Panic for a read of the unbound slot at `path`, naming the test root that should bind it when
/// one is registered.
#[cold]
fn unbound(path: &str) -> ! {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(module) = test_root::root_module() {
        panic!(
            "config: the slot `{path}` is not bound; the test root in `{module}` does not bind it, \
             so add it to that root's list"
        );
    }
    panic!(
        "config: the slot `{path}` is not bound; the payload binds every slot before reading the \
         configuration"
    )
}

/// The functions that find a slot's part in the root. Build one with
/// [`projection!`](crate::config::projection).
pub struct Projection<T: 'static> {
    shared: for<'a> fn(&'a Root) -> &'a T,
    exclusive: for<'a> fn(&'a mut Root) -> &'a mut T,
}

impl<T: 'static> Projection<T> {
    pub const fn new(
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
#[macro_export]
macro_rules! projection {
    ($root:ty $(, $($field:ident).+)?) => {
        $crate::config::Projection::new(
            |root| &(*$crate::config::downcast_ref::<$root>(root))$($(.$field)+)?,
            |root| &mut (*$crate::config::downcast_mut::<$root>(root))$($(.$field)+)?,
        )
    };
}
pub use crate::projection;

/// A closure that binds each listed slot to its field in the part of `$parent`, wherever
/// `$parent` is bound: give it to [`SlotOptions::with_children`] on the parent's declaration.
///
/// ```ignore
/// pub static HUD_CONFIG: ConfigSlot<HudConfig> = ConfigSlot::with(
///     "hud",
///     SlotOptions::new().with_children(slot_children!(HUD_CONFIG { PANEL_CONFIG => egui_panel })),
/// );
/// ```
///
/// Each child's path must be the parent's path followed by the field path; a child at another
/// path panics when the parent is bound.
#[macro_export]
macro_rules! slot_children {
    ($parent:path { $($slot:path => $($field:ident).+),* $(,)? }) => {
        || {
            $(
                $crate::config::bind_child(
                    &$parent,
                    &$slot,
                    &[$(::std::stringify!($field)),+],
                    $crate::config::Projection::new(
                        |root| &$parent.project(root)$(.$field)+,
                        |root| &mut $parent.project_mut(root)$(.$field)+,
                    ),
                );
            )*
        }
    };
}

/// Bind `child` to the field path `fields` in the part of `parent`, through `projection`; see
/// [`slot_children!`](crate::slot_children).
#[doc(hidden)]
pub fn bind_child<P: 'static, T: SlotValue>(
    parent: &'static ConfigSlot<P>,
    child: &'static ConfigSlot<T>,
    fields: &[&str],
    projection: Projection<T>,
) {
    let path = format!("{}.{}", parent.path(), fields.join("."));
    assert!(
        child.path() == path,
        "config: the slot `{}` is bound as `{path}`, a child of `{}`; a slot is bound at its own \
         path",
        child.path(),
        parent.path(),
    );
    child.bind(projection);
}

/// Run the checks of every bound slot that has them on `root`, in the order the slots were bound,
/// pushing each reset. `root` must be of the registered schema's type.
pub(crate) fn validate_bound(root: &mut Root, resets: &mut Vec<Reset>) {
    let bound = BOUND.lock().clone();
    for slot in bound {
        slot.validate(root, resets);
    }
}

/// A bound slot, with its part's type erased: what a root that knows its parts only by their
/// slots needs to default, copy, save, load, and check each part.
pub(crate) trait Bound: Sync {
    #[cfg(any(test, feature = "test-support"))]
    fn path(&self) -> &'static str;

    /// The part's default: the slot's declared default, or its type's.
    #[cfg(any(test, feature = "test-support"))]
    fn default_part(&self) -> Box<Root>;

    #[cfg(any(test, feature = "test-support"))]
    fn clone_part(&self, part: &Root) -> Box<Root>;

    #[cfg(any(test, feature = "test-support"))]
    fn part_to_value(&self, part: &Root) -> Result<Value, toml::ser::Error>;

    #[cfg(any(test, feature = "test-support"))]
    fn part_from_value(&self, value: Value) -> Result<Box<Root>, toml::de::Error>;

    /// The part as it is in `root`, as a value.
    #[cfg(any(test, feature = "test-support"))]
    fn value_in(&self, root: &Root) -> Result<Value, toml::ser::Error>;

    fn validate(&self, root: &mut Root, resets: &mut Vec<Reset>);
}

impl<T: SlotValue> Bound for ConfigSlot<T> {
    #[cfg(any(test, feature = "test-support"))]
    fn path(&self) -> &'static str {
        self.path
    }

    #[cfg(any(test, feature = "test-support"))]
    fn default_part(&self) -> Box<Root> {
        Box::new(
            self.options
                .default
                .map_or_else(T::default, |default| default()),
        )
    }

    #[cfg(any(test, feature = "test-support"))]
    fn clone_part(&self, part: &Root) -> Box<Root> {
        Box::new(part_ref::<T>(part).clone())
    }

    #[cfg(any(test, feature = "test-support"))]
    fn part_to_value(&self, part: &Root) -> Result<Value, toml::ser::Error> {
        Value::try_from(part_ref::<T>(part))
    }

    #[cfg(any(test, feature = "test-support"))]
    fn part_from_value(&self, value: Value) -> Result<Box<Root>, toml::de::Error> {
        Ok(Box::new(value.try_into::<T>()?))
    }

    #[cfg(any(test, feature = "test-support"))]
    fn value_in(&self, root: &Root) -> Result<Value, toml::ser::Error> {
        Value::try_from(self.project(root))
    }

    fn validate(&self, root: &mut Root, resets: &mut Vec<Reset>) {
        if let Some(validate) = self.options.validate {
            validate(self.project_mut(root), self.path, resets);
        }
    }
}

/// The paths of the bound slots whose default (the slot's declared default, or its type's, which a
/// test root gives the part) differs from the part in `root`, a root of defaults. The payload's
/// tests check its aggregate against every slot with it, so a test root defaults each part as the
/// aggregate does.
#[cfg(any(test, feature = "test-support"))]
pub fn mismatched_defaults(root: &Root) -> Vec<&'static str> {
    let bound = BOUND.lock().clone();
    bound
        .into_iter()
        .filter(|slot| {
            let default = slot.part_to_value(slot.default_part().as_ref());
            !slot.path().is_empty() && default.ok() != slot.value_in(root).ok()
        })
        .map(|slot| slot.path())
        .collect()
}

#[cfg(any(test, feature = "test-support"))]
fn part_ref<T: 'static>(part: &Root) -> &T {
    part.downcast_ref()
        .expect("config: a part is not of its slot's type")
}

static BOUND: Mutex<Vec<&'static dyn Bound>> = Mutex::new(Vec::new());

/// The guard [`ConfigSlot::lock`] returns.
pub struct SlotGuard<T: 'static> {
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
pub struct EditGuard<T: 'static> {
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
