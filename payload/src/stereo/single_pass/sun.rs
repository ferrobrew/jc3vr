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
//! The capture-gated diagnostic is deliberately collected from the raw non-indexed draw
//! ([`on_raw_draw`]), not from the pass walk: `DoDraw` can bind and restore state per render block, so
//! a reading around the whole walk is not the state the sun draw ran under.

use std::{
    cell::RefCell,
    ffi::c_void,
    sync::atomic::{AtomicBool, Ordering},
};

use jc3gi::graphics_engine::{graphics_engine::RenderContext, render_engine::RenderPassId};
use parking_lot::Mutex;
use windows::{
    Win32::{
        Foundation::{RECT, S_FALSE, S_OK},
        Graphics::Direct3D11::{
            D3D11_BLEND_DESC, D3D11_CULL_BACK, D3D11_DEPTH_STENCIL_DESC, D3D11_FILL_SOLID,
            D3D11_QUERY_DATA_PIPELINE_STATISTICS, D3D11_QUERY_DESC, D3D11_QUERY_OCCLUSION,
            D3D11_QUERY_PIPELINE_STATISTICS, D3D11_RASTERIZER_DESC, D3D11_TEXTURE2D_DESC,
            D3D11_VIEWPORT, ID3D11BlendState, ID3D11Buffer, ID3D11DepthStencilState,
            ID3D11DepthStencilView, ID3D11DeviceContext, ID3D11DomainShader, ID3D11GeometryShader,
            ID3D11HullShader, ID3D11PixelShader, ID3D11Query, ID3D11RasterizerState,
            ID3D11RenderTargetView, ID3D11Texture2D, ID3D11VertexShader,
        },
    },
    core::Interface,
};

use crate::{
    hooks::draw_count::DRAW_COUNTS,
    screenshot::harness::{
        self,
        bound_buffer::{ShaderStage, read_bound_constant_buffer},
    },
    stereo::single_pass::{
        BOUND_VS_PATCHED, CB13, COLLAPSE_FULL_VIEWPORT, EngineContext,
        cb13::{pin_rows_to_eye, pinned_eye_rows, write_cb13_rows},
        collapse_active, current_pass_id, eye_split_state, in_gbuffer_range,
        per_eye_reissue::PerEyeReissue,
        per_eye_reissue_eye,
        viewport::{
            bind_both_viewport_slots, capture_viewport_slots, eye_half_viewport,
            restore_viewport_slots,
        },
    },
};

const MAX_INVOCATIONS: usize = 4;

/// The most raw sun draws one bracketed pass logs.
const MAX_DRAWS_PER_EYE: usize = 4;

/// Whether `pass` is the sun's render pass.
pub fn is_sun_pass(pass: i16) -> bool {
    pass == RenderPassId::RP_SUN as i16
}

