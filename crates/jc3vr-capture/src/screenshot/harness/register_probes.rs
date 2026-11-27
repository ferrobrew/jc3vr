//! The capture tooling's side of the render hooks' probe table: what the screenshot harness, its
//! snapshots, the visual tests, and the benchmark observe at the hooks' events, registered at
//! startup, with the benchmark's sink for the GPU timestamp lane.
//!
//! The hooks call the table only once a request has armed it ([`jc3vr_render::probes::arm`]), except
//! for the shader tag. The events are generic; this module decides which diagnostic each one feeds
//! (a pass, a draw's pass and arguments, a submission's route) and forwards to the module that owns
//! it, which decides for itself whether its capture is pending. The table holds no state of its
//! own beyond the terrain-patch selection one game draw carries to its submissions.

use std::cell::Cell;

#[cfg(feature = "profiler")]
use jc3gi::graphics_engine::render_engine::RenderBlockTypeBase;
use jc3gi::graphics_engine::{graphics_engine::RenderContext, render_engine::RenderPassId};
#[cfg(feature = "profiler")]
use jc3vr_render::profiler::gpu_samples::{GpuDispatchSink, register_dispatch_sink};
use jc3vr_render::{
    probes,
    probes::{
        BlockScope, CaptureProbes, DrawArgs, DrawScope, GameDraw, PassScope, PerEyePassProbe,
        PipelinePoint, PostEffect, ProbeGuard, RawDrawKind, Route, Submission,
    },
    screenshot::CaptureSource,
};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceChild, ID3D11DeviceContext};

use crate::{
    screenshot::{
        harness,
        harness::{
            WaterReflection, constants, draw_census, patch_draw, resolve_probe, snapshot,
            snapshot::Plane,
            stages,
            stages::{Stage, aa, capture_prefix_depth, dof, patch_owners, proxy, ssr, velocity},
            sun_probe, water_gpu_probe,
        },
    },
    testing::{benchmark, visual},
};

/// Registers the capture tooling's probes, and the benchmark as the GPU timestamp lane's dispatch
/// sink. Once, at startup, before the hooks install.
pub fn register() {
    probes::register(&HARNESS_PROBES);
    #[cfg(feature = "profiler")]
    register_dispatch_sink(GpuDispatchSink {
        record: benchmark::record_gpu_dispatch,
        dropped: benchmark::note_gpu_dispatch_dropped,
    });
}

static HARNESS_PROBES: HarnessProbes = HarnessProbes;

struct HarnessProbes;

impl CaptureProbes for HarnessProbes {
    fn draw_task_scope(&self, body: &mut dyn FnMut()) {
        benchmark::note_draw_thread();
        let _sampled = benchmark::sampler::enter_draw_task();
        body();
    }

    #[cfg(feature = "profiler")]
    fn block_type_switch(&self, next: Option<*mut RenderBlockTypeBase>) {
        velocity::type_switch(next);
        draw_census::type_switch(next);
        if let Some(next) = next {
            benchmark::sampler::note_block_type(next);
        }
    }

    fn point(&self, point: PipelinePoint<'_>) {
        match point {
            PipelinePoint::Geometry => stages::capture_geometry(),
            PipelinePoint::AfterSun => stages::capture_after_sun(),
            PipelinePoint::AfterResolve => stages::capture_after_resolve(),
            PipelinePoint::BeforeAtmosphere => stages::capture_before_atmosphere(),
            PipelinePoint::AfterAtmosphere => stages::capture_after_atmosphere(),
            PipelinePoint::BeforeWater => stages::capture_water_before(),
            PipelinePoint::AfterWater(eye) => stages::capture_water_after(eye),
            PipelinePoint::MainColor => stages::capture_main_color(),
            PipelinePoint::BeforeAntiAliasing { mgr, slot } => {
                stages::capture_slot(Stage::PreAa, mgr, slot)
            }
            PipelinePoint::AfterAntiAliasing { mgr, slot } => {
                stages::capture_slot(Stage::PostAa, mgr, slot)
            }
            PipelinePoint::FsrInputs { eye, motion, depth } => {
                snapshot::eye_plane_hook(Plane::FsrMotion, eye, motion);
                snapshot::eye_plane_hook(Plane::FsrDepth, eye, depth);
            }
            PipelinePoint::GlobalConstants {
                engine,
                ctx,
                cb13_uploaded,
            } => constants::capture(engine, ctx, cb13_uploaded),
            PipelinePoint::EngineDraw => snapshot::ambient_occlusion_draw_hook(),
        }
    }

