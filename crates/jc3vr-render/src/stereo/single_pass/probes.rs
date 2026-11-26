//! The capture probes the single-pass draw path calls: one table the capture harness registers at
//! install, read behind the capture arming flag ([`crate::capture_arming`]).
//!
//! The draw detours, the sun's re-driven pass, the FSR resolve, and the water block call into the
//! harness at fixed points of a draw (stage captures, the draw census, the water and sun treatments,
//! the submission trace). Each of those points is inert outside a capture, so the path reads the
//! flag with one relaxed load and skips the table while nothing has asked for a capture.

use std::{
    any::Any,
    ffi::c_void,
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use jc3gi::{graphics_engine::graphics_engine::RenderContext, types::math::Vector4};
use jc3vr_core::util::bump;
use windows::Win32::Graphics::Direct3D11::ID3D11Resource;

use crate::{
    capture_arming::{capture_armed, capture_or_benchmark_armed},
    stereo::single_pass::{cb13::STEREO_CB_TOTAL_ROWS, detour_timing},
};

/// The device-context draw entry a submission arrived at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum RawDrawKind {
    Indexed,
    NonIndexed,
    IndexedInstanced,
    IndexedInstancedIndirect,
    InstancedIndirect,
}

impl RawDrawKind {
    pub const COUNT: usize = 5;

    pub fn name(self) -> &'static str {
        match self {
            Self::Indexed => "indexed",
            Self::NonIndexed => "non_indexed",
            Self::IndexedInstanced => "indexed_instanced",
            Self::IndexedInstancedIndirect => "indexed_instanced_indirect",
            Self::InstancedIndirect => "instanced_indirect",
        }
    }
}

/// The FSR inputs a snapshot can keep per eye.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsrPlane {
    /// The motion vectors FSR is handed.
    Motion,
    /// The depth FSR is handed.
    Depth,
}

/// A probe's state held across a submission, released on drop.
pub type ProbeGuard = Box<dyn Any>;

/// The cb13 rows of the sun's re-driven pass.
pub type Cb13Rows = [Vector4; STEREO_CB_TOTAL_ROWS];

