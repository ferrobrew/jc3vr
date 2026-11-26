//! The set of single-pass switches a shader build bakes in.
//!
//! The switches themselves are declared in one table
//! ([`SinglePassSwitch`](jc3vr_view::stereo::config::SinglePassSwitch)), which records which of them
//! are rewrites applied at shader creation: those only take effect once the shaders are re-created.
//! [`ShaderBuild`] is the state the live shaders were created under, so a caller can tell whether
//! the bytecode matches the configuration.

use jc3vr_view::stereo::config::{STEREO_CONFIG, SinglePassConfig, SinglePassSwitch};
use serde::{Serialize, Serializer, ser::SerializeMap as _};

/// The state the live shaders were created under: whether single-pass and its collapse were live,
/// which gate the creation-time rewrites, and the value of every switch that is such a rewrite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShaderBuild {
    active: bool,
    collapse: bool,
    switches: [bool; SinglePassSwitch::COUNT],
}

impl ShaderBuild {
    /// The build a shader created now would get, from the live configuration and gates.
    pub fn current() -> Self {
        Self::from_config(
            crate::stereo::single_pass::active(),
            jc3vr_view::stereo::layout::collapse_active(),
            &STEREO_CONFIG.query(|config| config.single_pass),
        )
    }

    pub(crate) fn from_config(active: bool, collapse: bool, config: &SinglePassConfig) -> Self {
        let mut switches = [false; SinglePassSwitch::COUNT];
        for &switch in SinglePassSwitch::ALL {
            switches[switch as usize] = switch.rebuilds_shaders() && switch.get(config);
        }
        Self {
            active,
            collapse,
            switches,
        }
    }
}

impl Serialize for ShaderBuild {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("active", &self.active)?;
        map.serialize_entry("collapse", &self.collapse)?;
        map.serialize_entry("switches", &RebuildSwitches(&self.switches))?;
        map.end()
    }
}

struct RebuildSwitches<'a>(&'a [bool; SinglePassSwitch::COUNT]);

impl Serialize for RebuildSwitches<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let rebuilding = SinglePassSwitch::ALL
            .iter()
            .filter(|switch| switch.rebuilds_shaders());
        let mut map = serializer.serialize_map(Some(rebuilding.clone().count()))?;
        for switch in rebuilding {
            map.serialize_entry(switch.name(), &self.0[*switch as usize])?;
        }
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn a_build_differs_only_by_the_switches_it_bakes_in() {
        let base = SinglePassConfig::new();
        for &switch in SinglePassSwitch::ALL {
            let mut changed = base;
            switch.set(&mut changed, !switch.get(&base));
            assert_eq!(
                ShaderBuild::from_config(true, true, &base)
                    != ShaderBuild::from_config(true, true, &changed),
                switch.rebuilds_shaders(),
                "{}",
                switch.name()
            );
        }
    }

    #[test]
    fn a_build_serializes_the_rebuilding_switches_by_name() {
        let value = serde_json::to_value(ShaderBuild::from_config(
            true,
            true,
            &SinglePassConfig::new(),
        ))
        .unwrap();
        assert_eq!(value["active"], true);
        assert_eq!(value["switches"]["ssr_per_eye"], true);
        assert!(value["switches"].get("fsr_per_eye").is_none());
        assert_eq!(
            value["switches"].as_object().unwrap().len(),
            SinglePassSwitch::ALL
                .iter()
                .filter(|switch| switch.rebuilds_shaders())
                .count()
        );
    }
}
