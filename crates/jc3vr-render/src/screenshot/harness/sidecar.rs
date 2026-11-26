//! The harness's sidecar for a captured frame's image: the capture metadata written beside the PNG,
//! and the stage artifacts written before it, each named `<id>.<stage>.eye<N>.<extension>` beside
//! the image and recorded in the metadata it writes after them.

use std::{any::Any, io::Write, path::Path};

use anyhow::Context as _;

use crate::{
    image_writer::{ImageSidecar, write_png},
    screenshot::harness::{
        resolve_probe,
        stages::{Stage, StageArtifact, StageImage, StageRecord},
        types::CaptureMetadata,
    },
};

/// A captured frame's sidecar: its metadata and the stage images to write before it.
pub(crate) struct CaptureSidecar {
    metadata: Box<CaptureMetadata>,
    stages: Vec<StageImage>,
    written: WrittenStages,
}

/// What the writer made of a capture's stages: their records with the paths written or the errors
/// met, and the resolve inputs whose artifacts were written.
#[derive(Default)]
pub(crate) struct WrittenStages {
    pub(crate) stages: Vec<StageRecord>,
    pub(crate) resolve_inputs: Option<resolve_probe::Capture>,
}

impl CaptureSidecar {
    pub(crate) fn new(metadata: Box<CaptureMetadata>, stages: Vec<StageImage>) -> Self {
        Self {
            metadata,
            stages,
            written: WrittenStages::default(),
        }
    }

    /// What the writer made of the stages of the sidecar an outcome carries back; empty when the
    /// outcome carries none.
    pub(crate) fn take_written(sidecar: Option<Box<dyn ImageSidecar>>) -> WrittenStages {
        sidecar
            .and_then(|sidecar| sidecar.into_any().downcast::<Self>().ok())
            .map(|sidecar| sidecar.written)
            .unwrap_or_default()
    }
}

impl ImageSidecar for CaptureSidecar {
    fn write_artifacts(&mut self, path: &Path) {
        let id = self.metadata.id.clone();
        if let Some(capture) = &mut self.metadata.capture {
            for stage in std::mem::take(&mut self.stages) {
                let record = &mut capture.stages[stage.record];
                let name = stage_name(record.stage);
                let raw = matches!(&stage.artifact, StageArtifact::Raw(_));
                let extension = match (&stage.artifact, record.stage) {
                    (StageArtifact::Png(_), _) => "png",
                    (StageArtifact::Raw(_), stage) if stage.is_hdr() => "r11g11b10",
                    (StageArtifact::Raw(_), _) => "raw",
                };
                let stage_path =
                    path.with_file_name(format!("{}.{name}.eye{}.{extension}", id, record.eye));
                let result = match stage.artifact {
                    StageArtifact::Png(image) => write_png(&stage_path, image),
                    StageArtifact::Raw(bytes) => std::fs::write(&stage_path, bytes)
                        .with_context(|| format!("writing {}", stage_path.display())),
                };
                match result {
                    Ok(()) if raw => record.raw = Some(stage_path),
                    Ok(()) => record.png = Some(stage_path),
                    Err(error) => record.error = Some(format!("image writer: {error:#}")),
                }
            }
            if let Some(resolve) = capture.resolve_inputs.as_mut() {
                if let Some(folder) = path.parent() {
                    resolve.write_artifacts(folder, &id);
                }
                self.written.resolve_inputs = Some(resolve.clone());
            }
            self.written.stages = capture.stages.clone();
        }
    }

