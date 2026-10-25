use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};

use jc3gi::game::GameState;
use parking_lot::{Mutex, MutexGuard};
use re_utilities::{ThreadSuspender, hook_library::HookLibrary};

use crate::{
    controllers::weapons::reload,
    lifecycle::{
        self, DeferReason, ParticipantId, Preparation, PrepareOutcome, PreparedParticipant,
        ShutdownParticipant,
    },
    thread_role::{self, ThreadRole},
};

pub mod animation;
pub mod camera;
pub mod character;
pub mod clock;
pub mod draw_count;
pub mod game;
pub mod graphics_engine;
pub mod input;
#[cfg(feature = "profiler")]
pub mod profiler;
pub mod sim;
pub mod ui;
pub mod wndproc;

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

/// The mailbox remains closed until an adapter proves entry exclusion through hook removal.
///
/// HookLibrary does not supply exclusion. ThreadSuspender snapshots threads, ignores suspension
/// failures, and does not drain an original call already inside the callback. The game pre-update
/// role alone does not exclude an unexpected worker entry. No safe activation adapter exists yet.
#[allow(dead_code)]
pub(crate) fn activate_magazine_executor() -> Result<(), DeferReason> {
    Err(DeferReason::HookEntryExclusionUnproven)
}

pub(super) fn install() {
    if let Err(error) =
        lifecycle::register_participant(ParticipantId::MagazineExecutor, Arc::new(MagazineShutdown))
    {
        tracing::error!(%error, "hook installation refused without shutdown preparation");
        return;
    }
    let mut patcher = re_utilities::Patcher::new();
    let hook_library = ThreadSuspender::for_block(|| {
        let library = HookLibrary::new()
            .with_hook_library(game::hook_library())
            .with_hook_library(sim::hook_library())
            .with_hook_library(clock::hook_library())
            .with_hook_library(graphics_engine::hook_library())
            .with_hook_library(draw_count::hook_library())
            .with_hook_library(camera::hook_library())
            .with_hook_library(wndproc::hook_library())
            .with_hook_library(character::hook_library())
            .with_hook_library(animation::hook_library())
            .with_hook_library(input::hook_library())
            .with_hook_library(ui::hook_library())
            .with_hook_library(crate::pause_menu::hook_library())
            .with_hook_library(crate::controllers::hook_library())
            .with_hook_library(crate::grapple::physical::hook_library());
        #[cfg(feature = "profiler")]
        let library = library.with_hook_library(profiler::hook_library());
        Ok(library.enable(&mut patcher)?)
    });
    let hook_library = match hook_library {
        Ok(hook_library) => hook_library,
        Err(e) => {
            tracing::error!("Failed to enable the hook library: {e:?}");
            return;
        }
    };
    let _ = HOOK_STATE.set(HookState {
        patcher: Mutex::new(patcher),
        hook_library,
    });
}

pub(super) fn uninstall() -> bool {
    if !crate::is_shutting_down() {
        tracing::error!("hook removal refused before shutdown commit");
        return false;
    }
    // The single-pass COM-vtable detours inline-patch the DXVK functions to jump into payload code
    // and are managed outside the HookLibrary, so tear them down explicitly before the DLL unloads --
    // otherwise the dangling jumps crash the game on the next D3D call.
    crate::stereo::single_pass::uninstall_com_detours();
    let Some(state) = HOOK_STATE.get() else {
        tracing::error!("hook removal: installation state is unavailable");
        return false;
    };
    if let Err(error) = ThreadSuspender::for_block(|| {
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
    true
}

struct MagazineShutdown;

impl ShutdownParticipant for MagazineShutdown {
    fn prepare(&self, preparation: Preparation) -> PrepareOutcome<Box<dyn PreparedParticipant>> {
        if thread_role::current() != ThreadRole::Game {
            return PrepareOutcome::Deferred(DeferReason::ParticipantBusy(
                ParticipantId::MagazineExecutor,
            ));
        }
        // This cancels only extraction. It must not stand down other gameplay during a retry.
        reload::cancel_extraction_on_standdown();
        match magazine_preparation(
            reload::serial::stop_and_drain(),
            MAGAZINE_BINDER_INSTALLED.load(Ordering::Acquire),
        ) {
            PrepareOutcome::Ready(()) => {
                PrepareOutcome::Ready(Box::new(InactiveMagazine { preparation }))
            }
            PrepareOutcome::Deferred(reason) => PrepareOutcome::Deferred(reason),
            PrepareOutcome::Failed(error) => PrepareOutcome::Failed(error),
        }
    }
}

fn magazine_preparation(drained: bool, binder_installed: bool) -> PrepareOutcome<()> {
    if !drained {
        PrepareOutcome::Deferred(DeferReason::MagazineBusy)
    } else if binder_installed {
        PrepareOutcome::Deferred(DeferReason::HookEntryExclusionUnproven)
    } else {
        PrepareOutcome::Ready(())
    }
}

struct InactiveMagazine {
    preparation: Preparation,
}

impl PreparedParticipant for InactiveMagazine {
    fn validate(&self, preparation: Preparation) -> bool {
        self.preparation == preparation
            && !MAGAZINE_BINDER_INSTALLED.load(Ordering::Acquire)
            && reload::serial::stop_and_drain()
    }

    fn commit(&mut self) {}
}

// This remains false because activation is blocked, and the character library omits the binder.
// A future installer must publish installation before enabling it, even when acceptance is closed.
static MAGAZINE_BINDER_INSTALLED: AtomicBool = AtomicBool::new(false);

pub(super) fn patcher() -> Option<MutexGuard<'static, re_utilities::Patcher>> {
    HOOK_STATE.get().map(|state| state.patcher.lock())
}

#[cfg(test)]
mod lifecycle_tests;
