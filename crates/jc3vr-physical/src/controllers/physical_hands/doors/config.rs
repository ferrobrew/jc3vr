//! The vehicle door tearing's fixed tuning.

/// Deliberate tearing of a detachable door off its hinge after an open grip settles, with a yank, a
/// hard pull, or a push past its opening limit; distances and speeds use player metres. The tuning
/// is fixed at [`TEARING`].
///
/// The lags are measured above the grip's settled lag, the lag it held while it settled: a heavy
/// door held through a joint hangs a few centimetres off the hand, and a door taken by a reach
/// sits wherever the hand stopped, so a settled grip need not be a close one.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TearTuning {
    /// How long the grip's lag must hold steady before a gesture tears, seconds.
    pub settle_s: f32,
    /// The fastest the lag may change for the grip to count as steady, metres per second.
    pub settle_speed_mps: f32,
    pub direction_cos: f32,
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
    settle_s: 0.2,
    settle_speed_mps: 0.15,
    direction_cos: 0.866_025_4,
    yank_lag_m: 0.16,
    yank_speed_mps: 0.6,
    wrench_lag_m: 0.28,
    wrench_s: 0.18,
    free_swing_share: 0.85,
    push_lag_m: 0.14,
    push_hold_s: 0.35,
    push_load: 0.65,
    limit_margin_rad: 0.08,
};
