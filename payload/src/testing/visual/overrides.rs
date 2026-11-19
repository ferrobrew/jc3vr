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
    stereo::{config::SinglePassConfig, dispatch::RenderMode, switches::SinglePassSwitch},
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

/// A validated, nonempty set of overrides. Each kind is independent of the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct VisualOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sun_shadows: Option<SunShadowsOverride>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screen_space_reflections: Option<ReflectionOverride>,
    #[serde(skip_serializing_if = "SinglePassOverrides::is_empty")]
    pub single_pass: SinglePassOverrides,
}

/// The `[visual_overrides]` table of a request file, before validation.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VisualOverridesTable {
    sun_shadows: Option<SunShadowsOverride>,
    screen_space_reflections: Option<ReflectionOverride>,
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
            Some(entries) if entries.is_empty() => return Err(OverrideError::EmptySinglePass),
            Some(entries) => {
                let mut overrides = SinglePassOverrides::default();
                for (name, value) in entries {
                    let switch = SinglePassSwitch::from_name(&name)
                        .ok_or(OverrideError::UnknownSwitch(name))?;
                    overrides.0[switch as usize] = Some(value);
                }
                overrides
            }
        };
        let overrides = Self {
            sun_shadows: table.sun_shadows,
            screen_space_reflections: table.screen_space_reflections,
            single_pass,
        };
        if overrides.sun_shadows.is_none()
            && overrides.screen_space_reflections.is_none()
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
        self.single_pass.apply(&mut config.stereo.single_pass);
    }
}

impl SinglePassOverrides {
    pub(crate) fn is_empty(&self) -> bool {
        self.0.iter().all(Option::is_none)
    }

    /// The overridden switches and their values, in [`SinglePassSwitch::ALL`] order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (SinglePassSwitch, bool)> + '_ {
        SinglePassSwitch::ALL
            .iter()
            .filter_map(|&switch| self.0[switch as usize].map(|value| (switch, value)))
    }

    fn apply(&self, config: &mut SinglePassConfig) {
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