/// The capture harness's probes into the single-pass draw path. Each method is the harness side of
/// one point of a draw; the draw path calls it only while a capture has armed the probes.
pub trait SinglePassProbes: Sync {
    /// A submission arriving at a device-context draw entry.
    ///
    /// # Safety
    ///
    /// `context` must be the live immediate context the detoured entry was called on.
    unsafe fn raw_entry(&self, kind: RawDrawKind, context: *mut c_void);
    /// The requested water reflection treatment for the draw, held until it is submitted.
    ///
    /// # Safety
    ///
    /// `context` must be the live immediate context the detoured entry was called on.
    unsafe fn reflection_treatment(&self, context: *mut c_void) -> Option<ProbeGuard>;
    /// Submit a draw through the submission trace, which calls `draw` exactly once.
    fn submit(
        &self,
        context: *mut c_void,
        kind: &'static str,
        args: [i64; 5],
        draw: &mut dyn FnMut(),
    );
    /// Submit a GPU-indirect draw through the submission trace, which calls `draw` exactly once.
    fn submit_indirect(
        &self,
        context: *mut c_void,
        buffer: *mut c_void,
        offset: u32,
        draw: &mut dyn FnMut(),
    );
    /// Observe a non-indexed draw for the anti-aliasing stage.
    ///
    /// # Safety
    ///
    /// `context` must be the live immediate context the detoured entry was called on.
    unsafe fn aa_observe(&self, context: *mut c_void, vertex_count: u32, start_vertex: u32);
    /// Draw the anti-aliasing resolve through its colour-edge treatment, reporting whether it did.
    ///
    /// # Safety
    ///
    /// `context` must be the live immediate context the detoured entry was called on.
    unsafe fn aa_with_color_edge(&self, context: *mut c_void, submit: &dyn Fn()) -> bool;
    /// The sun's requested depth-clip treatment for the raw sun draw, held until it is submitted.
    ///
    /// # Safety
    ///
    /// `context` must be the live immediate context the detoured entry was called on.
    unsafe fn sun_depth_clip_treatment(&self, context: *mut c_void) -> Option<ProbeGuard>;
    /// Collect the raw sun draw's state before it is submitted.
    ///
    /// # Safety
    ///
    /// `context` must be the live immediate context the detoured entry was called on.
    unsafe fn sun_raw_draw(&self, context: *mut c_void, vertex_count: u32, start_vertex: u32);
    /// Read back the GPU work of the raw sun draw after it is submitted.
    ///
    /// # Safety
    ///
    /// `context` must be the live immediate context the detoured entry was called on.
    unsafe fn sun_after_raw_draw(&self, context: *mut c_void);
    /// The sun probe for one re-driven sun pass, when a capture asks for it.
    fn sun_pass(&self, ctx: *const RenderContext) -> Option<Box<dyn SunPassProbe>>;
    /// Draw the water reflection planes' proxy quad through its capture, reporting whether it did.
    ///
    /// # Safety
    ///
    /// `context` must be the live immediate context the detoured entry was called on.
    unsafe fn water_proxy_draw(&self, context: *mut c_void, submit: &dyn Fn()) -> bool;
    /// Draw the depth-of-field fullscreen pass through its capture, reporting whether it did.
    ///
    /// # Safety
    ///
    /// `context` must be the live immediate context the detoured entry was called on.
    unsafe fn dof_draw(&self, context: *mut c_void, submit: &dyn Fn()) -> bool;
    /// Draw the fog-volume generation through its capture, reporting whether it did.
    ///
    /// # Safety
    ///
    /// `context` must be the live immediate context the detoured entry was called on.
    unsafe fn fog_generation_draw(&self, context: *mut c_void, submit: &dyn Fn()) -> bool;
    /// Capture the fog-volume application's inputs.
    ///
    /// # Safety
    ///
    /// `context` must be the live immediate context the detoured entry was called on.
    unsafe fn fog_inputs(&self, context: *mut c_void);
    /// The deferred resolve's probe for a fullscreen draw, held until it is submitted.
    ///
    /// # Safety
    ///
    /// `context` must be the live immediate context the detoured entry was called on.
    unsafe fn resolve_draw(
        &self,
        context: *mut c_void,
        vertex_count: u32,
        start_vertex: u32,
    ) -> Option<ProbeGuard>;
    /// The screen-space reflection march's probe for a draw, dropped once it is submitted.
    ///
    /// # Safety
    ///
    /// `context` must be the live immediate context the detoured entry was called on.
    unsafe fn ssr_before_draw(
        &self,
        context: *mut c_void,
        vertex_count: u32,
        start_vertex: u32,
    ) -> Option<ProbeGuard>;
    /// The screen-space reflection stages after a draw is submitted.
    ///
    /// # Safety
    ///
    /// `context` must be the live immediate context the detoured entry was called on.
    unsafe fn ssr_after_draw(&self, context: *mut c_void, vertex_count: u32, start_vertex: u32);
    /// Count a terrain-patch indirect draw against the requested ordinals: `Some(secondary)` for the
    /// draw a lake patch diagnostic selects.
    fn patch_draw_select(&self) -> Option<bool>;
    /// Observe one submission of a selected terrain-patch draw, held until it is submitted.
    ///
    /// # Safety
    ///
    /// `context` must be the live immediate context the detoured entry was called on.
    unsafe fn patch_draw_observe(
        &self,
        context: *mut c_void,
        offset: u32,
        expected: Option<usize>,
        secondary: bool,
    ) -> Option<ProbeGuard>;
    /// Whether a lake camera experiment brackets the selected patch draw with stage captures.
    fn patch_camera_experiment(&self) -> bool;
    /// Open the patch-owner trace for an indirect draw, reporting whether it did.
    fn patch_owners_before(&self, offset: u32) -> bool;
    /// Close the patch-owner trace for an indirect draw.
    fn patch_owners_after(&self, offset: u32);
    /// Capture the planes around the selected patch draw.
    fn capture_patch_submission(&self, after: bool);
    /// Keep one eye's FSR input for a pending snapshot.
    fn fsr_plane(&self, plane: FsrPlane, eye: usize, resource: &ID3D11Resource);
    /// The water GPU probe for one water surface draw, when a capture asks for it.
    fn water_gpu_probe(&self, pass: i32, eye: usize) -> Option<Box<dyn WaterGpuRun>>;
    /// The water input capture for one water surface draw, held until it is drawn.
    fn water_input_capture(&self, eye: usize) -> ProbeGuard;
    /// The water reflection producer diagnostic for one render pass, held for its walk.
    fn reflection_producer(&self, pass: i32) -> Option<ProbeGuard>;
}

