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
    time::Duration,
};

use windows::{
    Win32::{
        Foundation::{RECT, S_FALSE, S_OK},
        Graphics::Direct3D11::{
            D3D11_BLEND_DESC, D3D11_BLEND_ONE, D3D11_BLEND_OP_ADD, D3D11_BLEND_ZERO,
            D3D11_COLOR_WRITE_ENABLE_ALL, D3D11_COMPARISON_ALWAYS, D3D11_COMPARISON_LESS,
            D3D11_CULL_BACK, D3D11_DEPTH_STENCIL_DESC, D3D11_DEPTH_STENCILOP_DESC,
            D3D11_DEPTH_WRITE_MASK_ALL, D3D11_FILL_SOLID, D3D11_QUERY,
            D3D11_QUERY_DATA_PIPELINE_STATISTICS, D3D11_QUERY_DESC, D3D11_QUERY_OCCLUSION,
            D3D11_QUERY_PIPELINE_STATISTICS, D3D11_RASTERIZER_DESC, D3D11_RENDER_TARGET_BLEND_DESC,
            D3D11_STENCIL_OP_KEEP, D3D11_VIEWPORT, ID3D11DepthStencilView, ID3D11Device,
            ID3D11DeviceContext, ID3D11Query, ID3D11RenderTargetView,
        },
    },
    core::{Error as WindowsError, HRESULT, Interface},
};

use crate::stereo::single_pass::EngineContext;

#[derive(Clone, Copy)]
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
        log_raw(ctx, capture, kind, count, slot);
    });
}

pub(crate) struct WaterGpuProbe {
    d3d: EngineContext,
    context: ProbeContext,
    queries: Option<QueryPair>,
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
        let Some(queries) = self.queries.take() else {
            return;
        };
        self.d3d.with_lock(|ctx| {
            // SAFETY: both queries were created on this context's device and begun in `start`.
            unsafe {
                ctx.End(&queries.pipeline);
                ctx.End(&queries.occlusion);
            }
        });
        match resolve(self.d3d, &queries.pipeline, &queries.occlusion) {
            Ok(work) => work.log(self.context),
            Err(e) => tracing::warn!(target: "water_probe", "{e}"),
        }
        if let Some(capture) = &capture {
            log_raw_counts(capture);
        }
    }

    fn start(
        ctx: &ID3D11DeviceContext,
        d3d: EngineContext,
        context: ProbeContext,
    ) -> Result<Self, ProbeError> {
        let guard = ActiveGuard::arm(context).ok_or(ProbeError::AlreadyActive)?;
        // SAFETY: `ctx` is the live immediate context; `GetDevice` only reads its device.
        let device = unsafe { ctx.GetDevice() }.map_err(ProbeError::Device)?;
        let pipeline = create_query(
            &device,
            D3D11_QUERY_PIPELINE_STATISTICS,
            "pipeline statistics",
        )?;
        let occlusion = create_query(&device, D3D11_QUERY_OCCLUSION, "occlusion")?;
        log_state(ctx, context);
        // SAFETY: both queries are live and were created on this context's device; `finish` pairs
        // each `Begin` with its `End`.
        unsafe {
            ctx.Begin(&pipeline);
            ctx.Begin(&occlusion);
        }
        Ok(Self {
            d3d,
            context,
            queries: Some(QueryPair {
                pipeline,
                occlusion,
            }),
            _guard: Some(guard),
        })
    }
}

impl Drop for WaterGpuProbe {
    fn drop(&mut self) {
        let Some(queries) = self.queries.take() else {
            return;
        };
        self.d3d.with_lock(|ctx| {
            // SAFETY: both queries were created on this context's device and begun in `start`.
            unsafe {
                ctx.End(&queries.pipeline);
                ctx.End(&queries.occlusion);
            }
        });
    }
}

struct QueryPair {
    pipeline: ID3D11Query,
    occlusion: ID3D11Query,
}

const MAX_INVOCATIONS: usize = 64;

const POLLS: u32 = 20;
const POLL_INTERVAL: Duration = Duration::from_micros(250);

const RAW_LOG_CAP: usize = 32;

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
            });
            Some(Self {
                _not_send: PhantomData,
            })
        })
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QueryStatus {
    Ready,
    Pending,
}

struct GpuWork {
    pipeline: Option<D3D11_QUERY_DATA_PIPELINE_STATISTICS>,
    occlusion_samples: Option<u64>,
    polls: u32,
}

impl GpuWork {
    fn log(&self, context: ProbeContext) {
        let count = |value: Option<u64>| match value {
            Some(value) => value.to_string(),
            None => "pending".to_owned(),
        };
        let pipeline = self.pipeline;
        tracing::info!(
            target: "water_probe",
            "water probe: gpu work pass={} eye={} invocation={} predicted_display_time_ns={} \
             generation={} polls={} pipeline_stats={} occlusion={} ia_vertices={} \
             ia_primitives={} vs_invocations={} hs_invocations={} ds_invocations={} \
             gs_invocations={} clipper_invocations={} clipper_primitives={} ps_invocations={} \
             cs_invocations={} occlusion_samples={}",
            context.pass,
            context.eye,
            context.invocation,
            context.frame.display_time(),
            context.frame.visual_generation,
            self.polls,
            readiness(self.pipeline.is_some()),
            readiness(self.occlusion_samples.is_some()),
            count(pipeline.map(|p| p.IAVertices)),
            count(pipeline.map(|p| p.IAPrimitives)),
            count(pipeline.map(|p| p.VSInvocations)),
            count(pipeline.map(|p| p.HSInvocations)),
            count(pipeline.map(|p| p.DSInvocations)),
            count(pipeline.map(|p| p.GSInvocations)),
            count(pipeline.map(|p| p.CInvocations)),
            count(pipeline.map(|p| p.CPrimitives)),
            count(pipeline.map(|p| p.PSInvocations)),
            count(pipeline.map(|p| p.CSInvocations)),
            count(self.occlusion_samples),
        );
    }
}

