//! The UI's tuning, read from the shipped `tuning/ui.toml` ([`UI`]): how long the startup banner
//! stays up, how the frame-time chart reads, how long the debug readouts count as live, and how far
//! from flat the controls page may capture the wingsuit's neutral tilt.

use jc3vr_core::{
    config::engine::{Invalid, positive, range},
    tuning::Tuning,
};
use serde::Deserialize;

jc3vr_core::tuning_file! {
    /// The UI's tuning file.
    pub static UI: UiTuning = "ui.toml";
}

/// The UI's tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiTuning {
    pub banner: BannerTuning,
    pub performance: PerformanceTuning,
    pub readouts: ReadoutsTuning,
    pub controls: ControlsTuning,
}

impl Tuning for UiTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("banner.startup_s", self.banner.startup_s, 600.0)?;
        positive(
            "performance.chart_window_s",
            self.performance.chart_window_s,
            600.0,
        )?;
        range(
            "performance.chart_scale_headroom",
            self.performance.chart_scale_headroom,
            1.0,
            4.0,
        )?;
        positive(
            "readouts.unarmed_stale_s",
            self.readouts.unarmed_stale_s,
            600.0,
        )?;
        positive(
            "readouts.mirror_stale_s",
            self.readouts.mirror_stale_s,
            600.0,
        )?;
        // The settings' own check admits a neutral tilt within a right angle of flat.
        range(
            "controls.neutral_tilt_limit_deg",
            self.controls.neutral_tilt_limit_deg,
            0.0,
            90.0,
        )
    }
}

/// The startup banner ([`crate::ui::startup_banner`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BannerTuning {
    /// How long the banner stays up after the first frame, seconds.
    pub startup_s: f32,
}

/// The Performance tab's rolling frame-time chart.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerformanceTuning {
    /// The history the chart keeps and displays, seconds.
    pub chart_window_s: f32,
    /// How far above the 99th-percentile frame time the chart's scale must reach before it snaps
    /// to a band, as a factor, so the steady trace rides mid-chart rather than at its ceiling.
    pub chart_scale_headroom: f32,
}

/// How long a debug readout's last update counts as live.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadoutsTuning {
    /// The weapon-agnostic body's readout, after the local player's pose finalization last ran,
    /// seconds.
    pub unarmed_stale_s: f32,
    /// The body mirror's status row, after the mirror's last pass, seconds. The pause stops the
    /// pass, so the window is long enough for a mirror that drew before the menu opened to still
    /// read as drawing.
    pub mirror_stale_s: f32,
}

/// The pause menu's controls page.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlsTuning {
    /// The furthest from flat a captured neutral wrist tilt may be, either way, degrees.
    pub neutral_tilt_limit_deg: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(UI.get().validate(), Ok(()));
    }
}
