//! Detours that gate per-frame render-list state so it advances only once per *real* frame, even
//! though we render the scene twice (once per eye). See `docs/mod/stereo/single-pass-stereo.md`.
//!
//! `SetupRenderFrameData` (the per-batch list *build*, not the swap) and `HandBackBuffers`
//! (constant-buffer recycle) run on both eyes. The add/draw list parity is handled separately by
//! saving and restoring `current_add_buffer` in `game::game_update_render` between eyes, so the
//! per-frame `CKeep1000Frames` call (which toggles the parity and calls `SaveRenderFrameData` on
//! every pass) runs on both eyes and produces the same list pointers on both.

use std::{
    ffi::c_void,
    sync::atomic::{AtomicU32, Ordering},
};

use detours_macro::detour;
use jc3gi::graphics_engine::{
    graphics_engine::{HContext_t, RenderContext},
    render_engine::{RenderEngine, RenderPassId},
    render_pass::{RenderPass, RenderPassState},
    shadow_manager::ShadowManager,
};
use jc3vr_view::{
    stereo::{STEREO_STATE, config::STEREO_CONFIG, is_second_eye},
    vr::{
        FOVEATION_CONFIG,
        foveation::{EyeLayout, FORCE_STENCIL_TEST, FoveatedPasses, FoveationParams},
    },
};
use re_utilities::hook_library::HookLibrary;

#[cfg(feature = "profiler")]
use crate::profiler::gpu::seam;
use crate::{
    debug::trace::{TraceEvent, TraceState, tracing_active},
    hooks::graphics_engine::{
        probes,
        probes::{PipelinePoint, capture_point, scoped},
        screen::reconstruction,
    },
    stereo::single_pass::{ao_volumes, is_sun_pass, reissue_sun_pass},
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&RENDER_PASS_DRAW_BINDER)
        .with_static_binder(&DO_DRAW_BINDER)
        .with_static_binder(&DRAW_RENDER_PASS_RANGE_BINDER)
        .with_static_binder(&PRE_DRAW_BINDER)
        .with_static_binder(&DRAW_POSTEFFECTS_BINDER)
        .with_static_binder(&SETUP_RENDER_STATES_BINDER)
        .with_static_binder(&SET_GLOBAL_SHADER_CONSTANTS_BINDER)
        .with_static_binder(&SHADOW_MANAGER_UPDATE_RENDER_BINDER)
}

// RenderPass::Draw -- the per-pass draw entry.
#[detour(address = jc3gi::graphics_engine::render_pass::RenderPass::Draw_ADDRESS)]
fn render_pass_draw(this: *mut RenderPass) {
    let pass_id = unsafe { this.as_ref() }.map(|pass| pass.m_Index);
    scoped(
        probes::armed(),
        // SAFETY: `this` is the live render pass the engine is drawing.
        |probes, body| unsafe { probes.pass_draw_scope(this, pass_id, body) },
        || RENDER_PASS_DRAW.get().unwrap().call(this),
    );
}

// RenderPass::DoDraw -- one pass's walk over its sorted draw list.
#[detour(address = jc3gi::graphics_engine::render_pass::RenderPass::DoDraw_ADDRESS)]
fn do_draw(this: *mut RenderPass, ctx: *mut RenderContext, color_mask: u32) -> bool {
    // A CPU scope named for the pass being drawn; the per-render-block-type runs opened inside it
    // by `profiler::change_render_block_type` nest within this scope.
    #[cfg(feature = "profiler")]
    // SAFETY: `this` is the live render pass being drawn.
    let _pass = unsafe { crate::profiler::gpu::pass_scope(this) };
    // Publish the pass for the duration of the walk: the draw detours see only a D3D context, and
    // which pass a draw belongs to is what tells a geometry draw apart from a fullscreen one.
    // SAFETY: `this` is the live render pass; `m_Index` is its render-pass id.
    let pass_id = unsafe { (*this).m_Index };
    let previous_pass = crate::stereo::single_pass::set_current_pass(pass_id);
    scoped(
        probes::armed(),
        // SAFETY: `this` and `ctx` are the live pass and render context of this draw call.
        |probes, body| unsafe { probes.pass_walk_scope(this, ctx, pass_id, body) },
        || {
            // Capture-gated producer diagnostic for the passes that feed the water's reflection
            // inputs; inert for every other pass and for an unarmed capture.
            let _reflection_producer =
                crate::stereo::single_pass::reflection_producer(pass_id.into());
            // The ambient-occlusion volumes' per-eye rows, bound for the walk of their pass.
            // SAFETY: `ctx` is the live render context of this pass.
            let _ao_volume_rows = (pass_id == RP_AO_VOLUMES as i16)
                .then(|| unsafe { ao_volumes::bind_rows(ctx) })
                .flatten();
            let sun_pass = is_sun_pass(pass_id);
            let result = if sun_pass {
                reissue_sun_pass(ctx, &|| DO_DRAW.get().unwrap().call(this, ctx, color_mask))
                    .unwrap_or_else(|| DO_DRAW.get().unwrap().call(this, ctx, color_mask))
            } else {
                DO_DRAW.get().unwrap().call(this, ctx, color_mask)
            };
            crate::stereo::single_pass::restore_current_pass(previous_pass);
            if sun_pass {
                capture_point(PipelinePoint::AfterSun);
            }
            // Close the final render-block-type run's scope before the pass scope closes.
            #[cfg(feature = "profiler")]
            jc3vr_core::profiler::type_scope::clear();
            result
        },
    )
}

