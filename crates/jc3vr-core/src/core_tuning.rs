//! The core crate's tuning, read from the shipped `tuning/core.toml` ([`CORE`]): when the settings
//! file is saved after a change, and how long a debug hotkey ignores the keyboard after a press.

use serde::Deserialize;

use crate::{
    config::engine::{Invalid, positive},
    tuning::Tuning,
};

crate::tuning_file! {
    /// The core crate's tuning file.
    pub static CORE: CoreTuning = "core.toml";
}

/// The core crate's tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreTuning {
    pub settings_save: SettingsSaveTuning,
    pub hotkey: HotkeyTuning,
}

impl Tuning for CoreTuning {
    fn validate(&self) -> Result<(), Invalid> {
        let save = &self.settings_save;
        millis("settings_save.quiet_ms", save.quiet_ms)?;
        millis("settings_save.max_delay_ms", save.max_delay_ms)?;
        millis("settings_save.retry_first_ms", save.retry_first_ms)?;
        millis("settings_save.retry_max_ms", save.retry_max_ms)?;
        millis(
            "settings_save.compare_interval_ms",
            save.compare_interval_ms,
        )?;
        if save.retry_max_ms < save.retry_first_ms {
            return Err(Invalid(format!(
                "settings_save.retry_max_ms must be at least settings_save.retry_first_ms ({}), \
                 got {}",
                save.retry_first_ms, save.retry_max_ms
            )));
        }
        millis("hotkey.repeat_lockout_ms", self.hotkey.repeat_lockout_ms)
    }
}

/// When the settings file is saved after a change
/// ([`crate::config::engine::persist`]), milliseconds.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsSaveTuning {
    /// How long the changes must be quiet before a save.
    pub quiet_ms: u64,
    /// The longest a change waits for the changes after it to settle.
    pub max_delay_ms: u64,
    /// How long the first retry of a failed save waits. Each further failure doubles it, up to
    /// [`SettingsSaveTuning::retry_max_ms`].
    pub retry_first_ms: u64,
    /// The longest a retry of a failed save waits.
    pub retry_max_ms: u64,
    /// How often a change that has not settled is compared with the file, for the save status.
    pub compare_interval_ms: u64,
}

/// The debug hotkeys' polling ([`crate::util::is_pressed`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HotkeyTuning {
    /// How long after a hotkey reads pressed every hotkey reads released, so one press acts once,
    /// milliseconds.
    pub repeat_lockout_ms: u64,
}

/// Checks a duration in milliseconds against the longest any of the file's waits can usefully be:
/// ten minutes.
fn millis(name: &str, value: u64) -> Result<(), Invalid> {
    positive(name, value as f32, 600_000.0)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(CORE.get().validate(), Ok(()));
    }
}
