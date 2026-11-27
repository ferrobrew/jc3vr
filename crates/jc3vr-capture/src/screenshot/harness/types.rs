use std::path::PathBuf;

use jc3vr_core::config::engine::Snapshot;
use jc3vr_render::probes::state::FrameDiagnostics;
use jc3vr_view::stereo::dispatch::RenderMode;
use serde::Serialize;

use crate::testing::visual::{DiagnosticProfile, VisualMetadata};

#[derive(Clone, Serialize)]
pub struct CaptureMetadata {
    pub(crate) id: String,
    pub(crate) state: CaptureState,
    #[serde(flatten)]
    pub(crate) readiness: Readiness,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub(crate) capture: Option<CapturedFrame>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CaptureState {
    Captured,
    Completed,
    Failed,
}

#[derive(Clone, Serialize)]
pub(crate) struct Readiness {
    pub(crate) gameplay_ready: bool,
    pub(crate) xr_session_running: bool,
    pub(crate) xr_frame_rendering: bool,
    pub(crate) build_stamp: &'static str,
}

#[derive(Clone, Serialize)]
pub(crate) struct CapturedFrame {
    pub(crate) png: PathBuf,
    pub(crate) metadata: PathBuf,
    pub(crate) frame_ordinal: u64,
    pub(crate) predicted_display_time_ns: i64,
    pub(crate) source: CaptureKind,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) eye_width: u32,
    pub(crate) eye_height: u32,
    pub(crate) format: String,
    pub(crate) fresh_eyes: [bool; 2],
    pub(crate) rendered_eyes: [bool; 2],
    pub(crate) selected_eye: Option<usize>,
    pub(crate) mode: RenderMode,
    pub(crate) visual: VisualMetadata,
    pub(crate) comparison_inputs: ComparisonInputs,
    pub(crate) effective_config: Snapshot,
    pub(crate) stages: Vec<crate::screenshot::harness::stages::StageRecord>,
    pub(crate) constants: crate::screenshot::harness::constants::CapturedConstants,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cascade_scale: Option<crate::screenshot::harness::CascadeScale>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) water_reflection: Option<crate::screenshot::harness::WaterReflection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) aa_weights: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) aa_draws: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) lake_owner_four: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) lake_producer: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) lake_gradients: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) lake_waves: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) lake_normals: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) lake_displacement: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) aa_color_edge: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) patch63_hull: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) owner_four_domain: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) lake_color_mirror: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) lake_depth_edge: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) lake_ds_mvp: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) lake_inner_edge: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) second_patch_ordinal: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) patch_draw_ordinal: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sun_depth_clip: Option<crate::screenshot::harness::SunDepthClip>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) resolve_inputs: Option<crate::screenshot::harness::resolve_probe::Capture>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) draw_census: Option<crate::screenshot::harness::draw_census::DrawCensus>,
    pub(crate) single_pass: Option<FrameDiagnostics>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CaptureKind {
    IsolatedEye,
    CollapsedBackBuffer,
    EyeTextures,
}

#[derive(Clone, Serialize)]
pub(crate) struct ComparisonInputs {
    pub(crate) build_stamp: &'static str,
    pub(crate) profile: DiagnosticProfile,
    pub(crate) camera: Option<CameraInput>,
    pub(crate) eyes: [Option<EyeInput>; 2],
    pub(crate) eye_width: u32,
    pub(crate) eye_height: u32,
    pub(crate) world_scale: f32,
}

#[derive(Clone, Serialize)]
pub(crate) struct CameraInput {
    pub(crate) position: [f32; 3],
    pub(crate) orientation: [f32; 4],
}

#[derive(Clone, Serialize)]
pub(crate) struct EyeInput {
    pub(crate) offset: [f32; 3],
    pub(crate) orientation_delta: [f32; 4],
    pub(crate) projection_standard: [f32; 16],
    pub(crate) projection_reverse_z: [f32; 16],
}