const RP_AO_VOLUMES: i32 = RenderPassId::RP_AO_VOLUMES as i32;
const RP_SCREEN_SPACE_REFLECTIONS: i32 = RenderPassId::RP_SCREEN_SPACE_REFLECTIONS as i32;
const RP_GLOBAL_ILLUMINATION: i32 = RenderPassId::RP_GLOBAL_ILLUMINATION as i32;

// RenderEngine::DrawRenderPassRange -- draws the half-open pass-index range [first, last). The engine
// calls it three times per dispatch, from `CGraphicsEngine::HandleDrawThreadTask` in this order:
// `DrawGBuffer` (`RP_Z_OCCLUDERS`..`RP_LAST_GBUFFER`), `Draw`
// (`RP_REFLECTIVE_WATER_PLANES`..`RP_POSTEFFECTS`), and `DrawPosteffects`
// (`RP_POSTEFFECTS`..`RP_LAST_MAIN`); the bounds are compile-time constants at each call site, so
// the ranges never vary between frames. `PreDraw` (the shadow, reflection, and vegetation prepasses)
// runs before all three and does not come through here.
//
// The capture overrides that leave out SSR, GI, or the AO volumes drop those passes by splitting
// the range around them, so every other pass runs untouched.
#[detour(address = jc3gi::graphics_engine::render_engine::RenderEngine::DrawRenderPassRange_ADDRESS)]
fn draw_render_pass_range(
    this: *mut c_void,
    ctx: *mut c_void,
    setup: *mut c_void,
    first: i32,
    last: i32,
) {
    #[cfg(feature = "profiler")]
    puffin::profile_scope!("DrawRenderPassRange", format!("{first}..{last}"));
    let original = DRAW_RENDER_PASS_RANGE.get().unwrap();
    let (skip_ssr, skip_gi, skip_ao_volumes) =
        STEREO_CONFIG.query(|c| (c.skip_ssr, c.skip_gi, c.skip_ao_volumes));

    let skipped = |pass: i32| {
        (skip_ssr && pass == RP_SCREEN_SPACE_REFLECTIONS)
            || (skip_gi && pass == RP_GLOBAL_ILLUMINATION)
            || (skip_ao_volumes && pass == RP_AO_VOLUMES)
    };

    // Draw maximal runs of non-skipped passes in [lo, hi), omitting the skipped ones.
    let draw_runs = |lo: i32, hi: i32| {
        let mut run_start = lo;
        for pass in lo..hi {
            if skipped(pass) {
                if run_start < pass {
                    original.call(this, ctx, setup, run_start, pass);
                }
                run_start = pass + 1;
            }
        }
        if run_start < hi {
            original.call(this, ctx, setup, run_start, hi);
        }
    };

    let bracket = (first == RP_Z_OCCLUDERS && last == RenderPassId::RP_LAST_GBUFFER as i32)
        .then(|| {
            let probes = probes::armed()?;
            Some((probes, probes.near_patch_bracket()?))
        })
        .flatten();
    let draw = |lo, hi| {
        if let Some((probes, bracket)) = bracket
            && (lo..hi).contains(&bracket.pass)
            && !skipped(bracket.pass)
        {
            probes.draw_near_patch_bracket(bracket, lo, hi, &skipped, &draw_runs);
        } else {
            draw_runs(lo, hi);
        }
    };

    // Single-pass stereo: mark the camera-scene geometry range so the dual-eye cb13,
    // viewport split, and instance doubling apply to it. `first >= RP_Z_OCCLUDERS` selects the main
    // camera scene, excluding the shadow/reflection prepasses (<= RP_LAST_PREPASS) that reuse the same
    // patched shaders under a different (sun/reflection) view. Without collapse the range is just the
    // G-buffer (`..RP_FIRST_SCENE`), a diagnostic that renders each eye into a half; with collapse it
    // extends to the whole camera scene so the later geometry passes (water, sky, transparents) route
    // to the eyes too -- there the viewport split moves to draw time (patched geometry only), so the
    // interleaved fullscreen lighting/post passes keep the full width. A no-op unless dual-eye is on.
    let scene_geometry = crate::stereo::single_pass::dual_eye_active()
        && first >= RP_Z_OCCLUDERS
        && (jc3vr_view::stereo::layout::collapse_active() || last <= RP_FIRST_SCENE);
    let range = scene_geometry.then(|| {
        let range = crate::stereo::single_pass::enter_gbuffer_range();
        // The main G-buffer viewport was bound before this range flag went up, so re-split it now.
        // (A no-op under collapse, which splits per-draw instead.)
        crate::stereo::single_pass::apply_eye_split_viewport();
        range
    });
    run_scene_range(first, last, &draw);
    capture_geometry_range(first, last);
    if range.is_some() {
        drop(range);
        crate::stereo::single_pass::log_draw_split(first as u32, last as u32);
    }
}

