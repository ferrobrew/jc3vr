//! Capture-gated diagnostic for the passes that produce the water's reflection inputs: the draws each
//! one issues and the state every draw runs under, plus the pass-level target state for context.
//!
//! The water draw probe records what the water samples; this records what produced it. The water
//! shader's dynamic reflection colour and alpha are the screen-space reflection pass's outputs
//! (`RenderContext::m_DynamicReflectionColorTexture` / `m_DynamicReflectionAlphaTexture` are assigned
//! from it), while the reflective-water-planes pass writes the water plane's depth mask, so both are
//! bracketed: which one a given input came from is then read off the pass id on each record.
//!
//! Draw-time state is the only reliable view of what the pass renders with: `DoDraw` binds targets and
//! programs per render block and restores them, so a reading taken around the whole walk shows the
//! state the walk left behind, not the state any reflection draw used. The draws are therefore
//! collected from the D3D draw entry itself ([`on_raw_draw`]), bounded per pass, while the walk-level
//! records stay only as context and are labelled as such. Budgeted independently of the water draw
//! probe, since it records once per pass rather than once per draw.

use std::{
    cell::RefCell,
    ffi::c_void,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use jc3gi::graphics_engine::render_engine::RenderPassId;
use jc3vr_render::probes::RawDrawKind;
use jc3vr_view::stereo::{draw_index, engine_context::EngineContext};
use windows::{
    Win32::Graphics::Direct3D11::{
        D3D11_TEXTURE2D_DESC, D3D11_VIEWPORT, ID3D11DepthStencilView, ID3D11DeviceContext,
        ID3D11PixelShader, ID3D11RenderTargetView, ID3D11Texture2D, ID3D11VertexShader,
    },
    core::Interface,
};

use crate::harness::water_gpu_probe::{pointer, viewport_text};

/// A scene costs fifteen records across its four modes (three isolated, six double-draw, three
/// single-pass), so sky and shore together cost thirty; this leaves headroom for one more scene while
/// still stopping well short of a whole-session trace.
const MAX_INVOCATIONS: usize = 40;

/// The most draws one bracketed pass logs. A reflection pass issues a handful of water draws; the cap
/// keeps a pathological pass from flooding the log.
const MAX_DRAWS: usize = 8;

static INVOCATIONS: AtomicUsize = AtomicUsize::new(0);
static BUDGET_EXHAUSTED: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// The pass currently bracketed on this thread, or `None` outside one.
    static ACTIVE: RefCell<Option<PassCapture>> = const { RefCell::new(None) };
}

/// The passes whose output the water samples: the screen-space reflection pass (the reflection colour
/// and alpha) and the reflective-water-planes pass (the water plane depth mask).
const PRODUCER_PASSES: [i32; 2] = [
    RenderPassId::RP_SCREEN_SPACE_REFLECTIONS as i32,
    RenderPassId::RP_REFLECTIVE_WATER_PLANES as i32,
];

/// Brackets one of the water's producer passes, or returns `None` for any other pass, an unarmed
/// capture, or a spent budget.
pub(crate) fn reflection_producer(pass: i32) -> Option<ReflectionProbe> {
    if !PRODUCER_PASSES.contains(&pass) {
        return None;
    }
    if !crate::harness::screenshot_wants_eyes() {
        return None;
    }
    let visual = crate::testing::visual::metadata();
    if !visual.ready {
        return None;
    }
    let invocation = INVOCATIONS.fetch_add(1, Ordering::Relaxed);
    if invocation >= MAX_INVOCATIONS {
        if !BUDGET_EXHAUSTED.swap(true, Ordering::Relaxed) {
            tracing::info!(
                target: "water_probe",
                "reflection producer probe: invocation budget of {MAX_INVOCATIONS} spent; later \
                 passes are not probed"
            );
        }
        return None;
    }
    let d3d = EngineContext::get()?;
    let capture = PassCapture {
        pass,
        invocation,
        display_time: jc3vr_view::vr::capture_views()
            .map(|views| views.predicted_display_time_ns)
            .map_or_else(|| "none".to_owned(), |ns| ns.to_string()),
        generation: visual.generation,
        draws: 0,
        counts: [0; RawDrawKind::COUNT],
    };
    let armed = ACTIVE.with(|state| {
        let mut state = state.borrow_mut();
        if state.is_some() {
            return false;
        }
        *state = Some(capture.clone());
        true
    });
    let probe = ReflectionProbe {
        d3d,
        capture,
        armed,
    };
    probe.log_walk("pre");
    Some(probe)
}

