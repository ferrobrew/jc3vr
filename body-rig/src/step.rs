//! The step commit: when Rico steps, and the velocity that carries his pelvis back under the
//! player's neck.
//!
//! Step demand near the hysteresis line flickers with every bob of the head, so a step commits only
//! once the demand passes a threshold, and a committed step runs until the pelvis is back under the
//! neck rather than until the demand drops. The drive is a closed loop on the remaining gap: the
//! velocity is the gap over a response time, capped, so it tapers on arrival and the walk cycle
//! does not overshoot. What is left once the step arrives is slid out slowly while Rico idles, and
//! a step that does not arrive (a wall) gives up rather than walking into it forever.
//!
//! A weight shift inside the lean's reach commits no step, and a player who stands still with one
//! hip out would otherwise be drawn bent for as long as they stand there. So while Rico idles and
//! the head holds still, an offset to the side or back that stays for a while is drifted out: the
//! pelvis shuffles under the neck at a slow speed, with no walk, until it is there.

use glam::Vec2;

/// The step commit's tuning.
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default)
)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StepTuning {
    /// The step demand that commits a step, metres.
    pub commit_m: f32,
    /// A step arrives once the pelvis is this close under the neck, metres.
    pub arrive_m: f32,
    /// The drive closes the gap at the gap over this, seconds.
    pub response_s: f32,
    /// The fastest the drive moves the pelvis, metres per second.
    pub max_speed_mps: f32,
    /// The speed the remainder is slid out at after a step arrives, metres per second.
    pub settle_speed_mps: f32,
    /// The settle stops once the pelvis is this close under the neck, metres.
    pub settle_done_m: f32,
    /// The longest a settle slides, seconds.
    pub settle_s: f32,
    /// A step that has not arrived after this long stops, seconds.
    pub give_up_s: f32,
    /// The idle drift starts once the drift has been longer than this, metres...
    pub drift_min_m: f32,
    /// ...for this long, seconds, with the head steady.
    pub drift_after_s: f32,
    /// The head is steady below this speed, metres per second.
    pub drift_steady_mps: f32,
    /// The speed the idle drift shuffles the pelvis at, metres per second.
    pub drift_speed_mps: f32,
}

impl StepTuning {
    pub const fn new() -> Self {
        Self {
            commit_m: 0.05,
            arrive_m: 0.06,
            response_s: 0.3,
            max_speed_mps: 1.8,
            settle_speed_mps: 0.15,
            settle_done_m: 0.02,
            settle_s: 0.6,
            give_up_s: 3.0,
            drift_min_m: 0.04,
            drift_after_s: 1.0,
            drift_steady_mps: 0.06,
            drift_speed_mps: 0.12,
        }
    }
}

impl Default for StepTuning {
    fn default() -> Self {
        Self::new()
    }
}

/// What the pelvis is doing.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum StepPhase {
    /// Standing: the spine absorbs the offset.
    #[default]
    Idle,
    /// A committed step, driving the pelvis under the neck.
    Stepping,
    /// The remainder of an arrived step, slid out slowly.
    Settling,
    /// A weight shift held while standing still, shuffled out slowly.
    Drifting,
}

impl StepPhase {
    /// Whether the phase moves the pelvis.
    pub fn drives(self) -> bool {
        self != StepPhase::Idle
    }

    /// Whether the phase is a step's: the body moves as a walk rather than a shuffle under a
    /// standing player.
    pub fn steps(self) -> bool {
        matches!(self, StepPhase::Stepping | StepPhase::Settling)
    }

    pub fn label(self) -> &'static str {
        match self {
            StepPhase::Idle => "idle",
            StepPhase::Stepping => "stepping",
            StepPhase::Settling => "settling",
            StepPhase::Drifting => "drifting",
        }
    }
}

/// One frame's inputs to the step commit.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StepInput {
    /// The step demand's length, metres.
    pub demand_m: f32,
    /// The pelvis's horizontal gap to the floor point under the neck, `(x, z)`.
    pub gap: Vec2,
    /// The part of the gap the idle drift takes out, `(x, z)`.
    pub drift: Vec2,
    /// How fast the neck moves horizontally, metres per second.
    pub head_speed_mps: f32,
    /// Seconds since the last frame.
    pub dt: f32,
}

/// The step commit's state.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct StepCommit {
    phase: StepPhase,
    /// Seconds in the current phase.
    elapsed: f32,
    /// Steps committed since the commit was made.
    steps: u64,
    /// How long, idle, the drift has stayed past its threshold with the head steady, seconds.
    drift_held: f32,
}

impl StepCommit {
    pub const fn new() -> Self {
        Self {
            phase: StepPhase::Idle,
            elapsed: 0.0,
            steps: 0,
            drift_held: 0.0,
        }
    }

    pub fn phase(&self) -> StepPhase {
        self.phase
    }

    /// Steps committed since the commit was made.
    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// Stand: whatever was under way stops, and the next step needs the threshold again.
    pub fn stand(&mut self) {
        self.phase = StepPhase::Idle;
        self.elapsed = 0.0;
        self.drift_held = 0.0;
    }