    fn write_json(&self, writer: &mut dyn Write) -> serde_json::Result<()> {
        serde_json::to_writer_pretty(writer, &self.metadata)
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any + Send> {
        self
    }
}

fn stage_name(stage: Stage) -> &'static str {
    match stage {
        Stage::GeometryDepth => "geometry_depth",
        Stage::SecondCameraSubmission => "second_camera_submission",
        Stage::DomainCameraSubmission => "domain_camera_submission",
        Stage::PatchDepthOwners => "patch_depth_owners",
        Stage::BeforePatchDrawDepth => "before_patch_draw_depth",
        Stage::BeforePatchDrawAlbedo => "before_patch_draw_albedo",
        Stage::AfterPatchDrawDepth => "after_patch_draw_depth",
        Stage::AfterPatchDrawAlbedo => "after_patch_draw_albedo",
        Stage::BeforeTessPatchDepth => "before_tess_patch_depth",
        Stage::AfterTessPatchDepth => "after_tess_patch_depth",
        Stage::BeforeTessPatchAlbedo => "before_tess_patch_albedo",
        Stage::AfterTessPatchAlbedo => "after_tess_patch_albedo",
        Stage::BeforeZPassDepth => "before_z_pass_depth",
        Stage::AfterZPassDepth => "after_z_pass_depth",
        Stage::AfterBaseNearDepth => "after_base_near_depth",
        Stage::AfterTessFarDepth => "after_tess_far_depth",
        Stage::AfterBaseFarDepth => "after_base_far_depth",
        Stage::AfterTessColorDepth => "after_tess_color_depth",
        Stage::AfterBaseColorDepth => "after_base_color_depth",
        Stage::AfterApplyNearDetailedDepth => "after_apply_near_detailed_depth",
        Stage::AfterApplyNearDepth => "after_apply_near_depth",
        Stage::BeforeDynamicModelsDepth => "before_dynamic_models_depth",
        Stage::AfterDynamicModelsDepth => "after_dynamic_models_depth",
        Stage::AfterStaticModelsDepth => "after_static_models_depth",
        Stage::AfterOpaqueVegetationDepth => "after_opaque_vegetation_depth",
        Stage::AfterFinsDepth => "after_fins_depth",
        Stage::AfterGroupDepth => "after_group_depth",
        Stage::AfterGroup2Depth => "after_group2_depth",
        Stage::BaseSources => "base_sources",
        Stage::TerrainSources => "terrain_sources",
        Stage::IntervalTrace => "interval_trace",
        Stage::AfterForestDepth => "after_forest_depth",
        Stage::BarkColorTrace => "bark_color_trace",
        Stage::VelocityTrace => "velocity_trace",
        Stage::AfterZVelocityDepth => "after_z_velocity_depth",
        Stage::BeforeNearPatchDepth => "before_near_patch_depth",
        Stage::AfterNearPatchDepth => "after_near_patch_depth",
        Stage::BeforeNearPatchAlbedo => "before_near_patch_albedo",
        Stage::AfterNearPatchAlbedo => "after_near_patch_albedo",
        Stage::GBuffer0 => "gbuffer0",
        Stage::GBuffer1 => "gbuffer1",
        Stage::GBuffer2 => "gbuffer2",
        Stage::GBuffer3 => "gbuffer3",
        Stage::Velocity => "velocity",
        Stage::MainColor => "maincolor",
        Stage::AfterApplyClouds => "after_apply_clouds",
        Stage::FogGenDepth => "fog_gen_depth",
        Stage::FogLocalUv => "fog_local_uv",
        Stage::FogGenerated => "fog_generated",
        Stage::FogInput => "fog_input",
        Stage::BeforeFogVolumeApply => "before_fog_volume_apply",
        Stage::AfterFogVolumeApply => "after_fog_volume_apply",
        Stage::PreAtmosphere => "pre_atmosphere",
        Stage::PostAtmosphere => "post_atmosphere",
        Stage::PostResolve => "post_resolve",
        Stage::SunDepthClipTreatment => "sun_depth_clip_treatment",
        Stage::PreSunColor => "pre_sun_color",
        Stage::PreSunDepth => "pre_sun_depth",
        Stage::PostSun => "post_sun",
        Stage::WaterBefore => "water_before",
        Stage::WaterAfterLeft => "water_after_left",
        Stage::WaterAfterRight => "water_after_right",
        Stage::WaterDepthBefore => "water_depth_before",
        Stage::WaterDepthAfterLeft => "water_depth_after_left",
        Stage::WaterDepthAfterRight => "water_depth_after_right",
        Stage::WaterReflectionTreatment => "water_reflection_treatment",
        Stage::WaterInputReflection => "water_input_t1",
        Stage::WaterInputStaticReflection => "water_input_t2",
        Stage::WaterInputReflectionAlpha => "water_input_t12",
        Stage::WaterInputRefraction => "water_input_t3",
        Stage::WaterInputRefractionDepth => "water_input_t0",
        Stage::LakeResponseColor => "lake_response_color",
        Stage::LakeResponseUv => "lake_response_uv",
        Stage::LakeResponseTint => "lake_response_tint",
        Stage::LakeResponseOutput => "lake_response_output",
        Stage::ResolveHalfSamples => "resolve_half_samples",
        Stage::ProxyNativeObserved => "proxy_native_observed",
        Stage::SsrGridNative => "ssr_grid_native",
        Stage::SsrGridPhysical => "ssr_grid_physical",
        Stage::SsrVetoPacked => "ssr_veto_packed",
        Stage::SsrVetoNative => "ssr_veto_native",
        Stage::SsrMarchNative => "ssr_march_native",
        Stage::SsrMarchPacked => "ssr_march_packed",
        Stage::ProxyEyeApplied => "proxy_eye_applied",
        Stage::SsrPrepDepth => "ssr_prep_depth",
        Stage::SsrPrepNormal => "ssr_prep_normal",
        Stage::SsrPrepPlane => "ssr_prep_plane",
        Stage::SsrPrepScene => "ssr_prep_scene",
        Stage::SsrHistoryUv => "ssr_history_uv",
        Stage::SsrHistorySample => "ssr_history_sample",
        Stage::SsrHistoryNative => "ssr_history_native",
        Stage::SsrHistoryPhysical => "ssr_history_physical",
        Stage::SsrHistorySource => "ssr_history_source",
        Stage::SsrRayInput => "ssr_ray_input",
        Stage::SsrRayNormal => "ssr_ray_normal",
        Stage::SsrRaySegment => "ssr_ray_segment",
        Stage::SsrRayWorld => "ssr_ray_world",
        Stage::SsrRayMarch => "ssr_ray_march",
        Stage::SsrRayMask => "ssr_ray_mask",
        Stage::SsrRayColor => "ssr_ray_color",
        Stage::SsrRayAlpha => "ssr_ray_alpha",
        Stage::SsrBlurColor => "ssr_blur_color",
        Stage::SsrBlurAlpha => "ssr_blur_alpha",
        Stage::ResolveShadowTerms => "resolve_shadow_terms",
        Stage::ResolveShadowTap => "resolve_shadow_tap",
        Stage::ResolveFullSamples => "resolve_full_samples",
        Stage::ResolveWorld => "resolve_world",
        Stage::ResolveView => "resolve_view",
        Stage::LakeGradientSample => "lake_gradient_sample",
        Stage::LakeGradientUv => "lake_gradient_uv",
        Stage::LakeGradientCoarseX => "lake_gradient_coarse_x",
        Stage::LakeGradientCoarseY => "lake_gradient_coarse_y",
        Stage::LakeGradientFineX => "lake_gradient_fine_x",
        Stage::LakeGradientFineY => "lake_gradient_fine_y",
        Stage::LakeWaveSamples78 => "lake_wave_samples_7_8",
        Stage::LakeWaveSamples910 => "lake_wave_samples_9_10",
        Stage::LakeWaveUv78 => "lake_wave_uv_7_8",
        Stage::LakeWaveUv910 => "lake_wave_uv_9_10",
        Stage::LakeWaveWeights => "lake_wave_weights",
        Stage::LakeWaveCombined => "lake_wave_combined",
        Stage::LakeNormalMesh => "lake_normal_mesh",
        Stage::LakeNormalMixed => "lake_normal_mixed",
        Stage::LakeDisplacementDepth => "lake_displacement_depth",
        Stage::LakeDisplacementSurface => "lake_displacement_surface",
        Stage::LakeResponseAir => "lake_response_air",
        Stage::LakeLightingBase => "lake_lighting_base",
        Stage::LakeLightingMixed => "lake_lighting_mixed",
        Stage::LakeLightingLit => "lake_lighting_lit",
        Stage::LakeLightingOutput => "lake_lighting_output",
        Stage::LakeSampleUv => "lake_sample_uv",
        Stage::LakeSampleDepth => "lake_sample_depth",
        Stage::LakeSampleColor => "lake_sample_color",
        Stage::LakeSamplePlanar => "lake_sample_planar",
        Stage::WaterInputWave0 => "water_input_t7",
        Stage::WaterInputWave1 => "water_input_t8",
        Stage::WaterInputWave2 => "water_input_t9",
        Stage::WaterInputWave3 => "water_input_t10",
        Stage::WaterInputAmbientOcclusion => "water_input_t11",

        Stage::DofEdgeLocal => "dof_edge_local",
        Stage::AaWeights => "aa_weights",
        Stage::DofScene => "dof_scene",
        Stage::DofComposite => "dof_composite",
        Stage::DofReturned => "dof_returned",
        Stage::PreAa => "pre_aa",
        Stage::PostAa => "post_aa",
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        image_writer::{ChannelOrder, Image, ImageTarget, try_submit},
        screenshot::harness::{
            stages::Source,
            types::{CaptureKind, CaptureState, CapturedFrame, ComparisonInputs, Readiness},
        },
        testing::visual,
    };

