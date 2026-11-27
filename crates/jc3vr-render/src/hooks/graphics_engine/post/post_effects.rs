//! Detours on the post-effects chain.
//!
//! Two concerns share this file. First, the motion blur and the depth of field smear the view with
//! the camera's motion and focus, which in the headset are the player's own head and eyes, so
//! neither runs while a VR session runs ([`vr_forces_blurs_off`]): the motion blur's `Apply` is
//! skipped (it returns its source slot index, so returning `input` is a clean pass-through), and the
//! depth of field's own passes are dropped by answering `IsBokehActive` with false, which leaves the
//! chain on the plain blur and the composite without depth of field. The composite itself is the
//! HDR-to-LDR tonemap, so `DepthOfFieldEffect::Apply` always runs in VR. Outside VR the
//! `post_fx.skip_*` switches skip the passes as bisection aids. Second, the manager-level filter
//! enqueues (`ApplyWorldFilters` / `ApplyGlobalFilters`) step dt-driven accumulators that must only
//! advance once per real frame, so their `dt` is zeroed on later scene dispatches.

use std::{
    ffi::c_void,
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};

use detours_macro::detour;
use jc3gi::graphics_engine::{
    graphics_engine::RenderContextFlags,
    post_effects::{
        AAMode, AntiAliasingEffect, PostEffectContext, PostEffectsManager, SunHaloEffect,
    },
};
use jc3vr_view::{
    fsr::{AntiAliasing, FSR_CONFIG, TaaConfig},
    hooks::graphics_engine::capture::{
        POST_STAGE_DOF, POST_STAGE_MB, capture_post_stage, previews_visible,
    },
    stereo,
    stereo::{config::SinglePassSwitch, draw_index, later_scene},
};
use re_utilities::hook_library::HookLibrary;

use crate::{
    debug::trace::{TraceEvent, TraceState},
    hooks::graphics_engine::config::POST_FX_CONFIG,
    probes,
    probes::{BlockScope, PipelinePoint, PostEffect, scoped},
};

/// Whether the motion blur and the depth of field are forced off: while a VR session runs, whatever
/// the `post_fx` switches say. The eye images then go to the headset, where both effects would blur
/// the view with the player's own head motion and gaze.
pub fn vr_forces_blurs_off() -> bool {
    jc3vr_view::vr::is_running_published()
}

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&RENDER_BLOCK_POST_EFFECTS_DRAW_BINDER)
        .with_static_binder(&APPLY_WORLD_FILTERS_BINDER)
        .with_static_binder(&APPLY_GLOBAL_FILTERS_BINDER)
        .with_static_binder(&MOTION_BLUR_APPLY_BINDER)
        .with_static_binder(&DOF_APPLY_BINDER)
        .with_static_binder(&IS_BOKEH_ACTIVE_BINDER)
        .with_static_binder(&FADE_APPLY_BINDER)
        .with_static_binder(&GLARE_APPLY_BINDER)
        .with_static_binder(&PLAYER_DAMAGE_APPLY_BINDER)
        .with_static_binder(&SUN_HALO_PRE_APPLY_BINDER)
        .with_static_binder(&SUN_HALO_APPLY_BINDER)
        .with_static_binder(&ANTI_ALIASING_APPLY_BINDER)
}

/// The mod's temporal resolve chosen for this frame, with its settings.
#[derive(Clone, Copy)]
enum TemporalResolve {
    /// FSR, with its optional RCAS strength.
    Fsr(Option<f32>),
    /// The mod's own resolve.
    Taa(TaaConfig),
}

impl TemporalResolve {
    const fn code(self) -> u8 {
        match self {
            Self::Fsr(_) => 1,
            Self::Taa(_) => 2,
        }
    }
}

/// The code of the resolve chosen in the last AA stage, or [`NO_RESOLVE`] when the engine's own AA
/// was.
static LAST_RESOLVE: AtomicU8 = AtomicU8::new(NO_RESOLVE);
const NO_RESOLVE: u8 = 0;

