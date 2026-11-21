//! The named single-pass production switches, and the set of them a shader build bakes in.
//!
//! Every production fix in [`SinglePassConfig`] is a boolean switch. [`SinglePassSwitch`] names
//! the ones a caller may turn on or off on their own (the structural switches that make up the
//! single-pass mode itself, and the diagnostics, are not among them), and records which of them
//! are rewrites applied at shader creation: those only take effect once the shaders are re-created.
//! [`ShaderBuild`] is the state the live shaders were created under, so a caller can tell whether
//! the bytecode matches the configuration.

use serde::{Serialize, Serializer, ser::SerializeMap as _};

use crate::{config::Config, stereo::config::SinglePassConfig};

macro_rules! single_pass_switches {
    ($($variant:ident => $field:ident, $rebuilds:literal;)*) => {
        /// A single-pass production switch: one boolean field of [`SinglePassConfig`].
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(crate) enum SinglePassSwitch {
            $($variant,)*
        }

        impl SinglePassSwitch {
            /// Every switch, in declaration order.
            pub(crate) const ALL: &[Self] = &[$(Self::$variant,)*];

            /// The switch's configuration field name, which is also its name on the wire.
            pub(crate) const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => stringify!($field),)*
                }
            }

            /// Whether the switch is a rewrite applied when a shader is created, so a change only
            /// takes effect once the shaders are re-created.
            pub(crate) const fn rebuilds_shaders(self) -> bool {
                match self {
                    $(Self::$variant => $rebuilds,)*
                }
            }

            pub(crate) fn get(self, config: &SinglePassConfig) -> bool {
                match self {
                    $(Self::$variant => config.$field,)*
                }
            }

            pub(crate) fn set(self, config: &mut SinglePassConfig, value: bool) {
                match self {
                    $(Self::$variant => config.$field = value,)*
                }
            }
        }
    };
}

single_pass_switches! {
    Reproject => reproject, true;
    ReprojectCameraOnly => reproject_camera_only, true;
    Terrain => terrain, true;
    TerrainPatchPerEye => terrain_patch_per_eye, true;
    TerrainEyeRows => terrain_eye_rows, true;
    FullViewProjectionRows => full_view_projection_rows, true;
    TreeImpostors => tree_impostors, true;
    TreeImpostorPixelSizePerEye => tree_impostor_pixel_size_per_eye, false;
    TreeImpostorScreenDoorPerEye => tree_impostor_screen_door_per_eye, true;
    StaticLightsPerEye => static_lights_per_eye, true;
    MoonPerEye => moon_per_eye, false;
    WaterPlanePerEye => water_plane_per_eye, true;
    CloudsPerEye => clouds_per_eye, true;
    StarsPerEye => stars_per_eye, true;
    Bark => bark, true;
    Foliage => foliage, true;
    VegetationOnce => vegetation_once, false;
    Occluder => occluder, false;
    InstancedPerEye => instanced_per_eye, false;
    HwInstancedOnce => hw_instanced_once, false;
    IndirectPerEye => indirect_per_eye, false;
    VertexGlobalsPerEye => vertex_globals_per_eye, false;
    ReconstructPerEye => reconstruct_per_eye, false;
    FullscreenCameraPerEye => fullscreen_camera_per_eye, false;
    PixelCameraPerEye => pixel_camera_per_eye, true;
    LocalLightsPerEye => local_lights_per_eye, true;
    ScreenDoorPerEye => screen_door_per_eye, true;
    VelocityPerEye => velocity_per_eye, true;
    ClipOriginPerEye => clip_origin_per_eye, true;
    TerrainDetailEyeRows => terrain_detail_eye_rows, false;
    TerrainCullBothEyes => terrain_cull_both_eyes, false;
    AtmosphericPerEye => atmospheric_per_eye, false;
    FogVolumePerEye => fog_volume_per_eye, false;
    WaterUvPerEye => water_uv_per_eye, false;
    NvwaterPerEye => nvwater_per_eye, false;
    WaterReflectionPerEye => water_reflection_per_eye, false;
    WaterReflectionEyeSize => water_reflection_eye_size, false;
    WaterCameraPerEye => water_camera_per_eye, false;
    SsdecalGeometryPerEye => ssdecal_geometry_per_eye, false;
    Slot13PerEye => slot13_per_eye, false;
    NonIndexedGeometryOnce => non_indexed_geometry_once, false;
    CollapseViewportFollowsTarget => collapse_viewport_follows_target, false;
    SsaoPerEye => ssao_per_eye, true;
    SsaoExactSampling => ssao_exact_sampling, true;
    AoVolumesPerEye => ao_volumes_per_eye, true;
    SsrPerEye => ssr_per_eye, true;
    SubsurfacePerEye => subsurface_per_eye, false;
    DofPerEye => dof_per_eye, false;
    SsdecalPerEye => ssdecal_per_eye, true;
    CompositeOverlaysPerEye => composite_overlays_per_eye, true;
    GlarePerEye => glare_per_eye, true;
    FoveationPerEye => foveation_per_eye, false;
    FoveationGbuffer => foveation_gbuffer, false;
    FoveationLighting => foveation_lighting, false;
    FoveationQuads => foveation_quads, false;
    FsrPerEye => fsr_per_eye, false;
    ClusteredPerEye => clustered_per_eye, false;
    ClusteredPerEyeLightView => clustered_per_eye_light_view, false;
    UniformViewportSlots => uniform_viewport_slots, false;
    ViewportBindCache => viewport_bind_cache, false;
}

