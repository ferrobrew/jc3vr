//! The sun-shadow diagnostic override: the stereo configuration's switch that forces the game's
//! sun shadows off through the engine's own settings path, and whether it is engaged.

use std::sync::atomic::{AtomicU8, Ordering};

use jc3gi::graphics_engine::graphics_engine::GraphicsEngine;

/// Whether the override holds the sun shadows off.
pub(crate) fn override_active() -> bool {
    SAVED_SHADOWS_ENABLED.load(Ordering::Relaxed) != u8::MAX
}

/// Force the sun-shadow system off (or restore it) through the engine's own settings path:
/// `ShadowManager::m_Enabled`, which the sim-side `UpdateRender` syncs the engine to via
/// `SetEnabled` -- the same route the graphics menu takes, so resources are torn down and recreated
/// cleanly. Called on the game thread before the frame's update render.
pub fn apply_override(disable: bool) {
    // SAFETY: the graphics-engine singleton and its shadow manager are live once the engine is
    // initialised; both are null-checked, and the flag is a plain settings toggle.
    unsafe {
        let Some(manager) = GraphicsEngine::get().and_then(|ge| ge.m_ShadowManager.as_mut()) else {
            return;
        };
        let saved = SAVED_SHADOWS_ENABLED.load(Ordering::Relaxed);
        if disable && saved == u8::MAX {
            SAVED_SHADOWS_ENABLED.store(u8::from(manager.m_Enabled), Ordering::Relaxed);
            manager.m_Enabled = false;
        } else if !disable && saved != u8::MAX {
            manager.m_Enabled = saved != 0;
            SAVED_SHADOWS_ENABLED.store(u8::MAX, Ordering::Relaxed);
        }
    }
}

/// The pre-override value of the shadow manager's settings-side enabled flag, captured when the
/// diagnostic engages so releasing it restores whatever the game's own settings had. `u8::MAX` =
/// no override active.
static SAVED_SHADOWS_ENABLED: AtomicU8 = AtomicU8::new(u8::MAX);