/// Restart a temporal resolve's history when it is chosen after a frame that chose another AA: its
/// history then predates the frames in between, and reprojecting it would smear the old view over
/// the new one.
fn note_resolve_choice(resolve: Option<TemporalResolve>) {
    let code = resolve.map_or(NO_RESOLVE, TemporalResolve::code);
    let previous = LAST_RESOLVE.swap(code, Ordering::Relaxed);
    if code != NO_RESOLVE && previous != code {
        crate::fsr::request_history_reset();
    }
}

/// Dispatch the chosen temporal resolve for the current eye, reading the chain's current slot color
/// (`mgr[slot + PostEffectsManager::RESULT_TEXTURE_SLOT_BASE]`) as
/// input and writing the anti-aliased result back into it. Returns whether it ran (in which case
/// the engine AA should be neutralized to a passthrough). Mirrors `capture_post_result`'s slot
/// access.
fn temporal_dispatch(mgr: *mut c_void, slot: u32, resolve: TemporalResolve) -> bool {
    if mgr.is_null() {
        return false;
    }
    // SAFETY: at the AA stage
    // `mgr[slot + PostEffectsManager::RESULT_TEXTURE_SLOT_BASE]` is the chain's current result
    // texture (the LDR, post-tonemap color), and the engine buffers are live on the render thread.
    let ran = unsafe {
        let slot_color = (mgr as *const *mut jc3gi::graphics_engine::texture::Texture)
            .add(slot as usize + PostEffectsManager::RESULT_TEXTURE_SLOT_BASE as usize)
            .read();
        let Some(ge) = jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() else {
            return false;
        };
        let (Some(device), Some(color), Some(depth), Some(velocity)) = (
            ge.m_Device.as_ref(),
            slot_color.as_ref(),
            ge.m_MainDepthTexture.as_ref(),
            ge.m_VelocityBufferTexture.as_ref(),
        ) else {
            return false;
        };
        let layout = if jc3vr_view::stereo::layout::double_wide_active()
            && crate::stereo::single_pass::config_flags().has(SinglePassSwitch::FsrPerEye)
        {
            crate::fsr::EyeLayout::SideBySide
        } else {
            crate::fsr::EyeLayout::Single(draw_index())
        };
        let textures = crate::fsr::ChainTextures {
            slot_color: color,
            depth,
            velocity,
        };
        #[cfg(feature = "profiler")]
        let _gpu = crate::profiler::gpu::nested_seam(crate::profiler::gpu::GpuSeam::Fsr);
        match resolve {
            TemporalResolve::Fsr(sharpness) => crate::fsr::dispatch(
                &mut crate::fsr::FSR_STATE.lock(),
                device,
                &textures,
                layout,
                sharpness,
            ),
            TemporalResolve::Taa(tuning) => crate::fsr::taa::dispatch(
                &mut crate::fsr::taa::TAA_STATE.lock(),
                device,
                &textures,
                layout,
                tuning,
            ),
        }
    };
    TraceState::record_eye(TraceEvent::FsrDispatch { input: slot, ran });
    ran
}

/// Whether the world post-effects block has already drawn during the current dispatch. Reset by the
/// Draw driver at each dispatch begin ([`reset_post_block_gate`]).
static WORLD_POST_BLOCK_RAN: AtomicBool = AtomicBool::new(false);

/// Re-arm the once-per-dispatch world post-effects block gate. Called by the Draw driver (the
/// payload's `hooks::game`) at the start of every dispatch.
pub fn reset_post_block_gate() {
    WORLD_POST_BLOCK_RAN.store(false, Ordering::Relaxed);
}