    unsafe fn block_scope(&self, block: BlockScope, body: &mut dyn FnMut()) {
        match block {
            BlockScope::AntiAliasing(mode) => {
                let _probe = aa::Scope::begin(mode);
                body();
            }
            BlockScope::DeferredResolve { deferred_lights } => {
                let _scope = resolve_probe::begin(deferred_lights);
                body();
            }
            BlockScope::AmbientOcclusion => {
                snapshot::ambient_occlusion_block(true);
                body();
                snapshot::ambient_occlusion_block(false);
                snapshot::ambient_occlusion_hook();
            }
            BlockScope::ScreenSpaceReflection => ssr::scope(body),
            BlockScope::WaterReflectionProxy { block, rc } => proxy::scope(block, rc, body),
            BlockScope::WaterSurface { pass, eye } => {
                let probe = water_gpu_probe::WaterGpuProbe::begin(pass, eye);
                let inputs = water_gpu_probe::begin_water_input_capture(eye);
                body();
                drop(inputs);
                if let Some(probe) = probe {
                    probe.finish();
                }
            }
        }
    }

    fn post_effect_scope(&self, effect: PostEffect, body: &mut dyn FnMut() -> u32) -> u32 {
        match effect {
            PostEffect::DepthOfField { pec, input, mgr } => {
                let probe = dof::begin(pec, input);
                let slot = body();
                if let Some(probe) = probe {
                    probe.finish(mgr, slot);
                }
                slot
            }
        }
    }

    unsafe fn pass_scope(&self, pass: PassScope, body: &mut dyn FnMut()) {
        match pass {
            PassScope::Draw { id } => {
                if let Some(id) = id {
                    stages::capture_before_fog_apply(id);
                }
                draw_census::begin_pass();
                body();
                draw_census::end_pass();
                if let Some(id) = id {
                    stages::capture_late_sky(id);
                }
            }
            PassScope::Walk { pass, ctx, id } => {
                let _lake_producer = (RenderPassId::PRE_RP_REFLECTION_DISTANT_BACKDROP as i16
                    ..=RenderPassId::PRE_RP_REFLECTION_CLOUDS as i16)
                    .contains(&id)
                    // SAFETY: `pass` and `ctx` are the live pass and render context, per the
                    // caller contract.
                    .then(|| unsafe { stages::capture_producer_context(pass, ctx) });
                let _reflection_producer = water_gpu_probe::reflection_producer(id.into());
                body();
            }
        }
    }

    fn draw_gbuffer_passes(&self, lo: i32, hi: i32, draw: &dyn Fn(i32, i32)) {
        match near_patch_bracket() {
            Some(bracket) if (lo..hi).contains(&bracket.pass) => {
                draw_near_patch_bracket(bracket, lo, hi, draw)
            }
            _ => draw(lo, hi),
        }
    }

    fn per_eye_pass(&self, ctx: *const RenderContext) -> Option<Box<dyn PerEyePassProbe>> {
        sun_probe::sun_pass(ctx)
    }

    unsafe fn draw_scope(&self, draw: &GameDraw) -> Option<DrawScope> {
        // SAFETY: per the trait's contract.
        unsafe { water_gpu_probe::reflection_treatment(draw.context) }.map(|guard| DrawScope {
            guard: Box::new(guard),
            binds_pixel_shader: true,
        })
    }

    unsafe fn draw_bound(&self, draw: &GameDraw) -> Option<ProbeGuard> {
        // SAFETY: per the trait's contract.
        unsafe { water_gpu_probe::raw_entry(draw.entry, draw.context) };
        match (draw.entry, draw.args) {
            (RawDrawKind::InstancedIndirect, DrawArgs::Indirect { offset, .. }) => {
                PatchDraw::begin(offset).map(|guard| Box::new(guard) as ProbeGuard)
            }
            _ => None,
        }
    }

