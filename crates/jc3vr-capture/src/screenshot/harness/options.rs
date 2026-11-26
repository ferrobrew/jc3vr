//! A screenshot request's options: what the harness records or tries beside the eye images, read
//! from the request file once and held by the active capture.
//!
//! Most options are diagnostics built on one another, so an option names the one it depends on and
//! [`CaptureRequest::conflict`] refuses a request that leaves a dependency out. The checks that
//! depend on the render mode or on the shader census run when the harness admits the request.

use jc3vr_view::stereo::dispatch::RenderMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WaterReflection {
    PlanarUv,
    LakeSamples,
    LakeLighting,
    LakeSky,
    LakeSkyBuffer,
    LakeResponse,
    LakeNearPatch,
    LakeTessPatch,
    LakePatchDraw,
    LakePatchOwners,
    LakeCameraControl,
    LakeHullCamera,
    LakeDomainCamera,
    EyeWidthPlanar,
    MirroredPlanar,
    ProjectedPlanar,
    PristineLake,
    ZeroWeight,
    ZeroPlanar,
    CenterPlanar,
    PackedRefraction,
}

impl WaterReflection {
    /// Whether the treatment reads the selected lake draw's surface inputs, so a request for it
    /// must capture them.
    pub(crate) fn reads_surface_inputs(self) -> bool {
        matches!(
            self,
            Self::EyeWidthPlanar
                | Self::LakeSamples
                | Self::LakeLighting
                | Self::LakeSky
                | Self::LakeSkyBuffer
                | Self::LakeResponse
                | Self::LakeNearPatch
                | Self::LakeTessPatch
                | Self::LakePatchDraw
                | Self::LakeCameraControl
                | Self::LakePatchOwners
                | Self::LakeDomainCamera
                | Self::LakeHullCamera
        )
    }

    /// Whether the treatment binds a capture-only rewrite of the lake shader.
    fn rewrites_lake(self) -> bool {
        matches!(
            self,
            Self::LakeSamples
                | Self::LakeLighting
                | Self::LakeSky
                | Self::LakeSkyBuffer
                | Self::LakeResponse
                | Self::PlanarUv
                | Self::EyeWidthPlanar
                | Self::MirroredPlanar
                | Self::ProjectedPlanar
                | Self::CenterPlanar
        )
    }

