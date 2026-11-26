//! Each eye's copy of the vertex `GlobalConstants`, bound for the unpatched draws the collapse
//! re-issues once per eye.
//!
//! An unpatched vertex shader under the collapse is drawn once into each eye's half, but it reads the
//! engine's vertex `GlobalConstants` (`cb0`), which hold the centred render camera: its position, its
//! orientation, and eye 0's projection. With symmetric, parallel eyes that costs only the parallax
//! between an eye and the centre, which the re-issue accepted. With asymmetric frusta the right eye
//! is drawn through the left eye's projection, and with canted displays both eyes are drawn without
//! their cant, so whatever such a shader places from those rows (the halos, the sky and cloud
//! families the remap does not claim, the particles that billboard from the camera) lands in the
//! wrong place within each eye's own image.
//!
//! This module keeps two copies of the block, one per eye, with the per-view rows an eye-sized render
//! of that eye would carry: the full view-projection, the camera position and axis, the render
//! camera's position, and the offset view-projection. The previous-frame rows and the frustum planes
//! stay as the engine staged them, since an eye-sized render stages the centred camera's there too
//! (the velocity pass's previous camera, and the cull camera's planes). The copies are rewritten every
//! time the engine uploads its global constants inside the routed range under the collapse, from the
//! rows the `cb13` upload has just derived, and an unpatched per-eye re-issue binds each eye's copy at
//! vertex slot 0 for that eye's submission and puts the engine's buffer back afterwards.

use crate::{
    stereo::{engine_context::EngineContext, single_pass::*},
    vr::render_params,
};

/// Bound around an unpatched per-eye re-issue: [`bind`](Self::bind) puts an eye's copy at vertex slot
/// 0, and dropping it puts back the buffer the engine had bound.
pub(super) struct EyeVertexGlobals {
    d3d: EngineContext,
    buffers: [ID3D11Buffer; 2],
    saved: Option<ID3D11Buffer>,
}

impl EyeVertexGlobals {
    /// Start a binding when the switch is on and both copies hold this pass's rows; `None` leaves the
    /// draw reading the engine's buffer.
    pub(super) fn begin() -> Option<Self> {
        if !config_flags().has(Flag::VertexGlobalsPerEye) {
            return None;
        }
        let buffers = STATE.lock().current()?;
        let d3d = EngineContext::get()?;
        let mut saved = [None];
        // SAFETY: `VSGetConstantBuffers` writes an owned reference (or null) into `saved`.
        d3d.with_lock(|ctx| unsafe { ctx.VSGetConstantBuffers(GLOBALS_SLOT, Some(&mut saved)) });
        let [saved] = saved;
        Some(Self {
            d3d,
            buffers,
            saved,
        })
    }

    /// Bind `eye`'s copy for the next submission.
    pub(super) fn bind(&self, eye: usize) {
        let buffer = Some(self.buffers[eye].clone());
        // SAFETY: the buffer is a live constant buffer of the engine's block size, created on this
        // device.
        self.d3d
            .with_lock(|ctx| unsafe { ctx.VSSetConstantBuffers(GLOBALS_SLOT, Some(&[buffer])) });
    }
}

impl Drop for EyeVertexGlobals {
    fn drop(&mut self) {
        let saved = self.saved.take();
        // SAFETY: puts back exactly the buffer `begin` found bound.
        self.d3d
            .with_lock(|ctx| unsafe { ctx.VSSetConstantBuffers(GLOBALS_SLOT, Some(&[saved])) });
    }
}