fn capture_geometry_range(first: i32, last: i32) {
    if first == RP_Z_OCCLUDERS && last == RenderPassId::RP_LAST_GBUFFER as i32 {
        capture_point(PipelinePoint::Geometry);
    }
}

/// The first pass of the scene (lighting/main) block.
const RP_FIRST_SCENE: i32 = RenderPassId::RP_REFLECTIVE_WATER_PLANES as i32;
/// The head of the G-buffer range (where the depth clear lands via the pass's own PreDraw flags).
const RP_Z_OCCLUDERS: i32 = RenderPassId::RP_Z_OCCLUDERS as i32;

/// One (possibly sub-)range of the G-buffer/scene pass chain: the maximal-run skip splitting plus
/// the foveation bracketing.
fn run_scene_range(first: i32, last: i32, draw: &dyn Fn(i32, i32)) {
    // Foveation (issue #29): write the peripheral stencil mask just before the first foveated pass,
    // then force the peripheral stencil test through each run of foveated passes so the GPU skips the
    // dropped pixels. The reconstruction fill-in runs once the last foveated pass has lit the kept
    // pixels: here, before the first pass that reads the lit colour's neighbourhood, when the
    // lighting passes are foveated, and otherwise in the `DrawPosteffects` hook. `plan` is `None`
    // when foveation is off or this is not a VR eye render.
    let Some(plan) = foveation_plan(first, last) else {
        draw(first, last);
        return;
    };
    let fill_before = plan.passes.fill_before();
    let mut start = first;
    while start < last {
        if fill_before == Some(start) {
            run_foveation_pass(
                jc3vr_view::vr::foveation::fill_in_dropped,
                plan.params,
                "foveation fill-in",
            );
        }
        let foveated = plan.passes.contains(start);
        let end = (start + 1..last)
            .find(|&pass| plan.passes.contains(pass) != foveated || fill_before == Some(pass))
            .unwrap_or(last);
        if foveated {
            if start == plan.passes.first() {
                run_foveation_pass(
                    jc3vr_view::vr::foveation::mask_write,
                    plan.params,
                    "foveation mask-write",
                );
            }
            FORCE_STENCIL_TEST.store(true, Ordering::Relaxed);
            draw(start, end);
            FORCE_STENCIL_TEST.store(false, Ordering::Relaxed);
        } else {
            draw(start, end);
        }
        start = end;
    }
}

/// The foveation bracketing for one [`draw_render_pass_range`] call: which passes take the forced
/// stencil test, and the per-eye pass parameters. The mask-write runs in the call whose range holds
/// the first foveated pass, so it runs once even if the scene range is split across calls.
struct FoveationPlan {
    passes: FoveatedPasses,
    params: FoveationParams,
}

/// Build the [`FoveationPlan`] for a draw-range call, or `None` when foveation is disabled, this is not a
/// VR eye render, or no foveated pass lies in `[first, last)`. Publishes the per-draw stencil test bits
/// ([`jc3vr_view::vr::foveation::STENCIL_TEST_BITS`]) when active.
fn foveation_plan(first: i32, last: i32) -> Option<FoveationPlan> {
    let cfg = FOVEATION_CONFIG.lock().clone();
    let (passes, quads) = STEREO_CONFIG.query(|c| {
        let sp = &c.single_pass;
        (
            FoveatedPasses {
                gbuffer: sp.foveation_gbuffer,
                lighting: sp.foveation_lighting,
            },
            sp.foveation_quads,
        )
    });
    if !cfg.enabled {
        return None;
    }
    if let Err(e) = cfg.validate() {
        tracing::warn!("foveation disabled: {e}");
        return None;
    }
    if !(first..last).any(|pass| passes.contains(pass) || passes.fill_before() == Some(pass)) {
        return None;
    }
    let layout = foveation_layout()?;
    jc3vr_view::vr::foveation::STENCIL_TEST_BITS.store(
        jc3vr_view::vr::foveation::packed_stencil_test(cfg.mask_bit),
        Ordering::Relaxed,
    );
    Some(FoveationPlan {
        passes,
        params: foveation_params(&cfg, layout, quads),
    })
}

