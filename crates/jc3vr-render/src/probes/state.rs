//! The render state the capture probes read beside the events: the pass and eye a draw belongs to,
//! the bound vertex shader, the `cb13` buffer and its per-eye rows, the sun's far-depth observation,
//! the eyes' cameras, and the split fullscreen runs.
//!
//! The items behind these readers are crate-private; this module is the one place the capture side
//! reads them from, so what it depends on in the stereo path is listed here and nowhere else.

use jc3gi::types::math::{Matrix4, Vector4};
use windows::Win32::Graphics::Direct3D11::D3D11_VIEWPORT;

pub use crate::stereo::single_pass::{FrameDiagnostics, STEREO_CB_TOTAL_ROWS, VertexIdentity};
use crate::{hooks::graphics_engine::reconstruction, stereo::single_pass};

/// The render pass being walked.
pub fn current_pass_id() -> u8 {
    single_pass::current_pass_id()
}

/// The eye a block re-issued once per eye is drawing, inside such a re-issue.
pub fn per_eye_reissue_eye() -> Option<usize> {
    single_pass::per_eye_reissue_eye()
}

/// Whether the bound vertex shader carries the stereo rewrite.
pub fn bound_vertex_shader_patched() -> bool {
    single_pass::bound_vertex_shader_patched()
}

/// The identity the vertex-shader census recorded for `shader`, when the census is armed and
/// complete.
pub fn vertex_identity_lookup(shader: usize) -> Result<Option<VertexIdentity>, &'static str> {
    single_pass::vertex_identity_lookup(shader)
}

/// The live `cb13` buffer's interface pointer, for comparing with what a draw has bound.
pub fn cb13_buffer_ptr() -> Option<usize> {
    single_pass::cb13_buffer_ptr()
}

/// The four view-projection rows and four `M_eye` rows a per-eye pin places in the first `cb13` slot
/// for `eye`.
pub fn pinned_eye_rows(
    rows: &[Vector4; STEREO_CB_TOTAL_ROWS],
    eye: usize,
) -> ([[f32; 4]; 4], [[f32; 4]; 4]) {
    single_pass::pinned_eye_rows(rows, eye)
}

/// The far-plane bits this thread's re-driven sun draw consumed from the observed `cb1` upload.
pub fn consumed_far_bits() -> Option<u32> {
    single_pass::consumed_far_bits()
}

/// Whether this thread's re-driven sun draw took the far-depth correction.
pub fn far_depth_corrected() -> bool {
    single_pass::far_depth_corrected()
}

/// Eye `eye`'s full world-to-clip view-projection, jitter included.
pub fn full_eye_view_projection(eye: usize) -> Option<Matrix4> {
    single_pass::full_eye_view_projection(eye)
}

/// Eye `eye`'s half of the scene viewport and the live `cb13` buffer's interface pointer: the
/// sources a per-eye terrain-patch submission draws from.
pub fn patch_camera_sources(eye: usize) -> Option<(D3D11_VIEWPORT, usize)> {
    single_pass::patch_camera_sources(eye)
}

/// The most recent frame's single-pass matrix diagnostics.
pub fn last_frame_diagnostics() -> Option<FrameDiagnostics> {
    single_pass::last_frame_diagnostics()
}

/// The eye of the split fullscreen run in flight on this thread, masked or not yet masked.
pub fn split_run_eye() -> Option<usize> {
    reconstruction::split_run_eye()
}

/// The eye of the split fullscreen run in flight on this thread, once its mask is applied.
pub fn masked_eye() -> Option<usize> {
    reconstruction::masked_eye()
}