    /// Advance one frame and return the phase.
    pub fn update(&mut self, input: &StepInput, tuning: &StepTuning) -> StepPhase {
        let dt = if input.dt.is_finite() {
            input.dt.max(0.0)
        } else {
            0.0
        };
        let gap = if input.gap.is_finite() {
            input.gap.length()
        } else {
            0.0
        };
        let demand = if input.demand_m.is_finite() {
            input.demand_m
        } else {
            0.0
        };
        let drift = if input.drift.is_finite() {
            input.drift.length()
        } else {
            0.0
        };
        let steady =
            input.head_speed_mps.is_finite() && input.head_speed_mps < tuning.drift_steady_mps;
        if self.phase == StepPhase::Idle && drift > tuning.drift_min_m && steady {
            self.drift_held += dt;
        } else {
            self.drift_held = 0.0;
        }
        self.elapsed += dt;
        let next = match self.phase {
            StepPhase::Idle | StepPhase::Settling | StepPhase::Drifting
                if demand > tuning.commit_m =>
            {
                self.steps += 1;
                StepPhase::Stepping
            }
            StepPhase::Idle if self.drift_held >= tuning.drift_after_s => StepPhase::Drifting,
            StepPhase::Idle => StepPhase::Idle,
            StepPhase::Drifting
                if drift <= tuning.settle_done_m || self.elapsed >= tuning.give_up_s =>
            {
                StepPhase::Idle
            }
            StepPhase::Drifting => StepPhase::Drifting,
            StepPhase::Stepping if gap <= tuning.arrive_m => {
                if gap > tuning.settle_done_m {
                    StepPhase::Settling
                } else {
                    StepPhase::Idle
                }
            }
            StepPhase::Stepping if self.elapsed >= tuning.give_up_s => StepPhase::Idle,
            StepPhase::Stepping => StepPhase::Stepping,
            StepPhase::Settling
                if gap <= tuning.settle_done_m || self.elapsed >= tuning.settle_s =>
            {
                StepPhase::Idle
            }
            StepPhase::Settling => StepPhase::Settling,
        };
        if next != self.phase {
            self.elapsed = 0.0;
        }
        self.phase = next;
        next
    }
}

