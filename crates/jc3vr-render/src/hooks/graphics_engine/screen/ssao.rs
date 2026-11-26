//! Detour on the SSAO pass.
//!
//! The SSAO history index advances once per `CRenderBlockSSAO::Draw` (an inlined
//! `SetNextHistoryBuffer` at the end of the apply), not in the per-frame list rotation. A stereo render
//! dispatches that draw twice per frame, so the two-slot AO history double-steps and each eye's
//! temporal filter blends against the other eye's AO.
//!
//! The engine's own "no valid history" lever is `m_FirstPass`: on, it skips the history resolve and
//! weights the temporal shader fully toward the current AO. The render block clears it after the first
//! apply; [`ssao_draw`] forces it back on before each stereo eye so every dispatch computes AO fresh
//! from its own depth, with the apply still running. Each eye packs and reads its own current
//! depth-buffer slot, so the still-advancing index causes no cross-eye depth contamination.
//!
//! With `m_FirstPass` forced, the temporal resolve only copies the blurred occlusion into the history
//! slot the composite then reads. Under `single_pass.ssao_skip_temporal` the hook also clears
//! `m_EnableTemporalFilter`, which skips that resolve: the composite sits outside the flag's branch
//! and reads the blurred occlusion directly instead. The game's own value of the flag is kept and put
//! back once stereo or the switch is off.

use std::{
    ptr,
    sync::atomic::{AtomicPtr, AtomicU8, Ordering},
};

use detours_macro::detour;
use jc3gi::graphics_engine::ssao::SSAOPass;
use jc3vr_view::{stereo, stereo::config::STEREO_CONFIG};
use re_utilities::hook_library::HookLibrary;

use crate::debug::trace::{TraceEvent, TraceState};

/// The live `CSSAOPass` instance, captured from [`ssao_draw`]. The between-eye SSAO history-index
/// restore in `game.rs` needs this pointer but has no singleton path to it, so the pass records itself
/// here each time it draws. Null until the first SSAO draw (so the first frame's restore is a no-op).
static SSAO_PASS: AtomicPtr<SSAOPass> = AtomicPtr::new(ptr::null_mut());

/// The last-seen `CSSAOPass` instance, or null before the pass has drawn once.
pub fn ssao_pass() -> *mut SSAOPass {
    SSAO_PASS.load(Ordering::Relaxed)
}

/// Clear the pass's temporal filter while `skip` holds, keeping the game's value to put back once it
/// no longer does. The flag is read by the render block when it runs later in the dispatch, so it
/// stays cleared between the calls rather than only around this one.
fn override_temporal_filter(pass: &mut SSAOPass, skip: bool) {
    if skip {
        let saved = if pass.m_EnableTemporalFilter {
            SAVED_TRUE
        } else {
            SAVED_FALSE
        };
        let _ = SAVED_TEMPORAL_FILTER.compare_exchange(
            NOT_OVERRIDDEN,
            saved,
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
        pass.m_EnableTemporalFilter = false;
    } else {
        match SAVED_TEMPORAL_FILTER.swap(NOT_OVERRIDDEN, Ordering::Relaxed) {
            SAVED_TRUE => pass.m_EnableTemporalFilter = true,
            SAVED_FALSE => pass.m_EnableTemporalFilter = false,
            _ => {}
        }
    }
}

/// The game's value of `m_EnableTemporalFilter` while [`override_temporal_filter`] holds it cleared.
static SAVED_TEMPORAL_FILTER: AtomicU8 = AtomicU8::new(NOT_OVERRIDDEN);
const NOT_OVERRIDDEN: u8 = 0;
const SAVED_FALSE: u8 = 1;
const SAVED_TRUE: u8 = 2;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&SSAO_DRAW_BINDER)
}

// SSAOPass::Draw -- the per-dispatch SSAO pass. Its history double-steps across the two stereo
// dispatches and the eyes would compound, so force m_FirstPass on (the engine's "compute fresh,
// ignore history" state) for each eye while stereo is active, so each eye computes AO from its own
// depth instead of blending against the other eye's history. The apply still runs; only the
// cross-eye temporal blend is suppressed.
#[detour(address = jc3gi::graphics_engine::ssao::SSAOPass::Draw_ADDRESS)]
fn ssao_draw(this: *mut SSAOPass) {
    // Record the pass instance for the between-eye history-index restore in `game.rs`.
    SSAO_PASS.store(this, Ordering::Relaxed);

    let stereo = stereo::active();
    let disable = STEREO_CONFIG.query(|c| c.disable_ssao);

    // Skipping the pass leaves the GBuffer-alpha AO untouched (material AO only, no screen AO) and
    // is fully reversible: turning it back on resumes the pass next dispatch with no engine state
    // clobbered.
    if stereo && disable {
        TraceState::record_eye(TraceEvent::SsaoDraw {
            forced_first_pass: false,
        });
        return;
    }

    let skip_temporal = stereo && STEREO_CONFIG.query(|c| c.single_pass.ssao_skip_temporal);
    if let Some(pass) = unsafe { this.as_mut() } {
        pass.m_FirstPass |= stereo;
        override_temporal_filter(pass, skip_temporal);
    }
    TraceState::record_eye(TraceEvent::SsaoDraw {
        forced_first_pass: stereo,
    });
    SSAO_DRAW.get().unwrap().call(this);
}