// RenderBlockPostEffects::Draw -- the world post-effect chain (histogram, blur, glare, DoF, motion
// blur, tonemap, AA/FSR, sun halo, fade). `ApplyWorldFilters` enqueues the block into pass
// RP_POSTEFFECTS's *draw* list at draw time, and the between-eye list-parity restore only zeroes the
// *add* lists -- so eye 0's entry survives into eye 1's list and eye 1 draws the block twice
// (confirmed by the render trace: three chains per real frame). The second run double-steps every
// temporal consumer in the chain -- most visibly FSR, whose eye-1 history then oscillates (the
// residual per-eye flicker of issue #10) -- and advances the post slot ring an extra step. Gate the
// block to once per dispatch: the duplicate entries reference the same stateless block object, and
// the chain's per-eye behaviour (histogram gates, FSR eye index) is decided live at run time, so
// running the first entry and skipping the repeat is exact.
#[detour(
    address = jc3gi::graphics_engine::post_effects::RenderBlockPostEffects::Draw_ADDRESS
)]
fn render_block_post_effects_draw(this: *mut c_void, ctx: *mut c_void, info: *mut c_void) -> u64 {
    let repeat = WORLD_POST_BLOCK_RAN.swap(true, Ordering::Relaxed);
    TraceState::record_eye(TraceEvent::PostEffectsBlockDraw {
        repeat,
        skip: repeat,
    });
    if repeat {
        return 0;
    }
    probes::point(PipelinePoint::MainColor);
    #[cfg(feature = "profiler")]
    let _gpu = crate::profiler::gpu::detail_interval("post: world block");
    RENDER_BLOCK_POST_EFFECTS_DRAW
        .get()
        .unwrap()
        .call(this, ctx, info)
}

// PostEffectsManager::ApplyWorldFilters -- enqueues the world post block and steps the world-fade
// accumulator (ApplyWorldFadeFilter) by `dt`. Zero `dt` on later scene dispatches so
// the fade advances once per real frame.
#[detour(address = jc3gi::graphics_engine::post_effects::PostEffectsManager::ApplyWorldFilters_ADDRESS)]
#[allow(clippy::too_many_arguments)]
fn apply_world_filters(
    this: *mut c_void,
    dt: f32,
    setup: *mut c_void,
    a4: *mut c_void,
    a5: *mut c_void,
    a6: *mut c_void,
    a7: *mut c_void,
    a8: *mut c_void,
) {
    let gated = later_scene();
    TraceState::record_eye(TraceEvent::ApplyWorldFilters { gated });
    let dt = if gated { 0.0 } else { dt };
    APPLY_WORLD_FILTERS
        .get()
        .unwrap()
        .call(this, dt, setup, a4, a5, a6, a7, a8);
}

// PostEffectsManager::ApplyGlobalFilters -- enqueues the global post block and steps its dt-driven
// accumulators (screen-fade alpha, sun-direction / heat-haze). Zero `dt` on later scene
// dispatches for the same once-per-real-frame reason.
#[detour(address = jc3gi::graphics_engine::post_effects::PostEffectsManager::ApplyGlobalFilters_ADDRESS)]
fn apply_global_filters(this: *mut c_void, dt: f32, ctx: *mut c_void) {
    let gated = later_scene();
    TraceState::record_eye(TraceEvent::ApplyGlobalFilters { gated });
    let dt = if gated { 0.0 } else { dt };
    APPLY_GLOBAL_FILTERS.get().unwrap().call(this, dt, ctx);
}