/// Collects one raw draw issued inside a bracketed pass. Called from the D3D draw entry, where the
/// bound state is the draw's own.
///
/// # Safety
///
/// `context` must be the live immediate context the detoured entry was called on.
pub(super) unsafe fn on_raw_draw(kind: RawDrawKind, context: *mut c_void) {
    ACTIVE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(capture) = state.as_mut() else {
            return;
        };
        let count = &mut capture.counts[kind as usize];
        *count += 1;
        let count = *count;
        if capture.draws >= MAX_DRAWS {
            return;
        }
        let index = capture.draws;
        capture.draws += 1;
        // SAFETY: per this function's contract, the pointer is the live COM interface pointer; the
        // borrowed reference ends with this call and takes no reference count.
        let Some(ctx) =
            (unsafe { <ID3D11DeviceContext as Interface>::from_raw_borrowed(&context) })
        else {
            return;
        };
        log_draw(ctx, capture, kind, index, count);
    });
}

/// One bracketed pass. The walk records on creation and drop are context only; the draws collected
/// through [`on_raw_draw`] are the pass's actual rendering state.
pub(crate) struct ReflectionProbe {
    d3d: EngineContext,
    capture: PassCapture,
    armed: bool,
}

impl ReflectionProbe {
    /// Logs the target state outside any draw, which `DoDraw` may have set up before the walk or left
    /// behind after it. Labelled `walk` so it is never read as a draw's state.
    fn log_walk(&self, phase: &str) {
        let (rtv, dsv, viewports, count, vs, ps, (target_res, target)) =
            self.d3d.with_lock(read_walk_state);
        tracing::info!(
            target: "water_probe",
            "reflection producer: pass={} state=walk phase={phase} eye={} invocation={} \
             predicted_display_time_ns={} generation={} rtv={rtv:#x} rtv_res={:#x} rtv_tex={}x{}/{:?} \
             dsv={dsv:#x} viewports={count} vp0={} vp1={} vs={vs:#x} ps={ps:#x}",
            self.capture.pass,
            draw_index(),
            self.capture.invocation,
            self.capture.display_time,
            self.capture.generation,
            target_res,
            target.Width,
            target.Height,
            target.Format,
            viewport_text(&viewports[0]),
            viewport_text(&viewports[1]),
        );
    }

    fn log_summary(&self, collected: &PassCapture) {
        let counts = collected.counts;
        let seen: usize = counts.iter().sum();
        tracing::info!(
            target: "water_probe",
            "reflection producer: pass={} state=summary eye={} invocation={} \
             predicted_display_time_ns={} generation={} draws={seen} logged={} indexed={} \
             non_indexed={} indexed_instanced={} indexed_instanced_indirect={} \
             instanced_indirect={}",
            self.capture.pass,
            draw_index(),
            self.capture.invocation,
            self.capture.display_time,
            self.capture.generation,
            collected.draws,
            counts[RawDrawKind::Indexed as usize],
            counts[RawDrawKind::NonIndexed as usize],
            counts[RawDrawKind::IndexedInstanced as usize],
            counts[RawDrawKind::IndexedInstancedIndirect as usize],
            counts[RawDrawKind::InstancedIndirect as usize],
        );
    }
}

impl Drop for ReflectionProbe {
    fn drop(&mut self) {
        self.log_walk("post");
        // Take the thread-local state rather than reading this probe's own copy: the draws accumulate
        // in the copy the draw entry holds. A probe that did not arm must not take another's state.
        let collected = if self.armed {
            ACTIVE.with(|state| state.borrow_mut().take())
        } else {
            None
        };
        if let Some(collected) = collected {
            self.log_summary(&collected);
        }
    }
}

/// The pass-level target state: what the walk has bound outside any draw.
type WalkState = (
    usize,
    usize,
    [D3D11_VIEWPORT; 2],
    u32,
    usize,
    usize,
    TargetResource,
);