/// The velocity that drives the pelvis along `gap` (the pelvis to the floor point under the neck,
/// `(x, z)`, or the drift while drifting) in `phase`, for a step of `dt` seconds: never further than
/// the gap in one step.
pub fn drive_velocity(phase: StepPhase, gap: Vec2, dt: f32, tuning: &StepTuning) -> Vec2 {
    let Some(direction) = gap.try_normalize() else {
        return Vec2::ZERO;
    };
    let distance = gap.length();
    let speed = match phase {
        StepPhase::Idle => 0.0,
        StepPhase::Stepping => distance / tuning.response_s.max(1.0e-3),
        StepPhase::Settling => tuning.settle_speed_mps,
        StepPhase::Drifting => tuning.drift_speed_mps,
    };
    let cap = tuning.max_speed_mps.max(0.0);
    let speed = speed.clamp(0.0, cap);
    let speed = if dt.is_finite() && dt > 0.0 {
        speed.min(distance / dt)
    } else {
        speed
    };
    let velocity = direction * speed;
    if velocity.is_finite() {
        velocity
    } else {
        Vec2::ZERO
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const TUNING: StepTuning = StepTuning::new();
    const DT: f32 = 1.0 / 90.0;

    fn input(demand_m: f32, gap: f32) -> StepInput {
        StepInput {
            demand_m,
            gap: Vec2::new(0.0, -gap),
            drift: Vec2::ZERO,
            head_speed_mps: 0.0,
            dt: DT,
        }
    }

    #[test]
    fn demand_below_the_threshold_never_steps() {
        let mut commit = StepCommit::new();
        for i in 0..1000 {
            let wobble = TUNING.commit_m * 0.9 * ((i as f32) * 0.3).sin().abs();
            assert_eq!(
                commit.update(&input(wobble, 0.25), &TUNING),
                StepPhase::Idle
            );
        }
        assert_eq!(commit.steps(), 0);
    }

    #[test]
    fn a_committed_step_runs_until_the_pelvis_is_under_the_neck() {
        let mut commit = StepCommit::new();
        assert_eq!(
            commit.update(&input(0.2, 0.4), &TUNING),
            StepPhase::Stepping
        );
        // The demand falls away as the pelvis closes, but the step keeps going.
        assert_eq!(
            commit.update(&input(0.0, 0.15), &TUNING),
            StepPhase::Stepping
        );
        assert_eq!(
            commit.update(&input(0.0, TUNING.arrive_m * 0.5), &TUNING),
            StepPhase::Settling
        );
        assert_eq!(commit.update(&input(0.0, 0.0), &TUNING), StepPhase::Idle);
        assert_eq!(commit.steps(), 1);
    }

    #[test]
    fn a_settle_ends_on_time() {
        let mut commit = StepCommit::new();
        commit.update(&input(0.2, 0.4), &TUNING);
        commit.update(&input(0.0, 0.04), &TUNING);
        let ticks = (TUNING.settle_s / DT).ceil() as usize + 1;
        let mut phase = StepPhase::Settling;
        for _ in 0..ticks {
            phase = commit.update(&input(0.0, 0.04), &TUNING);
        }
        assert_eq!(phase, StepPhase::Idle);
    }

    #[test]
    fn a_blocked_step_gives_up() {
        let mut commit = StepCommit::new();
        commit.update(&input(0.0, 0.0), &TUNING);
        commit.update(&input(0.3, 0.5), &TUNING);
        let ticks = (TUNING.give_up_s / DT).ceil() as usize + 1;
        for _ in 0..ticks {
            commit.update(&input(0.0, 0.5), &TUNING);
        }
        assert_eq!(commit.phase(), StepPhase::Idle);
    }

    fn drifting(drift: f32, head_speed_mps: f32) -> StepInput {
        StepInput {
            drift: Vec2::new(drift, 0.0),
            head_speed_mps,
            gap: Vec2::new(drift, 0.0),
            ..input(0.0, 0.0)
        }
    }

    #[test]
    fn a_held_weight_shift_drifts_out() {
        let mut commit = StepCommit::new();
        let hold = (TUNING.drift_after_s / DT).ceil() as usize + 1;
        let mut phase = StepPhase::Idle;
        for _ in 0..hold {
            phase = commit.update(&drifting(0.1, 0.0), &TUNING);
        }
        assert_eq!(phase, StepPhase::Drifting);
        assert_eq!(commit.steps(), 0);
        assert_eq!(
            commit.update(&drifting(0.01, 0.0), &TUNING),
            StepPhase::Idle
        );
    }

    #[test]
    fn a_moving_head_or_a_small_shift_never_drifts() {
        let mut commit = StepCommit::new();
        for _ in 0..1000 {
            assert_eq!(
                commit.update(&drifting(0.1, TUNING.drift_steady_mps * 2.0), &TUNING),
                StepPhase::Idle
            );
            assert_eq!(
                commit.update(&drifting(TUNING.drift_min_m * 0.5, 0.0), &TUNING),
                StepPhase::Idle
            );
        }
    }

    #[test]
    fn the_drive_tapers_and_caps() {
        let far = drive_velocity(StepPhase::Stepping, Vec2::new(0.0, -2.0), DT, &TUNING);
        assert!((far.length() - TUNING.max_speed_mps).abs() < 1e-5);
        let near = drive_velocity(StepPhase::Stepping, Vec2::new(0.0, -0.1), DT, &TUNING);
        assert!((near.length() - 0.1 / TUNING.response_s).abs() < 1e-5);
        assert_eq!(
            drive_velocity(StepPhase::Idle, Vec2::new(1.0, 0.0), DT, &TUNING),
            Vec2::ZERO
        );
        let settle = drive_velocity(StepPhase::Settling, Vec2::new(0.05, 0.0), DT, &TUNING);
        assert!((settle.length() - TUNING.settle_speed_mps).abs() < 1e-5);
    }

    #[test]
    fn a_driven_pelvis_arrives_without_overshooting() {
        let mut commit = StepCommit::new();
        let mut pelvis = Vec2::ZERO;
        let neck = Vec2::new(0.3, -0.6);
        let mut closest = f32::MAX;
        for _ in 0..600 {
            let gap = neck - pelvis;
            let demand = (gap.length() - 0.2).max(0.0);
            let phase = commit.update(
                &StepInput {
                    demand_m: demand,
                    gap,
                    drift: Vec2::ZERO,
                    head_speed_mps: 0.0,
                    dt: DT,
                },
                &TUNING,
            );
            let velocity = drive_velocity(phase, gap, DT, &TUNING);
            pelvis += velocity * DT;
            closest = closest.min((neck - pelvis).length());
            // Never past the neck along the original line.
            assert!((pelvis).dot(neck.normalize()) <= neck.length() + 1e-4);
        }
        assert!((neck - pelvis).length() <= TUNING.settle_done_m + 1e-3);
        assert_eq!(commit.phase(), StepPhase::Idle);
        assert!(closest <= TUNING.settle_done_m + 1e-3);
    }

    proptest! {
        #[test]
        fn the_drive_never_passes_the_gap(
            x in -2.0f32..2.0, z in -2.0f32..2.0, dt in 0.001f32..0.2,
        ) {
            let gap = Vec2::new(x, z);
            for phase in [
                StepPhase::Idle,
                StepPhase::Stepping,
                StepPhase::Settling,
                StepPhase::Drifting,
            ] {
                let v = drive_velocity(phase, gap, dt, &TUNING);
                prop_assert!(v.length() * dt <= gap.length() + 1e-4);
                prop_assert!(v.length() <= TUNING.max_speed_mps + 1e-4);
                prop_assert!(v.dot(gap) >= -1e-6);
            }
        }
    }
}
