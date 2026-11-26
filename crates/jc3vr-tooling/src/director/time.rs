//! Time control: slow motion through the slow-motion manager's normal game speed, and a hold on
//! the engine clock.
//!
//! The clock's own `m_UpdateSpeed` is not the knob: `CSlowMotionManager::Update` rewrites it every
//! frame from `s_NormalGameSpeed` blended with any active effect, so a write there lasts one frame.
//! Writing the normal speed instead persists and keeps the game's scripted slow-motion effects
//! (the near-miss, the death camera) blending against the directed rate rather than fighting it.

use std::sync::atomic::{AtomicBool, Ordering};

use jc3gi::{clock::Clock, slow_motion::get_SlowMotionManager_NormalGameSpeed};

pub(crate) use crate::controllers::clock::{MAX_TIME_SCALE, MIN_TIME_SCALE};
use crate::director::DirectorConfig;

/// Whether the last applied pass held the clock, so the pause call is made on the edge only.
static PAUSED: AtomicBool = AtomicBool::new(false);

/// Reconcile the clock against the config. Main thread.
pub(super) fn apply(cfg: &DirectorConfig) {
    // SAFETY: the normal game speed is a plain static float the slow-motion manager reads on the
    // main thread; the clock singleton is read and written on the main thread.
    unsafe {
        let speed = get_SlowMotionManager_NormalGameSpeed();
        if *speed != cfg.time_scale {
            tracing::info!("director: time scale {} -> {}", *speed, cfg.time_scale);
            *speed = cfg.time_scale;
        }
        if PAUSED.swap(cfg.paused, Ordering::Relaxed) != cfg.paused
            && let Some(clock) = Clock::get()
        {
            clock.Pause(cfg.paused);
            tracing::info!(
                "director: clock {}",
                if cfg.paused { "paused" } else { "resumed" }
            );
        }
    }
}

/// Toggle between real time and the configured slow-motion rate.
pub fn toggle_slow_motion(cfg: &mut DirectorConfig) {
    cfg.time_scale = if cfg.time_scale == 1.0 {
        cfg.slow_motion_scale
    } else {
        1.0
    };
}

/// Step the time scale by `factor` (multiplicative, so repeated presses walk a geometric ladder),
/// clamped to a usable range.
pub fn step_time_scale(cfg: &mut DirectorConfig, factor: f32) {
    cfg.time_scale = (cfg.time_scale * factor).clamp(MIN_TIME_SCALE, MAX_TIME_SCALE);
}
