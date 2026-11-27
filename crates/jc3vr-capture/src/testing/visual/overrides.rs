//! The visual-configuration overrides: independent, optional departures from the controlled
//! capture profile that one `visual_configure` request applies for the captures that follow it.
//!
//! The profile resets every one of them on each configuration, so an override lasts exactly as long
//! as the request that names it. The single-pass switches are what lets one schedule capture the
//! single-pass mode with a production fix turned off next to the default capture, from the same
//! scene state.

use std::collections::BTreeMap;

use jc3vr_core::config::Parts;
use jc3vr_render::hooks::graphics_engine::config::{
    EXPOSURE_CONFIG, ExposureConfig, POST_FX_CONFIG,
};
use jc3vr_view::{
    fsr::{AntiAliasing, FSR_CONFIG, FsrConfig},
    stereo::{
        config::{STEREO_CONFIG, SinglePassConfig, SinglePassSwitch},
        dispatch::RenderMode,
    },
    vr::{FOVEATION_CONFIG, FoveationConfig, VR_CONFIG},
};
use serde::{Deserialize, Serialize, Serializer, ser::SerializeMap as _};

use crate::testing::visual::headset::HeadsetRig;

/// Put back, in every mode, a setting the profile turns off or pins, at the value the mod ships with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShippedOverride {
    Enabled,
}

/// Leave FSR's stereo motion-vector correction out in every mode, to attribute a difference to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionOverride {
    Disabled,
}

/// A render resolution scale (each side), kept in thousandths so that the overrides compare
/// exactly. Within [`ResolutionScaleOverride::MIN`] and [`ResolutionScaleOverride::MAX`]: the top
/// is twice the session's pixels, past which the card's video memory runs out at the headset's size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolutionScaleOverride(u16);

impl ResolutionScaleOverride {
    pub(crate) const MIN: f64 = 0.5;
    pub(crate) const MAX: f64 = 1.415;

    pub(crate) fn scale(self) -> f32 {
        f32::from(self.0) / 1000.0
    }
}

impl<'de> Deserialize<'de> for ResolutionScaleOverride {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = f64::deserialize(deserializer)?;
        if !(Self::MIN..=Self::MAX).contains(&value) {
            return Err(serde::de::Error::custom(format!(
                "resolution_scale {value} is outside [{}, {}]",
                Self::MIN,
                Self::MAX
            )));
        }
        Ok(Self((value * 1000.0).round() as u16))
    }
}

impl Serialize for ResolutionScaleOverride {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(f64::from(self.0) / 1000.0)
    }
}

/// FSR's sharpening strength in every mode, in place of the shipped one, held in thousandths so the
/// overrides compare exactly. Within [0, 1].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SharpnessOverride(u16);

impl SharpnessOverride {
    pub(crate) fn sharpness(self) -> f32 {
        f32::from(self.0) / 1000.0
    }
}

impl<'de> Deserialize<'de> for SharpnessOverride {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = f64::deserialize(deserializer)?;
        if !(0.0..=1.0).contains(&value) {
            return Err(serde::de::Error::custom(format!(
                "fsr_sharpness {value} is outside [0, 1]"
            )));
        }
        Ok(Self((value * 1000.0).round() as u16))
    }
}

impl Serialize for SharpnessOverride {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(f64::from(self.0) / 1000.0)
    }
}

/// A tuning value of the mod's temporal resolve in every mode, in place of the shipped one, held
/// in thousandths so the overrides compare exactly. Within [0, 4].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TaaTuningOverride(u16);

impl TaaTuningOverride {
    pub(crate) fn value(self) -> f32 {
        f32::from(self.0) / 1000.0
    }
}

impl<'de> Deserialize<'de> for TaaTuningOverride {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = f64::deserialize(deserializer)?;
        if !(0.0..=4.0).contains(&value) {
            return Err(serde::de::Error::custom(format!(
                "taa tuning {value} is outside [0, 4]"
            )));
        }
        Ok(Self((value * 1000.0).round() as u16))
    }
}

impl Serialize for TaaTuningOverride {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(f64::from(self.0) / 1000.0)
    }
}

/// Restrict the stage images every capture writes, to keep captures at a large eye size small.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StagesOverride {
    /// Only the geometry stages: the depth, the albedo and normal G-buffers, and the velocity.
    Geometry,
}

