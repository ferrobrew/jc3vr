//! The vehicle doors' settings, and the tearing's fixed tuning.

use serde::{Deserialize, Serialize};

use crate::controllers::physical_hands::doors::hinge::HingeConfig;

/// Opening vehicle doors by hand (see [`crate::controllers::physical_hands::doors`]): a grip on a
/// closed door's handle area takes hold of it, a pull unlatches it onto its hinge, and the hand
/// swings it from there. A shove swings it shut, and the game latches it.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DoorsConfig {
    pub enabled: bool,
    /// Tear a detachable door off its hinge by hand, with a yank, a hard pull, or a push past its
    /// opening limit once the grip has settled ([`TEARING`]).
    pub tear: bool,
    /// What keeps a door the hands opened where it is left, and when it latches.
    pub hinge: HingeConfig,
}

impl DoorsConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            tear: true,
            hinge: HingeConfig::new(),
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
///
/// The lags are measured above the grip's settled lag, the lag it held while it settled: a heavy
/// door held through a joint hangs a few centimetres off the hand, and a door taken by a reach
/// sits wherever the hand stopped, so a settled grip need not be a close one.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TearTuning {
    /// Whether a grip may tear its door at all.
    pub enabled: bool,
    /// How long the grip's lag must hold steady before a gesture tears, seconds.
    pub settle_s: f32,
    /// The fastest the lag may change for the grip to count as steady, metres per second.
    pub settle_speed_mps: f32,
    /// The lag a yank opens above the settled lag, metres.
    pub yank_lag_m: f32,
    /// How fast the lag must grow, and the hand pull away from the door, for a yank, metres per
    /// second: a pull a player makes on a door that does not give.
    pub yank_speed_mps: f32,
    /// The lag a hard pull holds above the settled lag, however fast it got there, metres. Short of
    /// the distance at which a hand's body lets go of what it is joined to.
    pub wrench_lag_m: f32,
    /// How long a hard pull must hold its lag, seconds.
    pub wrench_s: f32,
    /// The share of the lag along the door's free swing above which a yank or a hard pull does not
    /// count away from the hinge's limits: there the door would swing with the hand, and a lag is
    /// a heavy door catching up rather than the hinge holding it.
    pub free_swing_share: f32,
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
    settle_speed_mps: 0.15,
    yank_lag_m: 0.16,
    yank_speed_mps: 0.6,
    wrench_lag_m: 0.28,
    wrench_s: 0.08,
    free_swing_share: 0.85,
    push_lag_m: 0.14,
    push_hold_s: 0.18,
    push_load: 0.65,
    limit_margin_rad: 0.08,
};
