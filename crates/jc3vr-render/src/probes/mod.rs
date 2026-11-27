//! The capture probes: what the capture tooling observes in the render hooks, registered once at
//! startup so the hooks name none of it.
//!
//! The hooks call one table ([`CaptureProbes`]) at generic events: points in the frame,
//! scoped blocks, passes, and post effects, the game's draws as they arrive at the device-context
//! entries, and the submissions the stereo draw path makes for them. The capture side registers the
//! table ([`register`]) before the hooks install, and decides at each event what, if anything, its
//! pending request reads there. The hooks name no diagnostic.
//!
//! Every probe but the shader tag is inert until the capture side receives a request, so the hooks
//! reach the table through [`armed`] (or [`armed_for_benchmark`] for the probes a benchmark reads),
//! which reads the arming flag ([`arming`]) with one relaxed load and answers `None` while nothing has
//! asked for a capture. Unarmed, or with no table registered, the hooks make no call into the table:
//! a scope runs its body directly, a point is skipped, a draw is submitted straight through, and a
//! query answers no. The shader tag ([`registered`]) records state a later capture reads, so it runs
//! whenever a table is registered.

mod arming;
mod events;
pub mod state;

use std::{
    ffi::c_void,
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use jc3gi::graphics_engine::graphics_engine::RenderContext;
#[cfg(feature = "profiler")]
use jc3gi::graphics_engine::render_engine::RenderBlockTypeBase;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceChild, ID3D11DeviceContext};

pub use crate::probes::{
    arming::{Arming, arm},
    events::{
        BlockScope, Cb13Rows, DrawArgs, DrawScope, GameDraw, PassScope, PerEyePassProbe,
        PipelinePoint, PostEffect, ProbeGuard, RawDrawKind, Route, Submission,
    },
};
use crate::{
    draw_path::bump,
    probes::arming::{capture_armed, capture_or_benchmark_armed},
    screenshot::CaptureSource,
    stereo::single_pass::{current_pass_id, detour_timing, per_eye_reissue_eye},
};

/// What the capture tooling does at the render hooks' events. Every method has an inert default.
///
/// A scope method runs `body` exactly once, at the point its doc names, and brackets it with the
/// probe's own work.
pub trait CaptureProbes: Sync {
    /// Brackets one draw-thread task (`CGraphicsEngine::HandleDrawThreadTask`). Called while a
    /// capture or a benchmark is armed.
    fn draw_task_scope(&self, body: &mut dyn FnMut()) {
        body();
    }

    /// Runs as the render pass changes its render-block type: with `None` before the change, while
    /// the outgoing run's type is still current, and with the incoming type after it. Called while
    /// a capture or a benchmark is armed.
    #[cfg(feature = "profiler")]
    fn block_type_switch(&self, _next: Option<*mut RenderBlockTypeBase>) {}

