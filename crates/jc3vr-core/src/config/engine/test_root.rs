//! A crate's test configuration root, declared with [`test_root!`](crate::test_root).
//!
//! The payload binds every slot to its settings aggregate at startup. That aggregate sits above
//! every other crate, so each crate whose tests read the configuration registers a root of its own
//! instead: the groups its code reads through their slots, at the paths the aggregate keeps them
//! and with its defaults. A test reads and changes the configuration through the slots exactly as
//! the payload does, and takes it exclusively with [`test_fixture`](crate::config::engine::test_fixture).
//!
//! The macro checks what a hand-written root could get wrong. Each slot is bound at the path its
//! declaration names, so a binding at another path panics at registration. A slot the root does not
//! bind panics on its first read with a message naming the root to add it to. The registration runs
//! from the C runtime's initializer table (`.CRT$XCU`), before the test harness's `main`, so a test
//! finds every slot bound whichever test runs first, and no test calls a registration of its own.

use std::sync::OnceLock;

#[doc(hidden)]
pub use toml::Table;

use crate::config::engine::{ConfigSlot, Projection};

/// Declare the crate's test configuration root, `TestRoot`, bind the listed slots to it, and
/// register its schema before the test harness runs. Invoke it once per crate, in a
/// `#[cfg(test)]` module; the crate must depend on `serde` with its derives.
///
/// ```ignore
/// jc3vr_core::test_root! {
///     root {
///         stereo: StereoConfig = StereoConfig::new(),
///         hud: Hud {
///             egui_panel: EguiPanel {
///                 enabled: bool = false,
///             },
///         },
///     }
///     binds {
///         STEREO_CONFIG => stereo,
///         PANEL_ENABLED => hud.egui_panel.enabled,
///     }
/// }
/// ```
///
/// `root` lists the root's fields, each a group with its type and default (`name: Type = default`)
/// or a table of further fields under a struct name the macro declares (`name: Name { ... }`);
/// attributes before a field (`#[serde(default)]`) pass through to it. `binds` maps each slot to
/// its field path, which must be the slot's own path. An optional `validate: path` names a
/// `fn(&mut TestRoot, &mut Vec<Reset>)` the root's validation runs; without it the root validates
/// nothing.
#[macro_export]
macro_rules! test_root {
    (@struct $name:ident [$($decl:tt)*] [$($default:tt)*]) => {
        #[derive(Clone, ::serde::Serialize, ::serde::Deserialize)]
        struct $name {
            $($decl)*
        }

        impl $name {
            fn test_defaults() -> Self {
                Self {
                    $($default)*
                }
            }
        }
    };
    (
        @struct $name:ident [$($decl:tt)*] [$($default:tt)*]
        $(#[$meta:meta])* $field:ident : $table:ident { $($body:tt)* }
        $(, $($rest:tt)*)?
    ) => {
        $crate::test_root!(@struct $table [] [] $($body)*);
        $crate::test_root!(
            @struct $name
            [$($decl)* $(#[$meta])* $field: $table,]
            [$($default)* $field: $table::test_defaults(),]
            $($($rest)*)?
        );
    };
    (
        @struct $name:ident [$($decl:tt)*] [$($default:tt)*]
        $(#[$meta:meta])* $field:ident : $ty:ty = $value:expr
        $(, $($rest:tt)*)?
    ) => {
        $crate::test_root!(
            @struct $name
            [$($decl)* $(#[$meta])* $field: $ty,]
            [$($default)* $field: $value,]
            $($($rest)*)?
        );
    };
    (@validate $root:ident $resets:ident) => {
        let _ = ($root, $resets);
    };
    (@validate $root:ident $resets:ident $validate:path) => {
        $validate($root, $resets)
    };
    (
        root { $($root:tt)* }
        binds { $($slot:path => $($field:ident).+),* $(,)? }
        $(validate: $validate:path $(,)?)?
    ) => {
        $crate::test_root!(@struct TestRoot [] [] $($root)*);

        impl $crate::config::engine::ConfigRoot for TestRoot {
            fn defaults() -> Self {
                Self::test_defaults()
            }

            fn migrate(
                _file: &mut $crate::config::engine::test_root::Table,
                _format: $crate::config::engine::format::Format,
            ) -> ::std::vec::Vec<$crate::config::engine::format::Migrated> {
                ::std::vec::Vec::new()
            }

            fn validate(
                &mut self,
                resets: &mut ::std::vec::Vec<$crate::config::engine::load::Reset>,
            ) {
                let root = self;
                $crate::test_root!(@validate root resets $($validate)?);
            }

            fn note_saved(_text: &str) {}
        }

        /// Bind the listed slots to the test root and register its schema, once. The C runtime runs
        /// it before the test harness's `main`, so a test need not call it.
        pub(crate) fn register() {
            static SCHEMA: $crate::config::engine::Schema =
                $crate::config::engine::Schema::of::<TestRoot>();
            static REGISTER: ::std::sync::Once = ::std::sync::Once::new();
            REGISTER.call_once(|| {
                $crate::config::engine::test_root::note_root(::std::module_path!());
                $(
                    $crate::config::engine::test_root::bind_at(
                        &$slot,
                        &[$(::std::stringify!($field)),+],
                        $crate::projection!(TestRoot, $($field).+),
                    );
                )*
                $crate::config::engine::store::register(&SCHEMA);
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

/// Record the module that declares the test root, for the message an unbound slot panics with.
#[doc(hidden)]
pub fn note_root(module: &'static str) {
    // A second root in one test binary would fail the schema registration that follows.
    let _ = ROOT_MODULE.set(module);
}

/// Bind `slot` to the test root at the field path `fields`, which must be the slot's own path.
#[doc(hidden)]
pub fn bind_at<T: 'static>(slot: &ConfigSlot<T>, fields: &[&str], projection: Projection<T>) {
    let path = fields.join(".");
    assert!(
        slot.path() == path,
        "config: the test root in `{}` binds the slot `{}` at `{path}`; a slot is bound at its own \
         path",
        ROOT_MODULE.get().copied().unwrap_or("?"),
        slot.path(),
    );
    slot.bind(projection);
}

/// The module that declares the registered test root, if one is registered.
pub(crate) fn root_module() -> Option<&'static str> {
    ROOT_MODULE.get().copied()
}

static ROOT_MODULE: OnceLock<&'static str> = OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::engine::projection;

    struct Root {
        group: Group,
    }

    struct Group {
        value: bool,
    }

    #[test]
    fn a_slot_binds_at_its_own_path() {
        static SLOT: ConfigSlot<bool> = ConfigSlot::new("group.value");
        bind_at(&SLOT, &["group", "value"], projection!(Root, group.value));
    }

    #[test]
    #[should_panic(expected = "binds the slot `group.value` at `group.other`")]
    fn a_slot_bound_at_another_path_panics() {
        static SLOT: ConfigSlot<bool> = ConfigSlot::new("group.value");
        bind_at(&SLOT, &["group", "other"], projection!(Root, group.value));
    }

    #[test]
    #[should_panic(expected = "does not bind it, so add it to that root's `binds`")]
    fn reading_a_slot_the_test_root_does_not_bind_names_the_root() {
        static SLOT: ConfigSlot<bool> = ConfigSlot::new("group.value");
        note_root(module_path!());
        SLOT.query(|value| *value);
    }
}
