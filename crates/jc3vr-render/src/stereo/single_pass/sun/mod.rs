//! The sun's `RP_SUN` pass, re-driven once per eye under the collapse.
//!
//! The pass has one owner and one non-indexed quad. Its vertex shader takes clip from a baked
//! `cb1[0..3]` matrix, and the shader rewrite already reprojects that family by `M_eye` when it is
//! named `open`; reprojecting the same upload again here would transform it twice. The pass is
//! therefore re-driven with both `cb13` eye slots and both viewport slots pinned to one eye, which
//! gives a patched shader that eye's projection and an unpatched one the centre projection in the
//! eye's half. The pass allowlist in `draw_detours` deliberately does not carry this pass: the fix is
//! the pass's own ownership, not a claim that every environment draw is geometry.
//!
//! The capture harness brackets each re-driven eye through the capture probes
//! ([`crate::probes::CaptureProbes::per_eye_pass`]) and reads this module's upload observation back
//! ([`consumed_far_bits`], [`far_depth_corrected`]).

use std::{
    cell::RefCell,
    ffi::c_void,
    sync::atomic::{AtomicBool, Ordering},
};

use jc3gi::{
    graphics_engine::{graphics_engine::RenderContext, render_engine::RenderPassId},
    types::math::Vector4,
};
use jc3vr_view::stereo::{engine_context::EngineContext, layout::collapse_active};
use windows::{
    Win32::Graphics::Direct3D11::{ID3D11Buffer, ID3D11DeviceContext, ID3D11VertexShader},
    core::Interface,
};

use crate::{
    probes,
    stereo::single_pass::{
        CB13, PATCHED_VS_NAMES,
        cb13::{
            STEREO_CB_TOTAL_ROWS, pin_rows_to_eye, preserve_reprojection_depth, write_cb13_rows,
        },
        eye_split_state, in_gbuffer_range,
        per_eye_reissue::PerEyeReissue,
        per_eye_reissue_eye,
        viewport::{
            bind_both_viewport_slots, capture_viewport_slots, eye_half_viewport,
            restore_viewport_slots,
        },
        viewport_record,
    },
};

/// Whether `pass` is the sun's render pass.
pub(crate) fn is_sun_pass(pass: i16) -> bool {
    pass == RenderPassId::RP_SUN as i16
}

/// Re-drive the sun pass once per eye with `cb13` and both viewport slots pinned to that eye.
/// Returns `None` when the collapse state is not available, in which case the caller draws the pass
/// normally once.
///
/// The gate is deliberately checked here rather than left to the per-eye helper: its inputs are
/// cached across frames, so a stale full viewport or `cb13` row set from an earlier collapsed session
/// would otherwise make it re-issue the sun in a multipass frame.
pub(crate) fn reissue_sun_pass(ctx: *const RenderContext, draw: &dyn Fn() -> bool) -> Option<bool> {
    if !reissue_ready(
        collapse_active(),
        in_gbuffer_range(),
        per_eye_reissue_eye().is_some(),
        eye_split_state().is_some(),
    ) {
        return None;
    }
    let full = viewport_record::scene_viewport()?;
    let d3d = EngineContext::get()?;
    let rows = CB13.lock().live_rows()?;
    let saved = capture_viewport_slots(d3d);
    let mut probe = probes::armed().and_then(|probes| probes.per_eye_pass(ctx));

    let mut result = true;
    let mut submitted = 0;
    for eye in 0..2 {
        let _reissue = PerEyeReissue::enter(eye, full);
        let pinned = pin_rows_to_eye(&rows, eye);
        if !write_cb13_rows(d3d, &pinned) {
            break;
        }
        bind_both_viewport_slots(d3d, eye_half_viewport(full, eye));
        if let Some(probe) = probe.as_mut() {
            probe.begin_eye(eye, &rows, &pinned);
        }
        let context = unsafe { ctx.as_ref() }.map_or(0, |rc| rc.m_Context as usize);
        let _upload_scope = UploadScope::enter(context);
        result &= draw();
        if let Some(probe) = probe.as_mut() {
            probe.end_eye(eye);
        }
        submitted += 1;
    }

    if !write_cb13_rows(d3d, &rows) {
        tracing::warn!(
            target: "single_pass",
            "sun: cb13 restore failed; later passes may render from one eye"
        );
    }
    restore_viewport_slots(d3d, saved);
    if submitted == 0 {
        return None;
    }
    if !SUN_REISSUED.swap(true, Ordering::Relaxed) {
        tracing::info!(target: "single_pass", "sun: re-issued RP_SUN once per eye");
    }
    Some(result)
}

pub(super) struct SunDepthGuard {
    d3d: EngineContext,
    original: [Vector4; STEREO_CB_TOTAL_ROWS],
}

impl Drop for SunDepthGuard {
    fn drop(&mut self) {
        if !write_cb13_rows(self.d3d, &self.original) {
            tracing::warn!(target: "single_pass", "sun: restoring pinned cb13 after far-depth draw failed");
        }
    }
}