/// Rewrite both eyes' copies from the block the engine just staged, with the per-eye rows of
/// `cb13_rows` (the dual-eye rows [`mirror_and_bind_cb13`] derived for this upload). Called under the
/// engine's context mutex on the render thread, inside the routed range under the collapse.
pub(super) fn sync(
    engine: &RenderEngine,
    cb13_rows: &[Vector4; STEREO_CB_TOTAL_ROWS],
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
) {
    let mut state = STATE.lock();
    state.valid = false;
    if !config_flags().has(Flag::VertexGlobalsPerEye) {
        return;
    }
    let Some(blocks) = eye_blocks(&engine.m_VPGlobalConstData, cb13_rows) else {
        return;
    };
    for (eye, block) in blocks.iter().enumerate() {
        // SAFETY: on the render thread under the engine's context mutex.
        if let Err(error) = unsafe { state.write(eye, device, context, block) } {
            if !WRITE_WARNED.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    target: "single_pass",
                    "per-eye vertex globals: {error}; unpatched re-issues keep the centred rows"
                );
            }
            return;
        }
    }
    state.valid = true;
}

/// Mark both copies stale, so no re-issue binds rows from another pass's upload. Called on every
/// global-constant upload that [`sync`] does not cover.
pub(super) fn invalidate() {
    STATE.lock().valid = false;
}

/// Unbind nothing (the copies are only ever bound for the length of a draw) and release the buffers.
/// Called from the eject path.
pub(super) fn release() {
    let mut state = STATE.lock();
    state.buffers = [None, None];
    state.valid = false;
}

/// The vertex constant-buffer slot the engine binds its `GlobalConstants` at.
const GLOBALS_SLOT: u32 = 0;

/// The rows of the vertex `GlobalConstants` that are the render camera's, by role.
const FULL_VIEW_PROJECTION_ROWS: [usize; 4] = [0, 1, 2, 3];
const CAMERA_POSITION_ROW: usize = 4;
const CAMERA_AXIS_ROW: usize = 5;
const RENDER_CAMERA_POSITION_ROW: usize = 9;
const OFFSET_VIEW_PROJECTION_ROWS: [usize; 4] = [29, 30, 31, 32];

/// The size of the vertex `GlobalConstants` block, in rows.
const BLOCK_ROWS: usize = 49;

type Block = [Vector4; BLOCK_ROWS];

/// Each eye's copy of `staged`, or `None` while an eye's parameters or the centred camera are not
/// available.
fn eye_blocks(staged: &Block, cb13_rows: &[Vector4; STEREO_CB_TOTAL_ROWS]) -> Option<[Block; 2]> {
    let center = glam::Mat4::from(crate::stereo::STEREO_STATE.lock().center_transform?);
    let mut blocks = [*staged; 2];
    for (eye, block) in blocks.iter_mut().enumerate() {
        let params = render_params(eye)?;
        let eye_world = params.resolve(center);
        let view_projection =
            Matrix4::from(glam::Mat4::from(params.projection_reverse_z) * eye_world.inverse());
        let axis = eye_world.z_axis.truncate();
        write_eye_rows(
            block,
            EyeRows {
                view_projection,
                axis,
                cb13: cb13_rows,
                eye,
            },
        );
    }
    blocks
        .iter()
        .all(|block| {
            block
                .iter()
                .all(|row| row.data.iter().all(|v| v.is_finite()))
        })
        .then_some(blocks)
}

/// What one eye's per-view rows are built from.
struct EyeRows<'a> {
    /// The eye's full view-projection, in the engine's row-major layout.
    view_projection: Matrix4,
    /// The eye camera's `z` axis, which the engine stages beside the position.
    axis: glam::Vec3,
    /// The dual-eye `cb13` rows, which hold the eye's offset view-projection, camera position, and
    /// copy of the render camera's position.
    cb13: &'a [Vector4; STEREO_CB_TOTAL_ROWS],
    eye: usize,
}

