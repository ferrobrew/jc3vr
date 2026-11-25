//! The ambient-occlusion volumes, shaded for both eyes from one draw under the collapse.
//!
//! Each volume (`CRenderBlockAOBox`, drawn with the `aobox` or `aosphere` pixel shader in
//! `RP_AO_VOLUMES`) maps the scene under it into the box's own space through a matrix its `Draw`
//! builds from the render context's camera, from the depth sampled at the screen UV its vertex shader
//! interpolates. Under the collapse the render context's camera is the centred one and the depth
//! texture spans both eyes, so each eye's half sampled the depth of the wrong place (the left half of
//! an eye reads the left eye's depth stretched across the whole eye, the right half the right eye's)
//! and mapped it through the wrong camera. In a lit tunnel that is a sharp edge down the middle of
//! each eye and occlusion wedges where no box is.
//!
//! The `aobox` vertex shader takes the reprojection, so the draw is instance-doubled and each eye's
//! half rasterizes the volume from that eye. [`dxbc_stereo::localize_ambient_occlusion_volume`]
//! rewrites the pixel shaders to work from the pixel position alone: sample both depths at the
//! pixel's double-wide UV, and map each pixel and depth into the centred camera's UV and depth before
//! the centred matrix reads them. This module builds that map for each eye, `P_eye`, and binds it for
//! the length of the pass. With `T` the shader UV to NDC, `M_eye` the reprojection from the centred
//! camera's clip space to the eye's, and `S_eye` the eye's pixel to its shader UV (the UV the eye's
//! own render would interpolate, scaled by the render context's `m_UVScale`):
//!
//! ```text
//! P_eye = T⁻¹ · M_eye⁻¹ · T · S_eye
//! ```
//!
//! The centred matrix is `Box⁻¹ · VP_centre⁻¹ · T`, so `Box⁻¹ · VP_centre⁻¹ · M_eye⁻¹ · T · S_eye`
//! is `Box⁻¹ · VP_eye⁻¹ · T · S_eye`: exactly the matrix the eye's own render builds, applied to the
//! UV that render reads. Outside an eye-split pass the rows map the whole target with no eye, which
//! is what the unrewritten shader computes from a draw over the whole target.

use std::sync::atomic::{AtomicBool, Ordering};

use dxbc_stereo::{
    AMBIENT_OCCLUSION_VOLUME_BOUNDARY_ROW, AMBIENT_OCCLUSION_VOLUME_CB_REGISTER,
    AMBIENT_OCCLUSION_VOLUME_CB_ROWS, AMBIENT_OCCLUSION_VOLUME_DEPTH_UV_ROW,
    AMBIENT_OCCLUSION_VOLUME_ROWS_PER_EYE, localize_ambient_occlusion_volume,
};
use glam::{Mat4, Vec4};
use jc3gi::{graphics_engine::graphics_engine::RenderContext, types::math::Vector4};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BUFFER_DESC, D3D11_CPU_ACCESS_WRITE, D3D11_MAP_WRITE_DISCARD,
    D3D11_MAPPED_SUBRESOURCE, D3D11_USAGE_DYNAMIC, ID3D11Buffer, ID3D11DeviceContext,
};

use super::*;

/// An ambient-occlusion volume pixel shader rewritten for the collapse, or `None` for any other
/// shader or when `enabled` is off.
///
/// Called for every fragment program the engine creates. A volume shader created with the rewrite
/// records that the pass must bind the rows.
pub(crate) fn rewrite_fragment_program(code: &[u8], enabled: bool) -> Option<Vec<u8>> {
    if !enabled {
        return None;
    }
    let rewritten = localize_ambient_occlusion_volume(code).ok()?;
    REWRITTEN.store(true, Ordering::Relaxed);
    if !LOGGED.swap(true, Ordering::Relaxed) {
        tracing::info!(
            target: "single_pass",
            "ao volumes: pixel shaders rewritten to map each eye's half through its own camera",
        );
    }
    Some(rewritten)
}