fn preserve_far_depth_ready(bits: Option<u32>, shader_is_open: bool) -> bool {
    bits.map(f32::from_bits)
        .is_some_and(|value| value.is_finite() && value != 0.0)
        && shader_is_open
}

pub(super) unsafe fn preserve_far_depth(context: *mut c_void) -> Option<SunDepthGuard> {
    let eye = per_eye_reissue_eye()?;
    let bits = UPLOAD_OBSERVATION.with(|slot| {
        slot.borrow_mut().as_mut().and_then(|value| {
            value.consumed = value.bits.take();
            value.corrected = false;
            value.consumed
        })
    });
    let ctx = unsafe { <ID3D11DeviceContext as Interface>::from_raw_borrowed(&context) }?;
    let mut vs: Option<ID3D11VertexShader> = None;
    let mut bound: [Option<ID3D11Buffer>; 1] = [None];
    unsafe {
        ctx.VSGetShader(&mut vs, None, None);
        ctx.VSGetConstantBuffers(13, Some(&mut bound));
    }
    let owned = CB13.lock().buffer_ptr()?;
    if bound[0]
        .as_ref()
        .is_none_or(|buffer| buffer.as_raw() as usize != owned)
    {
        return None;
    }
    let shader_is_open = vs.as_ref().is_some_and(|shader| {
        PATCHED_VS_NAMES
            .lock()
            .get(&(shader.as_raw() as usize))
            .is_some_and(|name| name == "open")
    });
    if !preserve_far_depth_ready(bits, shader_is_open) {
        return None;
    }
    let d3d = EngineContext::get()?;
    if !d3d.with_lock(|engine_ctx| engine_ctx.as_raw() == context) {
        return None;
    }
    let rows = CB13.lock().live_rows()?;
    let original = pin_rows_to_eye(&rows, eye);
    let mut corrected = original;
    preserve_reprojection_depth(&mut corrected);
    if !write_cb13_rows(d3d, &corrected) {
        return None;
    }
    UPLOAD_OBSERVATION.with(|slot| {
        if let Some(observation) = slot.borrow_mut().as_mut() {
            observation.corrected = true;
        }
    });
    Some(SunDepthGuard { d3d, original })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct UploadObservation {
    context: usize,
    bits: Option<u32>,
    consumed: Option<u32>,
    corrected: bool,
}

struct UploadScope(Option<UploadObservation>);

impl UploadScope {
    fn enter(context: usize) -> Self {
        Self(UPLOAD_OBSERVATION.with(|slot| {
            slot.replace(Some(UploadObservation {
                context,
                bits: None,
                consumed: None,
                corrected: false,
            }))
        }))
    }
}

impl Drop for UploadScope {
    fn drop(&mut self) {
        UPLOAD_OBSERVATION.with(|slot| {
            slot.replace(self.0.take());
        });
    }
}

fn upload_offset(
    expected_context: usize,
    context: usize,
    slot: i32,
    start: u32,
    count: u32,
) -> Option<usize> {
    if expected_context == 0
        || expected_context != context
        || slot != 1
        || start > 4
        || start.checked_add(count)? <= 4
    {
        return None;
    }
    usize::try_from(4 - start).ok()?.checked_mul(4)
}

pub(super) unsafe fn observe_vertex_upload(
    context: *mut c_void,
    slot: i32,
    start: u32,
    data: *const f32,
    count: u32,
) {
    UPLOAD_OBSERVATION.with(|observation| {
        let mut observation = observation.borrow_mut();
        let Some(observation) = observation.as_mut() else {
            return;
        };
        let Some(offset) = upload_offset(observation.context, context as usize, slot, start, count)
        else {
            return;
        };
        observation.bits = if data.is_null() {
            None
        } else {
            Some(unsafe { *data.add(offset) }.to_bits())
        };
    });
}

/// The far-plane bits this thread's re-driven sun draw consumed from the observed `cb1` upload.
pub(crate) fn consumed_far_bits() -> Option<u32> {
    UPLOAD_OBSERVATION.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|observation| observation.consumed)
    })
}

/// Whether this thread's re-driven sun draw took the far-depth correction.
pub(crate) fn far_depth_corrected() -> bool {
    UPLOAD_OBSERVATION.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|observation| observation.corrected)
    })
}

thread_local! {
    static UPLOAD_OBSERVATION: RefCell<Option<UploadObservation>> = const { RefCell::new(None) };
}

/// Whether the sun pass may be re-issued: collapsed, inside the G-buffer range, not already
/// inside a per-eye re-issue, and with this frame's per-eye state published. Split out so the mode,
/// nesting, and state combinations are testable without the engine state they read.
fn reissue_ready(collapsed: bool, in_range: bool, nested: bool, state_ready: bool) -> bool {
    collapsed && in_range && !nested && state_ready
}

/// Whether the pass has been re-issued at least once this session, so the confirmation is logged
/// once rather than on every frame.
static SUN_REISSUED: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
mod tests;
