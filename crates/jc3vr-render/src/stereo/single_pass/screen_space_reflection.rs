//! The screen-space reflection ray march, traced for each eye in one draw under the collapse.
//!
//! `CRenderBlockScreenSpaceReflection::Draw` traces reflections with one fullscreen draw into its
//! half-resolution reflection target. Its pixel shader rebuilds world positions along a view ray the
//! vertex shader interpolates from one basis, projects and marches the reflected ray with the render
//! context's view, projection, and camera transform, steps in the pixels of the whole target, and looks
//! the hit up in the previous frame's scene and the reflective water planes through two
//! view-projections. Under the collapse each of those is one matrix over both halves of the
//! double-wide target: the march projects through an eye-sized projection onto the double-wide depth,
//! so each eye's rays land in the wrong place, and the history lookup through the double-wide previous
//! view-projection lands both eyes on the middle of the previous frame.
//!
//! [`dxbc_stereo::localize_screen_space_reflection`] rewrites the ray march so each half traces its own
//! eye, reading that eye's rows from a mod-owned pixel constant buffer. This module builds those rows
//! and binds the buffer for the length of the block:
//!
//! - the eye's projection, view, and camera transform, exactly as an eye-sized render stages them, so
//!   the march runs in the eye's own pixels and stops at the eye's edge;
//! - the eye's view-projection, remapped onto the eye's half of the target, for the water-plane mask;
//! - the previous view-projection the engine staged, remapped onto the eye's half. The engine stages
//!   the centred camera's previous view-projection over the double-wide aspect, which is the eye-sized
//!   one an isolated eye render stages with its horizontal row halved, so moving it onto the eye's
//!   half reproduces the isolated lookup;
//! - the view ray, as the three coefficients of the screen UV the vertex shader's basis amounts to for
//!   the eye, with the eye's half folded in.
//!
//! The scene-colour capture before the march is per pixel, and the blur after it steps whole target
//! texels, with its taps clamped to the eye's half by [`dxbc_stereo::localize_reflection_blur`]. So the
//! block keeps its single run and the draw count of an eye-sized render.
//!
//! The rewrites are baked into the bytecode when the engine creates the shaders, so they are only right
//! while the collapse is live; the debug UI requests a shader reload when the switch changes, and the
//! block binds the rows only while the ray march it will run is a rewritten one.

use std::sync::atomic::{AtomicBool, Ordering};

use dxbc_stereo::{
    SCREEN_SPACE_REFLECTION_CB_REGISTER, SCREEN_SPACE_REFLECTION_ROWS_PER_EYE,
    ScreenSpaceReflectionRow, localize_reflection_blur, localize_screen_space_reflection,
};
use jc3gi::{
    graphics_engine::{graphics_engine::RenderContext, render_engine::RenderEngine},
    types::math::{Matrix4, Vector4},
};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BUFFER_DESC, D3D11_CPU_ACCESS_WRITE, D3D11_MAP_WRITE_DISCARD,
    D3D11_MAPPED_SUBRESOURCE, D3D11_USAGE_DYNAMIC, ID3D11Buffer, ID3D11DeviceContext,
};

use crate::{
    stereo::{STEREO_STATE, engine_context::EngineContext},
    vr::render_params,
};

/// A reflection pass's bytecode rewritten for the collapse, or `None` for any other shader.
///
/// Called for every fragment program the engine creates. `enabled` is whether the rewrite applies
/// now; a ray march created without it records that the block must not bind the eye rows. The blur
/// is rewritten alongside it and reads no eye rows.
pub(crate) fn rewrite_fragment_program(code: &[u8], enabled: bool) -> Option<Vec<u8>> {
    let rewritten = match localize_screen_space_reflection(code) {
        Ok(rewritten) => {
            REWRITTEN.store(enabled, Ordering::Relaxed);
            rewritten
        }
        Err(_) if !enabled => return None,
        Err(_) => localize_reflection_blur(code).ok()?,
    };
    if enabled && !LOGGED.swap(true, Ordering::Relaxed) {
        tracing::info!(
            target: "single_pass",
            "ssr: passes rewritten to trace each eye's half of the double-wide target",
        );
    }
    enabled.then_some(rewritten)
}