    fn submit(&self, submission: &Submission, draw: &dyn Fn()) {
        let trace = || trace(submission, draw);
        match submission.entry {
            RawDrawKind::NonIndexed if submission.route == Route::AsIssued => {
                // SAFETY: the submission's context is the live immediate context its entry was
                // called on.
                unsafe { submit_fullscreen(submission, &trace) }
            }
            RawDrawKind::InstancedIndirect => {
                let expected = match submission.route {
                    Route::PerEye(eye) => Some(eye),
                    Route::AsIssued | Route::Promoted => None,
                };
                let observed = PATCH_SELECTION.get().zip(match submission.args {
                    DrawArgs::Indirect { offset, .. } => Some(offset),
                    DrawArgs::Direct(_) => None,
                });
                // SAFETY: as above.
                let guard = observed.and_then(|(secondary, offset)| unsafe {
                    patch_draw::observe(submission.context, offset, expected, secondary)
                });
                trace();
                drop(guard);
            }
            _ => trace(),
        }
    }

    fn tag_shader(&self, program: &ID3D11DeviceChild, bytecode: &[u8]) {
        patch_draw::tag(program, bytecode);
    }

    fn wants_eyes(&self) -> bool {
        harness::wants_eyes()
    }

    fn note_eye(&self, eye: usize, fresh: bool) {
        harness::note_eye(eye, fresh);
    }

    fn capture_frame(
        &self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        source: CaptureSource<'_>,
    ) {
        snapshot::capture(device, context, &source);
        harness::capture(device, context, source);
    }

    fn clock_held(&self) -> bool {
        visual::clock_held()
    }
}

/// Submit through the velocity trace, under the name the trace records the submission by.
fn trace(submission: &Submission, draw: &dyn Fn()) {
    let promoted = submission.route == Route::Promoted;
    let kind = match (submission.entry, promoted) {
        (RawDrawKind::Indexed, false) => "indexed",
        (RawDrawKind::Indexed, true) => "indexed_promoted",
        (RawDrawKind::NonIndexed, false) => "draw",
        (RawDrawKind::NonIndexed, true) => "draw_promoted",
        (RawDrawKind::IndexedInstanced, false) => "indexed_instanced",
        (RawDrawKind::IndexedInstanced, true) => "indexed_instanced_promoted",
        (RawDrawKind::IndexedInstancedIndirect, false) => "indexed_instanced_indirect",
        (RawDrawKind::IndexedInstancedIndirect, true) => "indexed_instanced_indirect_promoted",
        (RawDrawKind::InstancedIndirect, _) => "instanced_indirect",
    };
    match (submission.entry, submission.args) {
        (RawDrawKind::InstancedIndirect, DrawArgs::Indirect { buffer, offset }) => {
            velocity::submit_indirect(submission.context, buffer, offset, draw)
        }
        (_, DrawArgs::Indirect { offset, .. }) => {
            velocity::submit(submission.context, kind, [offset.into(), 0, 0, 0, 0], draw)
        }
        (_, DrawArgs::Direct(args)) => velocity::submit(submission.context, kind, args, draw),
    }
}

/// Submit a non-indexed draw on the route no stereo treatment took (the fullscreen passes, the sun,
/// and the water reflection planes' proxy quad) through the diagnostics that read or treat it, each
/// keyed on the pass and the draw's vertex range. `trace` submits the draw through the velocity
/// trace; a treatment that draws once per eye calls it once per eye.
///
/// # Safety
///
/// `submission.context` is the live immediate context the draw's entry was called on.
unsafe fn submit_fullscreen(submission: &Submission, trace: &dyn Fn()) {
    let context = submission.context;
    let DrawArgs::Direct(args) = submission.args else {
        trace();
        return;
    };
    let (vertex_count, start_vertex) = (args[0] as u32, args[1] as u32);
    let fullscreen = (vertex_count, start_vertex) == (3, 0);
    let pass = submission.pass;
    // SAFETY (every call below): `context` is the live immediate context, per this function's
    // contract.
    unsafe { aa::observe(context, vertex_count, start_vertex) };
    if unsafe { aa::with_color_edge(context, trace) } {
        return;
    }
    if pass == RenderPassId::RP_SUN as u8 {
        let _depth_clip = unsafe { sun_probe::depth_clip_treatment(context) };
        unsafe { sun_probe::on_raw_draw(context, vertex_count, start_vertex) };
        trace();
        unsafe { sun_probe::after_raw_draw(context) };
        return;
    }
    if pass == RenderPassId::RP_REFLECTIVE_WATER_PLANES as u8
        && (vertex_count, start_vertex) == (6, 0)
        && unsafe { proxy::on_draw(context, &trace) }
    {
        return;
    }
    if pass == RenderPassId::RP_POSTEFFECTS as u8
        && fullscreen
        && unsafe { dof::on_draw(context, trace) }
    {
        return;
    }
    if pass == RenderPassId::RP_FOG_VOLUME_GENERATE as u8
        && fullscreen
        && unsafe { stages::capture_fog_generation(context, trace) }
    {
        return;
    }
    if pass == RenderPassId::RP_FOG_VOLUME_APPLY as u8 && fullscreen {
        unsafe { stages::capture_fog_inputs(context) };
    }
    let _cascade = unsafe { resolve_probe::on_draw(context, vertex_count, start_vertex) };
    let ray = unsafe { ssr::before_draw(context, vertex_count, start_vertex) };
    trace();
    drop(ray);
    unsafe { ssr::after_draw(context, vertex_count, start_vertex) };
}

