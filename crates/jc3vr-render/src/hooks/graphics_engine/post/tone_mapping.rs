//! Detours on the auto-exposure / tone-mapping path.
//!
//! The exposure pipeline runs once per scene dispatch, so a stereo (twice-per-frame) render
//! double-adapts and over-meters unless the work is gated to the first scene dispatch. These hooks
//! pin the exposure for the A/B, gate the smoother and histogram metering on later dispatches, gate the
//! `ToneMappingEffect::Update` step itself under a separate toggle (it advances internal histogram
//! ring indices that would otherwise step twice per real frame), and read the exposure internals
//! for tracing.

use std::{
    ffi::c_void,
    sync::atomic::{AtomicU32, Ordering},
};

use detours_macro::detour;
use jc3gi::graphics_engine::{
    post_effects::PostEffectContext,
    tone_mapping::{HistogramGeneration, ToneMappingEffect},
};
use jc3vr_view::{
    hooks::graphics_engine::capture::{capture_main_color, previews_visible},
    stereo::{draw_index, later_scene},
};
use re_utilities::hook_library::HookLibrary;

use crate::{
    debug::trace::{TraceEvent, TraceState},
    hooks::graphics_engine::config::{EXPOSURE_CONFIG, POST_FX_CONFIG},
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&SMOOTHED_EXPOSURE_UPDATE_BINDER)
        .with_static_binder(&CALC_HISTOGRAM_MID_BRIGHT_BINDER)
        .with_static_binder(&TONEMAPPING_UPDATE_BINDER)
        .with_static_binder(&TONEMAPPING_APPLY_BINDER)
        .with_static_binder(&GENERATE_FINAL_HISTOGRAM_BINDER)
}

/// The exposure the last `ToneMappingEffect::Update` left applied, as `f32` bits; NaN until the
/// first update.
static APPLIED_EXPOSURE: AtomicU32 = AtomicU32::new(f32::NAN.to_bits());

/// The exposure the post chain applies this frame (the engine's metered value, or the pinned one),
/// or `None` before the first exposure update. The visual harness reports it with every capture, so
/// a comparison can check that each mode settled on the same exposure.
pub fn applied_exposure() -> Option<f32> {
    let value = f32::from_bits(APPLIED_EXPOSURE.load(Ordering::Relaxed));
    (!value.is_nan()).then_some(value)
}

// ToneMappingEffect::SmoothedExposure::Update -- N-frame exposure smoother (no dt term, so it
// double-adapts when the scene renders twice per frame). Skip later scene dispatches so both eyes
// share the first scene dispatch's exposure without binocular rivalry.
#[detour(address = jc3gi::graphics_engine::tone_mapping::SmoothedExposure::Update_ADDRESS)]
fn smoothed_exposure_update(this: *mut c_void, exposure: f32) {
    let gated = later_scene() && EXPOSURE_CONFIG.query(|c| c.gate);
    TraceState::record_eye(TraceEvent::SmoothedExposureUpdate { gated, exposure });
    if gated {
        return;
    }
    SMOOTHED_EXPOSURE_UPDATE.get().unwrap().call(this, exposure);
}

// CalculateMidAndBrightPointForHistogram -- the per-frame histogram percentile computation; Update
// calls it once per histogram. The exposure readback happens in the Update detour (which holds
// the ToneMappingEffect directly and can read both histograms), so this only keeps the (inert) gate.
#[detour(address = jc3gi::graphics_engine::tone_mapping::CalculateMidAndBrightPointForHistogram_ADDRESS)]
fn calc_histogram_mid_bright(
    ctx: *mut c_void,
    arg1: f32,
    arg2: i32,
    arg3: f32,
    hist: *mut HistogramGeneration,
) {
    let gated = later_scene() && EXPOSURE_CONFIG.query(|c| c.gate);
    TraceState::record_eye(TraceEvent::CalcHistogramMidBright { gated });
    if gated {
        return;
    }
    CALC_HISTOGRAM_MID_BRIGHT
        .get()
        .unwrap()
        .call(ctx, arg1, arg2, arg3, hist);
}