fn readiness(resolved: bool) -> &'static str {
    if resolved { "ready" } else { "pending" }
}

fn create_query(
    device: &ID3D11Device,
    kind: D3D11_QUERY,
    name: &'static str,
) -> Result<ID3D11Query, ProbeError> {
    let desc = D3D11_QUERY_DESC {
        Query: kind,
        MiscFlags: 0,
    };
    let mut query = None;
    // SAFETY: `desc` is a live descriptor for the requested query type.
    unsafe { device.CreateQuery(&desc, Some(&mut query)) }
        .map_err(|error| ProbeError::CreateQuery { query: name, error })?;
    query.ok_or(ProbeError::QueryMissing { query: name })
}

fn resolve(
    d3d: EngineContext,
    pipeline: &ID3D11Query,
    occlusion: &ID3D11Query,
) -> Result<GpuWork, ProbeError> {
    let mut statistics = D3D11_QUERY_DATA_PIPELINE_STATISTICS::default();
    let mut samples = 0u64;
    let mut have_statistics = false;
    let mut have_samples = false;
    let mut polls = 0;
    loop {
        let (statistics_ready, samples_ready) = d3d.with_lock(|ctx| {
            let statistics_ready = if have_statistics {
                false
            } else {
                get_data(ctx, pipeline, &mut statistics)? == QueryStatus::Ready
            };
            let samples_ready = if have_samples {
                false
            } else {
                get_data(ctx, occlusion, &mut samples)? == QueryStatus::Ready
            };
            Ok::<_, ProbeError>((statistics_ready, samples_ready))
        })?;
        have_statistics |= statistics_ready;
        have_samples |= samples_ready;
        let Some(wait) = poll_wait(polls, have_statistics && have_samples) else {
            break;
        };
        std::thread::sleep(wait);
        polls += 1;
    }
    Ok(GpuWork {
        pipeline: have_statistics.then_some(statistics),
        occlusion_samples: have_samples.then_some(samples),
        polls,
    })
}

fn get_data<T>(
    ctx: &ID3D11DeviceContext,
    query: &ID3D11Query,
    data: &mut T,
) -> Result<QueryStatus, ProbeError> {
    let vtable = ctx.vtable();
    // SAFETY: `query` is live and `data` is a buffer of the size the query type writes.
    let hr = unsafe {
        (vtable.GetData)(
            ctx.as_raw(),
            query.as_raw(),
            std::ptr::from_mut(data).cast(),
            std::mem::size_of::<T>() as u32,
            0,
        )
    };
    classify(hr)
}

fn classify(hr: HRESULT) -> Result<QueryStatus, ProbeError> {
    if hr == S_OK {
        Ok(QueryStatus::Ready)
    } else if hr == S_FALSE {
        Ok(QueryStatus::Pending)
    } else {
        Err(ProbeError::GetData(hr))
    }
}

fn admit(invocation: usize) -> Option<usize> {
    (invocation < MAX_INVOCATIONS).then_some(invocation)
}

fn poll_wait(polls: u32, resolved: bool) -> Option<Duration> {
    (!resolved && polls < POLLS).then_some(POLL_INTERVAL)
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
    tracing::info!(
        target: "water_probe",
        "water probe: pre-draw pass={} eye={} invocation={} predicted_display_time_ns={} \
         generation={} rtv={:#x} dsv={:#x} viewports={count} vp0={} vp1={}",
        context.pass,
        context.eye,
        context.invocation,
        context.frame.display_time(),
        context.frame.visual_generation,
        pointer(&rtv),
        pointer(&dsv),
        viewport_text(&viewports[0]),
        viewport_text(&viewports[1]),
    );
}

fn viewport_text(v: &D3D11_VIEWPORT) -> String {
    format!(
        "({:.1},{:.1}) {:.1}x{:.1}",
        v.TopLeftX, v.TopLeftY, v.Width, v.Height
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
         raster[cull={} fill={} depth_clip={} scissor={}] viewports={viewport_count} vp0={} vp1={} \
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
    use windows::Win32::Foundation::E_FAIL;

    use super::*;

    #[test]
    fn query_status_separates_ready_pending_and_error() {
        assert_eq!(classify(S_OK).ok(), Some(QueryStatus::Ready));
        assert_eq!(classify(S_FALSE).ok(), Some(QueryStatus::Pending));
        assert!(matches!(classify(E_FAIL), Err(ProbeError::GetData(hr)) if hr == E_FAIL));
    }

    #[test]
    fn admission_spends_the_lifetime_budget_once() {
        assert_eq!(admit(0), Some(0));
        assert_eq!(admit(MAX_INVOCATIONS - 1), Some(MAX_INVOCATIONS - 1));
        assert_eq!(admit(MAX_INVOCATIONS), None);
        assert_eq!(admit(MAX_INVOCATIONS + 1), None);
    }

    #[test]
    fn polling_stops_at_the_cap_and_reports_pending() {
        assert_eq!(poll_wait(0, false), Some(POLL_INTERVAL));
        assert_eq!(poll_wait(POLLS - 1, false), Some(POLL_INTERVAL));
        assert_eq!(poll_wait(POLLS, false), None);
        assert_eq!(poll_wait(0, true), None);
        assert_eq!(poll_wait(POLLS, true), None);
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