/// One terrain-patch indirect draw's bracket: the patch selection its submissions read, the owner
/// trace, and the stage captures a lake camera experiment takes around the selected draw.
struct PatchDraw {
    offset: u32,
    owners: bool,
    bracket: bool,
}

impl PatchDraw {
    /// Count the draw against the requested ordinals and open its traces, `None` when nothing
    /// selects or traces it.
    fn begin(offset: u32) -> Option<Self> {
        let selection = patch_draw::select();
        let bracket = selection == Some(false) && patch_draw::camera_experiment();
        let owners = patch_owners::before(offset);
        if selection.is_none() && !owners {
            return None;
        }
        PATCH_SELECTION.set(selection);
        if bracket {
            stages::capture_patch_submission(false);
        }
        Some(Self {
            offset,
            owners,
            bracket,
        })
    }
}

impl Drop for PatchDraw {
    fn drop(&mut self) {
        PATCH_SELECTION.set(None);
        if self.owners {
            patch_owners::after(self.offset);
        }
        if self.bracket {
            stages::capture_patch_submission(true);
        }
    }
}

thread_local! {
    /// The selection of the terrain-patch draw in flight on this thread: `Some(secondary)` for the
    /// draw a lake patch diagnostic selects.
    static PATCH_SELECTION: Cell<Option<bool>> = const { Cell::new(None) };
}

/// The near terrain patch pass a pending capture brackets inside the main G-buffer range.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct NearPatchBracket {
    /// The render-pass id of the bracketed patch pass.
    pass: i32,
    /// Whether the bracketed pass is the tessellated near patch rather than the base mesh.
    tessellated: bool,
}

/// The near terrain patch pass the pending capture brackets in the main G-buffer range, if any.
fn near_patch_bracket() -> Option<NearPatchBracket> {
    match harness::requested_water_diagnostic()? {
        WaterReflection::LakeNearPatch => Some(NearPatchBracket {
            pass: RenderPassId::RP_TERRAINPATCH_BASEMESH_NEAR as i32,
            tessellated: false,
        }),
        WaterReflection::LakeTessPatch
        | WaterReflection::LakePatchDraw
        | WaterReflection::LakeCameraControl
        | WaterReflection::LakePatchOwners
        | WaterReflection::LakeDomainCamera
        | WaterReflection::LakeHullCamera => Some(NearPatchBracket {
            pass: RenderPassId::RP_TERRAINPATCH_BASEMESH_TESSELLATE_NEAR as i32,
            tessellated: true,
        }),
        _ => None,
    }
}