/// Bind the rows the rewritten volume shaders read, for as long as the returned guard lives. `None`,
/// with nothing bound, when no volume shader carries the rewrite.
///
/// # Safety
///
/// `rc` must be the live render context of the pass being drawn.
pub(crate) unsafe fn bind_rows(rc: *const RenderContext) -> Option<RowsBound> {
    if !REWRITTEN.load(Ordering::Relaxed) {
        return None;
    }
    // SAFETY: `rc` is live per the caller contract.
    let uv_scale = unsafe { rc.as_ref() }?.m_UVScale;
    let full = collapse_full_viewport()?;
    let target = Target {
        origin: [full.TopLeftX, full.TopLeftY],
        size: [full.Width, full.Height],
        uv_scale,
    };
    let split = dual_eye_active() && collapse_active() && in_gbuffer_range();
    let reprojection = split.then(|| *CURRENT_M_EYE.lock()).flatten();
    let rows = rows(target, reprojection);
    if rows
        .iter()
        .any(|row| row.data.iter().any(|v| !v.is_finite()))
    {
        return None;
    }
    let d3d = EngineContext::get()?;
    let result = d3d.with_lock(|ctx| {
        let mut buffer = BUFFER.lock();
        // SAFETY: on the render thread under the engine's context mutex, as every other write to the
        // immediate context.
        unsafe { upload_and_bind(&mut buffer, ctx, &rows) }
    });
    match result {
        Ok(()) => Some(RowsBound(d3d)),
        Err(error) => {
            if !BIND_WARNED.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    target: "single_pass",
                    "ao volumes: could not bind the per-eye rows ({error}); the volumes shade \
                     nothing",
                );
            }
            None
        }
    }
}

/// The scope opened by [`bind_rows`]; unbinds the rows when dropped.
pub(crate) struct RowsBound(EngineContext);

impl Drop for RowsBound {
    fn drop(&mut self) {
        // SAFETY: as in `bind_rows`.
        self.0.with_lock(|ctx| unsafe {
            ctx.PSSetConstantBuffers(AMBIENT_OCCLUSION_VOLUME_CB_REGISTER, Some(&[None]));
        });
    }
}

/// Release the mod-owned buffer, so it does not outlive an eject.
pub(super) fn release() {
    *BUFFER.lock() = None;
    REWRITTEN.store(false, Ordering::Relaxed);
}

const ROWS: usize = AMBIENT_OCCLUSION_VOLUME_CB_ROWS as usize;
const EYE_ROWS: usize = AMBIENT_OCCLUSION_VOLUME_ROWS_PER_EYE as usize;
const DEPTH_UV: usize = AMBIENT_OCCLUSION_VOLUME_DEPTH_UV_ROW as usize;
const BOUNDARY: usize = AMBIENT_OCCLUSION_VOLUME_BOUNDARY_ROW as usize;

/// Whether a volume shader the engine created carries the rewrite.
static REWRITTEN: AtomicBool = AtomicBool::new(false);
static LOGGED: AtomicBool = AtomicBool::new(false);
static BIND_WARNED: AtomicBool = AtomicBool::new(false);

static BUFFER: Mutex<Option<ID3D11Buffer>> = Mutex::new(None);

/// The target the volumes draw into: the viewport's origin and size in pixels, and the render
/// context's UV scale.
#[derive(Clone, Copy)]
struct Target {
    origin: [f32; 2],
    size: [f32; 2],
    uv_scale: [f32; 2],
}

