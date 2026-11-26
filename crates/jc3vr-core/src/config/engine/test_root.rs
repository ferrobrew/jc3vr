//! A crate's test configuration root, declared with [`test_root!`](crate::test_root).
//!
//! The payload binds every slot to its settings aggregate at startup. That aggregate sits above
//! every other crate, so each crate whose tests read the configuration registers a root of its own
//! instead, made of the slots its code reads: each listed slot holds its part, defaulted as the
//! aggregate defaults it (the slot's declared default, or its type's, which is the group's
//! `const fn new()`), and binds the slots nested in it as the aggregate does
//! ([`SlotOptions::with_children`](crate::config::engine::SlotOptions::with_children)). A test reads and changes the configuration through the slots
//! exactly as the payload does, its loads run the checks the slots carry, and it takes the
//! configuration exclusively with [`test_fixture`](crate::config::engine::test_fixture).
//!
//! A slot the root does not list or bind panics on its first read with a message naming the root
//! to add it to. The registration runs from the C runtime's initializer table (`.CRT$XCU`), before
//! the test harness's `main`, so a test finds every slot bound whichever test runs first, and no
//! test calls a registration of its own.

use std::{ptr, sync::OnceLock};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _, ser::Error as _};
use toml::{Table, Value};

use crate::config::engine::{
    ConfigRoot, ConfigSlot, Projection, Schema,
    slot::{Bound, SlotValue},
    store::{self, Root},
};

/// Declare the crate's test configuration root from the slots its code reads, bind them to it, and
/// register it before the test harness runs. Invoke it once per crate, in a `#[cfg(test)]` module.
///
/// ```ignore
/// jc3vr_core::test_root!(STEREO_CONFIG, CONTROLLERS_CONFIG, PANEL_CONFIG);
/// ```
///
/// A listed slot binds its children, so a root lists a group's slot or the slots nested in it,
/// never both. It also generates `register()`, which registers the root if the C runtime has not.
#[macro_export]
macro_rules! test_root {
    ($($slot:path),* $(,)?) => {
        /// Bind the listed slots to the test root and register it, once. The C runtime runs it
        /// before the test harness's `main`, so a test need not call it.
        pub(crate) fn register() {
            static REGISTER: ::std::sync::Once = ::std::sync::Once::new();
            REGISTER.call_once(|| {
                $crate::config::engine::test_root::register(
                    ::std::module_path!(),
                    ::std::vec![$(
                        $crate::config::engine::test_root::part(
                            &$slot,
                            $crate::config::engine::Projection::new(
                                |root| $crate::config::engine::test_root::TestRoot::of(root)
                                    .get(&$slot),
                                |root| $crate::config::engine::test_root::TestRoot::of_mut(root)
                                    .get_mut(&$slot),
                            ),
                        )
                    ),*],
                );
            });
        }

        /// The C runtime calls each pointer in the `.CRT$XCU` section before `main`.
        #[used]
        #[unsafe(link_section = ".CRT$XCU")]
        static REGISTER_BEFORE_TESTS: extern "C" fn() = {
            extern "C" fn register_before_tests() {
                register();
            }
            register_before_tests
        };
    };
}

/// A test root: the part of each listed slot, in the order the slots are listed.
pub struct TestRoot {
    parts: Vec<Box<Root>>,
}

impl TestRoot {
    /// The registered test root in `root`.
    pub fn of(root: &Root) -> &Self {
        store::downcast_ref(root)
    }

    /// The registered test root in `root`, to change.
    pub fn of_mut(root: &mut Root) -> &mut Self {
        store::downcast_mut(root)
    }

    /// The part of `slot`, which the root lists.
    pub fn get<T: 'static>(&self, slot: &'static ConfigSlot<T>) -> &T {
        self.parts[index(slot)]
            .downcast_ref()
            .expect("config: a test root's part is not of its slot's type")
    }

    /// The part of `slot`, which the root lists, to change.
    pub fn get_mut<T: 'static>(&mut self, slot: &'static ConfigSlot<T>) -> &mut T {
        self.parts[index(slot)]
            .downcast_mut()
            .expect("config: a test root's part is not of its slot's type")
    }
}

/// A listed slot and the binding of it to the test root.
#[doc(hidden)]
pub struct Part {
    slot: &'static dyn Bound,
    bind: Box<dyn FnOnce()>,
}

/// The listed `slot`, bound to the test root through `projection`.
#[doc(hidden)]
pub fn part<T: SlotValue>(slot: &'static ConfigSlot<T>, projection: Projection<T>) -> Part {
    Part {
        slot,
        bind: Box::new(move || slot.bind(projection)),
    }
}

