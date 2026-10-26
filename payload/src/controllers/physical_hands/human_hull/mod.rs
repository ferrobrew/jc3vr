//! Native width-only human proxy shapes and reversible live restoration.
//!
//! Construction hooks retain the authored table and install narrowed human entries. Live bodies
//! change only in the existing native physics phase. Shutdown requests keep constructors on the
//! authored geometry while that phase restores all tracked replacements.

#[cfg(test)]
mod clearance;
mod geometry;
mod native;
mod query;

use std::{
    ptr::NonNull,
    sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
};

use detours_macro::detour;
use jc3gi::physics::{SharedPfxCharacterSettings, hknpWorld};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;
use windows::Win32::System::Threading::GetCurrentThreadId;

use crate::{
    controllers::physical_hands::human_hull::native::NativeState,
    lifecycle::{
        DeferReason, ParticipantId, Preparation, PrepareOutcome, PreparedParticipant,
        ShutdownParticipant,
    },
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum AdapterAvailability {
    Unavailable,
    Active,
    Disabled,
    RestoreDeferred,
    Restored,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum ClearanceUnavailableReason {
    MissingDispatcherContract,
    NotRequested,
    RestorationRequested,
    Restored,
    Obstructed,
    IncompleteInventory,
    PendingBody,
    InvalidBounds,
    BudgetExhausted,
    MutationRejected,
    InvalidGeometry,
    UnsupportedShape,
    QueryIncomplete,
    QueryRejected,
    RollbackRejected,
    WrongPhase,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct HumanHullStatus {
    pub(crate) adapter: AdapterAvailability,
    pub(crate) clearance: ClearanceUnavailableReason,
    pub(crate) settings: usize,
    pub(crate) migrated_bodies: usize,
    pub(crate) enabled: bool,
}

impl HumanHullStatus {
    pub(crate) const UNAVAILABLE: Self = Self {
        adapter: AdapterAvailability::Unavailable,
        clearance: ClearanceUnavailableReason::MissingDispatcherContract,
        settings: 0,
        migrated_bodies: 0,
        enabled: true,
    };
}

impl Default for HumanHullStatus {
    fn default() -> Self {
        Self::UNAVAILABLE
    }
}

/// The readout copies state and never retains native pointers.
pub(crate) fn status() -> HumanHullStatus {
    let state = STATE.lock();
    HumanHullStatus {
        adapter: if state.committed {
            AdapterAvailability::Restored
        } else if state.shutdown_requested {
            if state.original_active {
                AdapterAvailability::Restored
            } else {
                AdapterAvailability::RestoreDeferred
            }
        } else if !state.enabled {
            if state.original_active {
                AdapterAvailability::Disabled
            } else {
                AdapterAvailability::RestoreDeferred
            }
        } else if state.tables.is_empty() {
            AdapterAvailability::Unavailable
        } else {
            AdapterAvailability::Active
        },
        clearance: state.clearance,
        settings: state.tables.len(),
        migrated_bodies: state.migrated_bodies,
        enabled: state.enabled,
    }
}

/// Disable requests restore at the next native phase; a blocked restore retains the narrow hull.
pub(crate) fn set_enabled(enabled: bool) {
    let mut state = STATE.lock();
    if !state.committed {
        state.enabled = enabled;
    }
}

pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&SETTINGS_CONSTRUCT_BINDER)
        .with_static_binder(&SETTINGS_DESTRUCT_BINDER)
}

/// Hook removal must keep the image mapped if a settings callback remains active after unpatching.
pub(crate) fn active_entries() -> usize {
    ACTIVE_ENTRIES.load(Ordering::Acquire)
}

/// Called before the hands' existing pre-simulation work, with its native write authority.
///
/// # Safety
///
/// The engine's pre-simulation callback, outside the world step, on the game thread.
pub(crate) unsafe fn step(world: *mut hknpWorld) {
    if world.is_null() {
        return;
    }
    let thread = unsafe { GetCurrentThreadId() };
    let established = PHYSICS_THREAD.load(Ordering::Acquire);
    if established != 0 && established != thread {
        STATE.lock().clearance = ClearanceUnavailableReason::WrongPhase;
        return;
    }
    PHYSICS_THREAD.store(thread, Ordering::Release);
    let mut state = STATE.lock();
    if state.committed {
        return;
    }
    if SHUTDOWN_REQUESTED.load(Ordering::Acquire) {
        state.request_shutdown();
    }
    if let Err(reason) = unsafe { state.discover(&*world) } {
        state.clearance = reason;
        return;
    }
    let result = if state.shutdown_requested {
        unsafe { state.restore_for_shutdown(&mut *world) }
    } else {
        unsafe { state.synchronize(&mut *world) }
    };
    if let Err(reason) = result {
        state.clearance = reason;
    }
}

/// Register this participant before enabling the construction hooks.
pub(crate) struct HumanHullShutdown;

impl ShutdownParticipant for HumanHullShutdown {
    fn prepare(&self, preparation: Preparation) -> PrepareOutcome<Box<dyn PreparedParticipant>> {
        let mut state = STATE.lock();
        // The request survives a deferred attempt; only the native phase can restore live bodies.
        state.request_shutdown();
        SHUTDOWN_REQUESTED.store(true, Ordering::Release);
        let Some(generation) = state.shutdown_generation() else {
            return PrepareOutcome::Deferred(DeferReason::ParticipantBusy(
                ParticipantId::HumanCollider,
            ));
        };
        if active_entries() != 0 {
            return PrepareOutcome::Deferred(DeferReason::ParticipantBusy(
                ParticipantId::HumanCollider,
            ));
        }
        PrepareOutcome::Ready(Box::new(RestoredHull {
            generation,
            preparation,
        }))
    }
}

static STATE: Mutex<NativeState> = Mutex::new(NativeState::new());
static PHYSICS_THREAD: AtomicU32 = AtomicU32::new(0);
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);
static ACTIVE_ENTRIES: AtomicUsize = AtomicUsize::new(0);

