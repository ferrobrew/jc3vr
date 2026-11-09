//! The physical grips on one retained mounted weapon, and the pull that tears it off.

use std::time::Instant;

use crate::controllers::physical_hands::pickup::HeldReport;

/// Which weapon and body the grips belong to, so a recycled address is not mistaken for the gun
/// the record was made for.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct Identity {
    pub(super) weapon: usize,
    pub(super) control: usize,
    pub(super) object: usize,
    pub(super) body: u32,
}

/// The grips and fire inputs one retained weapon has this tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Input {
    pub(crate) identity: Identity,
    pub(crate) hands: [bool; 2],
    pub(crate) fire: [bool; 2],
    pub(crate) at: Instant,
}

impl Input {
    /// Whether any hand still gripping the gun holds its fire input.
    pub(crate) fn trigger(self) -> bool {
        self.hands
            .into_iter()
            .zip(self.fire)
            .any(|(held, fire)| held && fire)
    }
}

/// A sustained pull at the end of the hands' reach.
#[derive(Debug, Copy, Clone, Default, PartialEq)]
pub(super) struct Tear {
    pull_s: f32,
}

impl Tear {
    /// Advance the pull by `dt` seconds from the held body's last report. A hand that let go, a
    /// report without a finite lag or load, or a pull short of either threshold resets it.
    pub(super) fn step(&mut self, dt: f32, report: Option<HeldReport>) -> bool {
        let Some(report) = report else {
            self.pull_s = 0.0;
            return false;
        };
        if !dt.is_finite() || dt <= 0.0 || !report.lag_m.is_finite() || !report.load.is_finite() {
            self.pull_s = 0.0;
            return false;
        }
        if report.lag_m >= TEAR_LAG_M && report.load >= TEAR_LOAD {
            self.pull_s += dt;
        } else {
            self.pull_s = 0.0;
        }
        self.pull_s >= TEAR_HOLD_S
    }
}

/// How far past the hand's target the gun must sit before the pull counts, metres.
const TEAR_LAG_M: f32 = 0.3;
/// The share of the hand's strongest pull the tear needs.
const TEAR_LOAD: f32 = 0.8;
/// How long the pull must be held, seconds.
const TEAR_HOLD_S: f32 = 0.4;

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;

    fn report(lag_m: f32, load: f32) -> HeldReport {
        HeldReport {
            mass_kg: 80.0,
            liftable: true,
            shared: true,
            centre: Vec3::ZERO,
            anchor: Vec3::ZERO,
            target: Vec3::new(0.0, 0.0, lag_m),
            lag_m,
            load,
            twist: 0.0,
            joined: true,
        }
    }

    #[test]
    fn a_sustained_pull_tears_the_gun_off_after_the_hold_time() {
        let mut tear = Tear::default();
        let dt = 0.1;
        for _ in 0..3 {
            assert!(!tear.step(dt, Some(report(TEAR_LAG_M, TEAR_LOAD))));
        }
        assert!(tear.step(dt, Some(report(TEAR_LAG_M, TEAR_LOAD))));
        assert!(tear.step(dt, Some(report(TEAR_LAG_M, TEAR_LOAD))));
    }

    #[test]
    fn letting_go_or_easing_the_pull_resets_the_tear() {
        let mut tear = Tear::default();
        for _ in 0..3 {
            tear.step(0.1, Some(report(TEAR_LAG_M, TEAR_LOAD)));
        }
        assert!(!tear.step(0.1, Some(report(TEAR_LAG_M * 0.5, TEAR_LOAD))));
        for _ in 0..3 {
            assert!(!tear.step(0.1, Some(report(TEAR_LAG_M, TEAR_LOAD))));
        }
        assert!(!tear.step(0.1, None));
        for _ in 0..3 {
            assert!(!tear.step(0.1, Some(report(TEAR_LAG_M, TEAR_LOAD))));
        }
    }

    #[test]
    fn an_invalid_report_or_tick_never_tears() {
        let mut tear = Tear::default();
        for dt in [f32::NAN, f32::INFINITY, 0.0, -1.0] {
            assert!(!tear.step(dt, Some(report(TEAR_LAG_M, TEAR_LOAD))));
        }
        for report in [
            report(f32::NAN, TEAR_LOAD),
            report(TEAR_LAG_M, f32::NAN),
            report(TEAR_LAG_M, f32::INFINITY),
        ] {
            assert!(!tear.step(0.1, Some(report)));
        }
    }

    #[test]
    fn only_a_gripping_hand_can_pull_the_trigger() {
        let input = |hands, fire| Input {
            identity: Identity {
                weapon: 1,
                control: 2,
                object: 3,
                body: 4,
            },
            hands,
            fire,
            at: Instant::now(),
        };
        assert!(!input([false; 2], [true; 2]).trigger());
        assert!(input([true, false], [true, false]).trigger());
        assert!(input([true, true], [false, true]).trigger());
        assert!(!input([true, true], [false, false]).trigger());
    }
}