    /// Keeps what the pipeline holds at `point`.
    fn point(&self, _point: PipelinePoint<'_>) {}

    /// Brackets `block`.
    ///
    /// # Safety
    ///
    /// The pointers `block` carries are live for the scope.
    unsafe fn block_scope(&self, _block: BlockScope, body: &mut dyn FnMut()) {
        body();
    }

    /// Brackets `effect`'s apply; `body` runs it and returns the post-chain slot it wrote, which the
    /// scope returns.
    fn post_effect_scope(&self, _effect: PostEffect, body: &mut dyn FnMut() -> u32) -> u32 {
        body()
    }

    /// Brackets `pass`.
    ///
    /// # Safety
    ///
    /// The pointers `pass` carries are live for the scope.
    unsafe fn pass_scope(&self, _pass: PassScope, body: &mut dyn FnMut()) {
        body();
    }

    /// Draws the main G-buffer passes `[lo, hi)` through `draw`, which draws a pass range, in as
    /// many pieces as the probe brackets.
    fn draw_gbuffer_passes(&self, lo: i32, hi: i32, draw: &dyn Fn(i32, i32)) {
        draw(lo, hi);
    }

    /// The probe's bracket of one pass the stereo path re-issues once per eye from render context
    /// `ctx`, if it wants one.
    fn per_eye_pass(&self, _ctx: *const RenderContext) -> Option<Box<dyn PerEyePassProbe>> {
        None
    }

    /// A game draw arriving at a detoured entry, before the stereo path binds anything for it. What
    /// the probe returns is held until the entry returns.
    ///
    /// # Safety
    ///
    /// `draw.context` is the live immediate context the entry was called on.
    unsafe fn draw_scope(&self, _draw: &GameDraw) -> Option<DrawScope> {
        None
    }

    /// The same draw once the stereo path has bound its own shader variants for it, before it
    /// submits anything. What the probe returns is held until the entry returns, and released
    /// before the stereo path's variants and the [`draw_scope`](Self::draw_scope).
    ///
    /// # Safety
    ///
    /// `draw.context` is the live immediate context the entry was called on.
    unsafe fn draw_bound(&self, _draw: &GameDraw) -> Option<ProbeGuard> {
        None
    }

    /// One submission the stereo draw path makes. `draw` submits it; the probe calls it at least
    /// once, and a treatment that draws the submission once per eye calls it once per eye.
    fn submit(&self, _submission: &Submission, draw: &dyn Fn()) {
        draw();
    }

    /// Records the bytecode a shader program was created from. Called whenever a table is
    /// registered, armed or not, so a capture can name the shaders created before it.
    fn tag_shader(&self, _program: &ID3D11DeviceChild, _bytecode: &[u8]) {}

    /// Whether a capture waits for this frame's eyes, so their captures must be kept fresh.
    fn wants_eyes(&self) -> bool {
        false
    }

    /// Notes whether eye `eye`'s capture holds this dispatch's image.
    fn note_eye(&self, _eye: usize, _fresh: bool) {}

    /// Captures the frame from `source`, on the frame's last near dispatch.
    fn capture_frame(
        &self,
        _device: &ID3D11Device,
        _context: &ID3D11DeviceContext,
        _source: CaptureSource<'_>,
    ) {
    }

    /// Whether the game clock is held paused for a capture.
    fn clock_held(&self) -> bool {
        false
    }
}

/// Registers the capture probes. Once, at startup, before the hooks install; a second registration
/// is refused with an error log.
pub fn register(probes: &'static dyn CaptureProbes) {
    if PROBES.set(probes).is_err() {
        tracing::error!("capture probes: already registered; the second registration is ignored");
    }
}

/// Whether the capture probes are registered, for the install's registration check.
pub fn is_registered() -> bool {
    PROBES.get().is_some()
}

/// Every submission the stereo draw path has made while a capture or a benchmark was armed, never
/// reset, for samplers that difference two readings (the capture crate's benchmark, whose request
/// arms the count). Counted only while the draw-path detours are installed.
pub fn submission_total() -> u64 {
    SUBMISSIONS.load(Ordering::Relaxed)
}

/// The registered probes while a capture has armed them, `None` otherwise.
#[inline]
pub(crate) fn armed() -> Option<&'static dyn CaptureProbes> {
    if !capture_armed() {
        return None;
    }
    registered()
}

/// The registered probes while a capture or a benchmark has armed them, `None` otherwise: for the
/// probes a benchmark reads (the draw-task scope and the render-block type switch).
#[inline]
pub(crate) fn armed_for_benchmark() -> Option<&'static dyn CaptureProbes> {
    if !capture_or_benchmark_armed() {
        return None;
    }
    registered()
}

/// The registered probes, armed or not: for the shader tag.
#[inline]
pub(crate) fn registered() -> Option<&'static dyn CaptureProbes> {
    PROBES.get().copied()
}

/// Hands `point` to the probes while a capture is armed.
#[inline]
pub(crate) fn point(point: PipelinePoint<'_>) {
    if let Some(probes) = armed() {
        probes.point(point);
    }
}

