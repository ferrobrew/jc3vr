//! The store: one configuration root behind one lock, its type erased, with the functions the
//! engine needs to default, copy, serialize, deserialize, upgrade, and validate it registered as a
//! [`Schema`].
//!
//! The root is the whole configuration, so a guard over it (or over one of its parts, through a
//! [`ConfigSlot`](crate::config::engine::ConfigSlot)) is a consistent snapshot: every read through
//! one guard sees the same configuration, and a change made through one is seen whole by the next
//! lock.
//!
//! # Lock nesting
//!
//! The lock is not reentrant, and every slot shares it, so a thread that locks the configuration
//! while it holds a guard deadlocks. That includes two guards in one statement
//! (`HUD.lock().distance > VR.lock().world_scale`), whose temporaries live to the statement's end.
//! The rule: take one guard per scope, read every part needed through it, and drop it before
//! anything else locks the configuration. To read parts of different slots together, lock the
//! slot that holds them both, or copy the first part out (`let distance = HUD.lock().distance;`)
//! before locking the second. In a debug build, a second lock on the same thread panics with a
//! message naming this rule instead of deadlocking.

#[cfg(debug_assertions)]
use std::cell::Cell;
use std::{
    any::Any,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    ptr,
    sync::OnceLock,
};

use parking_lot::{Mutex, MutexGuard};
use serde::{Serialize, de::DeserializeOwned};
use toml::{Table, Value};

use crate::config::engine::{
    format::{Format, Migrated},
    load::Reset,
};

/// The configuration root, with its type erased.
pub type Root = dyn Any + Send;

/// A configuration root's type: the whole configuration, as the settings file holds it.
pub trait ConfigRoot: Serialize + DeserializeOwned + Clone + Send + 'static {
    /// The defaults, which a setting the file lacks takes and which a save leaves out.
    fn defaults() -> Self;

    /// Upgrade a settings file's table, its version already taken out, from `format`: rename or
    /// remove the settings a later build replaced. Returns the settings it removed.
    fn migrate(file: &mut Table, format: Format) -> Vec<Migrated>;

    /// Reset the settings that fail their checks, pushing each reset.
    fn validate(&mut self, resets: &mut Vec<Reset>);

    /// A save is about to write a change; `text` holds every setting.
    fn note_saved(text: &str);
}

/// The functions the engine needs from a [`ConfigRoot`], with its type erased.
pub struct Schema {
    defaults: fn() -> Box<Root>,
    clone: fn(&Root) -> Box<Root>,
    serialize: fn(&Root) -> Result<String, toml::ser::Error>,
    to_value: fn(&Root) -> Result<Value, toml::ser::Error>,
    as_serialize: fn(&Root) -> &dyn erased_serde::Serialize,
    deserialize: fn(Value, &mut Ignored<'_>) -> Result<Box<Root>, DeserializeError>,
    migrate: fn(&mut Table, Format) -> Vec<Migrated>,
    validate: fn(&mut Root, &mut Vec<Reset>),
    note_saved: fn(&str),
}

/// The callback a deserialization reports each setting it skips to.
pub(crate) type Ignored<'a> = dyn FnMut(serde_ignored::Path<'_>) + 'a;

/// A deserialization's failure, at the path of the value it could not read.
pub(crate) type DeserializeError = serde_path_to_error::Error<toml::de::Error>;

impl Schema {
    /// The schema of the root type `C`.
    pub const fn of<C: ConfigRoot>() -> Self {
        Self {
            defaults: defaults_of::<C>,
            clone: clone_of::<C>,
            serialize: serialize_of::<C>,
            to_value: to_value_of::<C>,
            as_serialize: as_serialize_of::<C>,
            deserialize: deserialize_of::<C>,
            migrate: C::migrate,
            validate: validate_of::<C>,
            note_saved: C::note_saved,
        }
    }

    pub(crate) fn defaults(&self) -> Box<Root> {
        (self.defaults)()
    }

    pub(crate) fn clone_root(&self, root: &Root) -> Box<Root> {
        (self.clone)(root)
    }

    /// The root as TOML text.
    pub fn serialize(&self, root: &Root) -> Result<String, toml::ser::Error> {
        (self.serialize)(root)
    }

    /// The root serialized straight into a value tree, its floats widened from `f32`.
    pub(crate) fn to_value(&self, root: &Root) -> Result<Value, toml::ser::Error> {
        (self.to_value)(root)
    }

    /// The root as a value any serializer takes, emitting exactly what the root type's own
    /// `Serialize` emits.
    pub(crate) fn as_serialize<'a>(&self, root: &'a Root) -> &'a dyn erased_serde::Serialize {
        (self.as_serialize)(root)
    }

    /// Deserialize `value`, reporting each setting the root type does not know to `ignored`.
    pub(crate) fn deserialize(
        &self,
        value: Value,
        ignored: &mut Ignored<'_>,
    ) -> Result<Box<Root>, DeserializeError> {
        (self.deserialize)(value, ignored)
    }

    pub(crate) fn migrate(&self, file: &mut Table, format: Format) -> Vec<Migrated> {
        (self.migrate)(file, format)
    }

    pub fn validate(&self, root: &mut Root, resets: &mut Vec<Reset>) {
        (self.validate)(root, resets)
    }

    pub(crate) fn note_saved(&self, text: &str) {
        (self.note_saved)(text)
    }
}