/// Bind the listed `parts` and register the test root, declared in `module`.
#[doc(hidden)]
pub fn register(module: &'static str, parts: Vec<Part>) {
    note_root(module);
    let slots: Vec<&'static dyn Bound> = parts.iter().map(|part| part.slot).collect();
    for (i, outer) in slots.iter().enumerate() {
        for inner in &slots[i + 1..] {
            let (outer, inner) = (outer.path(), inner.path());
            let (parent, child) = if outer.len() <= inner.len() {
                (outer, inner)
            } else {
                (inner, outer)
            };
            assert!(
                !(child == parent || child.starts_with(&format!("{parent}."))),
                "config: the test root in `{module}` lists both `{parent}` and `{child}`; list \
                 only `{parent}`, whose slot binds the slots nested in it",
            );
        }
    }
    assert!(
        SLOTS.set(slots).is_ok(),
        "config: a second test root was registered from `{module}`"
    );
    for part in parts {
        (part.bind)();
    }
    store::register(&SCHEMA);
}

/// Record the module that declares the test root, for the message an unbound slot panics with.
#[doc(hidden)]
pub fn note_root(module: &'static str) {
    // A second root in one test binary fails the registration that follows.
    let _ = ROOT_MODULE.set(module);
}

/// The module that declares the registered test root, if one is registered.
pub(crate) fn root_module() -> Option<&'static str> {
    ROOT_MODULE.get().copied()
}

impl ConfigRoot for TestRoot {
    fn defaults() -> Self {
        Self {
            parts: slots().iter().map(|slot| slot.default_part()).collect(),
        }
    }

    fn migrate(
        _file: &mut Table,
        _format: crate::config::engine::format::Format,
    ) -> Vec<crate::config::engine::format::Migrated> {
        Vec::new()
    }

    fn note_saved(_text: &str) {}
}

impl Clone for TestRoot {
    fn clone(&self) -> Self {
        Self {
            parts: slots()
                .iter()
                .zip(&self.parts)
                .map(|(slot, part)| slot.clone_part(part.as_ref()))
                .collect(),
        }
    }
}

/// The parts, each at its slot's path, as the aggregate would write them.
impl Serialize for TestRoot {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut table = Table::new();
        for (slot, part) in slots().iter().zip(&self.parts) {
            let value = slot
                .part_to_value(part.as_ref())
                .map_err(S::Error::custom)?;
            let mut steps = slot.path().split('.').peekable();
            let mut at = &mut table;
            while let Some(step) = steps.next() {
                if steps.peek().is_none() {
                    at.insert(step.to_owned(), value);
                    break;
                }
                at = at
                    .entry(step.to_owned())
                    .or_insert_with(|| Value::Table(Table::new()))
                    .as_table_mut()
                    .ok_or_else(|| S::Error::custom(format!("{} nests in a value", slot.path())))?;
            }
        }
        table.serialize(serializer)
    }
}

/// Each part from its slot's path, or its default where the file has none.
impl<'de> Deserialize<'de> for TestRoot {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let table = Table::deserialize(deserializer)?;
        let parts = slots()
            .iter()
            .map(|slot| {
                let mut value = Some(&Value::Table(table.clone()));
                for step in slot.path().split('.') {
                    value = value.and_then(|v| v.get(step));
                }
                match value {
                    Some(value) => slot
                        .part_from_value(value.clone())
                        .map_err(|error| D::Error::custom(format!("{}: {error}", slot.path()))),
                    None => Ok(slot.default_part()),
                }
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { parts })
    }
}

static SCHEMA: Schema = Schema::of::<TestRoot>();

static SLOTS: OnceLock<Vec<&'static dyn Bound>> = OnceLock::new();

static ROOT_MODULE: OnceLock<&'static str> = OnceLock::new();

fn slots() -> &'static [&'static dyn Bound] {
    SLOTS.get().expect("config: no test root is registered")
}

/// The index of `slot` among the listed slots.
fn index<T: 'static>(slot: &'static ConfigSlot<T>) -> usize {
    slots()
        .iter()
        .position(|listed| ptr::addr_eq(*listed as *const dyn Bound, slot))
        .expect("config: the slot is not listed in the test root it is bound to")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::engine::{bind_child, projection};

    #[derive(Clone, Default, Serialize, Deserialize)]
    struct Group {
        value: bool,
        other: bool,
    }

    static GROUP: ConfigSlot<Group> = ConfigSlot::new("group");

    #[test]
    #[should_panic(
        expected = "the slot `group.value` is bound as `group.other`, a child of `group`"
    )]
    fn a_child_bound_at_another_path_panics() {
        static VALUE: ConfigSlot<bool> = ConfigSlot::new("group.value");
        bind_child(&GROUP, &VALUE, &["other"], projection!(Group, other));
    }

    #[test]
    #[should_panic(expected = "does not bind it, so add it to that root's list")]
    fn reading_a_slot_the_test_root_does_not_bind_names_the_root() {
        static SLOT: ConfigSlot<bool> = ConfigSlot::new("group.value");
        note_root(module_path!());
        SLOT.query(|value| *value);
    }
}
