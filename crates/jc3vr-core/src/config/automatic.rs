//! Switches the payload turned off on its own, and the values the user chose for them.
//!
//! A fallback that turns a setting off (the mirror after a removed device, the HUD redirect at
//! teardown) changes the live configuration, but the user did not choose it, and a save that
//! happens later for some other change must not persist it. [`disable_automatically`] records the
//! user's value beside the live one; a save writes the recorded value ([`apply`]) until the user
//! changes the switch through an edit ([`ConfigSlot::edit`]), which [`settle`] notices and which
//! ends the override.
//!
//! The overrides are only read and written with the configuration lock held, so they cannot change
//! under an edit. They have their own lock, a leaf taken under the configuration lock.

use std::ptr;

use parking_lot::Mutex;

use crate::config::{
    slot::ConfigSlot,
    store::{self, Root},
};

/// A boolean setting a fallback can turn off: the slot whose part holds it, its field there, and
/// its place in the part. Build one with [`switch!`](crate::switch).
pub struct Switch<T: 'static> {
    pub slot: &'static ConfigSlot<T>,
    pub field: &'static str,
    pub place: for<'a> fn(&'a mut T) -> &'a mut bool,
}

/// The [`Switch`] at the boolean field `$field` of the part of the slot `$slot`
/// (`switch!(VR_CONFIG, mirror)`).
#[macro_export]
macro_rules! switch {
    ($slot:path, $field:ident) => {
        $crate::config::Switch {
            slot: &$slot,
            field: ::std::stringify!($field),
            place: |part| &mut part.$field,
        }
    };
}

impl<T: 'static> Switch<T> {
    /// The setting's path in the settings file, for the log.
    fn path(&self) -> String {
        format!("{}.{}", self.slot.path(), self.field)
    }

    /// What identifies the setting among the overrides.
    fn key(&self) -> (*const (), &'static str) {
        (ptr::from_ref(self.slot).cast(), self.field)
    }
}

impl<T: 'static> Clone for Switch<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> Copy for Switch<T> {}

/// Switch `switch` off because the payload cannot keep it on (a fallback after a failure, or a
/// teardown), without saving that as the user's choice: the live configuration changes at once,
/// while a save keeps the value the user chose until the user changes the switch through an edit.
pub fn disable_automatically<T: 'static>(switch: Switch<T>) {
    let mut root = store::lock_root();
    OVERRIDES.lock().disable(&mut *root, switch);
}

/// The overridden switches' values when an edit began, in override order.
pub(crate) struct Observed(Vec<bool>);

/// The overridden switches' current values, at the start of an edit.
pub(crate) fn observe(root: &mut Root) -> Observed {
    OVERRIDES.lock().observe(root)
}

/// End the override of every switch the edit changed: the user has now chosen its value.
pub(crate) fn settle(root: &mut Root, before: &Observed) {
    if before.0.is_empty() {
        return;
    }
    OVERRIDES.lock().settle(root, before);
}

/// Put the user's chosen values back into `snapshot`, a copy about to be saved.
pub(crate) fn apply(snapshot: &mut Root) {
    OVERRIDES.lock().apply(snapshot);
}

/// The switches turned off automatically, in the order they were.
struct Overrides(Vec<Override>);

impl Overrides {
    const fn new() -> Self {
        Self(Vec::new())
    }

    /// Turn `switch` off in `root`, remembering the value the user chose.
    fn disable<T: 'static>(&mut self, root: &mut Root, switch: Switch<T>) {
        let place = switch.locate(root);
        let current = *place;
        *place = false;
        if self.0.iter().any(|o| o.switch.key() == switch.key()) {
            return;
        }
        if current {
            tracing::info!(
                switch = switch.path(),
                "settings: switched off automatically; the saved setting stays on",
            );
            self.0.push(Override {
                switch: Box::new(switch),
                chosen: current,
            });
        }
    }

    fn observe(&self, root: &mut Root) -> Observed {
        Observed(self.0.iter().map(|o| *o.switch.locate(root)).collect())
    }

    fn settle(&mut self, root: &mut Root, before: &Observed) {
        let mut index = 0;
        self.0.retain(|o| {
            let changed = before
                .0
                .get(index)
                .is_some_and(|&was| *o.switch.locate(root) != was);
            index += 1;
            !changed
        });
    }

    fn apply(&self, snapshot: &mut Root) {
        for o in &self.0 {
            *o.switch.locate(snapshot) = o.chosen;
        }
    }
}

struct Override {
    switch: Box<dyn Place>,
    chosen: bool,
}

/// A [`Switch`] with its slot's type erased.
trait Place: Send + Sync {
    fn key(&self) -> (*const (), &'static str);
    fn locate<'a>(&self, root: &'a mut Root) -> &'a mut bool;
}

impl<T: 'static> Place for Switch<T> {
    fn key(&self) -> (*const (), &'static str) {
        Switch::key(self)
    }

    fn locate<'a>(&self, root: &'a mut Root) -> &'a mut bool {
        (self.place)(self.slot.project_mut(root))
    }
}

static OVERRIDES: Mutex<Overrides> = Mutex::new(Overrides::new());

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::projection;

    #[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
    struct Sample {
        mirror: bool,
        sharpness: f32,
    }

    static MIRROR: ConfigSlot<bool> = ConfigSlot::new("mirror");

    fn root() -> Sample {
        Sample {
            mirror: true,
            sharpness: 0.2,
        }
    }

    #[test]
    fn an_automatic_switch_off_keeps_the_saved_choice_until_the_user_changes_it() {
        MIRROR.bind(projection!(Sample, mirror));
        let switch = Switch {
            slot: &MIRROR,
            field: "",
            place: |mirror| mirror,
        };
        let mut overrides = Overrides::new();
        let mut config = root();
        overrides.disable(&mut config, switch);
        assert!(!config.mirror);
        let mut snapshot = config.clone();
        overrides.apply(&mut snapshot);
        assert!(snapshot.mirror);

        // An edit that leaves the switch alone keeps the override.
        let before = overrides.observe(&mut config);
        config.sharpness = 0.07;
        overrides.settle(&mut config, &before);
        let mut snapshot = config.clone();
        overrides.apply(&mut snapshot);
        assert!(snapshot.mirror);

        // An edit that changes it makes the live value the user's.
        let before = overrides.observe(&mut config);
        config.mirror = true;
        overrides.settle(&mut config, &before);
        config.mirror = false;
        let mut snapshot = config.clone();
        overrides.apply(&mut snapshot);
        assert!(!snapshot.mirror);
    }

    #[test]
    fn a_switch_already_off_records_no_override() {
        static OFF: ConfigSlot<Sample> = ConfigSlot::new("");
        OFF.bind(projection!(Sample));
        let mut overrides = Overrides::new();
        let mut config = Sample {
            mirror: false,
            ..root()
        };
        let switch = crate::switch!(OFF, mirror);
        overrides.disable(&mut config, switch);
        assert!(overrides.0.is_empty());
        overrides.disable(&mut config, switch);
        assert!(overrides.0.is_empty());
    }
}
