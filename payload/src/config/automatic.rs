//! Switches the payload turned off on its own, and the values the user chose for them.
//!
//! A fallback that turns a setting off (the mirror after a removed device, the HUD redirect at
//! teardown) changes the live configuration, but the user did not choose it, and a save that
//! happens later for some other change must not persist it. [`disable`] records the user's value
//! beside the live one; a save writes the recorded value ([`apply`]) until the user changes the
//! switch through [`crate::config::edit`], which [`settle`] notices and which ends the override.
//!
//! The overrides are only read and written with the configuration lock held, so they cannot change
//! under an edit. They have their own lock, a leaf taken under the configuration lock.

use parking_lot::Mutex;

use crate::config::Config;

/// A boolean setting a fallback can turn off: its path in the settings file, for the log, and its
/// place in the configuration.
#[derive(Copy, Clone)]
pub(crate) struct Switch {
    pub path: &'static str,
    pub place: for<'a> fn(&'a mut Config) -> &'a mut bool,
}

/// The overridden switches' values when an edit began, in override order.
pub(crate) struct Observed(Vec<bool>);

/// Turn `switch` off in `config`, remembering the value the user chose. Called with the
/// configuration lock held.
pub(super) fn disable(config: &mut Config, switch: Switch) {
    let place = switch.place;
    let current = *place(config);
    *place(config) = false;
    let mut overrides = OVERRIDES.lock();
    if overrides.iter().any(|o| o.switch.path == switch.path) {
        return;
    }
    if current {
        tracing::info!(
            switch = switch.path,
            "settings: switched off automatically; the saved setting stays on",
        );
        overrides.push(Override {
            switch,
            chosen: current,
        });
    }
}

/// The overridden switches' current values, at the start of an edit.
pub(super) fn observe(config: &mut Config) -> Observed {
    Observed(
        OVERRIDES
            .lock()
            .iter()
            .map(|o| *(o.switch.place)(config))
            .collect(),
    )
}

/// End the override of every switch the edit changed: the user has now chosen its value.
pub(super) fn settle(config: &mut Config, before: &Observed) {
    if before.0.is_empty() {
        return;
    }
    let mut overrides = OVERRIDES.lock();
    let mut index = 0;
    overrides.retain(|o| {
        let changed = before
            .0
            .get(index)
            .is_some_and(|&was| *(o.switch.place)(config) != was);
        index += 1;
        !changed
    });
}

/// Put the user's chosen values back into `snapshot`, a copy about to be saved.
pub(super) fn apply(snapshot: &mut Config) {
    for o in OVERRIDES.lock().iter() {
        *(o.switch.place)(snapshot) = o.chosen;
    }
}

struct Override {
    switch: Switch,
    chosen: bool,
}

static OVERRIDES: Mutex<Vec<Override>> = Mutex::new(Vec::new());