/// Bind each eye's rows for the ray march, for as long as the returned guard lives. `None`, with
/// nothing bound, when the ray march is not a rewritten one or an eye's projection or pose is not
/// available.
pub(crate) fn bind_eye_rows(rc: &RenderContext) -> Option<EyeRowsBound> {
    if !REWRITTEN.load(Ordering::Relaxed) {
        return None;
    }
    let center = glam::Mat4::from(STEREO_STATE.lock().center_transform?);
    // SAFETY: the render engine singleton outlives the frame; the block reads the staged rows on this
    // same thread.
    let staged = unsafe { RenderEngine::get() }?.m_VPGlobalConstData;
    let previous = Matrix4 {
        data: std::array::from_fn(|i| staged[PREVIOUS_VIEW_PROJECTION_ROW + i / 4].data[i % 4]),
    };
    let screen = Screen {
        inverse_far: rc.CameraFar.recip(),
        uv_scale: rc.m_UVScale,
    };
    let mut rows = [Vector4::default(); ROWS];
    for eye in 0..2 {
        let params = render_params(eye)?;
        let block = eye_rows(
            &EyeCamera {
                projection: params.projection_reverse_z,
                world: params.resolve(center),
            },
            &previous,
            screen,
            eye,
        );
        if block
            .iter()
            .any(|row| row.data.iter().any(|v| !v.is_finite()))
        {
            return None;
        }
        rows[eye * EYE_ROWS..(eye + 1) * EYE_ROWS].copy_from_slice(&block);
    }
    let d3d = EngineContext::get()?;
    let result = d3d.with_lock(|ctx| {
        let mut buffer = BUFFER.lock();
        // SAFETY: on the render thread under the engine's context mutex, as every other write to the
        // immediate context.
        unsafe { upload_and_bind(&mut buffer, ctx, &rows) }
    });
    match result {
        Ok(()) => Some(EyeRowsBound(d3d)),
        Err(error) => {
            if !BIND_WARNED.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    target: "single_pass",
                    "ssr: could not bind the per-eye rows ({error}); the reflections find no hits",
                );
            }
            None
        }
    }
}

/// The scope opened by [`bind_eye_rows`]; unbinds the rows when dropped.
pub(crate) struct EyeRowsBound(EngineContext);

impl Drop for EyeRowsBound {
    fn drop(&mut self) {
        // SAFETY: as in `bind_eye_rows`.
        self.0.with_lock(|ctx| unsafe {
            ctx.PSSetConstantBuffers(SCREEN_SPACE_REFLECTION_CB_REGISTER, Some(&[None]));
        });
    }
}

/// Release the mod-owned buffer, so it does not outlive an eject.
pub(super) fn release() {
    *BUFFER.lock() = None;
    REWRITTEN.store(false, Ordering::Relaxed);
}

/// The rows of one eye's block.
const EYE_ROWS: usize = SCREEN_SPACE_REFLECTION_ROWS_PER_EYE as usize;
/// The whole buffer: both eyes' blocks.
const ROWS: usize = 2 * EYE_ROWS;

/// The first of the four vertex `GlobalConstants` rows holding the previous view-projection, which the
/// engine stages from the same render-context matrix the block uploads to the ray march.
const PREVIOUS_VIEW_PROJECTION_ROW: usize = 33;

/// Whether the ray march the engine last created carries the rewrite.
static REWRITTEN: AtomicBool = AtomicBool::new(false);
static LOGGED: AtomicBool = AtomicBool::new(false);
static BIND_WARNED: AtomicBool = AtomicBool::new(false);

