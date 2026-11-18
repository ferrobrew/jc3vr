//! Capture-gated diagnostic: the GPU work each WaveWorks per-eye water draw submits, measured with
//! public D3D11 pipeline-statistics and occlusion queries, plus the target, viewport, and pipeline
//! state it submits under. Armed only while the screenshot harness captures eyes, bounded to a fixed
//! invocation budget, and stamped with the capture frame's predicted display time so the evidence
//! stays attached to the capture it came from.

use std::{
    cell::RefCell,
    ffi::c_void,
    marker::PhantomData,
    rc::Rc,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use windows::{
    Win32::{
        Foundation::RECT,
        Graphics::Direct3D11::{
            D3D11_BLEND_DESC, D3D11_BLEND_ONE, D3D11_BLEND_OP_ADD, D3D11_BLEND_ZERO,
            D3D11_COLOR_WRITE_ENABLE_ALL, D3D11_COMPARISON_ALWAYS, D3D11_COMPARISON_LESS,
            D3D11_CULL_BACK, D3D11_DEPTH_STENCIL_DESC, D3D11_DEPTH_STENCILOP_DESC,
            D3D11_DEPTH_WRITE_MASK_ALL, D3D11_FILL_SOLID, D3D11_RASTERIZER_DESC,
            D3D11_RENDER_TARGET_BLEND_DESC, D3D11_STENCIL_OP_KEEP, D3D11_TEXTURE2D_DESC,
            D3D11_VIEWPORT, ID3D11DepthStencilView, ID3D11DeviceContext, ID3D11RenderTargetView,
            ID3D11Texture2D,
        },
    },
    core::{Error as WindowsError, HRESULT, Interface},
};

use crate::stereo::single_pass::EngineContext;

mod constants;
mod ia;
mod inputs;
mod reflection;
mod work;

pub(crate) use inputs::{begin_water_input_capture, reflection_treatment};
pub(crate) use reflection::reflection_producer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub(crate) enum RawDrawKind {
    Indexed,
    NonIndexed,
    IndexedInstanced,
    IndexedInstancedIndirect,
    InstancedIndirect,
}

impl RawDrawKind {
    const COUNT: usize = 5;

    fn name(self) -> &'static str {
        match self {
            Self::Indexed => "indexed",
            Self::NonIndexed => "non_indexed",
            Self::IndexedInstanced => "indexed_instanced",
            Self::IndexedInstancedIndirect => "indexed_instanced_indirect",
            Self::InstancedIndirect => "instanced_indirect",
        }
    }
}

/// # Safety
///
/// `context` must be the live immediate context the detoured entry was called on.
pub(crate) unsafe fn raw_entry(kind: RawDrawKind, context: *mut c_void) {
    // SAFETY: per this function's contract, `context` is the live immediate context the draw entry
    // was called on.
    unsafe { reflection::on_raw_draw(kind, context) };
    unsafe { inputs::on_raw_draw(context) };
    RAW_CAPTURE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(capture) = state.as_mut() else {
            return;
        };
        let count = &mut capture.counts[kind as usize];
        *count += 1;
        let count = *count;
        let index = capture.logged;
        capture.logged += 1;
        capture.close_open();
        let Some(slot) = raw_log_slot(index) else {
            return;
        };
        // SAFETY: per this function's contract, the pointer is the live COM interface pointer; the
        // borrowed reference ends with this call and takes no reference count.
        let Some(ctx) =
            (unsafe { <ID3D11DeviceContext as Interface>::from_raw_borrowed(&context) })
        else {
            return;
        };
        // SAFETY: `ctx` is the live immediate context; the getters only read bound state.
        let bound = unsafe { work::bound_state(ctx) };
        if capture.constants_logged < RAW_CONSTANTS_CAP && bound.dsv_bound {
            capture.constants_logged += 1;
            // SAFETY: `ctx` is the live immediate context.
            unsafe { ia::capture(ctx, capture.context) };
            // SAFETY: `ctx` is the live immediate context.
            unsafe { constants::capture(ctx, capture.context) };
        }
        capture.begin_segment(
            ctx,
            work::SegmentMeta {
                index,
                kind,
                scene: bound.scene,
            },
        );
        log_raw(ctx, capture, kind, count, slot);
    });
}