/// A validated, nonempty set of overrides. Each kind is independent of the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct VisualOverrides {
    /// FSR in place of the engine's anti-aliasing, at the shipped sharpening.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fsr: Option<ShippedOverride>,
    /// The mod's own temporal resolve in place of the engine's anti-aliasing, with FSR's shipped
    /// inputs (the jitter, the hybrid decode, and the correction).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub taa: Option<ShippedOverride>,
    /// The static foveation's peripheral drop and fill-in, at the shipped radii.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foveation: Option<ShippedOverride>,
    /// The engine's own exposure metering, histogram included, in place of the pinned exposure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) auto_exposure: Option<ShippedOverride>,
    /// The screen fade's pass.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) screen_fade: Option<ShippedOverride>,
    /// The player-damage overlay's pass.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) player_damage: Option<ShippedOverride>,
    /// FSR's stereo motion-vector correction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) fsr_mv_correction: Option<CorrectionOverride>,
    /// FSR's hybrid motion vectors (the camera's motion from depth where the engine wrote none).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) fsr_hybrid_velocity: Option<CorrectionOverride>,
    /// FSR's sub-pixel camera jitter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fsr_jitter: Option<CorrectionOverride>,
    /// The engine's anti-aliasing where FSR does not run: disabled leaves the image with none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) anti_aliasing: Option<CorrectionOverride>,
    /// FSR's sharpening pass.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) fsr_sharpen: Option<CorrectionOverride>,
    /// FSR's sharpening pass, turned on at this strength.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) fsr_sharpness: Option<SharpnessOverride>,
    /// The temporal resolve's history share at rest (`fsr.taa.history_weight`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) taa_history_weight: Option<TaaTuningOverride>,
    /// The temporal resolve's clip box at rest (`fsr.taa.clip_gamma_rest`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) taa_clip_gamma_rest: Option<TaaTuningOverride>,
    /// The temporal resolve's unjitter kernel falloff (`fsr.taa.current_sharpness`), in tenths.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) taa_current_sharpness: Option<TaaTuningOverride>,
    /// The temporal resolve's history share in motion (`fsr.taa.motion_history_weight`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) taa_motion_history_weight: Option<TaaTuningOverride>,
    /// A headset-like eye rig substituted for the located views in every mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headset: Option<HeadsetRig>,
    /// The render resolution scale (each side) in every mode, in place of the session's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution_scale: Option<ResolutionScaleOverride>,
    /// The stage images every capture writes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stages: Option<StagesOverride>,
    #[serde(skip_serializing_if = "SinglePassOverrides::is_empty")]
    pub single_pass: SinglePassOverrides,
}

/// The `[visual_overrides]` table of a request file, before validation.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisualOverridesTable {
    fsr: Option<ShippedOverride>,
    taa: Option<ShippedOverride>,
    foveation: Option<ShippedOverride>,
    auto_exposure: Option<ShippedOverride>,
    screen_fade: Option<ShippedOverride>,
    player_damage: Option<ShippedOverride>,
    fsr_mv_correction: Option<CorrectionOverride>,
    fsr_hybrid_velocity: Option<CorrectionOverride>,
    fsr_jitter: Option<CorrectionOverride>,
    anti_aliasing: Option<CorrectionOverride>,
    fsr_sharpen: Option<CorrectionOverride>,
    fsr_sharpness: Option<SharpnessOverride>,
    taa_history_weight: Option<TaaTuningOverride>,
    taa_clip_gamma_rest: Option<TaaTuningOverride>,
    taa_current_sharpness: Option<TaaTuningOverride>,
    taa_motion_history_weight: Option<TaaTuningOverride>,
    headset: Option<HeadsetRig>,
    resolution_scale: Option<ResolutionScaleOverride>,
    stages: Option<StagesOverride>,
    single_pass: Option<BTreeMap<String, bool>>,
}

/// Single-pass switches forced to a value, by [`SinglePassSwitch`]: a bit per switch for whether it
/// is forced, and one for the value it is forced to. Packed, since a request carries one and the
/// switch list keeps growing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SinglePassOverrides {
    forced: u128,
    values: u128,
}