/// Draws the main G-buffer passes `[lo, hi)` around the near terrain patch pass a pending capture
/// brackets: the depth boundaries before it, the patch draw itself under the patch probes, and the
/// depth boundaries of the geometry after it, each as the capture asks for them.
fn draw_near_patch_bracket(
    NearPatchBracket {
        pass: near_patch,
        tessellated: tess,
    }: NearPatchBracket,
    lo: i32,
    hi: i32,
    draw_runs: &dyn Fn(i32, i32),
) {
    let z = RenderPassId::RP_Z_PASS as i32;
    let velocity = RenderPassId::RP_Z_AND_VELOCITY_PASS as i32;
    if tess
        && lo <= z
        && velocity < near_patch
        && harness::frame_identity().is_some_and(harness::early_depth_requested)
    {
        draw_runs(lo, z);
        capture_prefix_depth(Stage::BeforeZPassDepth);
        draw_runs(z, velocity);
        capture_prefix_depth(Stage::AfterZPassDepth);
        velocity::scope(velocity::Scope::Velocity, || {
            draw_runs(velocity, velocity + 1)
        });
        capture_prefix_depth(Stage::AfterZVelocityDepth);
        draw_runs(velocity + 1, near_patch);
    } else {
        draw_runs(lo, near_patch);
    }
    stages::capture_near_patch(false, tess);
    harness::patch_draw::scope(tess, || {
        if tess {
            velocity::scope(velocity::Scope::Terrain, || {
                draw_runs(near_patch, near_patch + 1)
            });
        } else {
            draw_runs(near_patch, near_patch + 1);
        }
    });
    stages::capture_near_patch(true, tess);
    velocity::scope(velocity::Scope::BarkColor, || {
        let mut start = near_patch + 1;
        if harness::frame_identity().is_some_and(harness::post_geometry_requested) {
            for (end, stage) in [
                (
                    RenderPassId::RP_MODELS_DYNAMIC as i32,
                    Stage::BeforeDynamicModelsDepth,
                ),
                (
                    RenderPassId::RP_MODELS_DYNAMIC as i32 + 1,
                    Stage::AfterDynamicModelsDepth,
                ),
                (
                    RenderPassId::RP_MODELS_STATIC as i32 + 1,
                    Stage::AfterStaticModelsDepth,
                ),
                (
                    RenderPassId::RP_VEGETATION_OPAQUE as i32 + 1,
                    Stage::AfterOpaqueVegetationDepth,
                ),
                (
                    RenderPassId::RP_TERRAIN_FOREST as i32 + 1,
                    Stage::AfterForestDepth,
                ),
            ] {
                if start < end && end <= hi {
                    if stage == Stage::BeforeDynamicModelsDepth
                        && harness::frame_identity()
                            .is_some_and(harness::terrain_interval_requested)
                    {
                        let mut next = start;
                        for (pass, boundary) in [
                            (
                                RenderPassId::RP_TERRAINPATCH_BASEMESH_NEAR,
                                Stage::AfterBaseNearDepth,
                            ),
                            (
                                RenderPassId::RP_TERRAINPATCH_BASEMESH_TESSELLATE_FAR,
                                Stage::AfterTessFarDepth,
                            ),
                            (
                                RenderPassId::RP_TERRAINPATCH_BASEMESH_FAR,
                                Stage::AfterBaseFarDepth,
                            ),
                            (
                                RenderPassId::RP_TERRAINPATCH_BASEMESH_TESSELLATE_COLOR,
                                Stage::AfterTessColorDepth,
                            ),
                            (
                                RenderPassId::RP_TERRAINPATCH_BASEMESH_COLOR,
                                Stage::AfterBaseColorDepth,
                            ),
                            (
                                RenderPassId::RP_TERRAIN_APPLY_NEAR_DETAILED,
                                Stage::AfterApplyNearDetailedDepth,
                            ),
                            (
                                RenderPassId::RP_TERRAIN_APPLY_NEAR,
                                Stage::AfterApplyNearDepth,
                            ),
                        ] {
                            let after = pass as i32 + 1;
                            if pass == RenderPassId::RP_TERRAINPATCH_BASEMESH_NEAR {
                                velocity::scope(velocity::Scope::Base, || draw_runs(next, after));
                            } else {
                                draw_runs(next, after);
                            }
                            capture_prefix_depth(boundary);
                            next = after;
                        }
                        draw_runs(next, end);
                    } else if stage == Stage::AfterForestDepth
                        && harness::frame_identity().is_some_and(harness::interval_trace_requested)
                    {
                        velocity::scope(velocity::Scope::Interval, || {
                            for (pass, boundary) in [
                                (RenderPassId::RP_VEGETATIONFINS, Stage::AfterFinsDepth),
                                (RenderPassId::RP_VEGETATIONGROUP, Stage::AfterGroupDepth),
                                (RenderPassId::RP_VEGETATIONGROUP2, Stage::AfterGroup2Depth),
                                (RenderPassId::RP_TERRAIN_FOREST, Stage::AfterForestDepth),
                            ] {
                                draw_runs(pass as i32, pass as i32 + 1);
                                if boundary != Stage::AfterForestDepth {
                                    capture_prefix_depth(boundary);
                                }
                            }
                        });
                    } else {
                        draw_runs(start, end);
                    }
                    capture_prefix_depth(stage);
                    start = end;
                }
            }
        }
        draw_runs(start, hi);
    });
}