pub(crate) struct WaterGpuProbe {
    d3d: EngineContext,
    context: ProbeContext,
    _guard: Option<ActiveGuard>,
}

impl WaterGpuProbe {
    pub(crate) fn begin(pass: i32, eye: usize) -> Option<Self> {
        if !crate::screenshot::harness::wants_eyes() {
            return None;
        }
        let visual = crate::testing::visual::metadata();
        if !visual.ready {
            return None;
        }
        let invocation = INVOCATIONS.fetch_add(1, Ordering::Relaxed);
        if admit(invocation).is_none() {
            if !BUDGET_EXHAUSTED.swap(true, Ordering::Relaxed) {
                tracing::info!(
                    target: "water_probe",
                    "water probe: invocation budget of {MAX_INVOCATIONS} spent; later captures are \
                     not probed"
                );
            }
            return None;
        }
        let context = ProbeContext {
            pass,
            eye,
            invocation,
            frame: FrameStamp {
                predicted_display_time_ns: crate::vr::capture_views()
                    .map(|views| views.predicted_display_time_ns),
                visual_generation: visual.generation,
            },
        };
        let d3d = EngineContext::get()?;
        match d3d.with_lock(|ctx| Self::start(ctx, d3d, context)) {
            Ok(probe) => Some(probe),
            Err(e) => {
                tracing::warn!(target: "water_probe", "{e}");
                None
            }
        }
    }

    pub(crate) fn finish(mut self) {
        let capture = self._guard.take().and_then(ActiveGuard::disarm);
        let Some(mut capture) = capture else {
            return;
        };
        capture.close_open();
        let works = work::resolve(self.d3d, &capture.segments);
        for segment_work in &works {
            segment_work.log(self.context);
        }
        work::log_scene_aggregates(self.context, &works);
        work::log_aggregate(self.context, &works, capture.logged);
        log_raw_counts(&capture);
    }

    fn start(
        ctx: &ID3D11DeviceContext,
        d3d: EngineContext,
        context: ProbeContext,
    ) -> Result<Self, ProbeError> {
        let guard = ActiveGuard::arm(context).ok_or(ProbeError::AlreadyActive)?;
        // SAFETY: `ctx` is the live immediate context; `GetDevice` only reads its device.
        let device = unsafe { ctx.GetDevice() }.map_err(ProbeError::Device)?;
        let queries = work::create_pairs(&device, RAW_LOG_CAP)?;
        guard.install(queries);
        log_state(ctx, context);
        Ok(Self {
            d3d,
            context,
            _guard: Some(guard),
        })
    }
}

const MAX_INVOCATIONS: usize = 64;

const RAW_LOG_CAP: usize = 32;
const RAW_CONSTANTS_CAP: usize = 16;

static INVOCATIONS: AtomicUsize = AtomicUsize::new(0);
static BUDGET_EXHAUSTED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static RAW_CAPTURE: RefCell<Option<RawCapture>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy)]
struct ProbeContext {
    pass: i32,
    eye: usize,
    invocation: usize,
    frame: FrameStamp,
}

#[derive(Clone, Copy)]
struct FrameStamp {
    predicted_display_time_ns: Option<i64>,
    visual_generation: u64,
}

impl FrameStamp {
    fn display_time(self) -> String {
        self.predicted_display_time_ns
            .map_or_else(|| "none".to_owned(), |ns| ns.to_string())
    }
}

struct RawCapture {
    context: ProbeContext,
    counts: [usize; RawDrawKind::COUNT],
    logged: usize,
    constants_logged: usize,
    spare: Vec<work::QueryPair>,
    segments: Vec<work::Segment>,
    open: Option<work::Segment>,
}

