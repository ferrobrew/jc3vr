//! Each eye's camera for the overlay draw, camera-relative.
//!
//! The overlay projects positions relative to the eye through a translation-free view-projection,
//! as the engine's own geometry does, rather than absolute world positions through the full one:
//! at the game's world coordinates an `f32` world-to-clip transform loses about a millimetre to
//! cancellation, which shows as jitter on a line that starts a hand's length from the eye.
//!
//! The render camera is per eye in two-pass stereo (the camera hook offsets it before each eye's
//! dispatch). Under the single-pass collapse it stays centred, and each eye is the centre transform
//! moved by the eye's render parameters, as the collapse's own per-eye view-projection is.
//!
//! The scene depth is reverse-Z with a finite far plane: with the projection's z row `(A, B)`
//! (`A = P.z_axis.z`, `B = P.w_axis.z` in the glam bridge's layout), the device depth at view
//! distance `d` is `-A + B / d`, so `d = B / (depth + A)`.

use glam::{Mat4, Quat, Vec3, Vec4};
use jc3gi::camera::camera_manager::CameraManager;
use windows::Win32::Graphics::Direct3D11::D3D11_VIEWPORT;

use crate::stereo::{STEREO_STATE, single_pass::collapse_ui_eye_override};

/// One eye's camera.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct EyeView {
    /// The eye's world position.
    pub position: Vec3,
    /// The view-projection of a position relative to the eye.
    pub offset_view_projection: Mat4,
    /// The eye's reverse-Z projection.
    pub projection: Mat4,
}

impl EyeView {
    /// The eye of a camera with world transform `transform`, view `view` (its inverse), and
    /// `projection`.
    pub fn from_camera(transform: Mat4, view: Mat4, projection: Mat4) -> EyeView {
        let mut rotation = view;
        rotation.w_axis = Vec4::W;
        EyeView {
            position: transform.w_axis.truncate(),
            offset_view_projection: projection * rotation,
            projection,
        }
    }

    /// The eye with world transform `eye_world` and `projection`.
    pub fn from_eye_world(eye_world: Mat4, projection: Mat4) -> EyeView {
        let mut rotation = eye_world;
        rotation.w_axis = Vec4::W;
        EyeView {
            position: eye_world.w_axis.truncate(),
            offset_view_projection: projection * rotation.inverse(),
            projection,
        }
    }

    /// The projection's z row `(A, B)`, for [`linear_depth`].
    pub fn z_row(&self) -> (f32, f32) {
        (self.projection.z_axis.z, self.projection.w_axis.z)
    }

    /// The projection's horizontal scale: the NDC width of a unit at unit distance, halved.
    pub fn x_scale(&self) -> f32 {
        self.projection.x_axis.x
    }
}

/// An eye's world transform under the collapse: the centre transform moved by the eye's world
/// offset and turned by its orientation relative to the centre.
pub fn collapse_eye_world(center: Mat4, world_offset: Vec3, orientation_delta: Quat) -> Mat4 {
    let mut eye_world = center;
    eye_world.w_axis += world_offset.extend(0.0);
    eye_world * Mat4::from_quat(orientation_delta)
}