// AntiAliasingEffect::Apply -- the AA stage of the post chain. Three roles:
//   * FSR or the mod's TAA chosen: resolve the slot color ourselves (post-tonemap) via
//     temporal_dispatch, then neutralize the engine AA to AA_NONE so it passthrough-blits our result
//     onward (docs/mod/rendering/fsr.md, docs/mod/rendering/taa.md).
//   * else, in stereo, drop T2X to SMAA 1x -- T2X's temporal resolve blends a single history shared
//     across the two eye dispatches, so each eye ghosts the other.
//   * `post_fx.skip_anti_aliasing` drops the engine AA to AA_NONE without FSR, for a baseline with
//     no anti-aliasing at all.
// Either way, restore the real mode afterwards so the engine's own state stays intact.
#[detour(address = jc3gi::graphics_engine::post_effects::AntiAliasingEffect::Apply_ADDRESS)]
fn anti_aliasing_apply(
    this: *mut AntiAliasingEffect,
    ctx: *mut c_void,
    pec: *mut c_void,
    mgr: *mut c_void,
    slot: *mut u32,
) -> u64 {
    if let Some(probes) = probes::armed() {
        // SAFETY: the AA hook receives the current chain slot as a live in/out parameter.
        let slot = unsafe { *slot };
        probes.point(PipelinePoint::BeforeAntiAliasing { mgr, slot });
    }
    let force_smaa_1x = stereo::active();
    let fsr = FSR_CONFIG.query(|fsr| match fsr.antialiasing {
        AntiAliasing::Fsr => Some(TemporalResolve::Fsr(fsr.sharpen.then_some(fsr.sharpness))),
        AntiAliasing::Taa => Some(TemporalResolve::Taa(fsr.taa)),
        AntiAliasing::Engine => None,
    });
    let skip_aa = POST_FX_CONFIG.lock().skip_anti_aliasing;

    // When FSR or the mod's TAA is chosen, resolve into the current slot ourselves, then let the
    // engine AA run as a passthrough (AA_NONE blits the slot onward without filtering). The mod's
    // resolve replaces SMAA, so its 1x-forcing is moot here.
    let saved_mode = unsafe { (*this).m_Mode };
    let mut restore = false;
    note_resolve_choice(fsr);
    let fsr_ran = fsr.is_some_and(|resolve| {
        // SAFETY: `slot` is a valid out-param holding the current chain slot index.
        let slot_index = unsafe { *slot };
        temporal_dispatch(mgr, slot_index, resolve)
    });
    if fsr_ran || skip_aa {
        unsafe { (*this).m_Mode = AAMode::AA_NONE };
        restore = true;
    } else if fsr.is_none() && force_smaa_1x && saved_mode == AAMode::AA_SMAA_T2X {
        // T2X's temporal resolve blends a single shared history across the two eye dispatches, so each
        // eye ghosts the other; 1x carries no history.
        unsafe { (*this).m_Mode = AAMode::AA_SMAA };
        restore = true;
    }

    let r = scoped(
        probes::armed(),
        // SAFETY: `this` is the live effect, and the scope carries no pointer.
        |probes, body| unsafe {
            probes.block_scope(BlockScope::AntiAliasing((*this).m_Mode), body)
        },
        || {
            #[cfg(feature = "profiler")]
            let gpu = crate::profiler::gpu::detail_interval("post: engine anti-aliasing");
            let r = ANTI_ALIASING_APPLY
                .get()
                .unwrap()
                .call(this, ctx, pec, mgr, slot);
            #[cfg(feature = "profiler")]
            drop(gpu);
            if let Some(probes) = probes::armed() {
                // SAFETY: the AA result updates the same live slot parameter.
                let slot = unsafe { *slot };
                probes.point(PipelinePoint::AfterAntiAliasing { mgr, slot });
            }
            r
        },
    );
    if restore {
        unsafe { (*this).m_Mode = saved_mode };
    }
    r
}

/// Read a stage's slot result texture
/// (`CTX[slot + PostEffectsManager::RESULT_TEXTURE_SLOT_BASE]`, where `CTX` is the manager arg) and capture
/// it for the current eye into the debug overlay's per-stage preview. Debug-only: gated on the
/// Previews tab actually being open, so the CopyResource isn't paid for every dispatch by everyone.
fn capture_post_result(stage: usize, mgr: *mut c_void, slot: u32) {
    if mgr.is_null() || !previews_visible() {
        return;
    }
    let eye = draw_index();
    unsafe {
        let result = (mgr as *const *mut jc3gi::graphics_engine::texture::Texture)
            .add(slot as usize + PostEffectsManager::RESULT_TEXTURE_SLOT_BASE as usize)
            .read();
        capture_post_stage(stage, eye, result);
    }
}