/// Re-drive the sun pass once per eye with `cb13` and both viewport slots pinned to that eye.
/// Returns `None` when the collapse state is not available, in which case the caller draws the pass
/// normally once.
///
/// The gate is deliberately checked here rather than left to the per-eye helper: its inputs are
/// cached across frames, so a stale full viewport or `cb13` row set from an earlier collapsed session
/// would otherwise make it re-issue the sun in a multipass frame.
pub fn reissue_sun_pass(ctx: *const RenderContext, draw: &dyn Fn() -> bool) -> Option<bool> {
    if !reissue_ready(
        collapse_active(),
        in_gbuffer_range(),
        per_eye_reissue_eye().is_some(),
        eye_split_state().is_some(),
    ) {
        return None;
    }
    let full = (*COLLAPSE_FULL_VIEWPORT.lock())?;
    let d3d = EngineContext::get()?;
    let rows = CB13.lock().live_rows()?;
    let saved = capture_viewport_slots(d3d);

    let probe = probe_budget().map(|(capture_id, invocation)| {
        let (status, active_pass, frame) = unsafe { ctx.as_ref() }.map_or((0, 0, 0), |rc| {
            (
                rc.m_RenderStatus.bits(),
                rc.m_ActiveRenderPass,
                rc.m_RenderFrameIndex,
            )
        });
        Probe {
            capture_id,
            invocation,
            generation: crate::testing::visual::metadata().generation,
            status,
            active_pass,
            frame,
        }
    });

    let mut result = true;
    let mut submitted = 0;
    for eye in 0..2 {
        let _reissue = PerEyeReissue::enter(eye, full);
        let pinned = pin_rows_to_eye(&rows, eye);
        if !write_cb13_rows(d3d, &pinned) {
            break;
        }
        bind_both_viewport_slots(d3d, eye_half_viewport(full, eye));
        let gpu_before = probe
            .as_ref()
            .map(|_| DRAW_COUNTS.draw.load(Ordering::Relaxed));
        let armed = probe.as_ref().is_some_and(|probe| {
            let (vp, m_eye) = pinned_eye_rows(&rows, eye);
            ACTIVE.with(|state| {
                let mut state = state.borrow_mut();
                if state.is_some() {
                    return false;
                }
                *state = Some(Capture {
                    eye,
                    invocation: probe.invocation,
                    generation: probe.generation,
                    capture_id: probe.capture_id.clone(),
                    expected: pinned
                        .iter()
                        .flat_map(|row| row.data)
                        .map(f32::to_bits)
                        .collect(),
                    draws: 0,
                    vp,
                    m_eye,
                });
                true
            })
        });
        let context = unsafe { ctx.as_ref() }.map_or(0, |rc| rc.m_Context as usize);
        let _upload_scope = UploadScope::enter(context);
        result &= draw();
        let gpu_draws = gpu_before.map(|before| {
            DRAW_COUNTS
                .draw
                .load(Ordering::Relaxed)
                .wrapping_sub(before)
        });
        let capture = armed.then(|| ACTIVE.with(|state| state.borrow_mut().take()));
        if let (Some(probe), Some(gpu_draws)) = (probe.as_ref(), gpu_draws) {
            tracing::info!(
                target: "single_pass",
                "sun: pass eye={eye} invocation={} generation={} gpu_draws={gpu_draws} raw_draws={} \
                 status={:#x} pass={} frame={}",
                probe.invocation,
                probe.generation,
                capture.flatten().map_or(0, |capture| capture.draws),
                probe.status,
                probe.active_pass,
                probe.frame,
            );
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

pub(super) struct SunRasterGuard {
    context: ID3D11DeviceContext,
    previous: Option<ID3D11RasterizerState>,
}

impl Drop for SunRasterGuard {
    fn drop(&mut self) {
        unsafe { self.context.RSSetState(self.previous.as_ref()) };
    }
}

pub(super) unsafe fn depth_clip_treatment(context: *mut c_void) -> Option<SunRasterGuard> {
    if !raw_sun_pass(current_pass_id())
        || per_eye_reissue_eye() != Some(1)
        || !harness::sun_depth_clip_disabled_right()
    {
        return None;
    }
    let capture_id = harness::diagnostic_capture_id()?;
    let Some(ctx) = (unsafe { <ID3D11DeviceContext as Interface>::from_raw_borrowed(&context) })
    else {
        crate::screenshot::harness::stages::fail_sun_treatment(
            1,
            "sun depth clip: context unavailable",
        );
        return None;
    };
    // RSGetState is a void COM getter; the wrapper returns Err only for a null default state.
    let previous = unsafe { ctx.RSGetState() }.ok();
    let mut desc = D3D11_RASTERIZER_DESC {
        FillMode: D3D11_FILL_SOLID,
        CullMode: D3D11_CULL_BACK,
        FrontCounterClockwise: false.into(),
        DepthBias: 0,
        DepthBiasClamp: 0.0,
        SlopeScaledDepthBias: 0.0,
        DepthClipEnable: true.into(),
        ScissorEnable: false.into(),
        MultisampleEnable: false.into(),
        AntialiasedLineEnable: false.into(),
    };
    if let Some(state) = previous.as_ref() {
        unsafe { state.GetDesc(&mut desc) };
    }
    let original_depth_clip = desc.DepthClipEnable.as_bool();
    desc.DepthClipEnable = false.into();
    let create = || -> anyhow::Result<ID3D11RasterizerState> {
        let device = unsafe { ctx.GetDevice() }?;
        let mut state = None;
        unsafe { device.CreateRasterizerState(&desc, Some(&mut state)) }?;
        state.ok_or_else(|| anyhow::anyhow!("rasterizer state was not created"))
    };
    let state = match create() {
        Ok(state) => state,
        Err(error) => {
            let message = format!("sun depth clip: creating rasterizer state: {error:#}");
            crate::screenshot::harness::stages::fail_sun_treatment(1, &message);
            tracing::warn!(target: "single_pass", %capture_id, "{message}");
            return None;
        }
    };
    unsafe { ctx.RSSetState(&state) };
    let guard = SunRasterGuard {
        context: ctx.clone(),
        previous,
    };
    let actual = unsafe { ctx.RSGetState() };
    let effective = match actual {
        Ok(actual) => {
            let mut actual_desc = D3D11_RASTERIZER_DESC::default();
            unsafe { actual.GetDesc(&mut actual_desc) };
            Some(actual_desc.DepthClipEnable.as_bool())
        }
        Err(error) => {
            let message = format!("sun depth clip: reading bound rasterizer state: {error}");
            crate::screenshot::harness::stages::fail_sun_treatment(1, &message);
            tracing::warn!(target: "single_pass", %capture_id, "{message}");
            None
        }
    };
    tracing::info!(target: "single_pass", %capture_id, eye = 1, original_depth_clip, ?effective,
        "sun probe: disabled-right depth clip treatment");
    if effective == Some(false) {
        crate::screenshot::harness::stages::record_sun_treatment(1);
    } else {
        crate::screenshot::harness::stages::fail_sun_treatment(
            1,
            "sun depth clip: requested rasterizer state is not bound",
        );
    }
    Some(guard)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct UploadObservation {
    context: usize,
    bits: Option<u32>,
}

struct UploadScope(Option<UploadObservation>);

impl UploadScope {
    fn enter(context: usize) -> Self {
        Self(UPLOAD_OBSERVATION.with(|slot| {
            slot.replace(Some(UploadObservation {
                context,
                bits: None,
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

thread_local! {
    static UPLOAD_OBSERVATION: RefCell<Option<UploadObservation>> = const { RefCell::new(None) };
}

/// Collects the actual state of one non-indexed draw issued inside a bracketed sun pass. Called from
/// the D3D draw entry immediately before submission, where the bound state is the draw's own.
///
/// The sun is the only non-indexed `RP_SUN` submission (the live slot-13 census reports exactly one
/// such draw in that pass), so this is deliberately attached to the plain `Draw` path only.
///
/// # Safety
///
/// `context` must be the live immediate context the detoured entry was called on.
pub(super) unsafe fn on_raw_draw(context: *mut c_void, vertex_count: u32, start_vertex: u32) {
    if !raw_sun_pass(current_pass_id()) {
        return;
    }
    ACTIVE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(capture) = state.as_mut() else {
            // SAFETY: the caller supplies the live immediate context.
            unsafe { capture_unwrapped_draw(context) };
            return;
        };
        let index = capture.draws;
        capture.draws += 1;
        if index >= MAX_DRAWS_PER_EYE {
            return;
        }
        // SAFETY: per this function's contract, the pointer is the live COM interface pointer; the
        // borrowed reference ends with this call and takes no reference count.
        let Some(ctx) =
            (unsafe { <ID3D11DeviceContext as Interface>::from_raw_borrowed(&context) })
        else {
            return;
        };
        crate::screenshot::harness::stages::capture_before_sun(ctx, capture.eye);
        let state = render_state(ctx);
        log_bound_cb13(ctx, capture, index);
        log_draw_inputs(ctx, capture);
        begin_gpu_query(ctx, capture, index);
        let mut bound: [Option<ID3D11Buffer>; 1] = [None];
        let mut ps: Option<ID3D11PixelShader> = None;
        let mut blend: Option<ID3D11BlendState> = None;
        let mut blend_factor = [0f32; 4];
        let mut sample_mask = 0u32;
        let mut blend_desc = D3D11_BLEND_DESC::default();
        // SAFETY: every getter only reads this live immediate context's bound state.
        unsafe {
            ctx.VSGetConstantBuffers(13, Some(&mut bound));
            ctx.PSGetShader(&mut ps, None, None);
            ctx.OMGetBlendState(
                Some(&mut blend),
                Some(&mut blend_factor),
                Some(&mut sample_mask),
            );
            if let Some(state) = blend.as_ref() {
                state.GetDesc(&mut blend_desc);
            }
        }
        let owned_ptr = CB13.lock().buffer_ptr().unwrap_or(0);
        let bound_ptr = bound[0]
            .as_ref()
            .map_or(0, |buffer| buffer.as_raw() as usize);
        let target = &blend_desc.RenderTarget[0];
        let vp_text = rows_text(&capture.vp);
        let m_eye_text = rows_text(&capture.m_eye);
        tracing::info!(
            target: "single_pass",
            "sun: raw eye={} invocation={} generation={} draw={index} vertices={vertex_count} \
             start={start_vertex} patched={} gpu_draws={} b13_owned={owned_ptr:#x} \
             b13_bound={bound_ptr:#x} b13_match={} ps={:#x} blend={} src={} dst={} op={} \
             mask={:#x} vp=[{}] meye=[{}] {state}",
            capture.eye,
            capture.invocation,
            capture.generation,
            BOUND_VS_PATCHED.load(Ordering::Relaxed),
            DRAW_COUNTS.draw.load(Ordering::Relaxed),
            owned_ptr != 0 && owned_ptr == bound_ptr,
            ps.as_ref().map_or(0, |shader| shader.as_raw() as usize),
            target.BlendEnable.as_bool(),
            target.SrcBlend.0,
            target.DestBlend.0,
            target.BlendOp.0,
            target.RenderTargetWriteMask,
            vp_text,
            m_eye_text,
        );
    });
}

/// Whether the sun pass may be re-issued: collapsed, inside the camera-scene range, not already
/// inside a per-eye re-issue, and with this frame's per-eye state published. Split out so the mode,
/// nesting, and state combinations are testable without the engine state they read.
fn reissue_ready(collapsed: bool, in_range: bool, nested: bool, state_ready: bool) -> bool {
    collapsed && in_range && !nested && state_ready
}

/// The per-pass probe budget, or `None` outside a requested capture or after the budget is spent.
fn probe_budget() -> Option<(String, usize)> {
    if !harness::sun_inputs_requested() {
        return None;
    }
    let id = harness::diagnostic_capture_id()?;
    let invocation = reserve_probe(&mut PROBE_BUDGET.lock(), &id);
    if invocation == MAX_INVOCATIONS {
        tracing::warn!(target: "single_pass", capture_id = %id, "sun probe: invocation budget exhausted");
    }
    (invocation < MAX_INVOCATIONS).then_some((id, invocation))
}

fn raw_sun_pass(pass: u8) -> bool {
    pass == RenderPassId::RP_SUN as u8
}

fn reserve_probe(budget: &mut (Option<String>, usize), id: &str) -> usize {
    if budget.0.as_deref() != Some(id) {
        *budget = (Some(id.to_owned()), 0);
    }
    let invocation = budget.1;
    budget.1 = budget.1.saturating_add(1);
    invocation
}

unsafe fn capture_unwrapped_draw(context: *mut c_void) {
    let Some((capture_id, invocation)) = probe_budget() else {
        return;
    };
    let Some(ctx) = (unsafe { <ID3D11DeviceContext as Interface>::from_raw_borrowed(&context) })
    else {
        return;
    };
    let eye = crate::stereo::draw_index();
    crate::screenshot::harness::stages::capture_before_sun(ctx, eye);
    let capture = Capture {
        eye,
        capture_id,
        expected: Vec::new(),
        invocation,
        generation: crate::testing::visual::metadata().generation,
        draws: 0,
        vp: [[0.0; 4]; 4],
        m_eye: [[0.0; 4]; 4],
    };
    log_bound_cb13(ctx, &capture, 0);
    log_draw_inputs(ctx, &capture);
    begin_gpu_query(ctx, &capture, 0);
}

fn log_draw_inputs(ctx: &ID3D11DeviceContext, capture: &Capture) {
    let topology = unsafe { ctx.IAGetPrimitiveTopology() };
    let mut vs: Option<ID3D11VertexShader> = None;
    let mut gs: Option<ID3D11GeometryShader> = None;
    let mut hs: Option<ID3D11HullShader> = None;
    let mut ds: Option<ID3D11DomainShader> = None;
    unsafe {
        ctx.VSGetShader(&mut vs, None, None);
        ctx.GSGetShader(&mut gs, None, None);
        ctx.HSGetShader(&mut hs, None, None);
        ctx.DSGetShader(&mut ds, None, None);
    }
    let b1 = read_bound_constant_buffer(ctx, ShaderStage::Vertex, 1, 5);
    let cpu_bits = UPLOAD_OBSERVATION.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|observation| observation.bits)
    });
    let gpu_bits = b1.as_ref().ok().and_then(|buffer| {
        if !buffer.range_known || buffer.range_error.is_some() {
            return None;
        }
        buffer.values.as_ref()?.get(16).map(|value| value.to_bits())
    });
    tracing::info!(target: "single_pass", capture_id = %capture.capture_id, eye = capture.eye,
        invocation = capture.invocation, ?cpu_bits, ?gpu_bits,
        matches = ?cpu_bits.zip(gpu_bits).map(|(cpu, gpu)| cpu == gpu), "sun probe: PushFar upload comparison");
    let (identity_status, identity_name, identity_hash) = match vs.as_ref() {
        None => ("unbound", None, None),
        Some(shader) => match crate::stereo::single_pass::shader_detours::vertex_identity_lookup(
            shader.as_raw() as usize,
        ) {
            Ok(Some(identity)) => (
                "mapped",
                identity.name,
                Some(
                    identity
                        .actual_sha256
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>(),
                ),
            ),
            Ok(None) => ("unmapped", None, None),
            Err(status) => (status, None, None),
        },
    };
    tracing::info!(target: "single_pass", capture_id = %capture.capture_id, eye = capture.eye,
        invocation = capture.invocation, identity_status, ?identity_name, ?identity_hash,
        "sun probe: bound vertex identity");
    match b1 {
        Ok(buffer) => tracing::info!(target: "single_pass", capture_id = %capture.capture_id,
            eye = capture.eye, invocation = capture.invocation, topology = topology.0,
            vs = vs.as_ref().map(|s| s.as_raw() as usize), gs = gs.as_ref().map(|s| s.as_raw() as usize),
            hs = hs.as_ref().map(|s| s.as_raw() as usize), ds = ds.as_ref().map(|s| s.as_raw() as usize),
            b1_pointer = buffer.pointer, first_constant = buffer.first_constant,
            num_constants = buffer.num_constants, range_known = buffer.range_known,
            range_error = ?buffer.range_error, values = ?buffer.values,
            bits = ?buffer.values.as_ref().map(|values| values.iter().map(|value| value.to_bits()).collect::<Vec<_>>()), "sun probe: draw inputs"),
        Err(error) => tracing::warn!(target: "single_pass", capture_id = %capture.capture_id,
            eye = capture.eye, invocation = capture.invocation, "sun probe: VS b1 readback: {error:#}"),
    }
}

struct GpuQuery {
    pipeline: ID3D11Query,
    occlusion: ID3D11Query,
    capture_id: String,
    eye: usize,
    invocation: usize,
    draw: usize,
    frame: Option<i64>,
}

thread_local! {
    static GPU_QUERY: RefCell<Option<GpuQuery>> = const { RefCell::new(None) };
}

fn begin_gpu_query(ctx: &ID3D11DeviceContext, capture: &Capture, draw: usize) {
    let create = || -> anyhow::Result<GpuQuery> {
        let device = unsafe { ctx.GetDevice() }?;
        let make = |kind| -> anyhow::Result<ID3D11Query> {
            let mut query = None;
            unsafe {
                device.CreateQuery(
                    &D3D11_QUERY_DESC {
                        Query: kind,
                        MiscFlags: 0,
                    },
                    Some(&mut query),
                )
            }?;
            query.ok_or_else(|| anyhow::anyhow!("query was not created"))
        };
        Ok(GpuQuery {
            pipeline: make(D3D11_QUERY_PIPELINE_STATISTICS)?,
            occlusion: make(D3D11_QUERY_OCCLUSION)?,
            capture_id: capture.capture_id.clone(),
            eye: capture.eye,
            invocation: capture.invocation,
            draw,
            frame: crate::vr::capture_views().map(|v| v.predicted_display_time_ns),
        })
    };
    GPU_QUERY.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_some() {
            tracing::warn!(target: "single_pass", capture_id = %capture.capture_id, eye = capture.eye, "sun probe: nested GPU query declined");
            return;
        }
        match create() {
            Ok(query) => {
                unsafe { ctx.Begin(&query.pipeline); ctx.Begin(&query.occlusion); }
                *slot = Some(query);
            }
            Err(error) => tracing::warn!(target: "single_pass", capture_id = %capture.capture_id, eye = capture.eye, "sun probe: GPU query creation: {error:#}"),
        }
    });
}

pub(super) unsafe fn after_raw_draw(context: *mut c_void) {
    GPU_QUERY.with(|slot| {
        let Some(query) = slot.borrow_mut().take() else { return; };
        let Some(ctx) = (unsafe { <ID3D11DeviceContext as Interface>::from_raw_borrowed(&context) }) else {
            tracing::warn!(target: "single_pass", capture_id = %query.capture_id, eye = query.eye, "sun probe: GPU query context unavailable");
            return;
        };
        unsafe { ctx.End(&query.pipeline); ctx.End(&query.occlusion); }
        let mut pipeline = D3D11_QUERY_DATA_PIPELINE_STATISTICS::default();
        let mut samples = 0u64;
        let mut pipeline_status = S_FALSE;
        let mut sample_status = S_FALSE;
        for poll in 0..=20 {
            unsafe {
                if pipeline_status == S_FALSE {
                    pipeline_status = (ctx.vtable().GetData)(ctx.as_raw(), query.pipeline.as_raw(), std::ptr::from_mut(&mut pipeline).cast(), std::mem::size_of_val(&pipeline) as u32, 0);
                }
                if sample_status == S_FALSE {
                    sample_status = (ctx.vtable().GetData)(ctx.as_raw(), query.occlusion.as_raw(), std::ptr::from_mut(&mut samples).cast(), std::mem::size_of_val(&samples) as u32, 0);
                }
            }
            if (pipeline_status != S_FALSE && sample_status != S_FALSE) || poll == 20 { break; }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        tracing::info!(target: "single_pass", capture_id = %query.capture_id, eye = query.eye,
            invocation = query.invocation, draw = query.draw, frame = ?query.frame,
            pipeline_status = ?pipeline_status, sample_status = ?sample_status,
            ia_vertices = (pipeline_status == S_OK).then_some(pipeline.IAVertices),
            ia_primitives = (pipeline_status == S_OK).then_some(pipeline.IAPrimitives),
            vs_invocations = (pipeline_status == S_OK).then_some(pipeline.VSInvocations),
            clip_invocations = (pipeline_status == S_OK).then_some(pipeline.CInvocations),
            clip_primitives = (pipeline_status == S_OK).then_some(pipeline.CPrimitives),
            ps_invocations = (pipeline_status == S_OK).then_some(pipeline.PSInvocations),
            samples = (sample_status == S_OK).then_some(samples), "sun probe: GPU draw work");
    });
}

fn log_bound_cb13(ctx: &ID3D11DeviceContext, capture: &Capture, index: usize) {
    let frame = crate::vr::capture_views().map(|views| views.predicted_display_time_ns);
    match read_bound_constant_buffer(
        ctx,
        ShaderStage::Vertex,
        13,
        dxbc_stereo::STEREO_REPROJ_CB_ROWS,
    ) {
        Ok(readback) => {
            let actual = readback.values.as_deref();
            let owned = CB13.lock().buffer_ptr();
            let pointer_matches = owned.is_some_and(|pointer| pointer == readback.pointer);
            let comparable = !capture.expected.is_empty()
                && readback.range_known
                && pointer_matches
                && readback.range_error.is_none()
                && actual.is_some_and(|values| values.len() == capture.expected.len());
            let matches = comparable.then(|| {
                actual
                    .unwrap()
                    .iter()
                    .map(|value| value.to_bits())
                    .eq(capture.expected.iter().copied())
            });
            let words = actual.map(|values| {
                values
                    .iter()
                    .map(|value| format!("{:08x}", value.to_bits()))
                    .collect::<Vec<_>>()
                    .join(",")
            });
            tracing::info!(target: "single_pass", capture_id = %capture.capture_id, ?frame,
                eye = capture.eye, invocation = capture.invocation, draw = index,
                pointer = readback.pointer, ?owned, pointer_matches, byte_width = readback.byte_width,
                first_constant = readback.first_constant, num_constants = readback.num_constants,
                range_known = readback.range_known, range_error = ?readback.range_error,
                expected_words = capture.expected.len(), actual_words = actual.map(|v| v.len()),
                ?matches, ?words, "sun probe: bound VS b13");
        }
        Err(error) => tracing::warn!(target: "single_pass", capture_id = %capture.capture_id,
            ?frame, eye = capture.eye, invocation = capture.invocation, draw = index,
            "sun probe: bound VS b13 readback: {error:#}"),
    }
}

/// The bound viewport slots, scissor rects and enable bit, and depth/stencil state of a draw.
fn render_state(ctx: &ID3D11DeviceContext) -> String {
    let mut viewports = [D3D11_VIEWPORT::default(); 2];
    let mut viewport_count = 2u32;
    let mut scissors = [RECT::default(); 2];
    let mut scissor_count = 2u32;
    let mut raster = D3D11_RASTERIZER_DESC::default();
    let mut depth_stencil: Option<ID3D11DepthStencilState> = None;
    let mut stencil_ref = 0u32;
    let mut depth_stencil_desc = D3D11_DEPTH_STENCIL_DESC::default();
    let mut rtv: Option<ID3D11RenderTargetView> = None;
    let mut dsv: Option<ID3D11DepthStencilView> = None;
    let mut rtv_desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: every getter only reads this live immediate context's bound state.
    unsafe {
        ctx.RSGetViewports(&mut viewport_count, Some(viewports.as_mut_ptr()));
        ctx.RSGetScissorRects(&mut scissor_count, Some(scissors.as_mut_ptr()));
        if let Ok(state) = ctx.RSGetState() {
            state.GetDesc(&mut raster);
        }
        ctx.OMGetDepthStencilState(Some(&mut depth_stencil), Some(&mut stencil_ref));
        if let Some(state) = depth_stencil.as_ref() {
            state.GetDesc(&mut depth_stencil_desc);
        }
        ctx.OMGetRenderTargets(Some(std::slice::from_mut(&mut rtv)), Some(&mut dsv));
        if let Some(view) = rtv.as_ref()
            && let Ok(resource) = view.GetResource()
            && let Ok(texture) = resource.cast::<ID3D11Texture2D>()
        {
            texture.GetDesc(&mut rtv_desc);
        }
    }
    let vp = |v: &D3D11_VIEWPORT| {
        format!(
            "{:.0},{:.0},{:.0}x{:.0}",
            v.TopLeftX, v.TopLeftY, v.Width, v.Height
        )
    };
    let sc = |r: &RECT| format!("{},{},{},{}", r.left, r.top, r.right, r.bottom);
    format!(
        "rtv={:#x} rtv_tex={}x{} dsv={:#x} viewports={viewport_count} vp0={} vp1={} \
         scissors={scissor_count} sc0={} sc1={} scissor={} depth={} write={} func={} stencil={} \
         sref={} sread={:#x} swrite={:#x} front_func={} front_pass={}",
        rtv.as_ref().map_or(0, |view| view.as_raw() as usize),
        rtv_desc.Width,
        rtv_desc.Height,
        dsv.as_ref().map_or(0, |view| view.as_raw() as usize),
        vp(&viewports[0]),
        vp(&viewports[1]),
        sc(&scissors[0]),
        sc(&scissors[1]),
        raster.ScissorEnable.as_bool(),
        depth_stencil_desc.DepthEnable.as_bool(),
        depth_stencil_desc.DepthWriteMask.0,
        depth_stencil_desc.DepthFunc.0,
        depth_stencil_desc.StencilEnable.as_bool(),
        stencil_ref,
        depth_stencil_desc.StencilReadMask,
        depth_stencil_desc.StencilWriteMask,
        depth_stencil_desc.FrontFace.StencilFunc.0,
        depth_stencil_desc.FrontFace.StencilPassOp.0,
    )
}

// The pass currently bracketed on this thread, or `None` outside one.
thread_local! {
    static ACTIVE: RefCell<Option<Capture>> = const { RefCell::new(None) };
}

/// Formats four float4 rows for the raw-draw diagnostic.
fn rows_text(rows: &[[f32; 4]; 4]) -> String {
    rows.iter()
        .map(|row| format!("[{:.5},{:.5},{:.5},{:.5}]", row[0], row[1], row[2], row[3]))
        .collect::<Vec<_>>()
        .join(",")
}

/// One bracketed eye run. `draws` counts the raw non-indexed draws the pass issued.
struct Capture {
    eye: usize,
    capture_id: String,
    expected: Vec<u32>,
    invocation: usize,
    generation: u64,
    draws: usize,
    vp: [[f32; 4]; 4],
    m_eye: [[f32; 4]; 4],
}

/// The per-pass probe identity and the walk-level state, logged once per eye run.
struct Probe {
    capture_id: String,
    invocation: usize,
    generation: u64,
    status: u32,
    active_pass: i32,
    frame: u32,
}

/// Whether the pass has been re-issued at least once this session, so the confirmation is logged
/// once rather than on every frame.
static SUN_REISSUED: AtomicBool = AtomicBool::new(false);

static PROBE_BUDGET: Mutex<(Option<String>, usize)> = Mutex::new((None, 0));

#[cfg(test)]
#[path = "sun_tests.rs"]
mod tests;
