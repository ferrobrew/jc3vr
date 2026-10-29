//! Admission for HUD surfaces and panel geometry loaded from settings.

use crate::{
    config::validate::{Invalid, positive},
    hud::config::HudConfig,
};

/// The Direct3D 11 two-dimensional texture axis limit.
pub(crate) const MAX_TEXTURE_DIMENSION: u32 = 16384;

pub(crate) fn hud(c: &HudConfig) -> Result<(), Invalid> {
    positive("distance", c.distance, 1000.0)?;
    positive("panel_scale", c.panel_scale, 100.0)?;
    let e = &c.egui_panel;
    positive("egui_panel.aspect", e.aspect, 100.0)?;
    dimension("egui_panel.resolution[0]", e.resolution.0)?;
    dimension("egui_panel.resolution[1]", e.resolution.1)
}

pub(crate) fn dimension(name: &str, value: u32) -> Result<(), Invalid> {
    if !(1..=MAX_TEXTURE_DIMENSION).contains(&value) {
        return Err(Invalid(format!(
            "{name} must be between 1 and {MAX_TEXTURE_DIMENSION}, got {value}"
        )));
    }
    Ok(())
}
