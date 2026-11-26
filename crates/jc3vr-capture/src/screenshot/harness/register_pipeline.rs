//! The capture tooling's pipeline probes: what the screenshot harness, its snapshots, the visual
//! tests, and the benchmark observe at the graphics-engine pipeline's seams, registered into the
//! render hooks' [`PipelineProbes`] table at startup, with the benchmark's sink for the GPU
//! timestamp lane.
//!
//! The hooks call the table only once a request has armed it ([`jc3vr_render::capture_arming`]), except
//! for the shader tag. Each probe forwards to the capture module that owns it, which decides for
//! itself whether its capture is pending. The table holds no state of its own.

use std::ffi::c_void;

#[cfg(feature = "profiler")]
use jc3gi::graphics_engine::render_engine::RenderBlockTypeBase;
use jc3gi::graphics_engine::{
    graphics_engine::RenderContext,
    post_effects::{AAMode, PostEffectContext},
    render_block::RenderBlockWaterReflectionPlane,
    render_engine::{RenderEngine, RenderPassId},
    render_pass::RenderPass,
};
use jc3vr_render::{
    hooks::graphics_engine::probes::{
        AntiAliasingSide, NearPatchBracket, PipelinePoint, PipelineProbes, register_pipeline_probes,
    },
    screenshot::CaptureSource,
};
#[cfg(feature = "profiler")]
use jc3vr_render::{
    profiler::gpu_samples::GpuDispatchSink, profiler::gpu_samples::register_dispatch_sink,
};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceChild, ID3D11DeviceContext};

use crate::{
    screenshot::{
        harness,
        harness::{
            WaterReflection, constants, draw_census, patch_draw, resolve_probe, snapshot, stages,
            stages::{Stage, capture_prefix_depth, velocity},
        },
    },
    testing::{benchmark, visual},
};

/// Registers the capture tooling's pipeline probes, and the benchmark as the GPU timestamp lane's
/// dispatch sink. Once, at startup, before the hooks install.
pub fn register() {
    register_pipeline_probes(&CAPTURE_PROBES);
    #[cfg(feature = "profiler")]
    register_dispatch_sink(GpuDispatchSink {
        record: benchmark::record_gpu_dispatch,
        dropped: benchmark::note_gpu_dispatch_dropped,
    });
}

static CAPTURE_PROBES: CaptureProbes = CaptureProbes;

struct CaptureProbes;

impl PipelineProbes for CaptureProbes {
    fn draw_task_scope(&self, body: &mut dyn FnMut()) {
        benchmark::note_draw_thread();
        let _sampled = benchmark::sampler::enter_draw_task();
        body();
    }

    #[cfg(feature = "profiler")]
    fn before_block_type_switch(&self) {
        velocity::type_switch(None);
        draw_census::type_switch(None);
    }

    #[cfg(feature = "profiler")]
    fn after_block_type_switch(&self, next: *mut RenderBlockTypeBase) {
        velocity::type_switch(Some(next));
        draw_census::type_switch(Some(next));
        benchmark::sampler::note_block_type(next);
    }

    unsafe fn pass_draw_scope(
        &self,
        _pass: *mut RenderPass,
        pass_id: Option<i16>,
        body: &mut dyn FnMut(),
    ) {
        if let Some(id) = pass_id {
            stages::capture_before_fog_apply(id);
        }
        draw_census::begin_pass();
        body();
        draw_census::end_pass();
        if let Some(id) = pass_id {
            stages::capture_late_sky(id);
        }
    }

    unsafe fn pass_walk_scope(
        &self,
        pass: *mut RenderPass,
        ctx: *mut RenderContext,
        pass_id: i16,
        body: &mut dyn FnMut(),
    ) {
        let _lake_producer = (RenderPassId::PRE_RP_REFLECTION_DISTANT_BACKDROP as i16
            ..=RenderPassId::PRE_RP_REFLECTION_CLOUDS as i16)
            .contains(&pass_id)
            // SAFETY: `pass` and `ctx` are the live pass and render context, per the caller
            // contract.
            .then(|| unsafe { stages::capture_producer_context(pass, ctx) });
        body();
    }

    fn near_patch_bracket(&self) -> Option<NearPatchBracket> {
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

    fn draw_near_patch_bracket(
        &self,
        bracket: NearPatchBracket,
        lo: i32,
        hi: i32,
        skipped: &dyn Fn(i32) -> bool,
        draw_runs: &dyn Fn(i32, i32),
    ) {
        draw_near_patch_bracket(bracket, lo, hi, skipped, draw_runs);
    }

    fn capture_point(&self, point: PipelinePoint) {
        match point {
            PipelinePoint::Geometry => stages::capture_geometry(),
            PipelinePoint::AfterSun => stages::capture_after_sun(),
            PipelinePoint::AfterResolve => stages::capture_after_resolve(),
            PipelinePoint::BeforeAtmosphere => stages::capture_before_atmosphere(),
            PipelinePoint::AfterAtmosphere => stages::capture_after_atmosphere(),
            PipelinePoint::BeforeWater => stages::capture_water_before(),
            PipelinePoint::AfterWater(eye) => stages::capture_water_after(eye),
            PipelinePoint::MainColor => stages::capture_main_color(),
        }
    }

    fn capture_anti_aliasing_slot(&self, side: AntiAliasingSide, mgr: *mut c_void, slot: u32) {
        let stage = match side {
            AntiAliasingSide::Before => Stage::PreAa,
            AntiAliasingSide::After => Stage::PostAa,
        };
        stages::capture_slot(stage, mgr, slot);
    }

    fn anti_aliasing_scope(&self, mode: AAMode, body: &mut dyn FnMut()) {
        let _probe = stages::aa::Scope::begin(mode);
        body();
    }

    fn depth_of_field_scope(
        &self,
        pec: *const PostEffectContext,
        input: u32,
        mgr: *mut c_void,
        body: &mut dyn FnMut() -> u32,
    ) -> u32 {
        let probe = stages::dof::begin(pec, input);
        let slot = body();
        if let Some(probe) = probe {
            probe.finish(mgr, slot);
        }
        slot
    }

    fn resolve_scope(&self, deferred_lights: bool, body: &mut dyn FnMut()) {
        let _scope = resolve_probe::begin(deferred_lights);
        body();
    }

    fn ambient_occlusion_scope(&self, body: &mut dyn FnMut()) {
        snapshot::ambient_occlusion_block(true);
        body();
        snapshot::ambient_occlusion_block(false);
        snapshot::ambient_occlusion_hook();
    }

    fn screen_space_reflection_scope(&self, body: &mut dyn FnMut()) {
        stages::ssr::scope(body);
    }

    fn after_engine_draw(&self) {
        snapshot::ambient_occlusion_draw_hook();
    }

    unsafe fn reflection_proxy_scope(
        &self,
        this: *const RenderBlockWaterReflectionPlane,
        rc: *const RenderContext,
        body: &mut dyn FnMut(),
    ) {
        stages::proxy::scope(this, rc, body);
    }

    fn capture_constants(&self, engine: &RenderEngine, ctx: &RenderContext, cb13_uploaded: bool) {
        constants::capture(engine, ctx, cb13_uploaded);
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
    skipped: &dyn Fn(i32) -> bool,
    draw_runs: &dyn Fn(i32, i32),
) {
    let z = RenderPassId::RP_Z_PASS as i32;
    let velocity = RenderPassId::RP_Z_AND_VELOCITY_PASS as i32;
    if tess
        && lo <= z
        && velocity < near_patch
        && !skipped(z)
        && !skipped(velocity)
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