#[detour(address = jc3gi::graphics_engine::post_effects::MotionBlurEffect::Apply_ADDRESS)]
#[allow(clippy::too_many_arguments)]
fn motion_blur_apply(
    this: *mut c_void,
    ctx: *mut c_void,
    pec: *mut c_void,
    mgr: *mut c_void,
    input: u32,
    blur: f32,
    flag0: bool,
    flag1: bool,
) -> u32 {
    let (skip_motion_blur, skip_recon) =
        POST_FX_CONFIG.query(|c| (c.skip_motion_blur, c.skip_motion_blur_recon));
    let skip_motion_blur = skip_motion_blur || vr_forces_blurs_off();
    TraceState::record_eye(TraceEvent::MotionBlurApply {
        input,
        skip: skip_motion_blur,
    });
    let slot = if skip_motion_blur {
        input
    } else {
        // `flag0` (a7) gates the reconstruction-filter motion blur (`if g_EnableMotionBlur && !a7`).
        // Forcing it true skips that reprojection blur -- the flicker -- but keeps the composite draw.
        let flag0 = flag0 || skip_recon;
        #[cfg(feature = "profiler")]
        let _gpu = crate::profiler::gpu::detail_interval("post: motion blur");
        MOTION_BLUR_APPLY
            .get()
            .unwrap()
            .call(this, ctx, pec, mgr, input, blur, flag0, flag1)
    };
    capture_post_result(POST_STAGE_MB, mgr, slot);
    slot
}

#[detour(address = jc3gi::graphics_engine::post_effects::DepthOfFieldEffect::Apply_ADDRESS)]
fn dof_apply(
    this: *mut c_void,
    ctx: *mut c_void,
    pec: *mut PostEffectContext,
    mgr: *mut c_void,
    input: u32,
) -> u32 {
    let (skip_dof, dof_no_reproject) = POST_FX_CONFIG.query(|c| (c.skip_dof, c.dof_no_reproject));
    // The composite is also the tonemap, so skipping it in VR would hand the headset the HDR scene
    // unmapped; the depth of field is dropped in VR by `is_bokeh_active` instead.
    let skip_dof = skip_dof && !vr_forces_blurs_off();
    TraceState::record_eye(TraceEvent::DofApply {
        input,
        skip: skip_dof,
    });
    if skip_dof {
        return input;
    }
    let mut apply = || {
        #[cfg(feature = "profiler")]
        let gpu = crate::profiler::gpu::detail_interval("post: blur and composite");
        let slot = if dof_no_reproject {
            // The render context's `m_CameraUnderwater` flag selects the DoF composite's underwater
            // shader family -- the variant that carries the reprojection-style distortion this toggle
            // suppresses. Clear just that flag for the call so DoF composites through the plain
            // family, then restore.
            let rc = unsafe { pec.as_mut().and_then(|p| p.m_RenderContext.as_mut()) };
            if let Some(rc) = rc {
                let saved = rc.m_Flags;
                rc.m_Flags.remove(RenderContextFlags::m_CameraUnderwater);
                let result = DOF_APPLY.get().unwrap().call(this, ctx, pec, mgr, input);
                rc.m_Flags = saved;
                result
            } else {
                DOF_APPLY.get().unwrap().call(this, ctx, pec, mgr, input)
            }
        } else {
            DOF_APPLY.get().unwrap().call(this, ctx, pec, mgr, input)
        };
        #[cfg(feature = "profiler")]
        drop(gpu);
        slot
    };
    let slot = match probes::armed() {
        Some(probes) => probes.post_effect_scope(
            PostEffect::DepthOfField {
                pec: pec.cast_const(),
                input,
                mgr,
            },
            &mut apply,
        ),
        None => apply(),
    };
    capture_post_result(POST_STAGE_DOF, mgr, slot);
    slot
}

// PostEffectsManager::IsBokehActive -- the post block's choice between the depth of field's own
// passes (the downscale-and-focus prepass, the bokeh blur, and the depth-of-field composite) and the
// plain blur with the plain composite. Answering false in VR drops the depth of field and keeps the
// blur the glare reads and the tonemap.
#[detour(address = jc3gi::graphics_engine::post_effects::PostEffectsManager::IsBokehActive_ADDRESS)]
fn is_bokeh_active(this: *const PostEffectsManager) -> bool {
    if vr_forces_blurs_off() {
        return false;
    }
    IS_BOKEH_ACTIVE.get().unwrap().call(this)
}