impl SinglePassSwitch {
    pub(crate) const COUNT: usize = Self::ALL.len();

    /// The switch with this wire name, if there is one.
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|switch| switch.name() == name)
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// The state the live shaders were created under: whether single-pass and its collapse were live,
/// which gate the creation-time rewrites, and the value of every switch that is such a rewrite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShaderBuild {
    active: bool,
    collapse: bool,
    switches: [bool; SinglePassSwitch::COUNT],
}

impl ShaderBuild {
    /// The build a shader created now would get, from the live configuration and gates.
    pub(crate) fn current() -> Self {
        Self::from_config(
            crate::stereo::single_pass::active(),
            crate::stereo::single_pass::collapse_active(),
            &Config::lock_query(|config| config.stereo.single_pass),
        )
    }

    pub(crate) fn from_config(active: bool, collapse: bool, config: &SinglePassConfig) -> Self {
        let mut switches = [false; SinglePassSwitch::COUNT];
        for &switch in SinglePassSwitch::ALL {
            switches[switch.index()] = switch.rebuilds_shaders() && switch.get(config);
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
            map.serialize_entry(switch.name(), &self.0[switch.index()])?;
        }
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn names_round_trip_and_are_unique() {
        for &switch in SinglePassSwitch::ALL {
            assert_eq!(SinglePassSwitch::from_name(switch.name()), Some(switch));
        }
        assert_eq!(SinglePassSwitch::from_name("enabled"), None);
        assert_eq!(SinglePassSwitch::from_name("collapse"), None);
        assert_eq!(SinglePassSwitch::from_name("ssr_per_eye "), None);
    }

    #[test]
    fn set_writes_only_its_own_field() {
        for &switch in SinglePassSwitch::ALL {
            let mut config = SinglePassConfig::new();
            let before = config;
            switch.set(&mut config, !switch.get(&before));
            for &other in SinglePassSwitch::ALL {
                assert_eq!(
                    other.get(&config) != other.get(&before),
                    other == switch,
                    "{} changed {}",
                    switch.name(),
                    other.name()
                );
            }
        }
    }

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
        assert!(value["switches"].get("dof_per_eye").is_none());
        assert_eq!(
            value["switches"].as_object().unwrap().len(),
            SinglePassSwitch::ALL
                .iter()
                .filter(|switch| switch.rebuilds_shaders())
                .count()
        );
    }
}