/// Register the configuration's schema and put its defaults in the store. Every later call must
/// pass the same schema; registering another is a programming error and panics.
pub fn register(schema: &'static Schema) {
    if let Some(registered) = SCHEMA.get() {
        assert!(ptr::eq(*registered, schema), "{SECOND_SCHEMA}");
        return;
    }
    let _held = Held::acquire();
    let mut root = ROOT.lock();
    // The schema is set after the root, under the lock, so that a registered schema implies a
    // root.
    match SCHEMA.get() {
        Some(registered) => assert!(ptr::eq(*registered, schema), "{SECOND_SCHEMA}"),
        None => {
            *root = Some(schema.defaults());
            let _ = SCHEMA.set(schema);
        }
    }
}

/// The registered schema. Panics when none is registered.
pub fn schema() -> &'static Schema {
    SCHEMA.get().copied().expect(UNREGISTERED)
}

/// Lock the whole root. Panics when no schema is registered, and, in a debug build, when this
/// thread already holds the configuration lock.
pub(crate) fn lock_root() -> RootGuard {
    let held = Held::acquire();
    let guard = ROOT.lock();
    assert!(guard.is_some(), "{UNREGISTERED}");
    RootGuard { guard, _held: held }
}

/// A guard over the whole root.
pub(crate) struct RootGuard {
    guard: MutexGuard<'static, Option<Box<Root>>>,
    _held: Held,
}

impl RootGuard {
    /// Replace the root with `root`, which must be of the registered schema's type.
    pub(crate) fn replace(&mut self, root: Box<Root>) {
        *self.guard = Some(root);
    }
}

impl Deref for RootGuard {
    type Target = Root;

    fn deref(&self) -> &Root {
        self.guard.as_deref().expect(UNREGISTERED)
    }
}

impl DerefMut for RootGuard {
    fn deref_mut(&mut self) -> &mut Root {
        self.guard.as_deref_mut().expect(UNREGISTERED)
    }
}

/// Lock the root and map the guard to the part `project` picks, holding the nesting mark.
pub(crate) fn lock_mapped<T: 'static>(
    project: for<'a> fn(&'a mut Root) -> &'a mut T,
) -> (MappedGuard<T>, Held) {
    let held = Held::acquire();
    let guard = MutexGuard::map(ROOT.lock(), |root| {
        project(root.as_deref_mut().expect(UNREGISTERED))
    });
    (guard, held)
}

/// A guard over a part of the root.
pub(crate) type MappedGuard<T> = parking_lot::MappedMutexGuard<'static, T>;

/// The root as `C`, the registered schema's type. Panics when it is not.
pub fn downcast_ref<C: 'static>(root: &Root) -> &C {
    root.downcast_ref().expect(WRONG_TYPE)
}

/// The root as `C`, the registered schema's type. Panics when it is not.
pub fn downcast_mut<C: 'static>(root: &mut Root) -> &mut C {
    root.downcast_mut().expect(WRONG_TYPE)
}

/// The mark that this thread holds the configuration lock, which turns a nested lock into a panic
/// in a debug build ([the rule](self#lock-nesting)). It cannot leave its thread, so neither can a
/// guard that holds it.
pub(crate) struct Held {
    _thread: PhantomData<*const ()>,
}

impl Held {
    fn acquire() -> Self {
        #[cfg(debug_assertions)]
        if HELD.with(|held| held.replace(true)) {
            panic!("{NESTED}");
        }
        Self {
            _thread: PhantomData,
        }
    }
}

#[cfg(debug_assertions)]
impl Drop for Held {
    fn drop(&mut self) {
        HELD.with(|held| held.set(false));
    }
}

#[cfg(debug_assertions)]
thread_local! {
    static HELD: Cell<bool> = const { Cell::new(false) };
}

static ROOT: Mutex<Option<Box<Root>>> = Mutex::new(None);

static SCHEMA: OnceLock<&'static Schema> = OnceLock::new();

const UNREGISTERED: &str =
    "config: no schema is registered; the payload registers one before reading the configuration";

const SECOND_SCHEMA: &str = "config: a second, different schema was registered";

const WRONG_TYPE: &str = "config: the root is not of the registered schema's type";

#[cfg(debug_assertions)]
const NESTED: &str = "config: this thread already holds the configuration lock, which would \
                      deadlock; take one guard and read every part through it, or copy the first \
                      value out before locking again";

fn defaults_of<C: ConfigRoot>() -> Box<Root> {
    Box::new(C::defaults())
}

fn clone_of<C: ConfigRoot>(root: &Root) -> Box<Root> {
    Box::new(downcast_ref::<C>(root).clone())
}

fn serialize_of<C: ConfigRoot>(root: &Root) -> Result<String, toml::ser::Error> {
    toml::to_string(downcast_ref::<C>(root))
}

fn to_value_of<C: ConfigRoot>(root: &Root) -> Result<Value, toml::ser::Error> {
    Value::try_from(downcast_ref::<C>(root))
}

fn as_serialize_of<C: ConfigRoot>(root: &Root) -> &dyn erased_serde::Serialize {
    downcast_ref::<C>(root)
}

fn deserialize_of<C: ConfigRoot>(
    value: Value,
    mut ignored: &mut Ignored<'_>,
) -> Result<Box<Root>, DeserializeError> {
    let root: C =
        serde_path_to_error::deserialize(serde_ignored::Deserializer::new(value, &mut ignored))?;
    Ok(Box::new(root))
}

fn validate_of<C: ConfigRoot>(root: &mut Root, resets: &mut Vec<Reset>) {
    downcast_mut::<C>(root).validate(resets);
}