/// Where the eyes this dispatch draws lie on the main buffer, with their foveal centres. Under the
/// single-pass collapse with `single_pass.foveation_per_eye` on, the double-wide buffer holds both
/// eyes side by side; otherwise it holds the dispatch's eye (eye 0's under the collapse, whose one
/// fovea then lands on the seam). `None` when no VR frame is in flight.
fn foveation_layout() -> Option<EyeLayout> {
    if jc3vr_view::stereo::layout::double_wide_active()
        && STEREO_CONFIG.query(|c| c.single_pass.foveation_per_eye)
    {
        return Some(EyeLayout::SideBySide {
            center_uv: [foveal_center_uv(0)?, foveal_center_uv(1)?],
        });
    }
    Some(EyeLayout::Single {
        center_uv: foveal_center_uv(usize::from(is_second_eye()))?,
    })
}

/// Build the pass parameters from the config and the eyes' layout.
fn foveation_params(
    cfg: &jc3vr_view::vr::FoveationConfig,
    layout: EyeLayout,
    quads: bool,
) -> FoveationParams {
    FoveationParams {
        layout,
        inner_fraction: cfg.inner_fraction,
        outer_fraction: cfg.outer_fraction,
        max_drop: cfg.max_drop,
        mask_bit: cfg.mask_bit,
        debug_show_mask: cfg.debug_show_mask,
        cell_px: if quads { 2 } else { 1 },
    }
}

/// The per-eye foveal centre as a UV, from the eye's projection principal point (`m02`/`m12` shear terms
/// of the column-major projection): symmetric frustums map to the buffer centre, canted HMDs to their
/// off-axis centre. `None` when no VR frame is in flight. Clamped to the buffer. The projection is
/// taken without FSR's sub-pixel jitter, which moves the same terms, so the mask holds still in the
/// eye's pixels while the scene jitters under it.
fn foveal_center_uv(eye: usize) -> Option<glam::Vec2> {
    // The glam bridge's `z_axis` is the engine layout's `m02`/`m12` row.
    let m = jc3vr_view::vr::render_params(eye)?.unjittered_projection_standard();
    Some(glam::Vec2::new(
        (0.5 - 0.5 * m.z_axis.x).clamp(0.0, 1.0),
        (0.5 + 0.5 * m.z_axis.y).clamp(0.0, 1.0),
    ))
}

/// Run one foveation D3D pass, warning on any failure without disturbing the surrounding scene draw.
fn run_foveation_pass(
    pass: fn(FoveationParams) -> anyhow::Result<()>,
    params: FoveationParams,
    label: &'static str,
) {
    #[cfg(feature = "profiler")]
    let _gpu = crate::profiler::gpu::detail_interval(label);
    if let Err(e) = pass(params) {
        tracing::warn!(target: "vr", "{label} pass failed: {e:#}");
    }
}

// RenderEngine::DrawPosteffects -- the post-effects pass, drawn once per eye after the scene is fully
// composed (opaque + lighting + sky + transparency resolved into MainColor) and before post-processing
// reads it. Foveation's reconstruction fill-in runs here (issue #29): by now the peripheral pixels the
// mask/force-test dropped in the GBuffer passes have been lit black, so the fill reconstructs each from
// its kept neighbours in the finished MainColor. Runs for both eyes; a no-op when foveation is off.
#[detour(address = jc3gi::graphics_engine::render_engine::RenderEngine::DrawPosteffects_ADDRESS)]
fn draw_posteffects(this: *mut c_void, ctx: *mut c_void, setup: *mut c_void) {
    #[cfg(feature = "profiler")]
    puffin::profile_scope!("RenderEngine::DrawPosteffects");
    #[cfg(feature = "profiler")]
    // SAFETY: `ctx` is the live immediate-context handle for this dispatch.
    let _gpu = unsafe { seam(ctx.cast(), crate::profiler::gpu::GpuSeam::PostEffects) };
    let (lighting, quads) = STEREO_CONFIG.query(|c| {
        (
            c.single_pass.foveation_lighting,
            c.single_pass.foveation_quads,
        )
    });
    if let Some(cfg) = FOVEATION_CONFIG.query(|f| (f.enabled && !lighting).then(|| f.clone()))
        && let Some(layout) = foveation_layout()
    {
        run_foveation_pass(
            jc3vr_view::vr::foveation::fill_in,
            foveation_params(&cfg, layout, quads),
            "foveation fill-in",
        );
    }
    DRAW_POSTEFFECTS.get().unwrap().call(this, ctx, setup);
}