    #[test]
    fn stage_artifacts_finish_before_the_sidecar_and_report_failures() {
        crate::test_config::register();
        for fail in [false, true] {
            let directory = TestDirectory::new();
            let path = directory.0.join("writer-test.capture.png");
            let raw_path = directory.0.join("writer-test.maincolor.eye0.r11g11b10");
            let pre_path = directory.0.join("writer-test.pre_aa.eye0.png");
            let post_path = directory.0.join("writer-test.post_aa.eye1.png");
            if fail {
                std::fs::create_dir(&pre_path).unwrap();
            }
            let mut metadata = test_metadata();
            metadata.capture = Some(test_capture(&path));
            let raw = vec![0, 255, 128, 1];
            let stages = vec![
                StageImage {
                    record: 0,
                    artifact: StageArtifact::Raw(raw.clone()),
                },
                StageImage {
                    record: 1,
                    artifact: StageArtifact::Png(test_image()),
                },
                StageImage {
                    record: 2,
                    artifact: StageArtifact::Png(
                        Image::from_rows(&[10, 20, 30, 7], 4, 1, 1, ChannelOrder::Rgba).unwrap(),
                    ),
                },
            ];
            let outcome = write_capture(&path, metadata, stages);
            assert_eq!(outcome.result, Ok(path.clone()));
            assert_eq!(std::fs::read(&raw_path).unwrap(), raw);
            assert_eq!(outcome.stages[0].raw, Some(raw_path));
            assert_eq!(outcome.stages[2].png, Some(post_path.clone()));
            assert_eq!(
                image::open(post_path).unwrap().to_rgba8().as_raw(),
                &[10, 20, 30, 255]
            );
            if fail {
                assert!(outcome.stages[1].png.is_none());
                assert!(outcome.stages[1].error.as_ref().unwrap().contains("pre_aa"));
            } else {
                assert_eq!(outcome.stages[1].png, Some(pre_path.clone()));
                assert_eq!(
                    image::open(pre_path).unwrap().to_rgba8().as_raw(),
                    &[30, 20, 10, 255]
                );
            }
            let sidecar: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path.with_extension("json")).unwrap())
                    .unwrap();
            assert_eq!(
                sidecar["stages"],
                serde_json::to_value(outcome.stages).unwrap()
            );
        }
    }

    #[test]
    fn geometry_artifacts_keep_stable_names_alpha_and_acknowledged_metadata() {
        crate::test_config::register();
        let directory = TestDirectory::new();
        let path = directory.0.join("writer-test.capture.png");
        let mut metadata = test_metadata();
        let mut capture = test_capture(&path);
        let template = capture.stages[0].clone();
        capture.stages = [
            (Stage::GeometryDepth, "geometry_depth", 8),
            (Stage::WaterDepthBefore, "water_depth_before", 8),
            (Stage::WaterDepthAfterLeft, "water_depth_after_left", 8),
            (Stage::WaterDepthAfterRight, "water_depth_after_right", 8),
            (Stage::GBuffer0, "gbuffer0", 4),
            (Stage::GBuffer1, "gbuffer1", 4),
            (Stage::GBuffer2, "gbuffer2", 4),
            (Stage::GBuffer3, "gbuffer3", 4),
            (Stage::Velocity, "velocity", 4),
        ]
        .into_iter()
        .map(|(stage, name, pixel_bytes)| {
            assert_eq!(serde_json::to_value(stage).unwrap(), name);
            let mut record = template.clone();
            record.stage = stage;
            record.source.as_mut().unwrap().bytes_per_pixel = Some(pixel_bytes);
            record
        })
        .collect();
        let stages = capture
            .stages
            .iter()
            .enumerate()
            .map(|(record, stage)| StageImage {
                record,
                artifact: StageArtifact::Raw(
                    (0..stage.source.as_ref().unwrap().bytes_per_pixel.unwrap() as u8).collect(),
                ),
            })
            .collect();
        metadata.capture = Some(capture);
        let outcome = write_capture(&path, metadata, stages);
        assert_eq!(outcome.result, Ok(path.clone()));
        for record in &outcome.stages {
            let name = stage_name(record.stage);
            let raw_path = directory.0.join(format!("writer-test.{name}.eye0.raw"));
            assert_eq!(record.raw.as_ref(), Some(&raw_path));
            assert!(record.png.is_none());
            assert!(record.error.is_none());
            assert!(record.means.is_none());
            let bytes = std::fs::read(raw_path).unwrap();
            assert_eq!(bytes[3], 3);
            assert_eq!(
                bytes.len(),
                record.source.as_ref().unwrap().bytes_per_pixel.unwrap()
            );
        }
        let sidecar: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path.with_extension("json")).unwrap()).unwrap();
        assert_eq!(
            sidecar["stages"],
            serde_json::to_value(outcome.stages).unwrap()
        );
    }

    #[test]
    fn raw_stage_failure_is_acknowledged_in_the_sidecar() {
        crate::test_config::register();
        let directory = TestDirectory::new();
        let path = directory.0.join("writer-test.capture.png");
        let raw_path = directory.0.join("writer-test.geometry_depth.eye0.raw");
        std::fs::create_dir(&raw_path).unwrap();
        let mut metadata = test_metadata();
        let mut capture = test_capture(&path);
        capture.stages.truncate(1);
        capture.stages[0].stage = Stage::GeometryDepth;
        metadata.capture = Some(capture);
        let outcome = write_capture(
            &path,
            metadata,
            vec![StageImage {
                record: 0,
                artifact: StageArtifact::Raw(vec![0; 8]),
            }],
        );
        assert_eq!(outcome.result, Ok(path.clone()));
        let record = &outcome.stages[0];
        assert!(record.raw.is_none());
        assert!(
            record
                .error
                .as_ref()
                .unwrap()
                .contains("geometry_depth.eye0.raw")
        );
        let sidecar: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path.with_extension("json")).unwrap()).unwrap();
        assert_eq!(
            sidecar["stages"],
            serde_json::to_value(outcome.stages).unwrap()
        );
    }

    #[test]
    fn wave_stage_metadata_matches_the_raw_artifact_stage_name() {
        for stage in [
            Stage::LakeGradientSample,
            Stage::LakeGradientUv,
            Stage::LakeGradientCoarseX,
            Stage::LakeGradientCoarseY,
            Stage::LakeGradientFineX,
            Stage::LakeGradientFineY,
            Stage::LakeWaveSamples78,
            Stage::LakeWaveSamples910,
            Stage::LakeWaveUv78,
            Stage::LakeWaveUv910,
            Stage::LakeWaveWeights,
            Stage::LakeWaveCombined,
            Stage::LakeResponseOutput,
        ] {
            assert_eq!(serde_json::to_value(stage).unwrap(), stage_name(stage));
        }
    }

    #[test]
    fn hdr_stage_names_and_extensions_cover_every_hdr_stage() {
        crate::test_config::register();
        let directory = TestDirectory::new();
        let path = directory.0.join("writer-test.capture.png");
        let mut metadata = test_metadata();
        let mut capture = test_capture(&path);
        let template = capture.stages[0].clone();
        let stages = [
            (Stage::MainColor, "maincolor"),
            (Stage::PreAtmosphere, "pre_atmosphere"),
            (Stage::PostAtmosphere, "post_atmosphere"),
            (Stage::PostResolve, "post_resolve"),
            (Stage::PostSun, "post_sun"),
            (Stage::WaterBefore, "water_before"),
            (Stage::WaterAfterLeft, "water_after_left"),
            (Stage::WaterAfterRight, "water_after_right"),
        ];
        capture.stages = stages
            .into_iter()
            .map(|(stage, name)| {
                assert_eq!(stage_name(stage), name);
                assert!(stage.is_hdr());
                let mut record = template.clone();
                record.stage = stage;
                record
            })
            .collect();
        metadata.capture = Some(capture);
        let images = stages
            .into_iter()
            .enumerate()
            .map(|(record, _)| StageImage {
                record,
                artifact: StageArtifact::Raw(vec![0; 4]),
            })
            .collect();
        let outcome = write_capture(&path, metadata, images);
        assert_eq!(outcome.result, Ok(path));
        for (record, (_, name)) in outcome.stages.iter().zip(stages) {
            let raw_path = directory
                .0
                .join(format!("writer-test.{name}.eye0.r11g11b10"));
            assert_eq!(record.raw.as_ref(), Some(&raw_path));
            assert!(raw_path.is_file());
            assert!(record.png.is_none());
            assert!(record.error.is_none());
        }
    }

    /// What a test capture's write came to.
    struct TestOutcome {
        result: Result<PathBuf, String>,
        stages: Vec<StageRecord>,
    }

    /// Writes a test image at `path` with a capture sidecar, through the image writer, and waits for
    /// it.
    fn write_capture(
        path: &Path,
        metadata: CaptureMetadata,
        stages: Vec<StageImage>,
    ) -> TestOutcome {
        let outcome = try_submit(
            ImageTarget::Attached {
                path: path.to_owned(),
                sidecar: Box::new(CaptureSidecar::new(Box::new(metadata), stages)),
            },
            test_image(),
        )
        .unwrap()
        .recv()
        .unwrap();
        TestOutcome {
            result: outcome.result,
            stages: CaptureSidecar::take_written(outcome.sidecar).stages,
        }
    }

    fn test_capture(path: &std::path::Path) -> CapturedFrame {
        CapturedFrame {
            png: path.to_owned(),
            metadata: path.with_extension("json"),
            frame_ordinal: 1,
            predicted_display_time_ns: 42,
            source: CaptureKind::EyeTextures,
            width: 2,
            height: 1,
            eye_width: 1,
            eye_height: 1,
            format: "test".to_owned(),
            fresh_eyes: [true; 2],
            rendered_eyes: [true; 2],
            selected_eye: None,
            mode: crate::stereo::dispatch::RenderMode::Multipass,
            visual: visual::metadata(),
            comparison_inputs: ComparisonInputs {
                build_stamp: "test-build",
                profile: visual::DiagnosticProfile::Native,
                camera: None,
                eyes: [None, None],
                eye_width: 1,
                eye_height: 1,
                world_scale: 1.0,
            },
            effective_config: jc3vr_core::config::engine::Snapshot::defaults(),
            water_reflection: None,
            aa_weights: None,
            aa_draws: None,
            lake_owner_four: None,
            lake_producer: None,
            lake_gradients: None,
            lake_waves: None,
            lake_normals: None,
            lake_displacement: None,
            aa_color_edge: None,
            patch63_hull: None,
            owner_four_domain: None,
            lake_color_mirror: None,
            lake_depth_edge: None,
            lake_ds_mvp: None,
            lake_inner_edge: None,
            patch_draw_ordinal: None,
            second_patch_ordinal: None,
            cascade_scale: None,
            sun_depth_clip: None,
            resolve_inputs: None,
            draw_census: None,
            single_pass: None,
            constants: Default::default(),
            stages: [(Stage::MainColor, 0), (Stage::PreAa, 0), (Stage::PostAa, 1)]
                .into_iter()
                .map(|(stage, eye)| StageRecord {
                    frame: 42,
                    stage,
                    eye,
                    source: Some(Source {
                        width: 1,
                        height: 1,
                        format_id: 0,
                        bytes_per_pixel: Some(4),
                        format: if stage == Stage::MainColor {
                            "DXGI_FORMAT_R11G11B10_FLOAT"
                        } else {
                            "DXGI_FORMAT_R8G8B8A8_TYPELESS"
                        }
                        .to_owned(),
                    }),
                    viewports: Vec::new(),
                    scissors: Vec::new(),
                    scissor_enabled: None,
                    means: None,
                    png: None,
                    raw: None,
                    error: None,
                    water_input: None,
                    velocity: None,
                    aa: None,
                    sun_input: None,
                })
                .collect(),
        }
    }

    fn test_image() -> Image {
        Image::from_rows(&[10, 20, 30, 0], 4, 1, 1, ChannelOrder::Bgra).unwrap()
    }

    fn test_metadata() -> CaptureMetadata {
        CaptureMetadata {
            id: "writer-test".to_owned(),
            state: CaptureState::Captured,
            readiness: Readiness {
                gameplay_ready: true,
                xr_session_running: true,
                xr_frame_rendering: true,
                build_stamp: "test-build",
            },
            capture: None,
            error: None,
        }
    }

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            loop {
                let path = std::env::temp_dir().join(format!(
                    "jc3vr-capture-sidecar-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed),
                ));
                match std::fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("creating {}: {error}", path.display()),
                }
            }
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