/// The sun probe's bracket of one re-driven sun pass.
pub trait SunPassProbe {
    /// Open eye `eye`'s run, with the live `rows` and the `pinned` rows the eye draws with.
    fn begin_eye(&mut self, eye: usize, rows: &Cb13Rows, pinned: &Cb13Rows);
    /// Close eye `eye`'s run once its pass has drawn.
    fn end_eye(&mut self, eye: usize);
}

/// One water surface draw's GPU probe, finished after the draw.
pub trait WaterGpuRun {
    fn finish(self: Box<Self>);
}

/// Register the capture harness's probes. The first registration stands.
pub fn register_single_pass_probes(probes: &'static dyn SinglePassProbes) {
    if PROBES.set(probes).is_err() {
        tracing::warn!(
            "single pass: capture probes are already registered; the new ones are ignored"
        );
    }
}

/// The registered probes while a capture may be running, `None` otherwise.
#[inline]
pub(crate) fn armed() -> Option<&'static dyn SinglePassProbes> {
    if !capture_armed() {
        return None;
    }
    PROBES.get().copied()
}

/// Submit a draw: through the submission trace while armed, and otherwise straight through the
/// detour timing's forward. Either way the submission is counted while a capture or a benchmark is
/// armed ([`submission_total`]).
#[inline]
pub(crate) fn submit(
    probes: Option<&dyn SinglePassProbes>,
    context: *mut c_void,
    kind: &'static str,
    args: [i64; 5],
    draw: impl FnOnce(),
) {
    count_submission(probes.is_some());
    match probes {
        Some(probes) => {
            let mut draw = Some(draw);
            probes.submit(context, kind, args, &mut || {
                if let Some(draw) = draw.take() {
                    draw();
                }
            });
        }
        None => detour_timing::forward(draw),
    }
}

/// [`submit`] for a GPU-indirect draw.
#[inline]
pub(crate) fn submit_indirect(
    probes: Option<&dyn SinglePassProbes>,
    context: *mut c_void,
    buffer: *mut c_void,
    offset: u32,
    draw: impl FnOnce(),
) {
    count_submission(probes.is_some());
    match probes {
        Some(probes) => {
            let mut draw = Some(draw);
            probes.submit_indirect(context, buffer, offset, &mut || {
                if let Some(draw) = draw.take() {
                    draw();
                }
            });
        }
        None => detour_timing::forward(draw),
    }
}

/// Every submission the stereo draw path has made while a capture or a benchmark was armed, never
/// reset, for samplers that difference two readings (the capture crate's benchmark,
/// `testing::benchmark`, whose request arms the count). Counted only while the draw-path detours are installed.
pub fn submission_total() -> u64 {
    SUBMISSIONS.load(Ordering::Relaxed)
}

/// Keep one eye's FSR input for a pending snapshot.
pub(crate) fn fsr_plane(plane: FsrPlane, eye: usize, resource: &ID3D11Resource) {
    if let Some(probes) = armed() {
        probes.fsr_plane(plane, eye, resource);
    }
}

/// One water surface draw's GPU probe, as the water block brackets the draw with it.
pub(crate) struct WaterGpuProbe(Box<dyn WaterGpuRun>);

impl WaterGpuProbe {
    pub(crate) fn begin(pass: i32, eye: usize) -> Option<Self> {
        armed()?.water_gpu_probe(pass, eye).map(Self)
    }

    pub(crate) fn finish(self) {
        self.0.finish();
    }
}

/// The water input capture for one water surface draw, held until it is drawn.
pub(crate) fn begin_water_input_capture(eye: usize) -> Option<ProbeGuard> {
    armed().map(|probes| probes.water_input_capture(eye))
}

/// The water reflection producer diagnostic for one render pass, held for its walk.
pub(crate) fn reflection_producer(pass: i32) -> Option<ProbeGuard> {
    armed()?.reflection_producer(pass)
}

static PROBES: OnceLock<&'static dyn SinglePassProbes> = OnceLock::new();

/// Count one submission for [`submission_total`]. `capture_armed` is what the caller's
/// [`armed`] read found, which spares the flag a second load on the capture path.
#[inline]
fn count_submission(capture_armed: bool) {
    if capture_armed || capture_or_benchmark_armed() {
        bump(&SUBMISSIONS);
    }
}

static SUBMISSIONS: AtomicU64 = AtomicU64::new(0);