// FadeEffect::Apply -- alpha-blended fade quad. Skip = no-op (the u64 return is discarded).
#[detour(address = jc3gi::graphics_engine::post_effects::FadeEffect::Apply_ADDRESS)]
fn fade_apply(this: *mut c_void, a2: *mut c_void, a3: *mut c_void) -> u64 {
    let skip = POST_FX_CONFIG.lock().skip_fade;
    TraceState::record_eye(TraceEvent::FadeApply { skip });
    if skip {
        return 0;
    }
    #[cfg(feature = "profiler")]
    let _gpu = crate::profiler::gpu::detail_interval("post: fade");
    FADE_APPLY.get().unwrap().call(this, a2, a3)
}

// GlareEffect::Apply -- bloom/glare generator. Skip = no bloom. Otherwise, under the collapse, its
// filters read only their own eye's half of each source for the length of the call
// (`stereo::single_pass::post_filters`).
#[detour(address = jc3gi::graphics_engine::post_effects::GlareEffect::Apply_ADDRESS)]
fn glare_apply(
    this: *mut c_void,
    a2: *mut c_void,
    a3: *mut c_void,
    a4: *mut c_void,
    a5: *mut c_void,
) -> u64 {
    let skip = POST_FX_CONFIG.lock().skip_glare;
    TraceState::record_eye(TraceEvent::GlareApply { skip });
    if skip {
        return 0;
    }
    let _eye_half = crate::stereo::single_pass::post_filters::Scope::enter();
    #[cfg(feature = "profiler")]
    let _gpu = crate::profiler::gpu::detail_interval("post: glare");
    GLARE_APPLY.get().unwrap().call(this, a2, a3, a4, a5)
}

// PlayerDamageEffect::Apply -- red damage vignette. Slot-passthrough; skip = return input slot.
#[detour(address = jc3gi::graphics_engine::post_effects::PlayerDamageEffect::Apply_ADDRESS)]
fn player_damage_apply(
    this: *mut c_void,
    a2: *mut c_void,
    a3: *mut c_void,
    a4: *mut c_void,
    input: u32,
) -> u32 {
    let skip = POST_FX_CONFIG.lock().skip_player_damage;
    TraceState::record_eye(TraceEvent::PlayerDamageApply { input, skip });
    if skip {
        return input;
    }
    #[cfg(feature = "profiler")]
    let _gpu = crate::profiler::gpu::detail_interval("post: player damage");
    PLAYER_DAMAGE_APPLY
        .get()
        .unwrap()
        .call(this, a2, a3, a4, input)
}

// SunHaloEffect::PreApply -- prepares the halo. Skip = clear the ready flag so the paired Apply
// early-outs, then no-op.
#[detour(address = jc3gi::graphics_engine::post_effects::SunHaloEffect::PreApply_ADDRESS)]
fn sun_halo_pre_apply(
    this: *mut SunHaloEffect,
    a2: *mut c_void,
    a3: *mut c_void,
    a4: *mut c_void,
) -> u64 {
    let skip = POST_FX_CONFIG.lock().skip_sun_halo;
    TraceState::record_eye(TraceEvent::SunHaloPreApply { skip });
    if skip {
        if let Some(halo) = unsafe { this.as_mut() } {
            halo.m_Ready = false;
        }
        return 0;
    }
    #[cfg(feature = "profiler")]
    let _gpu = crate::profiler::gpu::detail_interval("post: sun halo prepare");
    SUN_HALO_PRE_APPLY.get().unwrap().call(this, a2, a3, a4)
}

// SunHaloEffect::Apply -- composites the halo additively. Skip = no-op.
#[detour(address = jc3gi::graphics_engine::post_effects::SunHaloEffect::Apply_ADDRESS)]
fn sun_halo_apply(this: *mut c_void, a2: *mut c_void) -> u64 {
    let skip = POST_FX_CONFIG.lock().skip_sun_halo;
    TraceState::record_eye(TraceEvent::SunHaloApply { skip });
    if skip {
        return 0;
    }
    #[cfg(feature = "profiler")]
    let _gpu = crate::profiler::gpu::detail_interval("post: sun halo");
    SUN_HALO_APPLY.get().unwrap().call(this, a2)
}
