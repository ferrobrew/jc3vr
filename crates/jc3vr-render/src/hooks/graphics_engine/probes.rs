//! The pipeline probes: what the capture tooling observes at the graphics-engine pipeline's seams,
//! registered once at startup so the hooks name none of it.
//!
//! The engine-pipeline detours (the passes, the G-buffer range, the post-effects chain, the
//! screen-space blocks, the water, the post-draw's eye copies, the clock, the draw counts, and the
//! render-block type switch) call into the [`PipelineProbes`] table at fixed points. The capture
//! side registers its table ([`register_pipeline_probes`]) before the hooks install.
//!
//! Every probe but the shader tag is inert until the capture side receives a request, so the hooks
//! reach the table through [`armed`] (or [`armed_for_benchmark`] for the probes a benchmark reads),
//! which reads the capture arming flag ([`crate::capture_arming`]) with one relaxed load and
//! answers `None` while nothing has asked for a capture. Unarmed, or with no table registered, the
//! hooks make no call into the table: a scope runs its body directly, a capture is skipped, and a
//! query answers no. Armed, each probe still decides for itself whether its capture is pending. The
//! shader tag ([`registered`]) records state a later capture reads, so it runs whenever a table is
//! registered.

use std::{ffi::c_void, sync::OnceLock};

#[cfg(feature = "profiler")]
use jc3gi::graphics_engine::render_engine::RenderBlockTypeBase;
use jc3gi::graphics_engine::{
    graphics_engine::RenderContext,
    post_effects::{AAMode, PostEffectContext},
    render_block::RenderBlockWaterReflectionPlane,
    render_engine::RenderEngine,
    render_pass::RenderPass,
};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceChild, ID3D11DeviceContext};

use crate::{
    capture_arming::{capture_armed, capture_or_benchmark_armed},
    screenshot::CaptureSource,
};

/// A point in the frame where the capture side may keep a stage image.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PipelinePoint {
    /// The main G-buffer range has completed on a near dispatch.
    Geometry,
    /// The sun's shadow pass has drawn.
    AfterSun,
    /// The clustered deferred lighting has resolved.
    AfterResolve,
    /// The scene's aerial-perspective composite is about to draw.
    BeforeAtmosphere,
    /// The scene's aerial-perspective composite has drawn.
    AfterAtmosphere,
    /// The water surface is about to draw.
    BeforeWater,
    /// The water surface has drawn for the eye this carries.
    AfterWater(usize),
    /// The world post-effects block is about to draw, with the lit scene in the main colour target.
    MainColor,
}

/// Which side of the engine's anti-aliasing a post-chain slot is captured on.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AntiAliasingSide {
    Before,
    After,
}

/// The near terrain patch pass a pending capture brackets inside the main G-buffer range.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct NearPatchBracket {
    /// The render-pass id of the bracketed patch pass.
    pub pass: i32,
    /// Whether the bracketed pass is the tessellated near patch rather than the base mesh.
    pub tessellated: bool,
}

/// What the capture tooling does at the pipeline's seams. Every method has an inert default.
///
/// A scope method runs `body` exactly once, at the point its doc names, and brackets it with the
/// probe's own work.
pub trait PipelineProbes: Sync {
    /// Brackets one draw-thread task (`CGraphicsEngine::HandleDrawThreadTask`). Called while a
    /// capture or a benchmark is armed.
    fn draw_task_scope(&self, body: &mut dyn FnMut()) {
        body();
    }

    /// Runs before the render pass changes its render-block type, while the outgoing run's type is
    /// still current. Called while a capture or a benchmark is armed.
    #[cfg(feature = "profiler")]
    fn before_block_type_switch(&self) {}

    /// Runs after the render pass changed its render-block type to `next`. Called while a capture
    /// or a benchmark is armed.
    #[cfg(feature = "profiler")]
    fn after_block_type_switch(&self, _next: *mut RenderBlockTypeBase) {}

    /// Brackets one `RenderPass::Draw`, whose pass id is `pass_id` when `pass` is non-null.
    ///
    /// # Safety
    ///
    /// `pass`, when non-null, is the live render pass being drawn.
    unsafe fn pass_draw_scope(
        &self,
        _pass: *mut RenderPass,
        _pass_id: Option<i16>,
        body: &mut dyn FnMut(),
    ) {
        body();
    }

    /// Brackets one `RenderPass::DoDraw` walk of pass `pass_id`, from the point the pass is
    /// published to the end of the walk.
    ///
    /// # Safety
    ///
    /// `pass` and `ctx` are the live pass and render context of the walk.
    unsafe fn pass_walk_scope(
        &self,
        _pass: *mut RenderPass,
        _ctx: *mut RenderContext,
        _pass_id: i16,
        body: &mut dyn FnMut(),
    ) {
        body();
    }

