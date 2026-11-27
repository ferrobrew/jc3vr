//! The core crate's tuning, read from the shipped `tuning/core.toml` ([`CORE`]): when the settings
//! file is saved after a change, and how long the eject lets the engine's frames run between its
//! teardown steps.

use serde::Deserialize;

use crate::{
    config::{Invalid, positive},
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
    pub(crate) settings_save: SettingsSaveTuning,
    pub teardown: TeardownTuning,
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
        positive(
            "teardown.settle_frames",
            self.teardown.settle_frames as f32,
            MAX_SETTLE_FRAMES,
        )?;
        millis("teardown.settle_budget_ms", self.teardown.settle_budget_ms)
    }
}

/// When the settings file is saved after a change
/// ([`crate::config::persist`]), milliseconds.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SettingsSaveTuning {
    /// How long the changes must be quiet before a save.
    pub(crate) quiet_ms: u64,
    /// The longest a change waits for the changes after it to settle.
    pub(crate) max_delay_ms: u64,
    /// How long the first retry of a failed save waits. Each further failure doubles it, up to
    /// [`SettingsSaveTuning::retry_max_ms`].
    pub(crate) retry_first_ms: u64,
    /// The longest a retry of a failed save waits.
    pub(crate) retry_max_ms: u64,
    /// How often a change that has not settled is compared with the file, for the save status.
    pub(crate) compare_interval_ms: u64,
}

/// The eject's settle waits ([`crate::lifecycle::settle_frames`]): after the cleanups, so the
/// still-installed hooks service what they requested (the native-resolution restore in the `Draw`
/// prologue, the per-frame restores), and after hook removal, so a detour entered before it has
/// returned.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeardownTuning {
    /// How many engine frames each wait lets run.
    pub settle_frames: u32,
    /// The longest each wait lasts when the frame loop does not advance, milliseconds.
    pub(crate) settle_budget_ms: u64,
}

/// The most frames a settle wait can usefully ask for: a second of frames at the fastest refresh.
const MAX_SETTLE_FRAMES: f32 = 144.0;

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
