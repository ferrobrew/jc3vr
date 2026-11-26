use std::sync::OnceLock;

use jc3gi::game::GameState;
use parking_lot::{Mutex, MutexGuard};
use re_utilities::hook_library::HookLibrary;

pub use crate::hooks::stages::{register_post_enable, register_post_unpatch, register_pre_unpatch};

pub mod online;
mod stages;
pub mod suspend;

/// Whether the game is in interactive gameplay (`GameState::E_GAME_RUN`), as opposed to any
/// non-gameplay state (install, init, frontend, loading, startup). The single gameplay-boundary
/// predicate the hooks, the HUD, and the VR runtime gate on, so "are we in gameplay" is defined in
/// exactly one place and every consumer flips together.
pub fn in_gameplay() -> bool {
    // SAFETY: reads the process-global game-state word.
    unsafe { GameState::get() == GameState::E_GAME_RUN }
}

static HOOK_STATE: OnceLock<HookState> = OnceLock::new();
struct HookState {
    patcher: Mutex<re_utilities::Patcher>,
    hook_library: HookLibrary,
}
unsafe impl Send for HookState {}
unsafe impl Sync for HookState {}

/// Install the hook library `build` composes, under thread suspension, and record it for
/// [`uninstall`]. The steps registered through [`register_post_enable`] run once it is enabled.
///
/// The caller registers every lifecycle participant and removal step first: a participant must be
/// registered before its engine policy is enabled or its work accepted.
pub fn install(build: impl FnOnce() -> HookLibrary) {
    tracing::info!("startup: lifecycle participants registered; hook installation begin");
    let mut patcher = re_utilities::Patcher::new();
    tracing::info!("startup: before thread suspension");
    let hook_library = suspend::for_block(|| {
        tracing::info!("startup: inside thread suspension, before hook library construction");
        let library = build();
        tracing::info!("startup: before enable");
        let library = library.enable(&mut patcher);
        tracing::info!("startup: after enable");
        Ok(library?)
    });
    let hook_library = match hook_library {
        Ok(hook_library) => hook_library,
        Err(e) => {
            tracing::error!("Failed to enable the hook library: {e:?}");
            return;
        }
    };
    if HOOK_STATE
        .set(HookState {
            patcher: Mutex::new(patcher),
            hook_library,
        })
        .is_ok()
    {
        stages::run_post_enable();
    }
}

pub fn uninstall() -> bool {
    if !crate::is_shutting_down() {
        tracing::error!("hook removal refused before shutdown commit");
        return false;
    }
    if !stages::run_pre_unpatch() {
        return false;
    }
    let Some(state) = HOOK_STATE.get() else {
        tracing::error!("hook removal: installation state is unavailable");
        return false;
    };
    if let Err(error) = suspend::for_block(|| {
        Ok(state
            .hook_library
            .set_enabled(&mut state.patcher.lock(), false)?)
    }) {
        tracing::error!(?error, "hook removal failed; the image must remain mapped");
        return false;
    }

    // `HookLibrary::set_enabled(false)` unpatches only the addresses it owns. Everything applied
    // through [`patcher`] shares the same `Patcher` but belongs to no hook library -- the
    // render-block-type kill switch, the far-field `IsEnabled` gates, the culling and terrain budget
    // scalars, the debug UI's config pokes -- and the `Patcher` lives in a `static` that
    // `FreeLibrary` never drops. Left standing, a gated vtable slot goes on pointing into the
    // unmapped payload and the game dies on its next dispatch.
    //
    // Dropping a `Patcher` restores every patch it still holds, so swapping in a fresh one reverts
    // the lot. Deliberately outside the suspender, for two reasons: the writes restore the game's
    // own original bytes, which needs no quiescence (a thread already inside a stub the slot pointed
    // at is unaffected, and the payload stays mapped until `module::exit`), while the drop frees the
    // patch map -- and a heap free under suspension can deadlock against a suspended thread holding
    // the heap lock, which is the hazard `uninstall_com_detours` is structured around.
    drop(std::mem::replace(
        &mut *state.patcher.lock(),
        re_utilities::Patcher::new(),
    ));
    stages::run_post_unpatch()
}

pub fn patcher() -> Option<MutexGuard<'static, re_utilities::Patcher>> {
    HOOK_STATE.get().map(|state| state.patcher.lock())
}