impl RawCapture {
    fn begin_segment(&mut self, ctx: &ID3D11DeviceContext, meta: work::SegmentMeta) {
        self.close_open();
        let Some(pair) = self.spare.pop() else {
            return;
        };
        self.open = Some(work::Segment::begin(ctx, meta, pair));
    }

    fn close_open(&mut self) {
        let Some(open) = self.open.take() else {
            return;
        };
        open.end();
        self.segments.push(open);
    }
}

impl Drop for RawCapture {
    fn drop(&mut self) {
        self.close_open();
    }
}

struct ActiveGuard {
    _not_send: PhantomData<Rc<()>>,
}

impl ActiveGuard {
    fn arm(context: ProbeContext) -> Option<Self> {
        RAW_CAPTURE.with(|state| {
            let mut state = state.borrow_mut();
            if state.is_some() {
                return None;
            }
            *state = Some(RawCapture {
                context,
                counts: [0; RawDrawKind::COUNT],
                logged: 0,
                constants_logged: 0,
                spare: Vec::new(),
                segments: Vec::new(),
                open: None,
            });
            Some(Self {
                _not_send: PhantomData,
            })
        })
    }

    fn install(&self, queries: Vec<work::QueryPair>) {
        RAW_CAPTURE.with(|state| {
            if let Some(capture) = state.borrow_mut().as_mut() {
                capture.spare = queries;
            }
        });
    }

    fn disarm(self) -> Option<RawCapture> {
        RAW_CAPTURE.with(|state| state.borrow_mut().take())
    }
}

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        RAW_CAPTURE.with(|state| *state.borrow_mut() = None);
    }
}

#[derive(Debug)]
enum ProbeError {
    Device(WindowsError),
    CreateQuery {
        query: &'static str,
        error: WindowsError,
    },
    QueryMissing {
        query: &'static str,
    },
    GetData(HRESULT),
    AlreadyActive,
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Device(error) => write!(f, "water probe: getting the device: {error}"),
            Self::CreateQuery { query, error } => {
                write!(f, "water probe: creating the {query} query: {error}")
            }
            Self::QueryMissing { query } => {
                write!(f, "water probe: the {query} query was not created")
            }
            Self::GetData(hr) => write!(
                f,
                "water probe: resolving a query: {}",
                WindowsError::from_hresult(*hr)
            ),
            Self::AlreadyActive => {
                write!(f, "water probe: a probe is already active on this thread")
            }
        }
    }
}

fn admit(invocation: usize) -> Option<usize> {
    (invocation < MAX_INVOCATIONS).then_some(invocation)
}

fn raw_log_slot(logged: usize) -> Option<usize> {
    (logged < RAW_LOG_CAP).then_some(logged)
}

fn log_state(ctx: &ID3D11DeviceContext, context: ProbeContext) {
    let mut rtv: Option<ID3D11RenderTargetView> = None;
    let mut dsv: Option<ID3D11DepthStencilView> = None;
    // SAFETY: the slice and out-parameters are sized for the one target slot read.
    unsafe {
        ctx.OMGetRenderTargets(Some(std::slice::from_mut(&mut rtv)), Some(&mut dsv));
    }
    let mut viewports = [D3D11_VIEWPORT::default(); 2];
    let mut count = 2u32;
    // SAFETY: `count` is the length of `viewports`, as `RSGetViewports` requires.
    unsafe { ctx.RSGetViewports(&mut count, Some(viewports.as_mut_ptr())) };
    // The target's own allocation, beside the viewport that writes into it: a half-width viewport
    // over a double-wide target is one eye's subrect, while a full-width viewport over the same
    // target is a whole-frame write.
    let target = rtv
        .as_ref()
        .and_then(|view| unsafe { view.GetResource() }.ok())
        .and_then(|resource| resource.cast::<ID3D11Texture2D>().ok());
    let mut target_desc = D3D11_TEXTURE2D_DESC::default();
    if let Some(target) = &target {
        // SAFETY: `target` is live; `GetDesc` only writes the out parameter.
        unsafe { target.GetDesc(&mut target_desc) };
    }
    tracing::info!(
        target: "water_probe",
        "water probe: pre-draw pass={} eye={} invocation={} predicted_display_time_ns={} \
         generation={} rtv={:#x} rtv_tex={}x{}/{:?} dsv={:#x} viewports={count} vp0={} vp1={}",
        context.pass,
        context.eye,
        context.invocation,
        context.frame.display_time(),
        context.frame.visual_generation,
        pointer(&rtv),
        target_desc.Width,
        target_desc.Height,
        target_desc.Format,
        pointer(&dsv),
        viewport_text(&viewports[0]),
        viewport_text(&viewports[1]),
    );
}