/// The buffer's rows: each eye's map from its pixel and depth to the centred camera's homogeneous UV
/// and depth, the depth UV, and the eye boundary. With `reprojection` (the two eyes' `M_eye`), the
/// target is split into two eye halves; without it, both blocks map the whole target and no pixel
/// reaches the boundary.
fn rows(target: Target, reprojection: Option<[Mat4; 2]>) -> [Vector4; ROWS] {
    let [x0, y0] = target.origin;
    let [width, height] = target.size;
    let [su, sv] = target.uv_scale;
    let eye_width = if reprojection.is_some() {
        width / 2.0
    } else {
        width
    };
    let mut rows = [Vector4::default(); ROWS];
    for eye in 0..2 {
        let (offset, m_eye) = match reprojection {
            Some(m) => (eye as f32 * eye_width, m[eye]),
            None => (0.0, Mat4::IDENTITY),
        };
        let map = uv_from_ndc()
            * m_eye.inverse()
            * ndc_from_uv()
            * pixel_to_uv([x0 + offset, y0], [eye_width, height], [su, sv]);
        for row in 0..EYE_ROWS {
            rows[eye * EYE_ROWS + row] = Vector4 {
                data: map.row(row).to_array(),
            };
        }
    }
    rows[DEPTH_UV] = Vector4 {
        data: [su / width, sv / height, -x0 * su / width, -y0 * sv / height],
    };
    let boundary = if reprojection.is_some() {
        x0 + eye_width
    } else {
        f32::MAX
    };
    rows[BOUNDARY] = Vector4 {
        data: [boundary, 0.0, 0.0, 0.0],
    };
    rows
}

/// `(x, y, depth, 1)` to the shader UV an eye-sized render over `origin` and `size` interpolates at
/// that pixel, scaled by `uv_scale`, with the depth kept.
fn pixel_to_uv(origin: [f32; 2], size: [f32; 2], uv_scale: [f32; 2]) -> Mat4 {
    let scale = [uv_scale[0] / size[0], uv_scale[1] / size[1]];
    Mat4::from_cols(
        Vec4::new(scale[0], 0.0, 0.0, 0.0),
        Vec4::new(0.0, scale[1], 0.0, 0.0),
        Vec4::Z,
        Vec4::new(-origin[0] * scale[0], -origin[1] * scale[1], 0.0, 1.0),
    )
}

/// The shader UV (`v` down) to normalized device coordinates (`y` up), the depth kept.
fn ndc_from_uv() -> Mat4 {
    Mat4::from_cols(
        Vec4::new(2.0, 0.0, 0.0, 0.0),
        Vec4::new(0.0, -2.0, 0.0, 0.0),
        Vec4::Z,
        Vec4::new(-1.0, 1.0, 0.0, 1.0),
    )
}