/// Runs `body` inside `block`'s probe scope while a capture is armed, and directly otherwise.
///
/// # Safety
///
/// The pointers `block` carries are live for the scope.
#[inline]
pub(crate) unsafe fn block_scope<R>(block: BlockScope, body: impl FnOnce() -> R) -> R {
    scoped(
        armed(),
        // SAFETY: per this function's contract.
        |probes, body| unsafe { probes.block_scope(block, body) },
        body,
    )
}

/// Runs `body` inside `pass`'s probe scope while a capture is armed, and directly otherwise.
///
/// # Safety
///
/// The pointers `pass` carries are live for the scope.
#[inline]
pub(crate) unsafe fn pass_scope<R>(pass: PassScope, body: impl FnOnce() -> R) -> R {
    scoped(
        armed(),
        // SAFETY: per this function's contract.
        |probes, body| unsafe { probes.pass_scope(pass, body) },
        body,
    )
}

/// Runs `body` inside a probe scope (one of the [`CaptureProbes`] scope methods, called through
/// `scope` on `probes`) and returns what `body` returned. With no `probes` (unarmed), runs `body`
/// directly.
#[inline]
pub(crate) fn scoped<R>(
    probes: Option<&'static dyn CaptureProbes>,
    scope: impl FnOnce(&'static dyn CaptureProbes, &mut dyn FnMut()),
    body: impl FnOnce() -> R,
) -> R {
    let Some(probes) = probes else {
        return body();
    };
    let mut body = Some(body);
    let mut result = None;
    scope(probes, &mut || {
        if let Some(body) = body.take() {
            result = Some(body());
        }
    });
    result.expect("capture probes: a scope must run its body")
}

/// Hands a game draw arriving at a detoured entry to `probes`, which are what [`armed`] answered:
/// [`CaptureProbes::draw_scope`].
///
/// # Safety
///
/// `draw.context` is the live immediate context the entry was called on.
#[inline]
pub(crate) unsafe fn draw_scope(
    probes: Option<&'static dyn CaptureProbes>,
    draw: &GameDraw,
) -> Option<DrawScope> {
    // SAFETY: per this function's contract.
    probes.and_then(|probes| unsafe { probes.draw_scope(draw) })
}

/// Hands a game draw whose variants are bound to `probes`: [`CaptureProbes::draw_bound`].
///
/// # Safety
///
/// `draw.context` is the live immediate context the entry was called on.
#[inline]
pub(crate) unsafe fn draw_bound(
    probes: Option<&'static dyn CaptureProbes>,
    draw: &GameDraw,
) -> Option<ProbeGuard> {
    // SAFETY: per this function's contract.
    probes.and_then(|probes| unsafe { probes.draw_bound(draw) })
}

/// Submit a draw: through `probes` (what [`armed`] answered) while armed, and otherwise straight
/// through the detour timing's forward. Either way the submission is counted while a capture or a
/// benchmark is armed ([`submission_total`]).
#[inline]
pub(crate) fn submit(
    probes: Option<&'static dyn CaptureProbes>,
    context: *mut c_void,
    entry: RawDrawKind,
    route: Route,
    args: DrawArgs,
    draw: impl Fn(),
) {
    count_submission(probes.is_some());
    match probes {
        Some(probes) => {
            let eye = match route {
                Route::PerEye(eye) => Some(eye),
                Route::AsIssued | Route::Promoted => per_eye_reissue_eye(),
            };
            let submission = Submission {
                context,
                entry,
                route,
                args,
                pass: current_pass_id(),
                eye,
            };
            probes.submit(&submission, &draw);
        }
        None => detour_timing::forward(draw),
    }
}

static PROBES: OnceLock<&'static dyn CaptureProbes> = OnceLock::new();

/// Count one submission for [`submission_total`]. `capture_armed` is what the caller's [`armed`]
/// read found, which spares the flag a second load on the capture path.
#[inline]
fn count_submission(capture_armed: bool) {
    if capture_armed || capture_or_benchmark_armed() {
        bump(&SUBMISSIONS);
    }
}

static SUBMISSIONS: AtomicU64 = AtomicU64::new(0);
