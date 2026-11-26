//! Heat control: set, clear, lock, and cap the wanted level through the heat manager.
//!
//! Lock and cap are re-asserted every frame because the heat manager's own reset (a load, a
//! mission restart) clears the lock and removes the cap. A cap the config does not hold is left to
//! the game, which sets its own during missions; the pass only writes the no-cap value once, on
//! the edge where the config releases its cap.

use jc3gi::{character::character::Character, heat_manager::HeatManager};
use parking_lot::Mutex;

use crate::director::DirectorConfig;

/// What the last pass asserted, so releasing the lock or the cap writes the game's default once.
static ASSERTED: Mutex<Asserted> = Mutex::new(Asserted {
    lock: false,
    cap: None,
});

struct Asserted {
    lock: bool,
    cap: Option<i32>,
}

/// Reconcile the heat manager against the config. Main thread.
pub(super) fn apply(cfg: &DirectorConfig) {
    // SAFETY: the heat manager is a main-thread singleton; every setter asserts the main thread.
    unsafe {
        let Some(heat) = HeatManager::get() else {
            return;
        };
        let mut asserted = ASSERTED.lock();

        if cfg.heat_lock {
            if !heat.m_SystemLocked {
                heat.m_SystemLocked = true;
                tracing::info!("director: heat locked");
            }
        } else if asserted.lock && heat.m_SystemLocked {
            heat.m_SystemLocked = false;
            tracing::info!("director: heat unlocked");
        }
        asserted.lock = cfg.heat_lock;

        match cfg.heat_cap {
            Some(cap) => {
                let cap = cap.clamp(0, HeatManager::MAX_LEVEL);
                if heat.m_LevelCap != cap {
                    heat.SetLevelCap(cap);
                    tracing::info!("director: heat cap {cap}");
                }
            }
            None => {
                if asserted.cap.is_some() {
                    heat.m_LevelCap = HeatManager::LEVEL_CAP_NONE;
                    tracing::info!("director: heat cap released");
                }
            }
        }
        asserted.cap = cfg.heat_cap;
    }
}

/// Set the heat level on the player, ignoring the cap. Main thread.
pub fn set_level(level: i32) {
    // SAFETY: main-thread singletons, called from the main-thread update or UI pass.
    unsafe {
        let (Some(heat), Some(player)) = (
            HeatManager::get(),
            Character::GetLocalPlayerCharacter().as_mut(),
        ) else {
            return;
        };
        let level = level.clamp(0, HeatManager::MAX_LEVEL);
        if level == 0 {
            heat.ClearHeat(false);
        } else {
            heat.SetHeat(level, player, true, false);
        }
        tracing::info!("director: heat set to {level}");
    }
}

/// Drop the heat to zero. Main thread.
pub fn clear() {
    set_level(0);
}

/// The live heat level and whether the manager reports itself locked, for the debug tab.
pub fn live_state() -> Option<(i32, bool, i32)> {
    // SAFETY: read-only access to a main-thread singleton from the UI pass on that thread.
    unsafe {
        let heat = HeatManager::get()?;
        Some((heat.m_Level, heat.m_SystemLocked, heat.m_LevelCap))
    }
}
