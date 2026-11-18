use std::path::PathBuf;

use serde::Serialize;

use crate::{
    config::Config,
    stereo::{dispatch::RenderMode, single_pass::FrameDiagnostics},
    testing::visual::{DiagnosticProfile, VisualMetadata},
};

#[derive(Clone, Serialize)]
pub(crate) struct CaptureMetadata {
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
    pub(crate) effective_config: Config,
    pub(crate) stages: Vec<crate::screenshot::harness::stages::StageRecord>,
    pub(crate) constants: crate::screenshot::harness::constants::CapturedConstants,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) water_reflection: Option<crate::screenshot::harness::WaterReflection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sun_depth_clip: Option<crate::screenshot::harness::SunDepthClip>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) resolve_inputs: Option<crate::screenshot::harness::resolve_probe::Capture>,
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