pub(super) fn viewport_text(v: &D3D11_VIEWPORT) -> String {
    format!(
        "({:.1},{:.1}) {:.1}x{:.1} depth={:.4}..{:.4}",
        v.TopLeftX, v.TopLeftY, v.Width, v.Height, v.MinDepth, v.MaxDepth
    )
}

fn scissor_text(r: &RECT) -> String {
    format!("({},{})-({},{})", r.left, r.top, r.right, r.bottom)
}

fn log_raw_counts(capture: &RawCapture) {
    let context = capture.context;
    let count = |kind: RawDrawKind| capture.counts[kind as usize];
    tracing::info!(
        target: "water_probe",
        "water probe: raw submissions pass={} eye={} invocation={} predicted_display_time_ns={} \
         generation={} indexed={} non_indexed={} indexed_instanced={} indexed_instanced_indirect={} \
         instanced_indirect={} (direct DrawInstanced not hooked)",
        context.pass,
        context.eye,
        context.invocation,
        context.frame.display_time(),
        context.frame.visual_generation,
        count(RawDrawKind::Indexed),
        count(RawDrawKind::NonIndexed),
        count(RawDrawKind::IndexedInstanced),
        count(RawDrawKind::IndexedInstancedIndirect),
        count(RawDrawKind::InstancedIndirect),
    );
}

