//! The visual-configuration overrides: independent, optional departures from the controlled
//! capture profile that one `visual_configure` request applies for the captures that follow it.
//!
//! The profile resets every one of them on each configuration, so an override lasts exactly as long
//! as the request that names it. The single-pass switches are what lets one schedule capture the
//! single-pass mode with a production fix turned off next to the default capture, from the same
//! scene state.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize, Serializer, ser::SerializeMap as _};

use crate::{
    config::Config,
    fsr::FsrConfig,
    hooks::graphics_engine::config::ExposureConfig,
    stereo::{config::SinglePassConfig, dispatch::RenderMode, switches::SinglePassSwitch},
    testing::visual::headset::HeadsetRig,
    vr::FoveationConfig,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SunShadowsOverride {
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReflectionOverride {
    Enabled,
}

/// Put back, in every mode, a setting the profile turns off or pins, at the value the mod ships with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ShippedOverride {
    Enabled,
}

/// Leave FSR's stereo motion-vector correction out in every mode, to attribute a difference to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CorrectionOverride {
    Disabled,
}

/// Leave out an ambient-occlusion source in every mode, to attribute a difference to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AmbientOcclusionOverride {
    Disabled,
}

/// Restrict the stage images every capture writes, to keep captures at a large eye size small.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StagesOverride {
    /// Only the geometry stages: the depth, the albedo and normal G-buffers, and the velocity.
    Geometry,
}

/// A validated, nonempty set of overrides. Each kind is independent of the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct VisualOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sun_shadows: Option<SunShadowsOverride>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screen_space_reflections: Option<ReflectionOverride>,
    /// The screen-space ambient occlusion block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssao: Option<AmbientOcclusionOverride>,
    /// The ambient-occlusion volumes' pass.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ao_volumes: Option<AmbientOcclusionOverride>,
    /// FSR in place of the engine's anti-aliasing, at the shipped sharpening.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fsr: Option<ShippedOverride>,
    /// The static foveation's peripheral drop and fill-in, at the shipped radii.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foveation: Option<ShippedOverride>,
    /// The engine's own exposure metering, histogram included, in place of the pinned exposure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_exposure: Option<ShippedOverride>,
    /// The screen fade's pass.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screen_fade: Option<ShippedOverride>,
    /// The player-damage overlay's pass.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub player_damage: Option<ShippedOverride>,
    /// FSR's stereo motion-vector correction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fsr_mv_correction: Option<CorrectionOverride>,
    /// A headset-like eye rig substituted for the located views in every mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headset: Option<HeadsetRig>,
    /// The stage images every capture writes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stages: Option<StagesOverride>,
    #[serde(skip_serializing_if = "SinglePassOverrides::is_empty")]
    pub single_pass: SinglePassOverrides,
}

/// The `[visual_overrides]` table of a request file, before validation.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VisualOverridesTable {
    sun_shadows: Option<SunShadowsOverride>,
    screen_space_reflections: Option<ReflectionOverride>,
    ssao: Option<AmbientOcclusionOverride>,
    ao_volumes: Option<AmbientOcclusionOverride>,
    fsr: Option<ShippedOverride>,
    foveation: Option<ShippedOverride>,
    auto_exposure: Option<ShippedOverride>,
    screen_fade: Option<ShippedOverride>,
    player_damage: Option<ShippedOverride>,
    fsr_mv_correction: Option<CorrectionOverride>,
    headset: Option<HeadsetRig>,
    stages: Option<StagesOverride>,
    single_pass: Option<BTreeMap<String, bool>>,
}

/// Single-pass switches forced to a value, by [`SinglePassSwitch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SinglePassOverrides([Option<bool>; SinglePassSwitch::COUNT]);

impl Default for SinglePassOverrides {
    fn default() -> Self {
        Self([None; SinglePassSwitch::COUNT])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OverrideError {
    /// The table names no override at all.
    Empty,
    /// The `single_pass` table is present but names no switch.
    EmptySinglePass,
    /// A `single_pass` key is not a switch that can be overridden.
    UnknownSwitch(String),
    /// Single-pass switches were requested for a mode that does not run single-pass.
    SinglePassOnMode(RenderMode),
}

impl std::fmt::Display for OverrideError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "visual overrides: ")?;
        match self {
            Self::Empty => write!(f, "the table names no override"),
            Self::EmptySinglePass => write!(f, "the single_pass table names no switch"),
            Self::UnknownSwitch(name) => write!(
                f,
                "single_pass.{name} is not an overridable switch (expected one of {})",
                SinglePassSwitch::ALL
                    .iter()
                    .map(|switch| switch.name())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::SinglePassOnMode(mode) => write!(
                f,
                "single_pass switches apply only to the single_pass mode, not {mode:?}"
            ),
        }
    }
}

