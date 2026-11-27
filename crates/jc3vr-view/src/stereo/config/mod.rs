//! Stereo rendering configuration. The live per-eye runtime state is [`crate::stereo::StereoState`].

use jc3vr_core::config::{ConfigSlot, Parts};
use serde::{Deserialize, Serialize};

use crate::vr::VR_CONFIG;

mod single_pass;

pub use single_pass::{SinglePassConfig, SinglePassSwitch};

/// Turn the whole single-pass feature on or off together, as the debug UI's one button does: the
/// master switch (which carries the dual-eye split, the collapse, and the double-wide target), the
/// reprojected scene families, and the terrain, along with the native resolution the double-wide
/// target needs, which turning the feature off leaves as it is. The defaults are the feature turned
/// on, so applying this with `on` to a default configuration changes nothing. The caller requests the
/// shader reload that applies the change.
pub fn set_single_pass_feature(config: &mut impl Parts, on: bool) {
    let sps = &mut config.part_mut(&STEREO_CONFIG).single_pass;
    sps.enabled = on;
    sps.reproject = on;
    sps.terrain = on;
    if on {
        config.part_mut(&VR_CONFIG).native_resolution = true;
    }
}

/// The stereo settings, at `stereo` in the settings file; the payload binds the slot at startup.
pub static STEREO_CONFIG: ConfigSlot<StereoConfig> = ConfigSlot::new("stereo");

/// Stereo rendering toggles. The live per-eye runtime state is [`crate::stereo::StereoState`].
#[derive(Clone, Serialize, Deserialize)]
pub struct StereoConfig {
    /// Single-pass stereo; see [`SinglePassConfig`].
    pub single_pass: SinglePassConfig,
    /// Refresh every sun-shadow cascade every frame, defeating the engine's update-pattern amortisation.
    /// The engine re-fits and re-renders cascade level L only every `2^L` frames (via
    /// `ShadowManager::m_CascadeUpdateLevels`), copying each cascade's fit forward between refreshes, so
    /// the coarse cascades hold still then snap to a new texel alignment periodically. Flatscreen's T2X
    /// averages those snaps away, but the mod forces SMAA 1x (no temporal history), so they surface as
    /// sun-shadow flicker (issue #31). Zeroing the per-cascade levels forces all cascades to update every
    /// frame -- smooth, at the cost of redrawing the coarse cascades each frame. VR only.
    pub shadow_update_every_frame: bool,
    /// Fit the sun-shadow cascades to the exact union of the two eyes' fields of view rather than
    /// the padded cull projection. The fit sizes each cascade from its camera's field of view, so
    /// the padding coarsens every cascade's texels over geometry neither eye sees. VR only.
    pub shadow_fit_unpadded: bool,
    /// Anchor the sun-shadow lookup at the camera position the cascade fit used, not the render
    /// camera's centre. The cascade transform is baked relative to the fit's camera, and the
    /// material shaders subtract the render camera's position before applying it, so any gap
    /// between the two shifts every shadow by that gap in the world.
    pub shadow_anchor_to_fit: bool,
}
impl StereoConfig {
    pub const fn new() -> Self {
        Self {
            single_pass: SinglePassConfig::new(),
            shadow_update_every_frame: false,
            shadow_fit_unpadded: true,
            shadow_anchor_to_fit: true,
        }
    }
}

impl Default for StereoConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use jc3vr_core::config::Snapshot;
    use pretty_assertions::assert_eq;

    use super::*;

    /// The defaults are the single-pass feature turned on, exactly as the debug UI's button turns it
    /// on, so the two cannot drift apart.
    #[test]
    fn defaults_are_the_single_pass_feature_turned_on() {
        crate::test_config::register();
        let defaults = Snapshot::defaults();
        let mut turned_on = Snapshot::defaults();
        set_single_pass_feature(&mut turned_on, true);
        let value = |config: &Snapshot| {
            (
                toml::Value::try_from(config.part(&STEREO_CONFIG)).unwrap(),
                toml::Value::try_from(config.part(&VR_CONFIG)).unwrap(),
            )
        };
        assert_eq!(value(&turned_on), value(&defaults));
    }
}