    pub(crate) fn supported(self, mode: RenderMode, selection: Option<u8>) -> bool {
        match self {
            Self::ZeroWeight => mode == RenderMode::Multipass,
            Self::PlanarUv
            | Self::LakeSamples
            | Self::LakeLighting
            | Self::LakeResponse
            | Self::LakeNearPatch
            | Self::LakeTessPatch
            | Self::LakePatchDraw
            | Self::LakePatchOwners
            | Self::LakeCameraControl => {
                selection == Some(7)
                    && matches!(
                        mode,
                        RenderMode::LeftOnly
                            | RenderMode::RightOnly
                            | RenderMode::Multipass
                            | RenderMode::SinglePass
                    )
            }
            Self::ZeroPlanar => {
                selection.is_some()
                    && matches!(
                        mode,
                        RenderMode::LeftOnly
                            | RenderMode::RightOnly
                            | RenderMode::Multipass
                            | RenderMode::SinglePass
                    )
            }
            Self::LakeHullCamera
            | Self::LakeDomainCamera
            | Self::EyeWidthPlanar
            | Self::LakeSky
            | Self::LakeSkyBuffer => mode == RenderMode::SinglePass && selection == Some(7),
            Self::MirroredPlanar
            | Self::ProjectedPlanar
            | Self::PristineLake
            | Self::PackedRefraction
            | Self::CenterPlanar => mode == RenderMode::SinglePass && selection.is_some(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SunDepthClip {
    DisabledRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CascadeScale {
    ShoreLow,
    ShoreHigh,
}

impl CascadeScale {
    pub(crate) fn bits(self) -> u32 {
        match self {
            Self::ShoreLow => 0x3c07e14c,
            Self::ShoreHigh => 0x3c07ec65,
        }
    }
}

/// The screenshot options of one request. Every option is off by default, so a request file names
/// only the ones it sets.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct CaptureRequest {
    pub water_reflection: Option<WaterReflection>,
    pub water_inputs: bool,
    pub water_tess_draw: Option<u8>,
    pub patch_draw_ordinal: Option<u8>,
    pub second_patch_ordinal: Option<u8>,
    pub water_surface_inputs: bool,
    pub late_sky_stages: bool,
    pub draw_census: bool,
    pub fog_inputs: bool,
    pub group_sources: bool,
    pub group_inputs: bool,
    pub terrain_interval: bool,
    pub base_sources: bool,
    pub base_projection: bool,
    pub group_cards: bool,
    pub group_residual: bool,
    pub aa_weights: bool,
    pub aa_draws: bool,
    pub lake_owner_four: bool,
    pub lake_producer: bool,
    pub lake_gradients: bool,
    pub lake_waves: bool,
    pub lake_normals: bool,
    pub lake_displacement: bool,
    pub lake_samplers: bool,
    pub tess_writer: bool,
    pub tess_owners: bool,
    pub resolve_shadow: bool,
    pub dof_inputs: bool,
    pub fog_generation: bool,
    pub sun_inputs: bool,
    pub resolve_inputs: bool,
    pub ssr_bracket: bool,
    pub ssr_operands: bool,
    pub ssr_prepare: bool,
    pub ssr_history: bool,
    pub dof_edge_local: Option<bool>,
    pub card_atlas: Option<bool>,
    pub group_fade: Option<bool>,
    pub group_atlas_scale: Option<bool>,
    pub aa_color_edge: Option<bool>,
    pub patch63_hull: Option<bool>,
    pub owner_four_domain: Option<bool>,
    pub lake_color_mirror: Option<bool>,
    pub lake_depth_edge: Option<bool>,
    pub lake_ds_mvp: Option<bool>,
    pub lake_inner_edge: Option<bool>,
    pub writer_domain_camera: Option<bool>,
    pub base_eye_vp: Option<bool>,
    pub fog_local_uv: Option<bool>,
    pub terrain_camera: Option<bool>,
    pub extra_bark_dither: Option<bool>,
    pub left_atlas_scale: Option<bool>,
    pub impostor_scale: Option<bool>,
    pub color_dither: Option<bool>,
    pub bark_dither: Option<bool>,
    pub terrain_sources: bool,
    pub residual_bark: bool,
    pub left_interval: bool,
    pub impostor_inputs: bool,
    pub interval_trace: bool,
    pub post_geometry: bool,
    pub bark_color_trace: bool,
    pub velocity_trace: bool,
    pub early_depth: bool,
    pub resolve_samples: bool,
    pub resolve_eye_camera: Option<bool>,
    pub proxy_eye: Option<bool>,
    pub march_eye: Option<bool>,
    pub veto_eye: Option<bool>,
    pub grid_eye: Option<bool>,
    pub history_center: Option<bool>,
    pub cascade_scale: Option<CascadeScale>,
    pub sun_depth_clip: Option<SunDepthClip>,
}

impl CaptureRequest {
    /// Why these options cannot run together, if they cannot: an option set without one it depends
    /// on, or one a motion capture does not take. `motion` says whether the request captures a
    /// motion phase.
    pub fn conflict(&self, motion: bool) -> Option<&'static str> {
        let o = self;
        let lake_camera = matches!(
            o.water_reflection,
            Some(WaterReflection::LakeCameraControl | WaterReflection::LakeHullCamera)
        );
        let hull_camera = o.water_reflection == Some(WaterReflection::LakeHullCamera);
        let rules = [
            (
                o.aa_color_edge.is_some() && (!o.aa_weights || o.patch63_hull != Some(true)),
                "the AA color edge requires AA weights and the physical hull",
            ),
            (o.aa_weights && !o.aa_draws, "AA weights require AA draws"),
            (o.aa_draws && !o.dof_inputs, "AA draws require DoF inputs"),
            (
                o.lake_owner_four && (!o.lake_producer || !o.tess_writer),
                "lake owner four requires the lake producer and the terrain writer",
            ),
            (
                o.lake_producer && !o.lake_displacement,
                "the lake producer requires the lake displacement",
            ),
            (
                o.lake_gradients && !o.lake_waves,
                "lake gradients require lake waves",
            ),
            (
                o.lake_waves && (!o.lake_displacement || o.lake_normals),
                "lake waves require the lake displacement without lake normals",
            ),
            (
                o.lake_normals && !o.lake_displacement,
                "lake normals require the lake displacement",
            ),
            (
                o.lake_displacement && (!o.lake_samplers || !o.tess_writer || !lake_camera),
                "the lake displacement requires lake samplers, the terrain writer, and a lake camera treatment",
            ),
            (
                o.patch63_hull.is_some()
                    && (o.owner_four_domain != Some(true) || o.writer_domain_camera != Some(true)),
                "the patch 63 hull requires both domain treatments",
            ),
            (
                o.owner_four_domain.is_some()
                    && (!o.lake_owner_four || o.lake_color_mirror != Some(true)),
                "the owner four domain requires lake owner four and the lake color mirror",
            ),
            (
                o.lake_color_mirror.is_some() && o.lake_depth_edge != Some(true),
                "the lake color mirror requires the lake depth edge",
            ),
            (
                o.lake_depth_edge.is_some()
                    && (o.lake_ds_mvp != Some(true) || o.lake_waves || o.lake_normals),
                "the lake depth edge requires the lake DS MVP without lake waves or normals",
            ),
            (
                o.lake_ds_mvp.is_some()
                    && (!o.lake_producer || o.card_atlas != Some(true) || !hull_camera),
                "the lake DS MVP requires the lake producer, the card atlas, and the hull camera",
            ),
            (
                o.card_atlas.is_some() && o.group_fade != Some(true),
                "the card atlas requires the group fade",
            ),
            (
                o.group_fade.is_some() && (!o.group_cards || o.lake_inner_edge != Some(true)),
                "the group fade requires group cards and the lake inner edge",
            ),
            (
                o.group_cards && !o.group_residual,
                "group cards require the group residual",
            ),
            (
                o.group_residual && !o.group_inputs,
                "the group residual requires group inputs",
            ),
            (
                o.lake_samplers && (!o.water_surface_inputs || o.water_tess_draw != Some(7)),
                "lake samplers require surface inputs at lake draw 7",
            ),
            (
                o.tess_writer
                    && (!o.terrain_sources || !o.tess_owners || o.terrain_camera.is_some()),
                "the terrain writer requires terrain sources and owners without the terrain camera",
            ),
            (
                o.tess_owners && !o.terrain_interval,
                "terrain owners require the terrain interval",
            ),
            (
                o.base_projection && !o.base_sources,
                "the basemesh projection requires basemesh sources",
            ),
            (
                o.base_sources && !o.terrain_interval,
                "basemesh sources require the terrain interval",
            ),
            (
                o.terrain_interval && !o.post_geometry,
                "the terrain interval requires the post-geometry boundaries",
            ),
            (
                o.group_inputs && !o.group_sources,
                "group inputs require group sources",
            ),
            (
                o.group_sources && !o.left_interval,
                "group sources require the left interval",
            ),
            (
                o.resolve_shadow && !o.resolve_samples,
                "the resolve shadow requires resolve samples",
            ),
            (
                o.dof_inputs && !o.fog_generation,
                "DoF inputs require the fog generation",
            ),
            (
                o.fog_generation && !o.fog_inputs,
                "the fog generation requires fog inputs",
            ),
            (
                o.fog_inputs && !o.late_sky_stages,
                "fog inputs require the late sky stages",
            ),
            (
                o.dof_edge_local.is_some() && !o.dof_inputs,
                "the local DoF edge requires DoF inputs",
            ),
            (
                o.lake_inner_edge.is_some()
                    && (!o.lake_samplers || o.writer_domain_camera != Some(true)),
                "the lake inner edge requires lake samplers and the writer domain camera",
            ),
            (
                o.writer_domain_camera.is_some() && (!o.tess_writer || o.base_eye_vp != Some(true)),
                "the writer domain camera requires the terrain writer and the basemesh eye VP",
            ),
            (
                o.base_eye_vp.is_some()
                    && (!o.base_projection || o.group_atlas_scale != Some(true)),
                "the basemesh eye VP requires the basemesh projection and the group atlas scale",
            ),
            (
                o.group_atlas_scale.is_some()
                    && (!o.group_inputs
                        || o.dof_edge_local != Some(true)
                        || o.left_atlas_scale != Some(true)),
                "the group atlas scale requires group inputs, the local DoF edge, and the left atlas scale",
            ),
            (
                o.fog_local_uv.is_some() && !o.fog_generation,
                "the local fog UV requires the fog generation",
            ),
            (
                o.extra_bark_dither.is_some()
                    && (!o.residual_bark || o.left_atlas_scale != Some(true)),
                "the extra bark dither requires the residual bark and the left atlas scale",
            ),
            (
                o.terrain_camera.is_some()
                    && (!o.terrain_sources || o.extra_bark_dither != Some(true)),
                "the terrain camera requires terrain sources and the extra bark dither",
            ),
            (
                o.terrain_sources && !o.early_depth,
                "terrain sources require early depth",
            ),
            (
                o.residual_bark && !o.left_interval,
                "the residual bark requires the left interval",
            ),
            (
                o.left_atlas_scale.is_some()
                    && (!o.left_interval || o.impostor_scale != Some(true)),
                "the left atlas scale requires the left interval and the impostor scale",
            ),
            (
                o.left_interval && !o.impostor_inputs,
                "the left interval requires impostor inputs",
            ),
            (
                o.impostor_scale.is_some() && (!o.impostor_inputs || o.color_dither != Some(true)),
                "the impostor scale requires impostor inputs and the color dither",
            ),
            (
                o.impostor_inputs && !o.interval_trace,
                "impostor inputs require the interval trace",
            ),
            (
                o.interval_trace && !o.post_geometry,
                "the interval trace requires the post-geometry boundaries",
            ),
            (
                o.post_geometry && !o.bark_color_trace,
                "the post-geometry boundaries require the bark color trace",
            ),
            (
                o.color_dither.is_some() && (!o.bark_color_trace || o.bark_dither != Some(true)),
                "the color dither requires the bark color trace and the bark dither",
            ),
            (
                (o.bark_dither.is_some() || o.bark_color_trace) && !o.velocity_trace,
                "the bark dither and the bark color trace require the velocity trace",
            ),
            (
                o.velocity_trace && !o.early_depth,
                "the velocity trace requires early depth",
            ),
            (
                o.early_depth && (o.patch_draw_ordinal.is_none() || !o.ssr_history),
                "early depth requires a patch draw ordinal and the SSR history",
            ),
            (
                o.ssr_history && !o.ssr_prepare,
                "the SSR history requires the SSR preparation",
            ),
            (
                o.ssr_prepare && !o.ssr_operands,
                "the SSR preparation requires SSR operands",
            ),
            (
                o.ssr_operands && !o.ssr_bracket,
                "SSR operands require the SSR bracket",
            ),
            (
                o.history_center.is_some() && (!o.ssr_history || o.veto_eye != Some(true)),
                "the history center requires the SSR history and the veto eye",
            ),
            (
                o.grid_eye.is_some() && o.veto_eye != Some(true),
                "the grid eye requires the veto eye",
            ),
            (
                o.veto_eye.is_some() && o.march_eye != Some(true),
                "the veto eye requires the march eye",
            ),
            (
                o.march_eye.is_some() && o.proxy_eye != Some(true),
                "the march eye requires the proxy eye",
            ),
            (
                o.proxy_eye.is_some() && (o.resolve_eye_camera != Some(true) || !o.ssr_prepare),
                "the proxy eye requires the resolve eye camera and the SSR preparation",
            ),
            (
                o.resolve_eye_camera.is_some() && !hull_camera,
                "the resolve eye camera requires the hull camera",
            ),
            (
                (o.resolve_eye_camera.is_some() || o.ssr_bracket) && !o.resolve_samples,
                "the resolve eye camera and the SSR bracket require resolve samples",
            ),
            (
                o.resolve_samples && (!o.resolve_inputs || o.cascade_scale.is_some() || motion),
                "resolve samples require resolve inputs without a cascade scale or a motion capture",
            ),
            (
                o.cascade_scale.is_some() && (!o.resolve_inputs || motion),
                "a cascade scale requires resolve inputs without a motion capture",
            ),
            (
                o.sun_depth_clip.is_some() && !o.sun_inputs,
                "the sun depth clip requires sun inputs",
            ),
            (
                o.water_reflection.is_some() && !o.water_inputs,
                "a water reflection treatment requires water inputs",
            ),
            (
                o.water_reflection.is_some() && motion,
                "a water reflection treatment does not take a motion capture",
            ),
            (
                o.water_reflection
                    .is_some_and(WaterReflection::reads_surface_inputs)
                    && !o.water_surface_inputs,
                "this water reflection treatment requires surface inputs",
            ),
            (
                o.water_tess_draw
                    .is_some_and(|n| !o.water_inputs || n >= 32),
                "a water draw selection requires water inputs and an index below 32",
            ),
            (
                o.water_surface_inputs
                    && (o.water_tess_draw.is_none()
                        || o.water_reflection
                            .is_some_and(|r| !r.reads_surface_inputs())
                        || motion),
                "surface inputs require a selected water draw, a treatment that reads them, and no motion capture",
            ),
            (
                o.second_patch_ordinal.is_some_and(|second| {
                    !o.patch_draw_ordinal.is_some_and(|first| first < second)
                }),
                "the second patch ordinal must follow the patch draw ordinal",
            ),
            (
                o.patch_draw_ordinal.is_some_and(|n| {
                    n == 0
                        || !matches!(
                            o.water_reflection,
                            Some(
                                WaterReflection::LakeCameraControl
                                    | WaterReflection::LakeDomainCamera
                                    | WaterReflection::LakeHullCamera
                            )
                        )
                }),
                "a patch draw ordinal must be positive and requires a lake camera treatment",
            ),
            (
                o.late_sky_stages && motion,
                "the late sky stages do not take a motion capture",
            ),
            (
                o.draw_census && motion,
                "a draw census does not take a motion capture",
            ),
        ];
        rules
            .into_iter()
            .find_map(|(broken, why)| broken.then_some(why))
    }

    /// The first option that binds a capture-only shader rewrite, which a payload built without the
    /// `capture-rewrites` feature does not carry.
    pub(crate) fn rewriting_option(&self) -> Option<&'static str> {
        let o = self;
        let options = [
            (o.resolve_samples, "resolve_samples"),
            (o.ssr_operands, "ssr_operands"),
            (o.fog_local_uv == Some(true), "fog_local_uv"),
            (o.aa_color_edge == Some(true), "aa_color_edge"),
            (o.dof_edge_local == Some(true), "dof_edge_local"),
            (o.bark_dither == Some(true), "bark_dither"),
            (o.color_dither == Some(true), "color_dither"),
            (o.impostor_scale == Some(true), "impostor_scale"),
            (o.group_atlas_scale == Some(true), "group_atlas_scale"),
            (o.card_atlas == Some(true), "card_atlas"),
            (o.group_fade == Some(true), "group_fade"),
            (o.lake_depth_edge == Some(true), "lake_depth_edge"),
            (
                o.water_reflection
                    .is_some_and(WaterReflection::rewrites_lake),
                "water_reflection",
            ),
        ];
        options
            .into_iter()
            .find_map(|(set, name)| set.then_some(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conflict(options: CaptureRequest) -> Option<&'static str> {
        options.conflict(false)
    }

    #[test]
    fn no_options_conflict_with_nothing() {
        assert_eq!(CaptureRequest::default().conflict(false), None);
        assert_eq!(CaptureRequest::default().conflict(true), None);
    }

    #[test]
    fn a_water_reflection_treatment_needs_water_inputs_and_no_motion() {
        let treated = CaptureRequest {
            water_reflection: Some(WaterReflection::ZeroWeight),
            ..Default::default()
        };
        assert!(conflict(treated.clone()).is_some());
        let with_inputs = CaptureRequest {
            water_inputs: true,
            ..treated
        };
        assert_eq!(with_inputs.conflict(false), None);
        assert!(with_inputs.conflict(true).is_some());
    }

    #[test]
    fn untimed_diagnostics_refuse_a_motion_capture() {
        for options in [
            CaptureRequest {
                late_sky_stages: true,
                ..Default::default()
            },
            CaptureRequest {
                draw_census: true,
                ..Default::default()
            },
        ] {
            assert_eq!(options.conflict(false), None);
            assert!(options.conflict(true).is_some());
        }
    }

    #[test]
    fn dependent_options_name_what_they_need() {
        assert!(
            conflict(CaptureRequest {
                cascade_scale: Some(CascadeScale::ShoreLow),
                ..Default::default()
            })
            .is_some()
        );
        assert_eq!(
            conflict(CaptureRequest {
                cascade_scale: Some(CascadeScale::ShoreHigh),
                resolve_inputs: true,
                ..Default::default()
            }),
            None
        );
        assert!(
            conflict(CaptureRequest {
                sun_depth_clip: Some(SunDepthClip::DisabledRight),
                ..Default::default()
            })
            .is_some()
        );
        assert_eq!(
            conflict(CaptureRequest {
                sun_depth_clip: Some(SunDepthClip::DisabledRight),
                sun_inputs: true,
                ..Default::default()
            }),
            None
        );
        assert!(
            conflict(CaptureRequest {
                water_tess_draw: Some(32),
                water_inputs: true,
                ..Default::default()
            })
            .is_some()
        );
    }

    #[test]
    fn only_treatment_arms_need_the_capture_rewrites() {
        assert_eq!(CaptureRequest::default().rewriting_option(), None);
        let control = CaptureRequest {
            aa_color_edge: Some(false),
            ..Default::default()
        };
        assert_eq!(control.rewriting_option(), None);
        let treatment = CaptureRequest {
            aa_color_edge: Some(true),
            ..Default::default()
        };
        assert_eq!(treatment.rewriting_option(), Some("aa_color_edge"));
        let shipped_lake = CaptureRequest {
            water_reflection: Some(WaterReflection::PackedRefraction),
            ..Default::default()
        };
        assert_eq!(shipped_lake.rewriting_option(), None);
    }
}
