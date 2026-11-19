//! Each eye's camera position for the pixel shaders, selected per pixel under the collapse.
//!
//! The engine stages the camera world position into the fragment `GlobalConstants` once per pass,
//! from the centred render camera, and pixel shaders measure their view vectors (specular, Fresnel,
//! cloth sheen, environment reflection), camera distances, and the sun-shadow cascade lookup from it.
//! An instance-doubled draw covers both eye halves of the double-wide target in one submission, so
//! the staging cannot be moved per eye the way it is for a split fullscreen pass
//! ([`eye_globals`](crate::stereo::single_pass::eye_globals)), and every material shaded that way is
//! lit as seen from the point between the eyes.
//!
//! [`dxbc_stereo::localize_eye_camera`] rewrites every pixel shader that reads those rows to read
//! them from a mod-owned buffer instead, picking the eye by which side of a boundary its pixel lies
//! on. This module owns that buffer. It is rewritten every time the engine uploads its global
//! constants, from the rows the engine just staged:
//!
//! - inside the routed range under the collapse, each eye's slot holds the staged rows moved to that
//!   eye (the same rows `eye_globals` stages for a split pass), and the boundary is the first pixel
//!   of the right eye's half of whatever target is bound, following the target as the engine rebinds
//!   its viewport;
//! - everywhere else (the shadow, reflection, and environment passes, and any frame without the
//!   collapse), both slots hold the staged rows unchanged and the boundary is past every pixel, so a
//!   rewritten shader reads exactly what it reads from `cb0`.
//!
//! The split fullscreen passes and the water's per-eye draws need nothing extra: each of their pixels
//! lies in the half of the eye whose rows `eye_globals` stages for it, and that eye's slot holds the
//! same values. Nothing is re-issued; the buffer is a constant-buffer write per pass and per target
//! change.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use dxbc_stereo::{
    EYE_CAMERA_BOUNDARY_ROW, EYE_CAMERA_CB_REGISTER, EYE_CAMERA_CB_ROWS, EYE_CAMERA_ROWS_PER_EYE,
    EYE_CAMERA_SOURCE_ROWS, localize_eye_camera,
};
use jc3gi::{graphics_engine::render_engine::RenderEngine, types::math::Vector4};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BUFFER_DESC, D3D11_CPU_ACCESS_WRITE, D3D11_MAP_WRITE_DISCARD,
    D3D11_MAPPED_SUBRESOURCE, D3D11_USAGE_DYNAMIC, ID3D11Buffer, ID3D11DeviceContext,
};

use crate::{
    config::Config,
    stereo::{
        engine_context::EngineContext,
        single_pass::{collapse_active, collapse_eye_boundary, eye_globals, in_gbuffer_range},
    },
};

/// A pixel shader's bytecode rewritten to read each eye's camera rows, or `None` when `enabled` is
/// off or the shader reads none of them. Called for every fragment program the engine creates.
pub(crate) fn rewrite_fragment_program(code: &[u8], enabled: bool) -> Option<Vec<u8>> {
    if !enabled {
        return None;
    }
    match localize_eye_camera(code) {
        Ok(rewritten) => {
            REWRITTEN.fetch_add(1, Ordering::Relaxed);
            Some(rewritten)
        }
        Err(dxbc_stereo::DxbcError::NoCameraPosition) => None,
        Err(error) => {
            DECLINED.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(
                target: "single_pass",
                "eye camera: a pixel shader reading the camera position was left unrewritten: {error}",
            );
            None
        }
    }
}

/// How many pixel shaders have been rewritten since injection, and how many that read the camera
/// position were declined.
pub(crate) fn counts() -> (usize, usize) {
    (
        REWRITTEN.load(Ordering::Relaxed),
        DECLINED.load(Ordering::Relaxed),
    )
}

/// Rewrite the buffer from the rows the engine just staged and bind it. Called from the
/// `SetAllGlobalShaderProgramConstants` detour after the engine's own upload, on the render thread.
pub(crate) fn sync(engine: &RenderEngine) {
    if REWRITTEN.load(Ordering::Relaxed) == 0 {
        return;
    }
    let staged = &engine.m_FPGlobalConstData;
    let mirrored = EYE_CAMERA_SOURCE_ROWS.map(|row| staged[row as usize]);
    let per_eye = (collapse_active()
        && in_gbuffer_range()
        && Config::lock_query(|c| c.stereo.single_pass.pixel_camera_per_eye))
    .then(|| {
        Some([
            eye_globals::eye_rows(staged, 0)?,
            eye_globals::eye_rows(staged, 1)?,
        ])
    })
    .flatten()
    .filter(|eyes| {
        eyes.iter()
            .flatten()
            .all(|row| row.data.iter().all(|v| v.is_finite()))
    });
    with_state(|ctx, state| {
        state.mirrored = mirrored;
        state.per_eye = per_eye;
        let rows = state.rows(collapse_eye_boundary());
        upload(ctx, state, &rows, true);
    });
}