// Graphics::SetupRenderStates -- the pre-draw pipeline-state flush that decodes each dirty packed state
// index on the context and issues the underlying D3D `OMSet*State` binds. While foveation forces the
// peripheral stencil test (issue #29), rewrite the staged depth-stencil index of each non-stencil draw to
// add an `EQUAL 0` test against the mask bit, so the GPU skips the pixels the mask pass tagged. Patched
// before the original flushes; a no-op unless foveation is mid-range (see `jc3vr_view::vr::foveation`).
#[detour(address = jc3gi::graphics_engine::graphics_engine::SetupRenderStates_ADDRESS)]
fn setup_render_states(context: *mut HContext_t, a2: *mut c_void) {
    // SAFETY: `context` is the live render context the game passed to its own state flush.
    unsafe { jc3vr_view::vr::foveation::apply_force_test(context) };
    reconstruction::before_render_states();
    SETUP_RENDER_STATES.get().unwrap().call(context, a2);
    reconstruction::after_render_states();
}

// ShadowManager::UpdateRender -- the sim-side sun-shadow update, which fits the scheduled cascades to
// the active camera (the fit frustum comes from its m_ProjectionF via CFrustum::Compute). Two scoped
// tweaks around the fit, both restored after:
//   * Widen the two FOV-scale terms (data[0]/data[5]) to the union FOV so the cascades are fit to
//     cover BOTH eyes (VR only). The fit is once-per-frame from the narrow centre camera, so the wider,
//     laterally shifted VR eyes otherwise exceed the fitted coverage box -- their distant shadows fall
//     outside it and disagree between the eyes, and the boundary crawls under motion. This is the
//     coverage half of the shadow fix; the cascade-anchor correction in `set_global_shader_constants`
//     is the sampling half. Costs some shadow resolution. The centre, shear, and z (near/far/split)
//     terms are left untouched, so split distances are unchanged.
//   * Stabilize the fit against head orientation: horizontalize the active camera's forward vector (m_TransformT1 row 2,
//     data[8..10]) to yaw-only for the fit. The engine pushes each cascade box's centre forward along
//     that vector, so head pitch/roll slides the cascade centre and the shadows shift, re-quantize, and
//     scale as you look around (view-dependent shadows). Projecting the forward onto the ground plane
//     makes the centre follow heading but not head tilt; restored after so rendering is unaffected. The
//     box size (sphere) and orientation (sun-fixed) are already view-independent.
#[detour(
    address = jc3gi::graphics_engine::shadow_manager::ShadowManager::UpdateRender_ADDRESS
)]
fn shadow_manager_update_render(this: *mut c_void, dt: f32, dtf: f32) -> u64 {
    #[cfg(feature = "profiler")]
    puffin::profile_scope!("ShadowManager::UpdateRender");
    let original = SHADOW_MANAGER_UPDATE_RENDER.get().unwrap();
    let update_every_frame = STEREO_CONFIG.lock().shadow_update_every_frame;
    // Defeat the cascade update-pattern amortization (issue #31): force every cascade to update level 0
    // so it re-fits and re-renders each frame instead of on its 2^L cadence, eliminating the periodic
    // re-fit snaps that surface as flicker with the mod's forced SMAA 1x (no T2X to average them). The
    // levels persist frame to frame and are read by `SetActiveShadowPassCount` (which runs before this
    // in the sim's shadow update), so zeroing them here holds for the next frame's schedule build.
    if update_every_frame && let Some(manager) = unsafe { this.cast::<ShadowManager>().as_mut() } {
        manager.m_CascadeUpdateLevels = [0; 6];
    }
    // The widen uses the eyes' exact union (or the padded cull projection with the switch off);
    // `None` on flatscreen, so widening is VR-only.
    let union = if STEREO_CONFIG.query(|c| c.shadow_fit_unpadded) {
        jc3vr_view::vr::shadow_fit_projection_standard()
    } else {
        jc3vr_view::vr::cull_projection_standard()
    };
    // SAFETY: the camera-manager singleton and the active camera are live on the game thread for
    // the duration of this call; the projection and transform writes are scoped save/restore.
    unsafe {
        let camera = jc3gi::camera::camera_manager::CameraManager::get()
            .map(|cm| cm.m_ActiveCamera)
            .unwrap_or(std::ptr::null_mut());
        let Some(camera) = camera.as_mut() else {
            return original.call(this, dt, dtf);
        };
        // The fit bakes the cascade transform relative to this position (`m_TransformF`'s
        // translation); the shadow lookup is re-anchored to it in `set_global_shader_constants`.
        STEREO_STATE.lock().shadow_fit_position = Some(glam::Vec3::new(
            camera.m_TransformF.data[12],
            camera.m_TransformF.data[13],
            camera.m_TransformF.data[14],
        ));
        let saved_proj = [camera.m_ProjectionF.data[0], camera.m_ProjectionF.data[5]];
        if let Some(union) = union {
            camera.m_ProjectionF.data[0] = union.data[0];
            camera.m_ProjectionF.data[5] = union.data[5];
        }
        // Horizontalize the camera forward (row 2) so the cascade centre's forward-push is yaw-only.
        let saved_row2 = [
            camera.m_TransformT1.data[8],
            camera.m_TransformT1.data[9],
            camera.m_TransformT1.data[10],
        ];
        let len = (saved_row2[0] * saved_row2[0] + saved_row2[2] * saved_row2[2]).sqrt();
        if len > 1e-4 {
            camera.m_TransformT1.data[8] = saved_row2[0] / len;
            camera.m_TransformT1.data[9] = 0.0;
            camera.m_TransformT1.data[10] = saved_row2[2] / len;
        }
        let result = original.call(this, dt, dtf);
        camera.m_ProjectionF.data[0] = saved_proj[0];
        camera.m_ProjectionF.data[5] = saved_proj[1];
        camera.m_TransformT1.data[8] = saved_row2[0];
        camera.m_TransformT1.data[9] = saved_row2[1];
        camera.m_TransformT1.data[10] = saved_row2[2];
        result
    }
}