/// The current draw's eye, and the viewport to draw it into when it is not the whole target (one
/// half of the collapse's shared target). `None` when no camera is readable.
pub fn current(eye: usize) -> Option<(EyeView, Option<D3D11_VIEWPORT>)> {
    let vr = crate::vr::render_params(eye);
    if let Some((viewport, _)) = collapse_ui_eye_override() {
        let center = STEREO_STATE.lock().center_transform?;
        let params = vr?;
        let eye_world = collapse_eye_world(
            Mat4::from(center),
            params.world_offset,
            params.orientation_delta,
        );
        let view = EyeView::from_eye_world(eye_world, Mat4::from(params.projection_reverse_z));
        return Some((view, Some(viewport)));
    }
    // SAFETY: reads the live camera manager singleton's render camera, which the engine rebuilds
    // for each eye's dispatch before the dispatch's draw.
    let (transform, view, projection) = unsafe {
        let camera = CameraManager::get()?.m_RenderCamera.as_ref()?;
        (camera.m_TransformF, camera.m_View, camera.m_ProjectionF)
    };
    // The VR projection is the render camera's without the upscaler's sub-pixel jitter, so the
    // overlay stays still while the scene is jittered for temporal reconstruction.
    let projection = vr.map_or(projection, |p| p.projection_reverse_z);
    let view = EyeView::from_camera(
        Mat4::from(transform),
        Mat4::from(view),
        Mat4::from(projection),
    );
    Some((view, None))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEAR: f32 = 0.1;

    /// The view distance of a reverse-Z device depth, as the overlay's pixel shader computes it.
    fn linear_depth(device_depth: f32, (a, b): (f32, f32)) -> f32 {
        b / (device_depth + a)
    }
    const FAR: f32 = 38400.0;

    /// A symmetric reverse-Z projection with a finite far plane, in the glam bridge's layout.
    fn reverse_z(near: f32, far: f32) -> Mat4 {
        let a = near / (far - near);
        let b = near * far / (far - near);
        Mat4::from_cols(
            Vec4::new(1.2, 0.0, 0.0, 0.0),
            Vec4::new(0.0, 1.3, 0.0, 0.0),
            Vec4::new(0.0, 0.0, a, -1.0),
            Vec4::new(0.0, 0.0, b, 0.0),
        )
    }

    fn clip(m: Mat4, p: Vec3) -> Vec4 {
        m * p.extend(1.0)
    }

    fn close(a: Vec4, b: Vec4) -> bool {
        (a - b).abs().max_element() < 1e-3 * b.abs().max_element().max(1.0)
    }

    fn world() -> Mat4 {
        Mat4::from_rotation_translation(
            Quat::from_euler(glam::EulerRot::YXZ, 0.7, -0.2, 0.05),
            Vec3::new(12.0, 3.0, -40.0),
        )
    }

    #[test]
    fn the_offset_view_projection_matches_the_full_one() {
        let transform = world();
        let projection = reverse_z(NEAR, FAR);
        let full = projection * transform.inverse();
        let eye = EyeView::from_camera(transform, transform.inverse(), projection);
        for p in [
            Vec3::new(13.0, 3.5, -45.0),
            Vec3::new(8.0, 0.0, -60.0),
            Vec3::new(12.2, 2.9, -40.5),
        ] {
            let offset = clip(eye.offset_view_projection, p - eye.position);
            assert!(
                close(offset, clip(full, p)),
                "{offset} vs {}",
                clip(full, p)
            );
        }
    }

    #[test]
    fn the_linear_depth_inverts_the_projection() {
        let eye = EyeView::from_eye_world(Mat4::IDENTITY, reverse_z(NEAR, FAR));
        for d in [NEAR, 0.5, 2.0, 30.0, 900.0, FAR] {
            let c = clip(eye.offset_view_projection, Vec3::new(0.0, 0.0, -d));
            assert!((c.w - d).abs() < 1e-4 * d);
            let recovered = linear_depth(c.z / c.w, eye.z_row());
            assert!((recovered - d).abs() < 1e-3 * d, "{d} -> {recovered}");
        }
        let near = clip(eye.offset_view_projection, Vec3::new(0.0, 0.0, -NEAR));
        assert!((near.z / near.w - 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_collapse_eye_matches_the_collapse_view_projection() {
        let center = world();
        let offset = Vec3::new(0.032, 0.0, 0.004);
        let delta = Quat::from_rotation_y(0.087);
        let projection = reverse_z(NEAR, FAR);
        let eye_world = collapse_eye_world(center, offset, delta);
        let full = projection * eye_world.inverse();
        let eye = EyeView::from_eye_world(eye_world, projection);
        assert!(eye.position.distance(center.w_axis.truncate() + offset) < 1e-5);
        for p in [Vec3::new(13.0, 3.5, -45.0), Vec3::new(11.0, 2.0, -39.0)] {
            assert!(close(
                clip(eye.offset_view_projection, p - eye.position),
                clip(full, p)
            ));
        }
    }
}