// ToneMappingEffect::Update -- the per-frame exposure step (CPostEffectsManager::UpdateRender -> here).
// With the real 3-arg signature this is the canonical place to (a) pin the applied exposure for the A/B,
// and (b) read the exposure internals. The target divisor is m_Histogram2's mid-point -- what the
// converged exposure tracks, NOT m_Histogram. Update also advances internal histogram ring indices
// (notably the occlusion-query slot selector, [`ToneMappingEffect::m_OcclusionQuerySlotSelector`]);
// running it on both eyes steps those indices twice per real frame and can read back a stale
// histogram once head motion makes consecutive frames diverge. Gate it to the first scene dispatch
// under its own toggle so the effect is isolatable.
#[detour(address = jc3gi::graphics_engine::tone_mapping::ToneMappingEffect::Update_ADDRESS)]
fn tonemapping_update(
    this: *mut ToneMappingEffect,
    manager: *mut c_void,
    ctx: *mut PostEffectContext,
) {
    let gated = later_scene() && EXPOSURE_CONFIG.query(|c| c.gate_update);
    TraceState::record_eye(TraceEvent::ToneMappingUpdate { gated });
    if gated {
        return;
    }
    TONEMAPPING_UPDATE.get().unwrap().call(this, manager, ctx);
    let Some(tme) = (unsafe { this.as_mut() }) else {
        return;
    };
    let (force_exposure, forced_value) = EXPOSURE_CONFIG.query(|c| (c.force, c.forced_value));
    if force_exposure {
        tme.m_ExposureBrightPoint = forced_value;
    }
    APPLIED_EXPOSURE.store(tme.m_ExposureBrightPoint.to_bits(), Ordering::Relaxed);
    if crate::debug::trace::tracing_active() {
        let target_num = unsafe { ctx.as_ref() }
            .map(|c| c.m_AutoExposureKey)
            .unwrap_or_default();
        let pingpong = tme.m_HistogramPingPong;
        let divisor = tme.m_Histogram2.m_HistogramMidPoint;
        let n = (tme.m_NumBuckets as usize).min(tme.m_Histogram.m_NumPixelsInBuckets.len());
        TraceState::record_eye(TraceEvent::ExposureInternals {
            exposure: tme.m_ExposureBrightPoint,
            target_num,
            divisor,
            target: if divisor != 0.0 {
                target_num / divisor
            } else {
                0.0
            },
            hist1_bright: tme.m_Histogram.m_HistogramBrightPoint,
            hist1_mid: tme.m_Histogram.m_HistogramMidPoint,
            hist1_buckets: tme.m_Histogram.m_NumPixelsInBuckets[..n].to_vec(),
            hist2_bright: tme.m_Histogram2.m_HistogramBrightPoint,
            hist2_mid: tme.m_Histogram2.m_HistogramMidPoint,
            hist2_buckets: tme.m_Histogram2.m_NumPixelsInBuckets[..n].to_vec(),
            num_buckets: tme.m_NumBuckets,
            pingpong,
            forced: force_exposure,
        });
    }
}

// ToneMappingEffect::Apply -- the per-dispatch exposure publish (into the frame context's
// m_Exposure, the value the HDR->LDR composite multiplies the scene by) plus the exposure-weighted
// histogram meter. Skipping later scene dispatches prevents double metering, but the out-params
// must still be served: they are the context's exposure pair, shared by both eyes.
#[detour(address = jc3gi::graphics_engine::tone_mapping::ToneMappingEffect::Apply_ADDRESS)]
#[allow(clippy::too_many_arguments)]
fn tonemapping_apply(
    this: *mut ToneMappingEffect,
    ctx: *mut c_void,
    mgr: *mut c_void,
    pec: *mut c_void,
    dt: f32,
    out_exposure: *mut f32,
    out_bright_point: *mut f32,
) -> *mut f32 {
    // Gate histogram population on later scene dispatches, symmetric to the exposure-adaptation
    // gate (exposure.gate). Both eyes otherwise write the GPU luminance buckets, so the gated
    // per-frame exposure Update reads a histogram populated by both dispatches and settles too dark.
    let skip_histogram = POST_FX_CONFIG.lock().skip_histogram;
    let gate_exposure = EXPOSURE_CONFIG.lock().gate;
    let later_scene_gated = later_scene() && gate_exposure;
    let skip = skip_histogram || later_scene_gated;
    TraceState::record_eye(TraceEvent::ToneMappingApply { skip });
    // The histogram reads the final HDR scene (MainColor) for auto-exposure, so this is the first
    // point in the post chain where MainColor still holds this dispatch's clean scene -- grab it for
    // the per-eye "Scene" preview before the chain recycles it. Debug-only: gated on the Previews tab
    // actually being open, so the CopyResource isn't paid for every dispatch by everyone.
    if previews_visible() {
        capture_main_color(draw_index());
    }

    if skip {
        // Serve the out-param contract without metering: publish the same exposure pair the
        // original would.
        if let Some(tme) = unsafe { this.as_ref() } {
            unsafe {
                if !out_exposure.is_null() {
                    *out_exposure = tme.m_ExposureBrightPoint;
                }
                if !out_bright_point.is_null() {
                    *out_bright_point = tme.m_HistogramBrightPointWithExposure;
                }
            }
        }
        return out_bright_point;
    }
    #[cfg(feature = "profiler")]
    let _gpu = crate::profiler::gpu::detail_interval("post: tone-mapping histogram");
    TONEMAPPING_APPLY
        .get()
        .unwrap()
        .call(this, ctx, mgr, pec, dt, out_exposure, out_bright_point)
}

// ToneMappingEffect::GenerateHistogramForFinalScene -- meters the non-exposure-weighted final-scene
// histogram (m_Histogram2): the raw scene brightness that Update divides the auto-exposure target
// by. It runs once per dispatch, so ungated in stereo m_Histogram2 is metered on both eyes, its
// occlusion-query ring inflates and corrupts, and the exposure divides by a too-large brightness,
// which darkens the frame. Gating only the exposure-weighted meter is not enough. Gate it to the
// first scene dispatch (under exposure.gate), symmetric with tonemapping_apply, so it meters once.
#[detour(
    address = jc3gi::graphics_engine::tone_mapping::ToneMappingEffect::GenerateHistogramForFinalScene_ADDRESS
)]
fn generate_final_histogram(
    this: *mut c_void,
    ctx: *mut c_void,
    pec: *mut c_void,
    mgr: *mut c_void,
    index: u32,
) {
    let later_scene_gated = later_scene() && EXPOSURE_CONFIG.query(|c| c.gate);
    TraceState::record_eye(TraceEvent::GenerateFinalHistogram {
        skip: later_scene_gated,
    });
    if later_scene_gated {
        return;
    }
    #[cfg(feature = "profiler")]
    let _gpu = crate::profiler::gpu::detail_interval("post: final-scene histogram");
    GENERATE_FINAL_HISTOGRAM
        .get()
        .unwrap()
        .call(this, ctx, pec, mgr, index);
}