fn read_walk_state(ctx: &ID3D11DeviceContext) -> WalkState {
    let mut rtv: Option<ID3D11RenderTargetView> = None;
    let mut dsv: Option<ID3D11DepthStencilView> = None;
    let mut viewports = [D3D11_VIEWPORT::default(); 2];
    let mut count = 2u32;
    let mut vs: Option<ID3D11VertexShader> = None;
    let mut ps: Option<ID3D11PixelShader> = None;
    // SAFETY: every getter only reads this live immediate context's bound state.
    unsafe {
        ctx.OMGetRenderTargets(Some(std::slice::from_mut(&mut rtv)), Some(&mut dsv));
        ctx.RSGetViewports(&mut count, Some(viewports.as_mut_ptr()));
        ctx.VSGetShader(&mut vs, None, None);
        ctx.PSGetShader(&mut ps, None, None);
    }
    (
        pointer(&rtv),
        pointer(&dsv),
        viewports,
        count,
        pointer(&vs),
        pointer(&ps),
        target_resource(&rtv),
    )
}

/// The draw-time state of one raw draw inside the pass.
fn log_draw(
    ctx: &ID3D11DeviceContext,
    capture: &PassCapture,
    kind: RawDrawKind,
    index: usize,
    count: usize,
) {
    let mut rtv: Option<ID3D11RenderTargetView> = None;
    let mut dsv: Option<ID3D11DepthStencilView> = None;
    let mut viewports = [D3D11_VIEWPORT::default(); 2];
    let mut viewport_count = 2u32;
    let mut vs: Option<ID3D11VertexShader> = None;
    let mut ps: Option<ID3D11PixelShader> = None;
    // SAFETY: every getter only reads this live immediate context's bound state.
    unsafe {
        ctx.OMGetRenderTargets(Some(std::slice::from_mut(&mut rtv)), Some(&mut dsv));
        ctx.RSGetViewports(&mut viewport_count, Some(viewports.as_mut_ptr()));
        ctx.VSGetShader(&mut vs, None, None);
        ctx.PSGetShader(&mut ps, None, None);
    }
    let (target_res, target) = target_resource(&rtv);
    tracing::info!(
        target: "water_probe",
        "reflection producer: pass={} state=draw index={} n={count} kind={} eye={} invocation={} \
         predicted_display_time_ns={} generation={} rtv={:#x} rtv_res={:#x} rtv_tex={}x{}/{:?} dsv={:#x} \
         viewports={viewport_count} vp0={} vp1={} vs={:#x} ps={:#x}",
        capture.pass,
        index,
        kind.name(),
        draw_index(),
        capture.invocation,
        capture.display_time,
        capture.generation,
        pointer(&rtv),
        target_res,
        target.Width,
        target.Height,
        target.Format,
        pointer(&dsv),
        viewport_text(&viewports[0]),
        viewport_text(&viewports[1]),
        pointer(&vs),
        pointer(&ps),
    );
}

/// The bound render target's resource identity and allocation, so a target can be matched against the
/// resource a sampled view points at; both are empty when no target is bound.
type TargetResource = (usize, D3D11_TEXTURE2D_DESC);

fn target_resource(rtv: &Option<ID3D11RenderTargetView>) -> TargetResource {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    let resource = rtv
        .as_ref()
        .and_then(|view| unsafe { view.GetResource() }.ok());
    let resource_pointer = resource
        .as_ref()
        .map_or(0, |resource| resource.as_raw() as usize);
    let target = resource
        .as_ref()
        .and_then(|resource| resource.cast::<ID3D11Texture2D>().ok());
    if let Some(target) = &target {
        // SAFETY: `target` is live; `GetDesc` only writes the out parameter.
        unsafe { target.GetDesc(&mut desc) };
    }
    (resource_pointer, desc)
}

/// The pass-level collector state shared with the draw entry.
#[derive(Clone)]
struct PassCapture {
    pass: i32,
    invocation: usize,
    display_time: String,
    generation: u64,
    /// Draws logged so far, capped at [`MAX_DRAWS`].
    draws: usize,
    /// Every draw seen, by kind, so a pass that issues more than the log cap still reports its shape.
    counts: [usize; RawDrawKind::COUNT],
}