// RenderEngine::PreDraw -- the pre-pass dispatch (sky-lighting LUT, planar/env reflections, cloud
// shadows, vegetation, the sun-shadow cascade atlas, water sim, rain occluder). Most of these are driven
// by the sun / reflection / world-space cameras -- never the per-eye render camera -- and write separate
// persistent render targets the per-eye passes never overwrite, so their output is identical between the
// two eyes. On the SECOND eye, skip the view-independent categories (clear their m_Enabled so PreDraw's
// loop no-ops them) and reuse eye 0's output: this halves the second eye's pre-pass cost (shadow-map
// render, reflection proxies, water sim, cloud shadows), and renders the shared sun-shadow atlas once per
// frame instead of twice -- which also removes the per-eye shadow flicker (issue #31). PreDraw runs after
// CommitRenderPassSettings (which re-enables the frame's scheduled shadow cascades), so the clear here is
// the last word before the loop; the passes are re-enabled after so the next frame's first eye runs them.
// This relies on the between-eye frame-counter restore in `game::game_update_render`, so both eyes
// share the shadow-atlas parity slot (without it, eye 1 advances parity and would sample the other,
// unrendered slot).
//
// Under the single-pass collapse this whole path is inert, and correctly so: the collapse runs one
// dispatch, so `dispatch_ordinal()` is never > 0 and PreDraw already runs exactly once per frame.
// There is no second walk of the prepasses to elide, which is why extending the collapse over
// PreDraw would save nothing -- see "Why `PreDraw` is outside the collapse" in
// docs/mod/stereo/single-pass-stereo.md.
#[detour(address = jc3gi::graphics_engine::render_engine::RenderEngine::PreDraw_ADDRESS)]
fn pre_draw(this: *mut RenderEngine, ctx: *mut HContext_t) -> u64 {
    // The first seam of a dispatch: label this thread's puffin lane (a no-op on single-core,
    // where the dispatch runs inline on the already-labelled main thread), open the GPU dispatch
    // (disjoint query + CPU reference), and bracket PreDraw on both the CPU and GPU timelines.
    // The GPU dispatch is closed in `graphics_engine::render_engine_post_draw` (the last seam).
    #[cfg(feature = "profiler")]
    jc3vr_core::profiler::label_thread("draw");
    #[cfg(feature = "profiler")]
    puffin::profile_scope!("RenderEngine::PreDraw");
    #[cfg(feature = "profiler")]
    // SAFETY: `ctx` is the live immediate context for this dispatch.
    unsafe {
        crate::profiler::gpu::begin_dispatch(ctx)
    };
    #[cfg(feature = "profiler")]
    // SAFETY: `ctx` is the live immediate context for this dispatch.
    let _gpu = unsafe { seam(ctx, crate::profiler::gpu::GpuSeam::PreDraw) };
    // The first thing this dispatch does on the draw thread, and the one point in its sequence where
    // no single-pass G-buffer range can be live: close one left open by an interrupted dispatch here,
    // rather than from the game thread, which runs concurrently with it once the frame tail is
    // deferred.
    crate::stereo::single_pass::begin_dispatch();
    let original = PRE_DRAW.get().unwrap();
    if jc3vr_view::stereo::dispatch_ordinal() > 0 && jc3vr_view::stereo::active() {
        // SAFETY: `this` is the live render engine; its per-category `m_RenderPasses` vectors hold
        // engine-owned, null-checked pass pointers; the `m_Enabled` flag write mirrors the shadow
        // scheduler's own store in `commit_render_pass_settings`.
        let disabled = unsafe { disable_shared_prepasses(this) };
        let r = original.call(this, ctx);
        unsafe { reenable_passes(&disabled) };
        r
    } else {
        original.call(this, ctx)
    }
}