/// Follow the bound target: called when the collapse records a new engine viewport, so the boundary
/// stays on the seam of the target the next draws render into.
///
/// Runs on every viewport bind under the collapse, so it returns before taking any lock unless the
/// pass is eye-split and the boundary moved.
pub(super) fn viewport_changed() {
    if !SPLIT.load(Ordering::Relaxed) {
        return;
    }
    let boundary = collapse_eye_boundary();
    if boundary.map(f32::to_bits) == Some(UPLOADED_BOUNDARY.load(Ordering::Relaxed)) {
        return;
    }
    with_state(|ctx, state| {
        if state.per_eye.is_none() || state.buffer.is_none() {
            return;
        }
        let rows = state.rows(boundary);
        upload(ctx, state, &rows, false);
    });
}

/// Put both slots back to the staged rows when the routed range ends, so a draw between the range and
/// the engine's next upload reads what `cb0` holds.
pub(super) fn range_ended() {
    if !SPLIT.load(Ordering::Relaxed) {
        return;
    }
    with_state(|ctx, state| {
        if state.per_eye.take().is_none() || state.buffer.is_none() {
            return;
        }
        let rows = state.rows(None);
        upload(ctx, state, &rows, false);
    });
}

/// Unbind and release the buffer, so it does not outlive an eject.
pub(super) fn release() {
    if let Some(d3d) = EngineContext::get() {
        // SAFETY: on the eject path with the engine live, under the engine's context mutex.
        d3d.with_lock(|ctx| unsafe {
            ctx.PSSetConstantBuffers(EYE_CAMERA_CB_REGISTER, Some(&[None]));
        });
    }
    let mut state = STATE.lock();
    state.buffer = None;
    state.per_eye = None;
    SPLIT.store(false, Ordering::Relaxed);
    REWRITTEN.store(0, Ordering::Relaxed);
    DECLINED.store(0, Ordering::Relaxed);
}

/// The buffer's size in rows.
const ROWS: usize = EYE_CAMERA_CB_ROWS as usize;
/// Each eye's block of rows.
const EYE_ROWS: usize = EYE_CAMERA_ROWS_PER_EYE as usize;
/// The row holding the boundary.
const BOUNDARY: usize = EYE_CAMERA_BOUNDARY_ROW as usize;

const _: () = assert!(EYE_ROWS == eye_globals::EYE_ROW_COUNT);

/// A boundary no pixel reaches: every pixel reads the left eye's slot.
const NO_BOUNDARY: f32 = f32::MAX;

static REWRITTEN: AtomicUsize = AtomicUsize::new(0);
static DECLINED: AtomicUsize = AtomicUsize::new(0);
static UPLOAD_WARNED: AtomicBool = AtomicBool::new(false);
/// Whether the live buffer holds distinct per-eye rows, and the boundary it holds, so the per-bind
/// and per-range paths can return without locking.
static SPLIT: AtomicBool = AtomicBool::new(false);
static UPLOADED_BOUNDARY: AtomicU32 = AtomicU32::new(0);

static STATE: Mutex<State> = Mutex::new(State {
    buffer: None,
    mirrored: [Vector4 { data: [0.0; 4] }; EYE_ROWS],
    per_eye: None,
    uploaded: [Vector4 { data: [0.0; 4] }; ROWS],
});

struct State {
    buffer: Option<ID3D11Buffer>,
    /// The rows the engine staged at its last upload.
    mirrored: [Vector4; EYE_ROWS],
    /// Each eye's rows, while the current pass is an eye-split one.
    per_eye: Option<[[Vector4; EYE_ROWS]; 2]>,
    /// What the live buffer holds.
    uploaded: [Vector4; ROWS],
}

impl State {
    /// The buffer contents for the current pass, with the eye boundary at `boundary` while the pass
    /// is eye-split. Without a per-eye pass or a boundary, both slots hold the staged rows and no
    /// pixel reaches the boundary.
    fn rows(&self, boundary: Option<f32>) -> [Vector4; ROWS] {
        let (eyes, boundary) = match (self.per_eye, boundary) {
            (Some(eyes), Some(boundary)) => (eyes, boundary),
            _ => ([self.mirrored; 2], NO_BOUNDARY),
        };
        compose(eyes, boundary)
    }
}