static BUFFER: Mutex<Option<ID3D11Buffer>> = Mutex::new(None);

/// One eye's camera: its reverse-Z projection, in the engine's layout, and its world transform.
struct EyeCamera {
    projection: Matrix4,
    world: glam::Mat4,
}

/// What the block's vertex shader folds into the view ray besides the eye: the reciprocal far plane
/// it scales the ray by, and the UV scale it applies to the screen UV.
#[derive(Clone, Copy)]
struct Screen {
    inverse_far: f32,
    uv_scale: [f32; 2],
}

/// One eye's block, in the order [`ScreenSpaceReflectionRow`] lays it out.
fn eye_rows(
    camera: &EyeCamera,
    previous: &Matrix4,
    screen: Screen,
    eye: usize,
) -> [Vector4; EYE_ROWS] {
    let view = Matrix4::from(camera.world.inverse());
    let world = Matrix4::from(camera.world);
    let view_projection = remap_onto_half(&(view * camera.projection), 0.5, eye);
    let previous = remap_onto_half(previous, 1.0, eye);
    let mut rows = [Vector4::default(); EYE_ROWS];
    let mut put = |first: ScreenSpaceReflectionRow, matrix: &Matrix4| {
        for (row, values) in matrix.data.as_chunks::<4>().0.iter().enumerate() {
            rows[first as usize + row] = Vector4 { data: *values };
        }
    };
    put(ScreenSpaceReflectionRow::Projection, &camera.projection);
    put(ScreenSpaceReflectionRow::View, &view);
    put(ScreenSpaceReflectionRow::CameraTransform, &world);
    put(ScreenSpaceReflectionRow::ViewProjection, &view_projection);
    put(ScreenSpaceReflectionRow::PreviousViewProjection, &previous);
    let ray = ScreenSpaceReflectionRow::ViewRay as usize;
    for (row, coefficient) in view_ray(camera, screen, eye).into_iter().enumerate() {
        rows[ray + row] = Vector4 {
            data: coefficient.extend(0.0).to_array(),
        };
    }
    rows
}

/// A world-to-clip matrix whose horizontal clip row is `scale` times its own plus the eye's offset onto
/// its half of the target: `x' = scale * x + (eye - 1/2) * w`. An eye-sized projection takes a scale of
/// one half; the engine's double-wide one is already halved and takes one.
fn remap_onto_half(matrix: &Matrix4, scale: f32, eye: usize) -> Matrix4 {
    let offset = eye as f32 - 0.5;
    let mut out = *matrix;
    for row in out.data.as_chunks_mut::<4>().0 {
        row[0] = scale * row[0] + offset * row[3];
    }
    out
}

/// The view ray as the vertex shader would interpolate it for `eye`, written as
/// `uv.x * a + uv.y * b + c` over the double-wide target's screen UV.
///
/// The vertex shader takes each corner's clip position through the inverse of the projection and the
/// camera's rotation, divides by `w`, and scales by the reciprocal far plane. At a clip depth of zero
/// that is affine in the clip position, so it is read off at three points of the eye's own clip space.
/// The eye's clip `x` over the double-wide UV `u` is `4u - 2 * eye - 1`, and clip `y` is `1 - 2v`,
/// before the UV scale.
fn view_ray(camera: &EyeCamera, screen: Screen, eye: usize) -> [glam::Vec3; 3] {
    let mut rotation = camera.world;
    rotation.w_axis = glam::Vec4::W;
    let unproject = rotation * glam::Mat4::from(camera.projection).inverse();
    let ray = |x: f32, y: f32| {
        let point = unproject * glam::Vec4::new(x, y, 0.0, 1.0);
        point.truncate() / point.w * screen.inverse_far
    };
    let origin = ray(0.0, 0.0);
    let per_x = ray(1.0, 0.0) - origin;
    let per_y = ray(0.0, 1.0) - origin;
    let [scale_u, scale_v] = screen.uv_scale;
    [
        per_x * (4.0 / scale_u),
        per_y * (-2.0 / scale_v),
        origin - per_x * (2.0 * eye as f32 + 1.0) + per_y,
    ]
}