const _: () = assert!(SinglePassSwitch::COUNT <= u128::BITS as usize);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverrideError {
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
            fsr: table.fsr,
            taa: table.taa,
            foveation: table.foveation,
            auto_exposure: table.auto_exposure,
            screen_fade: table.screen_fade,
            player_damage: table.player_damage,
            fsr_mv_correction: table.fsr_mv_correction,
            fsr_hybrid_velocity: table.fsr_hybrid_velocity,
            fsr_jitter: table.fsr_jitter,
            anti_aliasing: table.anti_aliasing,
            fsr_sharpen: table.fsr_sharpen,
            fsr_sharpness: table.fsr_sharpness,
            taa_history_weight: table.taa_history_weight,
            taa_clip_gamma_rest: table.taa_clip_gamma_rest,
            taa_current_sharpness: table.taa_current_sharpness,
            taa_motion_history_weight: table.taa_motion_history_weight,
            headset: table.headset,
            resolution_scale: table.resolution_scale,
            stages: table.stages,
            single_pass,
        };
        if overrides.shipped().all(|(_, value)| value.is_none())
            && overrides.fsr_mv_correction.is_none()
            && overrides.fsr_hybrid_velocity.is_none()
            && overrides.fsr_jitter.is_none()
            && overrides.anti_aliasing.is_none()
            && overrides.fsr_sharpen.is_none()
            && overrides.fsr_sharpness.is_none()
            && overrides.taa_history_weight.is_none()
            && overrides.taa_clip_gamma_rest.is_none()
            && overrides.taa_current_sharpness.is_none()
            && overrides.taa_motion_history_weight.is_none()
            && overrides.headset.is_none()
            && overrides.resolution_scale.is_none()
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
    pub fn validate_for(&self, mode: RenderMode) -> Result<(), OverrideError> {
        if !self.single_pass.is_empty() && mode != RenderMode::SinglePass {
            return Err(OverrideError::SinglePassOnMode(mode));
        }
        Ok(())
    }

    /// Apply the overrides on top of a freshly applied capture profile.
    pub(crate) fn apply(&self, config: &mut impl Parts) {
        if let Some(ShippedOverride::Enabled) = self.fsr {
            *config.part_mut(&FSR_CONFIG) = FsrConfig::new();
        }
        if let Some(ShippedOverride::Enabled) = self.taa {
            *config.part_mut(&FSR_CONFIG) = FsrConfig::new();
            config.part_mut(&FSR_CONFIG).antialiasing = AntiAliasing::Taa;
        }
        if let Some(ShippedOverride::Enabled) = self.foveation {
            *config.part_mut(&FOVEATION_CONFIG) = FoveationConfig::new();
        }
        if let Some(ShippedOverride::Enabled) = self.auto_exposure {
            *config.part_mut(&EXPOSURE_CONFIG) = ExposureConfig::new();
            config.part_mut(&POST_FX_CONFIG).skip_histogram = false;
        }
        if let Some(ShippedOverride::Enabled) = self.screen_fade {
            config.part_mut(&POST_FX_CONFIG).skip_fade = false;
        }
        if let Some(ShippedOverride::Enabled) = self.player_damage {
            config.part_mut(&POST_FX_CONFIG).skip_player_damage = false;
        }
        if let Some(CorrectionOverride::Disabled) = self.fsr_mv_correction {
            config.part_mut(&FSR_CONFIG).stereo_mv_correction = false;
        }
        if let Some(CorrectionOverride::Disabled) = self.fsr_hybrid_velocity {
            config.part_mut(&FSR_CONFIG).hybrid_velocity = false;
        }
        if let Some(CorrectionOverride::Disabled) = self.fsr_jitter {
            config.part_mut(&FSR_CONFIG).jitter = false;
        }
        if let Some(CorrectionOverride::Disabled) = self.anti_aliasing {
            config.part_mut(&POST_FX_CONFIG).skip_anti_aliasing = true;
        }
        if let Some(sharpness) = self.fsr_sharpness {
            config.part_mut(&FSR_CONFIG).sharpen = true;
            config.part_mut(&FSR_CONFIG).sharpness = sharpness.sharpness();
        }
        if let Some(CorrectionOverride::Disabled) = self.fsr_sharpen {
            config.part_mut(&FSR_CONFIG).sharpen = false;
        }
        if let Some(weight) = self.taa_history_weight {
            config.part_mut(&FSR_CONFIG).taa.history_weight = weight.value();
        }
        if let Some(gamma) = self.taa_clip_gamma_rest {
            config.part_mut(&FSR_CONFIG).taa.clip_gamma_rest = gamma.value();
        }
        if let Some(weight) = self.taa_motion_history_weight {
            config.part_mut(&FSR_CONFIG).taa.motion_history_weight = weight.value();
        }
        if let Some(sharpness) = self.taa_current_sharpness {
            config.part_mut(&FSR_CONFIG).taa.current_sharpness = sharpness.value() * 10.0;
        }
        if let Some(scale) = self.resolution_scale {
            config.part_mut(&VR_CONFIG).resolution_scale = scale.scale();
        }
        self.single_pass
            .apply(&mut config.part_mut(&STEREO_CONFIG).single_pass);
    }

    /// The shipped-behaviour overrides by wire name.
    pub(crate) fn shipped(&self) -> impl Iterator<Item = (&'static str, Option<ShippedOverride>)> {
        [
            ("fsr", self.fsr),
            ("taa", self.taa),
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
            overrides.forced |= 1 << switch as usize;
            if value {
                overrides.values |= 1 << switch as usize;
            }
        }
        Ok(overrides)
    }

    pub fn is_empty(&self) -> bool {
        self.forced == 0
    }

    /// The overridden switches and their values, in [`SinglePassSwitch::ALL`] order.
    pub fn iter(&self) -> impl Iterator<Item = (SinglePassSwitch, bool)> + '_ {
        SinglePassSwitch::ALL
            .iter()
            .filter(|&&switch| self.forced & (1 << switch as usize) != 0)
            .map(|&switch| (switch, self.values & (1 << switch as usize) != 0))
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
    use jc3vr_core::config::Snapshot;
    use pretty_assertions::assert_eq;

    use super::*;

    fn parse(text: &str) -> Result<VisualOverrides, OverrideError> {
        VisualOverrides::try_from(toml::from_str::<VisualOverridesTable>(text).unwrap())
    }

    #[test]
    fn overrides_are_independent() {
        let overrides = parse(
            "fsr_jitter = 'disabled'\n[single_pass]\nssr_per_eye = false\nfsr_per_eye = true\n",
        )
        .unwrap();
        assert_eq!(overrides.fsr_jitter, Some(CorrectionOverride::Disabled));
        assert_eq!(
            overrides.single_pass.iter().collect::<Vec<_>>(),
            [
                (SinglePassSwitch::FsrPerEye, true),
                (SinglePassSwitch::SsrPerEye, false)
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
    fn a_resolution_scale_applies_within_its_range() {
        crate::test_config::register();
        let overrides = parse("resolution_scale = 1.414\n").unwrap();
        let mut config = Snapshot::defaults();
        overrides.apply(&mut config);
        assert_eq!(config.part(&VR_CONFIG).resolution_scale, 1.414);
        assert_eq!(
            serde_json::to_value(overrides).unwrap(),
            serde_json::json!({"resolution_scale": 1.414})
        );
        assert!(toml::from_str::<VisualOverridesTable>("resolution_scale = 2.0\n").is_err());
        assert!(toml::from_str::<VisualOverridesTable>("resolution_scale = 0.2\n").is_err());
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
        let jitter = parse("fsr_jitter = 'disabled'\n").unwrap();
        assert_eq!(jitter.validate_for(RenderMode::Multipass), Ok(()));
    }

    #[test]
    fn apply_sets_only_the_named_switches() {
        crate::test_config::register();
        let overrides = parse("[single_pass]\nssr_per_eye = false\n").unwrap();
        let mut config = Snapshot::defaults();
        let before = config.part(&STEREO_CONFIG).single_pass;
        overrides.apply(&mut config);
        for &switch in SinglePassSwitch::ALL {
            let expected = if switch == SinglePassSwitch::SsrPerEye {
                false
            } else {
                switch.get(&before)
            };
            assert_eq!(
                switch.get(&config.part(&STEREO_CONFIG).single_pass),
                expected
            );
        }
    }

    #[test]
    fn shipped_behaviours_are_restored_in_any_mode() {
        crate::test_config::register();
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
        let mut config = Snapshot::defaults();
        config.part_mut(&FSR_CONFIG).antialiasing = AntiAliasing::Engine;
        config.part_mut(&FSR_CONFIG).sharpness = 0.9;
        config.part_mut(&FOVEATION_CONFIG).enabled = false;
        config.part_mut(&FOVEATION_CONFIG).debug_show_mask = true;
        config.part_mut(&EXPOSURE_CONFIG).force = true;
        config.part_mut(&POST_FX_CONFIG).skip_histogram = true;
        config.part_mut(&POST_FX_CONFIG).skip_fade = true;
        config.part_mut(&POST_FX_CONFIG).skip_player_damage = true;
        config.part_mut(&POST_FX_CONFIG).skip_motion_blur = true;
        overrides.apply(&mut config);
        assert!(
            config.part(&FSR_CONFIG).antialiasing == AntiAliasing::Fsr
                && config.part(&FSR_CONFIG).sharpen == FsrConfig::new().sharpen
        );
        assert_eq!(
            config.part(&FSR_CONFIG).sharpness,
            FsrConfig::new().sharpness
        );
        assert!(
            config.part(&FOVEATION_CONFIG).enabled
                && !config.part(&FOVEATION_CONFIG).debug_show_mask
        );
        assert!(!config.part(&EXPOSURE_CONFIG).force);
        assert!(!config.part(&POST_FX_CONFIG).skip_histogram);
        assert!(
            !config.part(&POST_FX_CONFIG).skip_fade
                && !config.part(&POST_FX_CONFIG).skip_player_damage
        );
        assert!(
            config.part(&POST_FX_CONFIG).skip_motion_blur,
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
        crate::test_config::register();
        let overrides = parse("fsr_mv_correction = 'disabled'\n").unwrap();
        assert_eq!(overrides.validate_for(RenderMode::LeftOnly), Ok(()));
        let mut config = Snapshot::defaults();
        overrides.apply(&mut config);
        assert!(!config.part(&FSR_CONFIG).stereo_mv_correction);
        assert_eq!(
            serde_json::to_value(overrides).unwrap(),
            serde_json::json!({"fsr_mv_correction": "disabled"})
        );
        assert!(toml::from_str::<VisualOverridesTable>("fsr_mv_correction = 'enabled'\n").is_err());
    }

    #[test]
    fn fsrs_jitter_sharpening_and_the_game_anti_aliasing_can_be_overridden() {
        crate::test_config::register();
        let overrides = parse(
            "fsr = 'enabled'\nfsr_jitter = 'disabled'\nfsr_sharpness = 0.45\n\
             anti_aliasing = 'disabled'\n",
        )
        .unwrap();
        let mut config = Snapshot::defaults();
        overrides.apply(&mut config);
        assert!(
            config.part(&FSR_CONFIG).antialiasing == AntiAliasing::Fsr
                && !config.part(&FSR_CONFIG).jitter
                && config.part(&FSR_CONFIG).sharpen
        );
        assert!((config.part(&FSR_CONFIG).sharpness - 0.45).abs() < 1e-6);
        assert!(config.part(&POST_FX_CONFIG).skip_anti_aliasing);
        assert_eq!(
            serde_json::to_value(overrides).unwrap(),
            serde_json::json!({
                "fsr": "enabled",
                "fsr_jitter": "disabled",
                "fsr_sharpness": 0.45,
                "anti_aliasing": "disabled",
            })
        );
        let mut config = Snapshot::defaults();
        parse("fsr_sharpen = 'disabled'\n")
            .unwrap()
            .apply(&mut config);
        assert!(!config.part(&FSR_CONFIG).sharpen);
        assert!(toml::from_str::<VisualOverridesTable>("fsr_sharpness = 1.5\n").is_err());
    }

    #[test]
    fn a_headset_rig_applies_in_any_mode_and_leaves_the_config_alone() {
        crate::test_config::register();
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
        let mut config = Snapshot::defaults();
        let before = config.to_toml().unwrap();
        overrides.apply(&mut config);
        assert_eq!(config.to_toml().unwrap(), before);
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
    fn overrides_serialize_by_name() {
        let overrides = parse("[single_pass]\nssr_per_eye = false\n").unwrap();
        assert_eq!(
            serde_json::to_value(overrides).unwrap(),
            serde_json::json!({"single_pass": {"ssr_per_eye": false}})
        );
        let jitter = parse("fsr_jitter = 'disabled'\n").unwrap();
        assert_eq!(
            serde_json::to_value(jitter).unwrap(),
            serde_json::json!({"fsr_jitter": "disabled"})
        );
    }
}