struct RestoredHull {
    generation: u64,
    preparation: Preparation,
}

impl PreparedParticipant for RestoredHull {
    fn validate(&self, preparation: Preparation) -> bool {
        preparation == self.preparation
            && active_entries() == 0
            && STATE.lock().shutdown_generation() == Some(self.generation)
    }

    fn commit(&mut self) {
        let mut state = STATE.lock();
        // Pending constructors retain authored geometry, including between validation and commit.
        unsafe { state.commit_shutdown() };
    }
}

struct Entry;

impl Entry {
    fn enter() -> Self {
        ACTIVE_ENTRIES.fetch_add(1, Ordering::AcqRel);
        Self
    }
}

impl Drop for Entry {
    fn drop(&mut self) {
        ACTIVE_ENTRIES.fetch_sub(1, Ordering::Release);
    }
}

#[detour(address = SharedPfxCharacterSettings::Construct_ADDRESS)]
fn settings_construct(
    settings: *mut SharedPfxCharacterSettings,
) -> *mut SharedPfxCharacterSettings {
    let _entry = Entry::enter();
    let result = SETTINGS_CONSTRUCT.get().unwrap().call(settings);
    if let Some(settings) = NonNull::new(settings) {
        let mut state = STATE.lock();
        if SHUTDOWN_REQUESTED.load(Ordering::Acquire) {
            state.request_shutdown();
        }
        if let Err(error) = unsafe { state.adopt(settings) } {
            state.clearance = ClearanceUnavailableReason::InvalidGeometry;
            tracing::warn!(?error, "human hull: settings retain the authored geometry");
        }
    }
    result
}

#[detour(address = SharedPfxCharacterSettings::Destruct_ADDRESS)]
fn settings_destruct(settings: *mut SharedPfxCharacterSettings) {
    let _entry = Entry::enter();
    if let Some(settings) = NonNull::new(settings) {
        STATE.lock().forget(settings);
    }
    SETTINGS_DESTRUCT.get().unwrap().call(settings);
}
