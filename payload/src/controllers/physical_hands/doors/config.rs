//! The vehicle doors' settings, and the tearing's fixed tuning.

use serde::{Deserialize, Serialize};

/// Opening vehicle doors by hand (see [`crate::controllers::physical_hands::doors`]): a grip on a
/// closed door's handle area takes hold of it, a pull unlatches it onto its hinge, and the hand
/// swings it from there. A shove swings it shut, and the game latches it.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DoorsConfig {
    pub enabled: bool,
    /// Tear a detachable door off its hinge by hand, with a yank or a push past its opening limit
    /// once the grip has settled ([`TEARING`]).
    pub tear: bool,
}

impl DoorsConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            tear: true,
        }
    }
}

impl Default for DoorsConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Deliberate tearing after an open grip settles; distances and speeds use player metres. The
/// tuning is fixed at [`TEARING`], and the tick sets [`enabled`](Self::enabled) from the switches.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TearTuning {
    /// Whether a grip may tear its door at all.
    pub enabled: bool,
    /// How long the grip must stay within [`reset_lag_m`](Self::reset_lag_m) of the door before a
    /// gesture tears, seconds.
    pub settle_s: f32,
    /// The grip lag of a settled hand, metres.
    pub reset_lag_m: f32,
    /// The grip lag of a yank, metres.
    pub yank_lag_m: f32,
    /// The separating speed of a yank, metres per second.
    pub yank_speed_mps: f32,
    /// How far past the opening limit a push must carry the hand, metres.
    pub push_lag_m: f32,
    /// How long a push must last, seconds.
    pub push_hold_s: f32,
    /// The grip load of a push, `0..=1`.
    pub push_load: f32,
    /// How near its opening limit the door must be for a push to count, radians.
    pub limit_margin_rad: f32,
}

/// The tearing's tuning.
pub const TEARING: TearTuning = TearTuning {
    enabled: true,
    settle_s: 0.2,
    reset_lag_m: 0.045,
    yank_lag_m: 0.18,
    yank_speed_mps: 1.8,
    push_lag_m: 0.14,
    push_hold_s: 0.18,
    push_load: 0.65,
    limit_margin_rad: 0.08,
};