/// The pre-pass categories ([`RenderPassId`] indices) that render identically for both eyes and whose
/// outputs persist for the whole frame, so eye 1 can reuse eye 0's output instead of re-running
/// them: the reflection chain (which also fixes the per-eye shadow flicker #31), the sun-shadow
/// cascade atlas, and the water-simulation compute. Terrain-patch prep, the sky-lighting LUT,
/// vegetation, and the rain occluder are held per-eye for this conservative first cut.
const SHARED_PREPASS_CATEGORIES: &[(usize, usize)] = &[
    (
        RenderPassId::PRE_RP_REFLECTION_PRE as usize,
        RenderPassId::PRE_RP_CLOUDSHADOWS as usize,
    ),
    (
        RenderPassId::PRE_RP_STATIC_SHADOW_0 as usize,
        RenderPassId::PRE_RP_WATER_DISPLACEMENT_PRE as usize,
    ),
];

/// Clear [`RenderPassState::m_Enabled`] on every enabled pass in the shared pre-pass categories so
/// `PreDraw`'s loop skips them, returning the passes cleared so [`reenable_passes`] can restore them.
///
/// # Safety
///
/// `this` is the live render engine during its own `PreDraw`; the passes are engine-owned.
unsafe fn disable_shared_prepasses(this: *mut RenderEngine) -> Vec<*mut RenderPass> {
    let Some(engine) = (unsafe { this.as_mut() }) else {
        return Vec::new();
    };
    let mut disabled = Vec::new();
    for &(lo, hi) in SHARED_PREPASS_CATEGORIES {
        for category in &engine.m_RenderPasses[lo..=hi] {
            for &pass in unsafe { category.as_slice() } {
                if let Some(pass) = (unsafe { pass.as_mut() })
                    && pass.m_StateFlags.contains(RenderPassState::m_Enabled)
                {
                    pass.m_StateFlags.remove(RenderPassState::m_Enabled);
                    disabled.push(pass as *mut RenderPass);
                }
            }
        }
    }
    disabled
}

/// Re-enable the pre-passes disabled by [`disable_shared_prepasses`], so the next frame's first eye
/// renders them.
///
/// # Safety
///
/// The pointers came from the live `m_RenderPasses` vectors this same frame and are still valid.
unsafe fn reenable_passes(passes: &[*mut RenderPass]) {
    for &pass in passes {
        if let Some(pass) = unsafe { pass.as_mut() } {
            pass.m_StateFlags.insert(RenderPassState::m_Enabled);
        }
    }
}

// RenderEngine::SetGlobalShaderConstants -- stages the per-eye render context into the cb0 GlobalConstants
// (the shadow-cascade transform among them). The cascade transform is baked center-camera-relative, but
// the material shader anchors the shadow lookup at the per-eye camera position (cb0[4]), shifting each
// eye's shadow by `M * (eyePos - centerPos)` -- the per-eye sun-shadow mismatch. Adding `M * delta` to the
// transform's translation (with `delta = eyePos - centerPos`, the offset the camera hook applies)
// re-anchors the lookup to center while leaving stereo geometry untouched. Patched before the original
// stages the constants; a zero delta (no per-eye offset) makes it a no-op.
#[detour(address = jc3gi::graphics_engine::render_engine::RenderEngine::SetGlobalShaderConstants_ADDRESS)]
fn set_global_shader_constants(this: *mut c_void, ctx: *mut c_void) {
    if let Some(ctx) = unsafe { ctx.cast::<RenderContext>().as_mut() } {
        // The config is read before the stereo state's lock, never under it.
        let anchor_to_fit = STEREO_CONFIG.lock().shadow_anchor_to_fit;
        let delta = shadow_lookup_delta(&STEREO_STATE.lock(), anchor_to_fit);
        record_shadow_state(ctx, delta);
        apply_shadow_cascade_anchor_fix(ctx, delta);
    }
    SET_GLOBAL_SHADER_CONSTANTS.get().unwrap().call(this, ctx);
}