fn log_raw(
    ctx: &ID3D11DeviceContext,
    capture: &RawCapture,
    kind: RawDrawKind,
    count: usize,
    index: usize,
) {
    let context = capture.context;
    let mut viewports = [D3D11_VIEWPORT::default(); 2];
    let mut viewport_count = 2u32;
    let mut scissors = [RECT::default(); 2];
    let mut scissor_count = 2u32;
    let mut raster = default_raster_desc();
    let mut depth_stencil = None;
    let mut stencil_ref = 0u32;
    let mut depth_stencil_desc = default_depth_stencil_desc();
    let mut blend = None;
    let mut blend_factor = [0f32; 4];
    let mut sample_mask = 0u32;
    let mut blend_desc = default_blend_desc();
    let mut rtv: Option<ID3D11RenderTargetView> = None;
    let mut dsv: Option<ID3D11DepthStencilView> = None;
    let mut vs = None;
    let mut hs = None;
    let mut ds = None;
    let mut gs = None;
    let mut ps = None;
    // SAFETY: every getter only reads this live immediate context's bound state.
    unsafe {
        ctx.OMGetRenderTargets(Some(std::slice::from_mut(&mut rtv)), Some(&mut dsv));
        ctx.RSGetViewports(&mut viewport_count, Some(viewports.as_mut_ptr()));
        ctx.RSGetScissorRects(&mut scissor_count, Some(scissors.as_mut_ptr()));
        if let Ok(state) = ctx.RSGetState() {
            state.GetDesc(&mut raster);
        }
        ctx.OMGetDepthStencilState(Some(&mut depth_stencil), Some(&mut stencil_ref));
        if let Some(state) = depth_stencil.as_ref() {
            state.GetDesc(&mut depth_stencil_desc);
        }
        ctx.OMGetBlendState(
            Some(&mut blend),
            Some(&mut blend_factor),
            Some(&mut sample_mask),
        );
        if let Some(state) = blend.as_ref() {
            state.GetDesc(&mut blend_desc);
        }
        ctx.VSGetShader(&mut vs, None, None);
        ctx.HSGetShader(&mut hs, None, None);
        ctx.DSGetShader(&mut ds, None, None);
        ctx.GSGetShader(&mut gs, None, None);
        ctx.PSGetShader(&mut ps, None, None);
    }
    let topology = unsafe { ctx.IAGetPrimitiveTopology() };
    let render_target = &blend_desc.RenderTarget[0];
    tracing::info!(
        target: "water_probe",
        "water probe: raw draw pass={} eye={} invocation={} predicted_display_time_ns={} \
         generation={} kind={} n={count} log={}/{} topology={} rtv={:#x} dsv={:#x} \
         raster[cull={} fill={} depth_clip={} scissor={} bias={} slope_bias={}] viewports={viewport_count} vp0={} vp1={} \
         scissors={scissor_count} sc0={} sc1={} depth[enable={} write={} func={} stencil={} \
         ref={stencil_ref} read_mask={:#x} write_mask={:#x} front[func={} pass={} fail={} zfail={}] \
         back[func={} pass={} fail={} zfail={}]] blend[enable={} src={} dst={} op={} mask={:#x}] \
         vs={:#x} hs={:#x} ds={:#x} gs={:#x} ps={:#x}",
        context.pass,
        context.eye,
        context.invocation,
        context.frame.display_time(),
        context.frame.visual_generation,
        kind.name(),
        index,
        RAW_LOG_CAP,
        topology.0,
        pointer(&rtv),
        pointer(&dsv),
        raster.CullMode.0,
        raster.FillMode.0,
        raster.DepthClipEnable.as_bool() as u8,
        raster.ScissorEnable.as_bool() as u8,
        raster.DepthBias,
        raster.SlopeScaledDepthBias,
        viewport_text(&viewports[0]),
        viewport_text(&viewports[1]),
        scissor_text(&scissors[0]),
        scissor_text(&scissors[1]),
        depth_stencil_desc.DepthEnable.as_bool() as u8,
        depth_stencil_desc.DepthWriteMask.0,
        depth_stencil_desc.DepthFunc.0,
        depth_stencil_desc.StencilEnable.as_bool() as u8,
        depth_stencil_desc.StencilReadMask,
        depth_stencil_desc.StencilWriteMask,
        depth_stencil_desc.FrontFace.StencilFunc.0,
        depth_stencil_desc.FrontFace.StencilPassOp.0,
        depth_stencil_desc.FrontFace.StencilFailOp.0,
        depth_stencil_desc.FrontFace.StencilDepthFailOp.0,
        depth_stencil_desc.BackFace.StencilFunc.0,
        depth_stencil_desc.BackFace.StencilPassOp.0,
        depth_stencil_desc.BackFace.StencilFailOp.0,
        depth_stencil_desc.BackFace.StencilDepthFailOp.0,
        render_target.BlendEnable.as_bool() as u8,
        render_target.SrcBlend.0,
        render_target.DestBlend.0,
        render_target.BlendOp.0,
        sample_mask,
        pointer(&vs),
        pointer(&hs),
        pointer(&ds),
        pointer(&gs),
        pointer(&ps),
    );
}

fn stencil_ops() -> D3D11_DEPTH_STENCILOP_DESC {
    D3D11_DEPTH_STENCILOP_DESC {
        StencilFailOp: D3D11_STENCIL_OP_KEEP,
        StencilDepthFailOp: D3D11_STENCIL_OP_KEEP,
        StencilPassOp: D3D11_STENCIL_OP_KEEP,
        StencilFunc: D3D11_COMPARISON_ALWAYS,
    }
}

fn render_target_blend() -> D3D11_RENDER_TARGET_BLEND_DESC {
    D3D11_RENDER_TARGET_BLEND_DESC {
        BlendEnable: false.into(),
        SrcBlend: D3D11_BLEND_ONE,
        DestBlend: D3D11_BLEND_ZERO,
        BlendOp: D3D11_BLEND_OP_ADD,
        SrcBlendAlpha: D3D11_BLEND_ONE,
        DestBlendAlpha: D3D11_BLEND_ZERO,
        BlendOpAlpha: D3D11_BLEND_OP_ADD,
        RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
    }
}