impl std::error::Error for OverrideError {}

impl TryFrom<VisualOverridesTable> for VisualOverrides {
    type Error = OverrideError;

    fn try_from(table: VisualOverridesTable) -> Result<Self, Self::Error> {
        let single_pass = match table.single_pass {
            None => SinglePassOverrides::default(),
            Some(entries) => SinglePassOverrides::from_names(entries)?,
        };
        let overrides = Self {
            sun_shadows: table.sun_shadows,
            screen_space_reflections: table.screen_space_reflections,
            ssao: table.ssao,
            ao_volumes: table.ao_volumes,
            fsr: table.fsr,
            foveation: table.foveation,
            auto_exposure: table.auto_exposure,
            screen_fade: table.screen_fade,
            player_damage: table.player_damage,
            fsr_mv_correction: table.fsr_mv_correction,
            headset: table.headset,
            stages: table.stages,
            single_pass,
        };
        if overrides.sun_shadows.is_none()
            && overrides.screen_space_reflections.is_none()
            && overrides.ssao.is_none()
            && overrides.ao_volumes.is_none()
            && overrides.shipped().all(|(_, value)| value.is_none())
            && overrides.fsr_mv_correction.is_none()
            && overrides.headset.is_none()
            && overrides.stages.is_none()
            && overrides.single_pass.is_empty()
        {
            return Err(OverrideError::Empty);
        }
        Ok(overrides)
    }
}

impl VisualOverrides {
    /// Check the overrides against the mode they are requested with.
    pub(crate) fn validate_for(&self, mode: RenderMode) -> Result<(), OverrideError> {
        if !self.single_pass.is_empty() && mode != RenderMode::SinglePass {
            return Err(OverrideError::SinglePassOnMode(mode));
        }
        Ok(())
    }

    /// Apply the overrides on top of a freshly applied capture profile.
    pub(crate) fn apply(&self, config: &mut Config) {
        if let Some(SunShadowsOverride::Disabled) = self.sun_shadows {
            config.stereo.disable_sun_shadows = true;
        }
        if let Some(ReflectionOverride::Enabled) = self.screen_space_reflections {
            config.stereo.skip_ssr = false;
        }
        if let Some(AmbientOcclusionOverride::Disabled) = self.ssao {
            config.stereo.disable_ssao = true;
        }
        if let Some(AmbientOcclusionOverride::Disabled) = self.ao_volumes {
            config.stereo.skip_ao_volumes = true;
        }
        if let Some(ShippedOverride::Enabled) = self.fsr {
            config.fsr = FsrConfig::new();
        }
        if let Some(ShippedOverride::Enabled) = self.foveation {
            config.foveation = FoveationConfig::new();
        }
        if let Some(ShippedOverride::Enabled) = self.auto_exposure {
            config.exposure = ExposureConfig::new();
            config.post_fx.skip_histogram = false;
        }
        if let Some(ShippedOverride::Enabled) = self.screen_fade {
            config.post_fx.skip_fade = false;
        }
        if let Some(ShippedOverride::Enabled) = self.player_damage {
            config.post_fx.skip_player_damage = false;
        }
        if let Some(CorrectionOverride::Disabled) = self.fsr_mv_correction {
            config.fsr.stereo_mv_correction = false;
        }
        self.single_pass.apply(&mut config.stereo.single_pass);
    }

    /// The shipped-behaviour overrides by wire name.
    pub(crate) fn shipped(&self) -> impl Iterator<Item = (&'static str, Option<ShippedOverride>)> {
        [
            ("fsr", self.fsr),
            ("foveation", self.foveation),
            ("auto_exposure", self.auto_exposure),
            ("screen_fade", self.screen_fade),
            ("player_damage", self.player_damage),
        ]
        .into_iter()
    }
}

impl SinglePassOverrides {
    /// The switches a `single_pass` table names, by wire name. An empty table is refused, since it
    /// is a request for nothing.
    pub(crate) fn from_names(entries: BTreeMap<String, bool>) -> Result<Self, OverrideError> {
        if entries.is_empty() {
            return Err(OverrideError::EmptySinglePass);
        }
        let mut overrides = Self::default();
        for (name, value) in entries {
            let switch =
                SinglePassSwitch::from_name(&name).ok_or(OverrideError::UnknownSwitch(name))?;
            overrides.0[switch as usize] = Some(value);
        }
        Ok(overrides)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.iter().all(Option::is_none)
    }