/// Lay out both eyes' blocks and the boundary in the order the rewritten shaders read them.
fn compose(eyes: [[Vector4; EYE_ROWS]; 2], boundary: f32) -> [Vector4; ROWS] {
    let mut rows = [Vector4 { data: [0.0; 4] }; ROWS];
    for (eye, block) in eyes.iter().enumerate() {
        rows[eye * EYE_ROWS..(eye + 1) * EYE_ROWS].copy_from_slice(block);
    }
    rows[BOUNDARY].data[0] = boundary;
    rows
}

/// Run `f` on the immediate context under the engine's context mutex, then the buffer state, the lock
/// order every other constant-buffer write in the mod takes. A no-op before the engine's context is
/// live.
fn with_state(f: impl FnOnce(&ID3D11DeviceContext, &mut State)) {
    let Some(d3d) = EngineContext::get() else {
        return;
    };
    d3d.with_lock(|ctx| f(ctx, &mut STATE.lock()));
}

/// Write `rows` into the buffer, creating it on first use, and bind it on the pixel stage when `bind`
/// is set. A failure leaves the previous contents and warns once.
fn upload(ctx: &ID3D11DeviceContext, state: &mut State, rows: &[Vector4; ROWS], bind: bool) {
    // SAFETY: on the render thread under the engine's context mutex, as every other write to the
    // immediate context.
    match unsafe { write(&mut state.buffer, ctx, rows, bind) } {
        Ok(()) => {
            state.uploaded = *rows;
            SPLIT.store(state.per_eye.is_some(), Ordering::Relaxed);
            UPLOADED_BOUNDARY.store(rows[BOUNDARY].data[0].to_bits(), Ordering::Relaxed);
        }
        Err(error) => {
            if !UPLOAD_WARNED.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    target: "single_pass",
                    "eye camera: could not write the per-eye camera rows ({error}); pixel shaders \
                     keep the previous rows",
                );
            }
        }
    }
}

unsafe fn write(
    buffer: &mut Option<ID3D11Buffer>,
    ctx: &ID3D11DeviceContext,
    rows: &[Vector4; ROWS],
    bind: bool,
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
        if bind {
            ctx.PSSetConstantBuffers(EYE_CAMERA_CB_REGISTER, Some(&[Some(buffer.clone())]));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(value: f32) -> Vector4 {
        Vector4 {
            data: [value, value + 1.0, value + 2.0, 1.0],
        }
    }

    fn state(per_eye: bool) -> State {
        State {
            buffer: None,
            mirrored: [row(10.0), row(10.0), row(40.0)],
            per_eye: per_eye.then(|| {
                [
                    [row(9.0), row(9.0), row(39.0)],
                    [row(11.0), row(11.0), row(41.0)],
                ]
            }),
            uploaded: [Vector4 { data: [0.0; 4] }; ROWS],
        }
    }

    /// What a rewritten shader reads for a source row at pixel `x`: the eye is the side of the
    /// boundary, and the row is that eye's copy.
    fn read(rows: &[Vector4; ROWS], x: f32, source: usize) -> Vector4 {
        let eye = usize::from(x >= rows[BOUNDARY].data[0]);
        rows[eye * EYE_ROWS + source]
    }

    /// Outside an eye-split pass every pixel, at any position, reads the rows the engine staged.
    #[test]
    fn a_pass_that_is_not_eye_split_reads_the_staged_rows() {
        let state = state(false);
        for boundary in [None, Some(320.0)] {
            let rows = state.rows(boundary);
            for x in [0.5, 319.5, 320.5, 639.5, 1e9] {
                for (source, expected) in state.mirrored.iter().enumerate() {
                    assert_eq!(read(&rows, x, source).data, expected.data);
                }
            }
        }
    }

    /// Inside an eye-split pass each half reads its own eye's rows, whatever the target's width.
    #[test]
    fn each_half_of_an_eye_split_pass_reads_its_eye() {
        let state = state(true);
        let eyes = state.per_eye.expect("per-eye rows");
        for width in [640.0f32, 320.0, 1234.0] {
            let rows = state.rows(Some(width / 2.0));
            for (x, eye) in [(0.5, 0), (width / 2.0 - 0.5, 0), (width / 2.0 + 0.5, 1)] {
                for (source, expected) in eyes[eye].iter().enumerate() {
                    assert_eq!(read(&rows, x, source).data, expected.data);
                }
            }
        }
    }

    /// Without a recorded viewport there is no seam to split at, so the pass reads the staged rows.
    #[test]
    fn an_eye_split_pass_without_a_boundary_reads_the_staged_rows() {
        let state = state(true);
        let rows = state.rows(None);
        assert_eq!(rows[BOUNDARY].data[0], NO_BOUNDARY);
        for (source, expected) in state.mirrored.iter().enumerate() {
            assert_eq!(read(&rows, 639.5, source).data, expected.data);
        }
    }
}