/// Overwrite `block`'s per-view rows with one eye's.
fn write_eye_rows(block: &mut Block, rows: EyeRows<'_>) {
    for (r, &row) in FULL_VIEW_PROJECTION_ROWS.iter().enumerate() {
        block[row]
            .data
            .copy_from_slice(&rows.view_projection.data[r * 4..r * 4 + 4]);
    }
    let stride = PER_EYE_ROW_COUNT;
    for (r, &row) in OFFSET_VIEW_PROJECTION_ROWS.iter().enumerate() {
        block[row] = rows.cb13[rows.eye * stride + r];
    }
    block[CAMERA_POSITION_ROW] = rows.cb13[rows.eye * stride + 4];
    let w = block[CAMERA_AXIS_ROW].data[3];
    block[CAMERA_AXIS_ROW].data = [rows.axis.x, rows.axis.y, rows.axis.z, w];
    block[RENDER_CAMERA_POSITION_ROW] = rows.cb13[clip_origin_row(rows.eye)];
}

/// Whether a write has already failed, so a per-pass path cannot flood the log.
static WRITE_WARNED: AtomicBool = AtomicBool::new(false);

static STATE: Mutex<State> = Mutex::new(State {
    buffers: [None, None],
    valid: false,
});

struct State {
    buffers: [Option<ID3D11Buffer>; 2],
    /// Whether both buffers hold the rows of the upload in force.
    valid: bool,
}

impl State {
    fn current(&self) -> Option<[ID3D11Buffer; 2]> {
        if !self.valid {
            return None;
        }
        Some([self.buffers[0].clone()?, self.buffers[1].clone()?])
    }

    /// Write `block` into `eye`'s buffer, creating it on first use.
    unsafe fn write(
        &mut self,
        eye: usize,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        block: &Block,
    ) -> Result<(), windows::core::Error> {
        let buffer = match &self.buffers[eye] {
            Some(buffer) => buffer,
            None => {
                let mut created = None;
                unsafe {
                    device.CreateBuffer(
                        &D3D11_BUFFER_DESC {
                            ByteWidth: std::mem::size_of::<Block>() as u32,
                            Usage: D3D11_USAGE_DYNAMIC,
                            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                            ..Default::default()
                        },
                        Some(&D3D11_SUBRESOURCE_DATA {
                            pSysMem: block.as_ptr().cast(),
                            ..Default::default()
                        }),
                        Some(&mut created),
                    )?;
                }
                self.buffers[eye].insert(created.expect("CreateBuffer returned Ok with no buffer"))
            }
        };
        unsafe {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            context.Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))?;
            std::ptr::copy_nonoverlapping(block.as_ptr(), mapped.pData.cast(), BLOCK_ROWS);
            context.Unmap(buffer, 0);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(value: f32) -> Vector4 {
        Vector4 { data: [value; 4] }
    }

    /// Only the per-view rows change: the eye's view-projection, the cb13 rows that are the eye's, and
    /// the camera axis, which keeps the engine's `w`. The previous-frame rows and everything else stay.
    #[test]
    fn only_the_per_view_rows_are_the_eyes() {
        let staged: Block = std::array::from_fn(|r| row(r as f32));
        let cb13: [Vector4; STEREO_CB_TOTAL_ROWS] = std::array::from_fn(|r| row(100.0 + r as f32));
        let view_projection = Matrix4 {
            data: std::array::from_fn(|i| 1000.0 + i as f32),
        };
        for eye in 0..2 {
            let mut block = staged;
            write_eye_rows(
                &mut block,
                EyeRows {
                    view_projection,
                    axis: glam::Vec3::new(-1.0, 0.5, 0.25),
                    cb13: &cb13,
                    eye,
                },
            );
            for r in 0..4 {
                assert_eq!(block[r].data, view_projection.data[r * 4..r * 4 + 4]);
                assert_eq!(block[29 + r].data, cb13[eye * 5 + r].data);
            }
            assert_eq!(block[4].data, cb13[eye * 5 + 4].data);
            assert_eq!(block[5].data, [-1.0, 0.5, 0.25, 5.0]);
            assert_eq!(block[9].data, cb13[clip_origin_row(eye)].data);
            for r in (6..9).chain(10..29).chain(33..BLOCK_ROWS) {
                assert_eq!(block[r].data, staged[r].data, "row {r}");
            }
        }
    }
}