    /// The overridden switches and their values, in [`SinglePassSwitch::ALL`] order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (SinglePassSwitch, bool)> + '_ {
        SinglePassSwitch::ALL
            .iter()
            .filter_map(|&switch| self.0[switch as usize].map(|value| (switch, value)))
    }

    pub(crate) fn apply(&self, config: &mut SinglePassConfig) {
        for (switch, value) in self.iter() {
            switch.set(config, value);
        }
    }
}

impl Serialize for SinglePassOverrides {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.iter().count()))?;
        for (switch, value) in self.iter() {
            map.serialize_entry(switch.name(), &value)?;
        }
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn parse(text: &str) -> Result<VisualOverrides, OverrideError> {
        VisualOverrides::try_from(toml::from_str::<VisualOverridesTable>(text).unwrap())
    }

    #[test]
    fn overrides_are_independent() {
        let overrides = parse(
            "sun_shadows = 'disabled'\nscreen_space_reflections = 'enabled'\n\
             [single_pass]\nssr_per_eye = false\nglare_per_eye = true\n",
        )
        .unwrap();
        assert_eq!(overrides.sun_shadows, Some(SunShadowsOverride::Disabled));
        assert_eq!(
            overrides.screen_space_reflections,
            Some(ReflectionOverride::Enabled)
        );
        assert_eq!(
            overrides.single_pass.iter().collect::<Vec<_>>(),
            [
                (SinglePassSwitch::SsrPerEye, false),
                (SinglePassSwitch::GlarePerEye, true)
            ]
        );
    }

    #[test]
    fn empty_tables_and_unknown_switches_are_rejected() {
        assert_eq!(parse(""), Err(OverrideError::Empty));
        assert_eq!(
            parse("[single_pass]\n"),
            Err(OverrideError::EmptySinglePass)
        );
        assert_eq!(
            parse("[single_pass]\nssr_per_eye = false\nssr_per_eyes = false\n"),
            Err(OverrideError::UnknownSwitch("ssr_per_eyes".to_owned()))
        );
        for structural in [
            "enabled",
            "collapse",
            "double_wide",
            "dual_eye",
            "patch_dryrun",
        ] {
            assert_eq!(
                parse(&format!("[single_pass]\n{structural} = false\n")),
                Err(OverrideError::UnknownSwitch(structural.to_owned()))
            );
        }
        assert!(
            toml::from_str::<VisualOverridesTable>("[single_pass]\nssr_per_eye = 0\n").is_err()
        );
        assert!(toml::from_str::<VisualOverridesTable>("other = true\n").is_err());
    }

    #[test]
    fn single_pass_switches_need_the_single_pass_mode() {
        let overrides = parse("[single_pass]\nssr_per_eye = false\n").unwrap();
        assert_eq!(overrides.validate_for(RenderMode::SinglePass), Ok(()));
        for mode in [
            RenderMode::LeftOnly,
            RenderMode::RightOnly,
            RenderMode::Multipass,
        ] {
            assert_eq!(
                overrides.validate_for(mode),
                Err(OverrideError::SinglePassOnMode(mode))
            );
        }
        let shadows = parse("sun_shadows = 'disabled'\n").unwrap();
        assert_eq!(shadows.validate_for(RenderMode::Multipass), Ok(()));
    }

    #[test]
    fn apply_sets_only_the_named_switches() {
        let overrides = parse("[single_pass]\nssr_per_eye = false\n").unwrap();
        let mut config = Config::new();
        let before = config.stereo.single_pass;
        overrides.apply(&mut config);
        for &switch in SinglePassSwitch::ALL {
            let expected = if switch == SinglePassSwitch::SsrPerEye {
                false
            } else {
                switch.get(&before)
            };
            assert_eq!(switch.get(&config.stereo.single_pass), expected);
        }
        assert!(!config.stereo.disable_sun_shadows);
        assert!(!config.stereo.disable_ssao);
        assert!(!config.stereo.skip_ao_volumes);
    }

    #[test]
    fn shipped_behaviours_are_restored_in_any_mode() {
        let overrides = parse(
            "fsr = 'enabled'\nfoveation = 'enabled'\nauto_exposure = 'enabled'\n\
             screen_fade = 'enabled'\nplayer_damage = 'enabled'\n",
        )
        .unwrap();
        for mode in [
            RenderMode::LeftOnly,
            RenderMode::RightOnly,
            RenderMode::Multipass,
            RenderMode::SinglePass,
        ] {
            assert_eq!(overrides.validate_for(mode), Ok(()));
        }
        let mut config = Config::new();
        config.fsr.enabled = false;
        config.fsr.sharpness = 0.9;
        config.foveation.enabled = false;
        config.foveation.debug_show_mask = true;
        config.exposure.force = true;
        config.post_fx.skip_histogram = true;
        config.post_fx.skip_fade = true;
        config.post_fx.skip_player_damage = true;
        config.post_fx.skip_motion_blur = true;
        overrides.apply(&mut config);
        assert!(config.fsr.enabled && config.fsr.sharpen);
        assert_eq!(config.fsr.sharpness, FsrConfig::new().sharpness);
        assert!(config.foveation.enabled && !config.foveation.debug_show_mask);
        assert!(!config.exposure.force && config.exposure.gate && config.exposure.gate_update);
        assert!(!config.post_fx.skip_histogram);
        assert!(!config.post_fx.skip_fade && !config.post_fx.skip_player_damage);
        assert!(
            config.post_fx.skip_motion_blur,
            "an override left out keeps the profile's value"
        );
        assert_eq!(
            serde_json::to_value(overrides).unwrap(),
            serde_json::json!({
                "fsr": "enabled",
                "foveation": "enabled",
                "auto_exposure": "enabled",
                "screen_fade": "enabled",
                "player_damage": "enabled",
            })
        );
        assert!(toml::from_str::<VisualOverridesTable>("fsr = 'disabled'\n").is_err());
        assert!(toml::from_str::<VisualOverridesTable>("foveation = true\n").is_err());
    }

    #[test]
    fn the_fsr_correction_can_be_left_out_in_any_mode() {
        let overrides = parse("fsr_mv_correction = 'disabled'\n").unwrap();
        assert_eq!(overrides.validate_for(RenderMode::LeftOnly), Ok(()));
        let mut config = Config::new();
        overrides.apply(&mut config);
        assert!(!config.fsr.stereo_mv_correction);
        assert_eq!(
            serde_json::to_value(overrides).unwrap(),
            serde_json::json!({"fsr_mv_correction": "disabled"})
        );
        assert!(toml::from_str::<VisualOverridesTable>("fsr_mv_correction = 'enabled'\n").is_err());
    }

    #[test]
    fn a_headset_rig_applies_in_any_mode_and_leaves_the_config_alone() {
        let overrides = parse("headset = 'index'\n").unwrap();
        assert_eq!(overrides.headset, Some(HeadsetRig::Index));
        for mode in [
            RenderMode::LeftOnly,
            RenderMode::RightOnly,
            RenderMode::Multipass,
            RenderMode::SinglePass,
        ] {
            assert_eq!(overrides.validate_for(mode), Ok(()));
        }
        let mut config = Config::new();
        let before = serde_json::to_value(&config).unwrap();
        overrides.apply(&mut config);
        assert_eq!(serde_json::to_value(&config).unwrap(), before);
        assert_eq!(
            serde_json::to_value(overrides).unwrap(),
            serde_json::json!({"headset": "index"})
        );
        assert!(toml::from_str::<VisualOverridesTable>("headset = 'vive'\n").is_err());
    }

    #[test]
    fn the_stage_set_can_be_restricted_in_any_mode() {
        let overrides = parse("stages = 'geometry'\n").unwrap();
        assert_eq!(overrides.stages, Some(StagesOverride::Geometry));
        assert_eq!(overrides.validate_for(RenderMode::LeftOnly), Ok(()));
        assert_eq!(
            serde_json::to_value(overrides).unwrap(),
            serde_json::json!({"stages": "geometry"})
        );
        assert!(toml::from_str::<VisualOverridesTable>("stages = 'all'\n").is_err());
    }

    #[test]
    fn ambient_occlusion_sources_can_be_left_out_in_any_mode() {
        let overrides = parse(
            "ssao = 'disabled'
ao_volumes = 'disabled'
",
        )
        .unwrap();
        assert_eq!(overrides.validate_for(RenderMode::LeftOnly), Ok(()));
        let mut config = Config::new();
        overrides.apply(&mut config);
        assert!(config.stereo.disable_ssao);
        assert!(config.stereo.skip_ao_volumes);
        assert_eq!(
            serde_json::to_value(
                parse(
                    "ao_volumes = 'disabled'
"
                )
                .unwrap()
            )
            .unwrap(),
            serde_json::json!({"ao_volumes": "disabled"})
        );
        assert!(
            toml::from_str::<VisualOverridesTable>(
                "ssao = 'skipped'
"
            )
            .is_err()
        );
    }

    #[test]
    fn overrides_serialize_by_name() {
        let overrides = parse("[single_pass]\nssr_per_eye = false\n").unwrap();
        assert_eq!(
            serde_json::to_value(overrides).unwrap(),
            serde_json::json!({"single_pass": {"ssr_per_eye": false}})
        );
        let shadows = parse("sun_shadows = 'disabled'\n").unwrap();
        assert_eq!(
            serde_json::to_value(shadows).unwrap(),
            serde_json::json!({"sun_shadows": "disabled"})
        );
    }
}