/// The world offset to re-anchor the sun-shadow lookup by: from the point the cascade transform is
/// baked relative to, to the camera position the material shaders subtract. That is the eye's offset
/// from the render camera's centre, plus, with `anchor_to_fit` (`stereo.shadow_anchor_to_fit`), the centre's offset
/// from the camera position the fit read. The second gap is published as
/// [`shadow_fit_gap_m`] for the debug UI.
fn shadow_lookup_delta(state: &jc3vr_view::stereo::StereoState, anchor_to_fit: bool) -> glam::Vec3 {
    let to_fit = match (state.center_transform, state.shadow_fit_position) {
        (Some(center), Some(fit)) => {
            let center = glam::Vec3::new(center.data[12], center.data[13], center.data[14]);
            center - fit
        }
        _ => glam::Vec3::ZERO,
    };
    let to_fit = if to_fit.is_finite() {
        to_fit
    } else {
        glam::Vec3::ZERO
    };
    SHADOW_FIT_GAP_BITS.store(to_fit.length().to_bits(), Ordering::Relaxed);
    if anchor_to_fit {
        state.shadow_anchor_delta + to_fit
    } else {
        state.shadow_anchor_delta
    }
}

/// How far the render camera's centre sat from the camera position the sun-shadow cascade fit read,
/// at the last staging of the shader constants, metres.
pub fn shadow_fit_gap_m() -> f32 {
    f32::from_bits(SHADOW_FIT_GAP_BITS.load(Ordering::Relaxed))
}

static SHADOW_FIT_GAP_BITS: AtomicU32 = AtomicU32::new(0);

/// Record the staged sun-shadow constants into the active render trace (no-op outside a trace) --
/// the raw parity-slot values, read before the anchor correction mutates them. See
/// [`TraceEvent::ShadowState`] for how the series is analysed.
fn record_shadow_state(ctx: &RenderContext, anchor_delta: glam::Vec3) {
    if !tracing_active() {
        return;
    }
    // SAFETY: the render-frame counters live for the process.
    let counters = unsafe { *jc3gi::graphics_engine::graphics_engine::get_render_frame_counters() };
    // SAFETY: the graphics engine and its shadow manager are live on the render thread during a dispatch.
    let shadow_fade = unsafe {
        jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get()
            .and_then(|ge| ge.m_ShadowManager.as_ref())
            .map_or(1.0, |mgr| mgr.GetShadowFade())
    };
    // The interpolated active-camera translation the sun-shadow fit anchors to, and the sub-frame
    // interpolation fraction that produced it -- so the cascade `translation` zig-zag can be correlated
    // against the camera the fit chases (issue #31 / #20).
    // SAFETY: the camera-manager singleton and its active camera are live on the render thread during a
    // dispatch.
    let (cam_translation, cam_forward) = unsafe {
        jc3gi::camera::camera_manager::CameraManager::get()
            .and_then(|cm| cm.m_ActiveCamera.as_ref())
            .map_or(([0.0; 3], [0.0; 3]), |c| {
                let m = glam::Mat4::from(c.m_TransformF);
                (
                    m.w_axis.truncate().to_array(),
                    m.z_axis.truncate().to_array(),
                )
            })
    };
    let t = &ctx.m_ShadowCascades.m_Transform.data;
    TraceState::record_eye(TraceEvent::ShadowState {
        counter: counters.m_Counter,
        frame_index: counters.m_FrameIndex,
        translation: [t[12], t[13], t[14], t[15]],
        scale_blend: std::array::from_fn(|i| ctx.m_ShadowCascades.m_ScaleBlend[i].data),
        offset_radius: std::array::from_fn(|i| ctx.m_ShadowCascades.m_OffsetRadius[i].data),
        active_cascades: u32::from(ctx.m_ActiveCascadeCount),
        anchor_delta: anchor_delta.to_array(),
        shadow_fade,
        dtf: jc3vr_view::hooks::camera::last_dtf(),
        cam_translation,
        cam_forward,
    });
}

/// Add `M * delta` to the cascade transform translation row (`m_Transform` row 3), where `M`'s columns
/// are the transform's three linear rows (row 0/1/2 -- cb0[45..47] in the shader). Re-anchors the
/// sun-shadow lookup from the per-eye camera back to the center camera the cascade map was fit to. The
/// full `float4` add is used; the linear rows' `.w` are 0 for the affine cascade transform, so the
/// translation's `.w` is unchanged.
fn apply_shadow_cascade_anchor_fix(ctx: &mut RenderContext, delta: glam::Vec3) {
    let m = &mut ctx.m_ShadowCascades.m_Transform.data;
    for i in 0..4 {
        m[12 + i] += delta.x * m[i] + delta.y * m[4 + i] + delta.z * m[8 + i];
    }
}
