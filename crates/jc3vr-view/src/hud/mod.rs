//! The render-side pieces of the HUD: the render camera's pose, the pointer's panel math
//! ([`panel`]: where a ray meets the floating panel), and the world markers' stage frame
//! ([`stage_frame`], which the marker layout projects through). The HUD itself lives in the HUD
//! layer.

use glam::{Mat3, Mat4, Quat, Vec3};
use jc3gi::camera::camera_manager::CameraManager;

pub mod panel;
pub mod stage_frame;

/// Extract the head's world-space pose `(position, rotation)` from the render camera's world
/// transform, or `None` if the camera is not available.
///
/// The camera's world transform stores its basis vectors in rows (pyxis docs): right (+X), up (+Y),
/// the +Z basis (back), then the translation. A row-major, row-vector matrix's flat array is the
/// column-major array of the equivalent glam matrix, so those rows arrive as `Mat4`'s `x_axis`,
/// `y_axis`, `z_axis`, and `w_axis`. The rotation converts to a quaternion that maps camera-local to
/// world space, so `quat * Vec3::NEG_Z` yields the forward direction.
pub fn render_camera_pose() -> Option<(Vec3, Quat)> {
    // Prefer the center (un-offset) transform snapshotted in `SetupRenderCamera` before the per-eye
    // parallax offset is applied. By the time `draw_quad` runs on eye 0, `m_TransformF` already
    // carries eye 0's half-IPD offset, which would leak into the cached panel position and double
    // the stereo disparity for eye 1. Fall back to the live transform when the snapshot is absent
    // (e.g. stereo not active, or the hook has not run yet).
    let transform = crate::stereo::STEREO_STATE
        .lock()
        .center_transform
        .or_else(|| unsafe {
            let cm = CameraManager::get()?;
            let cam = cm.m_RenderCamera.as_ref()?;
            Some(cam.m_TransformF)
        })?;

    let transform = Mat4::from(transform);
    Some((
        transform.w_axis.truncate(),
        Quat::from_mat3(&Mat3::from_mat4(transform)),
    ))
}