fn default_raster_desc() -> D3D11_RASTERIZER_DESC {
    D3D11_RASTERIZER_DESC {
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
    }
}

fn default_depth_stencil_desc() -> D3D11_DEPTH_STENCIL_DESC {
    D3D11_DEPTH_STENCIL_DESC {
        DepthEnable: true.into(),
        DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ALL,
        DepthFunc: D3D11_COMPARISON_LESS,
        StencilEnable: false.into(),
        StencilReadMask: 0xFF,
        StencilWriteMask: 0xFF,
        FrontFace: stencil_ops(),
        BackFace: stencil_ops(),
    }
}

fn default_blend_desc() -> D3D11_BLEND_DESC {
    D3D11_BLEND_DESC {
        AlphaToCoverageEnable: false.into(),
        IndependentBlendEnable: false.into(),
        RenderTarget: [render_target_blend(); 8],
    }
}

fn pointer<T: Interface>(value: &Option<T>) -> usize {
    value.as_ref().map_or(0, |value| value.as_raw() as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_spends_the_lifetime_budget_once() {
        assert_eq!(admit(0), Some(0));
        assert_eq!(admit(MAX_INVOCATIONS - 1), Some(MAX_INVOCATIONS - 1));
        assert_eq!(admit(MAX_INVOCATIONS), None);
        assert_eq!(admit(MAX_INVOCATIONS + 1), None);
    }

    #[test]
    fn raw_snapshots_stop_at_the_cap_without_stopping_counts() {
        assert_eq!(raw_log_slot(0), Some(0));
        assert_eq!(raw_log_slot(RAW_LOG_CAP - 1), Some(RAW_LOG_CAP - 1));
        assert_eq!(raw_log_slot(RAW_LOG_CAP), None);
        assert_eq!(raw_log_slot(RAW_LOG_CAP + 1), None);
    }

    fn test_context() -> ProbeContext {
        ProbeContext {
            pass: 0,
            eye: 1,
            invocation: 7,
            frame: FrameStamp {
                predicted_display_time_ns: None,
                visual_generation: 3,
            },
        }
    }

    #[test]
    fn guard_arms_records_and_disarms() {
        let guard = ActiveGuard::arm(test_context()).expect("armed");
        RAW_CAPTURE.with(|state| {
            let mut state = state.borrow_mut();
            let capture = state.as_mut().expect("armed");
            capture.counts[RawDrawKind::Indexed as usize] = 3;
            capture.logged = 5;
        });
        let capture = guard.disarm().expect("disarmed");
        assert_eq!(capture.context.invocation, 7);
        assert_eq!(capture.counts[RawDrawKind::Indexed as usize], 3);
        assert_eq!(capture.logged, 5);
        assert!(RAW_CAPTURE.with(|state| state.borrow().is_none()));
    }

    #[test]
    fn guard_rejects_a_second_arm_and_keeps_the_first() {
        let guard = ActiveGuard::arm(test_context()).expect("armed");
        assert!(ActiveGuard::arm(test_context()).is_none());
        RAW_CAPTURE.with(|state| {
            let state = state.borrow();
            let capture = state.as_ref().expect("first arm preserved");
            assert_eq!(capture.context.invocation, 7);
        });
        drop(guard);
        assert!(RAW_CAPTURE.with(|state| state.borrow().is_none()));
    }

    #[test]
    fn guard_drop_clears_recording() {
        {
            let _guard = ActiveGuard::arm(test_context()).expect("armed");
            assert!(RAW_CAPTURE.with(|state| state.borrow().is_some()));
        }
        assert!(RAW_CAPTURE.with(|state| state.borrow().is_none()));
    }

    #[test]
    fn recording_is_thread_local() {
        let _guard = ActiveGuard::arm(test_context()).expect("armed");
        let other_thread_idle =
            std::thread::spawn(|| RAW_CAPTURE.with(|state| state.borrow().is_none()))
                .join()
                .expect("join");
        assert!(other_thread_idle);
    }
}