    /// The near terrain patch pass a pending capture brackets in the main G-buffer range, if any.
    fn near_patch_bracket(&self) -> Option<NearPatchBracket> {
        None
    }

    /// Draws the passes `[lo, hi)`, which contain `bracket.pass`, with the capture's brackets around
    /// the patch pass and the depth boundaries it asks for. `draw_runs` draws a pass range minus the
    /// passes `skipped` names.
    fn draw_near_patch_bracket(
        &self,
        _bracket: NearPatchBracket,
        lo: i32,
        hi: i32,
        _skipped: &dyn Fn(i32) -> bool,
        draw_runs: &dyn Fn(i32, i32),
    ) {
        draw_runs(lo, hi);
    }

    /// Keeps the stage image of `point`.
    fn capture_point(&self, _point: PipelinePoint) {}

    /// Keeps post-chain slot `slot` of the post-effects manager `mgr`, on `side` of the engine's
    /// anti-aliasing.
    fn capture_anti_aliasing_slot(&self, _side: AntiAliasingSide, _mgr: *mut c_void, _slot: u32) {}

    /// Brackets the engine's anti-aliasing draw in `mode`.
    fn anti_aliasing_scope(&self, _mode: AAMode, body: &mut dyn FnMut()) {
        body();
    }

    /// Brackets the depth-of-field effect's apply on chain slot `input`; `body` runs it and returns
    /// the slot it wrote, which the scope returns.
    fn depth_of_field_scope(
        &self,
        _pec: *const PostEffectContext,
        _input: u32,
        _mgr: *mut c_void,
        body: &mut dyn FnMut() -> u32,
    ) -> u32 {
        body()
    }

    /// Brackets the clustered lighting's `DrawClustered`; `deferred_lights` says whether it runs in
    /// the deferred-lights pass.
    fn resolve_scope(&self, _deferred_lights: bool, body: &mut dyn FnMut()) {
        body();
    }

    /// Brackets the ambient-occlusion block's draw.
    fn ambient_occlusion_scope(&self, body: &mut dyn FnMut()) {
        body();
    }

    /// Brackets the screen-space reflection block's draw.
    fn screen_space_reflection_scope(&self, body: &mut dyn FnMut()) {
        body();
    }

    /// Runs after each engine `Draw` call.
    fn after_engine_draw(&self) {}

    /// Brackets a water reflection plane's proxy draw.
    ///
    /// # Safety
    ///
    /// `this` and `rc` are the live block and render context of the draw.
    unsafe fn reflection_proxy_scope(
        &self,
        _this: *const RenderBlockWaterReflectionPlane,
        _rc: *const RenderContext,
        body: &mut dyn FnMut(),
    ) {
        body();
    }

    /// Keeps the global shader constants the render engine just uploaded for render context
    /// `ctx`; `cb13_uploaded` says whether the dual-eye constant buffer went with them.
    fn capture_constants(
        &self,
        _engine: &RenderEngine,
        _ctx: &RenderContext,
        _cb13_uploaded: bool,
    ) {
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

/// Registers the pipeline probes. Once, at startup, before the hooks install; a second registration
/// is refused with an error log.
pub fn register_pipeline_probes(probes: &'static dyn PipelineProbes) {
    if PROBES.set(probes).is_err() {
        tracing::error!("pipeline probes: already registered; the second registration is ignored");
    }
}

/// The registered probes while a capture has armed them, `None` otherwise.
#[inline]
pub(crate) fn armed() -> Option<&'static dyn PipelineProbes> {
    if !capture_armed() {
        return None;
    }
    registered()
}

/// The registered probes while a capture or a benchmark has armed them, `None` otherwise: for the
/// probes a benchmark reads (the draw-task scope and the render-block type switch).
#[inline]
pub(crate) fn armed_for_benchmark() -> Option<&'static dyn PipelineProbes> {
    if !capture_or_benchmark_armed() {
        return None;
    }
    registered()
}

/// The registered probes, armed or not: for the shader tag.
#[inline]
pub(crate) fn registered() -> Option<&'static dyn PipelineProbes> {
    PROBES.get().copied()
}

/// Keeps the stage image of `point` while a capture is armed.
#[inline]
pub(crate) fn capture_point(point: PipelinePoint) {
    if let Some(probes) = armed() {
        probes.capture_point(point);
    }
}

/// Runs `body` inside a probe scope (one of the [`PipelineProbes`] scope methods, called through
/// `scope` on `probes`) and returns what `body` returned. With no `probes` (unarmed), runs `body`
/// directly.
#[inline]
pub(crate) fn scoped<R>(
    probes: Option<&'static dyn PipelineProbes>,
    scope: impl FnOnce(&'static dyn PipelineProbes, &mut dyn FnMut()),
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
    result.expect("pipeline probes: a scope must run its body")
}

static PROBES: OnceLock<&'static dyn PipelineProbes> = OnceLock::new();