/// Homogeneous clip coordinates back to homogeneous shader UV: the inverse of [`ndc_from_uv`], which
/// keeps `w` so the shader's divide by it lands on the UV.
fn uv_from_ndc() -> Mat4 {
    ndc_from_uv().inverse()
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
            AMBIENT_OCCLUSION_VOLUME_CB_REGISTER,
            Some(&[Some(buffer.clone())]),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use glam::{Quat, Vec3};

    use super::*;

    const TARGET: Target = Target {
        origin: [0.0, 0.0],
        size: [640.0, 360.0],
        uv_scale: [1.0, 1.0],
    };

    /// What the rewritten shader computes from a block of rows: the map applied to the pixel and
    /// depth, divided through by `w`.
    fn mapped(rows: &[Vector4; ROWS], eye: usize, pixel: [f32; 2], depth: f32) -> Vec3 {
        let point = Vec4::new(pixel[0], pixel[1], depth, 1.0);
        let h = Vec4::from_array(std::array::from_fn(|row| {
            Vec4::from_array(rows[eye * EYE_ROWS + row].data).dot(point)
        }));
        h.truncate() / h.w
    }

    /// The UV and depth a camera's clip transform gives a world point, as a shader reads them.
    fn screen(view_projection: Mat4, world: Vec3) -> Vec3 {
        let clip = view_projection * world.extend(1.0);
        let ndc = clip.truncate() / clip.w;
        Vec3::new(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5, ndc.z)
    }

    fn view_projection(position: Vec3, yaw: f32, shear: f32) -> Mat4 {
        let mut projection = Mat4::perspective_infinite_reverse_rh(1.2, 320.0 / 360.0, 0.1);
        projection.z_axis.x = shear;
        projection * Mat4::from_rotation_translation(Quat::from_rotation_y(yaw), position).inverse()
    }

    /// Outside an eye split, the rows reproduce the screen UV a draw over the whole target
    /// interpolates, and no pixel reaches the boundary.
    #[test]
    fn whole_target_maps_pixels_to_their_uv() {
        let rows = rows(TARGET, None);
        let point = mapped(&rows, 0, [160.5, 90.5], 0.25);
        assert!(point.abs_diff_eq(Vec3::new(160.5 / 640.0, 90.5 / 360.0, 0.25), 1e-6));
        assert_eq!(rows[BOUNDARY].data[0], f32::MAX);
        let uv = rows[DEPTH_UV].data;
        assert!((160.5 * uv[0] + uv[2] - 160.5 / 640.0).abs() < 1e-7);
    }

    /// In each eye's half, the centred matrix applied to the mapped pixel and depth gives what the
    /// eye's own matrix gives the UV and depth the eye's own render reads there, UV scale included;
    /// the depth is sampled at the pixel's double-wide UV, and the boundary is the seam.
    #[test]
    fn each_half_reads_through_the_centred_matrix_as_its_own_eye() {
        let centre = view_projection(Vec3::new(5.0, 2.0, -3.0), 0.3, 0.0);
        let eyes = [
            view_projection(Vec3::new(4.968, 2.0, -3.0), 0.31, 0.12),
            view_projection(Vec3::new(5.032, 2.0, -3.0), 0.29, -0.12),
        ];
        let m_eye = eyes.map(|eye| eye * centre.inverse());
        let uv_scale = [0.75, 0.5];
        let rows = rows(Target { uv_scale, ..TARGET }, Some(m_eye));
        assert_eq!(rows[BOUNDARY].data[0], 320.0);
        // The matrix a render builds for a camera, with the box at the origin: the shader UV and
        // depth back to the world.
        let screen_to_world = |view_projection: Mat4| view_projection.inverse() * ndc_from_uv();
        let apply = |matrix: Mat4, point: Vec3| {
            let h = matrix * point.extend(1.0);
            h.truncate() / h.w
        };
        let world = Vec3::new(6.0, 1.5, -9.0);
        for (eye, view_projection) in eyes.into_iter().enumerate() {
            let seen = screen(view_projection, world);
            let pixel = [seen.x * 320.0 + eye as f32 * 320.0, seen.y * 360.0];
            let read = Vec3::new(seen.x * uv_scale[0], seen.y * uv_scale[1], seen.z);
            let own = apply(screen_to_world(view_projection), read);
            let collapsed = apply(screen_to_world(centre), mapped(&rows, eye, pixel, seen.z));
            assert!(
                own.abs_diff_eq(collapsed, 1e-3),
                "eye {eye}: {collapsed} vs {own}"
            );
            let uv = rows[DEPTH_UV].data;
            let depth_uv = [pixel[0] * uv[0] + uv[2], pixel[1] * uv[1] + uv[3]];
            let wide = [
                (seen.x + eye as f32) / 2.0 * uv_scale[0],
                seen.y * uv_scale[1],
            ];
            assert!((depth_uv[0] - wide[0]).abs() < 1e-5 && (depth_uv[1] - wide[1]).abs() < 1e-5);
        }
        // With no UV scale the eye's world point itself comes back.
        let rows = super::rows(TARGET, Some(m_eye));
        let seen = screen(eyes[1], world);
        let pixel = [seen.x * 320.0 + 320.0, seen.y * 360.0];
        let collapsed = apply(screen_to_world(centre), mapped(&rows, 1, pixel, seen.z));
        assert!(collapsed.abs_diff_eq(world, 1e-2), "{collapsed} vs {world}");
    }
}