/// Create the buffer on first use, write `rows` into it, and bind it on the pixel stage.
unsafe fn upload_and_bind(
    buffer: &mut Option<ID3D11Buffer>,
    ctx: &ID3D11DeviceContext,
    rows: &[Vector4; ROWS],
) -> windows::core::Result<()> {
    let buffer = match buffer {
        Some(buffer) => buffer,
        None => {
            // SAFETY: the context belongs to the live device, which creates the buffer.
            let device = unsafe { ctx.GetDevice()? };
            let mut created = None;
            // SAFETY: a valid dynamic constant-buffer description with no initial data.
            unsafe {
                device.CreateBuffer(
                    &D3D11_BUFFER_DESC {
                        ByteWidth: std::mem::size_of_val(rows) as u32,
                        Usage: D3D11_USAGE_DYNAMIC,
                        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                        ..Default::default()
                    },
                    None,
                    Some(&mut created),
                )?
            };
            buffer.insert(created.ok_or_else(windows::core::Error::empty)?)
        }
    };
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    // SAFETY: the buffer is dynamic and CPU-writable, and the mapping is `size_of_val(rows)` bytes.
    unsafe {
        ctx.Map(&*buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))?;
        std::ptr::copy_nonoverlapping(rows.as_ptr(), mapped.pData.cast(), ROWS);
        ctx.Unmap(&*buffer, 0);
        ctx.PSSetConstantBuffers(
            SCREEN_SPACE_REFLECTION_CB_REGISTER,
            Some(&[Some(buffer.clone())]),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use glam::{Mat4, Quat, Vec3, Vec4};

    use super::*;

    /// An off-axis reverse-Z projection, as a VR eye has.
    fn projection() -> Matrix4 {
        let (near, far) = (0.1f32, 40_000.0f32);
        let scale = near / (far - near);
        Matrix4::from(Mat4::from_cols(
            Vec4::new(0.9, 0.0, 0.0, 0.0),
            Vec4::new(0.0, 0.8, 0.0, 0.0),
            Vec4::new(0.1, -0.05, scale, -1.0),
            Vec4::new(0.0, 0.0, scale * far, 0.0),
        ))
    }

    fn camera(offset: f32) -> EyeCamera {
        EyeCamera {
            projection: projection(),
            world: Mat4::from_rotation_translation(
                Quat::from_euler(glam::EulerRot::YXZ, 0.4, -0.2, 0.087),
                Vec3::new(10.0 + offset, 2.0, -3.0),
            ),
        }
    }

    const SCREEN: Screen = Screen {
        inverse_far: 1.0 / 40_000.0,
        uv_scale: [1.0, 1.0],
    };

    /// Project a world point through an engine-layout world-to-clip matrix.
    fn clip(matrix: &Matrix4, point: Vec3) -> Vec4 {
        Mat4::from(*matrix) * point.extend(1.0)
    }

    /// Each eye's half of the double-wide UV rebuilds the ray an eye-sized target interpolates over the
    /// eye's own UV: the point at linear depth `d` along it reprojects through the eye's own
    /// view-projection to the eye-local UV the pixel stands for.
    #[test]
    fn view_ray_maps_each_half_onto_the_eye() {
        for eye in 0..2 {
            let camera = camera(eye as f32 * 0.064);
            let [a, b, c] = view_ray(&camera, SCREEN, eye);
            let view_projection = Matrix4::from(camera.world.inverse()) * camera.projection;
            for (u, v) in [(0.1f32, 0.2f32), (0.4, 0.9), (0.25, 0.5)] {
                let wide = (u + eye as f32) / 2.0;
                let ray = a * wide + b * v + c;
                let world = camera.world.w_axis.truncate() + ray / SCREEN.inverse_far * 0.01;
                let p = clip(&view_projection, world);
                let local = Vec3::new(p.x / p.w * 0.5 + 0.5, 0.5 - p.y / p.w * 0.5, 0.0);
                assert!(
                    (local.x - u).abs() < 1e-3 && (local.y - v).abs() < 1e-3,
                    "eye {eye} at ({u}, {v}): reprojected to {local}"
                );
            }
        }
    }

    /// The remapped view-projection lands a point on the double-wide UV of its eye's half.
    #[test]
    fn view_projection_lands_on_the_eye_half() {
        for eye in 0..2 {
            let camera = camera(0.0);
            let rows = eye_rows(&camera, &Matrix4::from(Mat4::IDENTITY), SCREEN, eye);
            let first = ScreenSpaceReflectionRow::ViewProjection as usize;
            let remapped = Matrix4 {
                data: std::array::from_fn(|i| rows[first + i / 4].data[i % 4]),
            };
            let own = Matrix4::from(camera.world.inverse()) * camera.projection;
            let point = camera.world.transform_point3(Vec3::new(0.3, -0.2, -5.0));
            let (p, q) = (clip(&own, point), clip(&remapped, point));
            let local = p.x / p.w * 0.5 + 0.5;
            let wide = q.x / q.w * 0.5 + 0.5;
            assert!((wide - (local + eye as f32) / 2.0).abs() < 1e-5);
            assert!((q.y / q.w - p.y / p.w).abs() < 1e-6 && (q.z / q.w - p.z / p.w).abs() < 1e-6);
        }
    }

    /// The double-wide previous view-projection the engine stages is the eye-sized one with its
    /// horizontal row halved; moving it onto an eye's half gives the coordinate an eye-sized lookup
    /// reads, placed in that eye's half.
    #[test]
    fn previous_view_projection_moves_onto_the_eye_half() {
        let camera = camera(0.0);
        let eye_sized = Matrix4::from(camera.world.inverse()) * camera.projection;
        let mut double_wide = eye_sized;
        for row in double_wide.data.as_chunks_mut::<4>().0 {
            row[0] *= 0.5;
        }
        let point = camera.world.transform_point3(Vec3::new(-0.4, 0.1, -7.0));
        let p = clip(&eye_sized, point);
        let local = p.x / p.w * 0.5 + 0.5;
        for eye in 0..2 {
            let rows = eye_rows(&camera, &double_wide, SCREEN, eye);
            let first = ScreenSpaceReflectionRow::PreviousViewProjection as usize;
            let moved = Matrix4 {
                data: std::array::from_fn(|i| rows[first + i / 4].data[i % 4]),
            };
            let q = clip(&moved, point);
            assert!((q.x / q.w * 0.5 + 0.5 - (local + eye as f32) / 2.0).abs() < 1e-5);
        }
    }

    /// The camera rows are the eye's own matrices, unremapped: the march runs in the eye's pixels.
    #[test]
    fn camera_rows_are_the_eye_matrices() {
        let camera = camera(0.0);
        let rows = eye_rows(&camera, &Matrix4::from(Mat4::IDENTITY), SCREEN, 1);
        let read = |first: ScreenSpaceReflectionRow| Matrix4 {
            data: std::array::from_fn(|i| rows[first as usize + i / 4].data[i % 4]),
        };
        assert_eq!(
            read(ScreenSpaceReflectionRow::Projection).data,
            projection().data
        );
        assert_eq!(
            read(ScreenSpaceReflectionRow::CameraTransform).data,
            Matrix4::from(camera.world).data
        );
        let round_trip = Mat4::from(read(ScreenSpaceReflectionRow::View)) * camera.world;
        assert!(round_trip.abs_diff_eq(Mat4::IDENTITY, 1e-3));
    }
}
