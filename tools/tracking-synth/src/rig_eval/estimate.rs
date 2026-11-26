//! The learned body estimate fed to the evaluated rig: EgoPoser run over the recording's raw poses
//! (`crate::estimate`), then handed to the rig as the game hands it, at the game's submission rate
//! and a frame or so late, so the evaluation sees the estimate the payload's rig would.
//!
//! The estimate is in the fixed room frame the network ran in (every recenter undone); the rig's
//! headings are in the cockpit frame, which each recenter turns, so a heading moves from the one to
//! the other by the recenters' turns so far.

use body_estimator::{
    EgoPoser,
    smpl::{AMASS_FROM_ROOM, JOINT_COUNT},
};
use body_rig::{BodyEstimate, EstimateTuning, math::angle::wrap};
use glam::Quat;
use tracking_format::Frame;

use crate::estimate::{self, EstimateRow, EstimateSummary};

/// How the game hands the estimate to the rig.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct EstimateCadence {
    /// How many inferences a second are submitted.
    pub submit_hz: f64,
    /// How long after its window's last pose an estimate reaches the rig, seconds: the wait for the
    /// next submission's frame and the readback's.
    pub latency_s: f64,
}

impl EstimateCadence {
    /// The payload's submission rate (its `SUBMIT_HZ`), read back on the next frame.
    pub const GAME: EstimateCadence = EstimateCadence {
        submit_hz: 30.0,
        latency_s: 0.025,
    };
}

/// The estimate's pelvis and chest headings over a recording, in the room frame, as they reach the
/// rig.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EstimateTrack {
    estimates: Vec<TimedEstimate>,
}

impl EstimateTrack {
    /// Run `model` over the whole of `frames` and keep the estimates `cadence` submits.
    pub fn compute(
        frames: &[Frame],
        model: &EgoPoser,
        cadence: EstimateCadence,
    ) -> (EstimateTrack, EstimateSummary) {
        let (Some(first), Some(last)) = (frames.first(), frames.last()) else {
            return (EstimateTrack::default(), EstimateSummary::default());
        };
        let span = (first.time_ns as f64 / 1.0e9, last.time_ns as f64 / 1.0e9);
        let (rows, summary) = estimate::estimate(frames, model, span, 0.0);
        (EstimateTrack::from_rows(&rows, cadence), summary)
    }

    /// The estimates among `rows` (oldest first) that `cadence` submits.
    pub fn from_rows(rows: &[EstimateRow], cadence: EstimateCadence) -> EstimateTrack {
        let interval = if cadence.submit_hz > 0.0 {
            1.0 / cadence.submit_hz
        } else {
            0.0
        };
        let mut next = f64::NEG_INFINITY;
        let mut estimates = Vec::new();
        for row in rows {
            // Within half a resampled frame counts as due, so 30 Hz takes every other 60 Hz row.
            if row.time_s + 0.5 / estimate::RATE_HZ < next {
                continue;
            }
            next = row.time_s.max(next) + interval;
            let joints = row.output.global_rotations().map(|rotation| {
                Quat::from_mat3(&(AMASS_FROM_ROOM.transpose() * rotation).as_mat3()).normalize()
            });
            estimates.push(TimedEstimate {
                window_end_s: row.time_s,
                arrives_s: row.time_s + cadence.latency_s.max(0.0),
                pelvis_yaw: row.output.pelvis_yaw() as f32,
                chest_yaw: row.output.chest_yaw() as f32,
                joints,
            });
        }
        EstimateTrack { estimates }
    }

    pub fn is_empty(&self) -> bool {
        self.estimates.is_empty()
    }

    /// Every joint's global rotation the FK has at `time_ns`, in the cockpit frame `turn` (the
    /// recenters' turns so far) turns the room frame into, while the latest estimate is fresh.
    pub fn joints_at(&self, time_ns: u64, turn: f32) -> Option<[Quat; JOINT_COUNT]> {
        let t = time_ns as f64 / 1.0e9;
        let index = self.estimates.partition_point(|e| e.arrives_s <= t);
        let latest = self.estimates.get(index.checked_sub(1)?)?;
        if t - latest.window_end_s > JOINTS_STALE_S {
            return None;
        }
        let cockpit = Quat::from_rotation_y(turn);
        Some(latest.joints.map(|joint| (cockpit * joint).normalize()))
    }

    /// The estimate the rig has at `time_ns` on the recording's clock, its headings turned into the
    /// cockpit frame by `turn` (the recenters' turns so far), with the confidence its age leaves
    /// it. `None` before the first estimate arrives and once the latest has gone stale.
    pub fn at(&self, time_ns: u64, turn: f32, tuning: &EstimateTuning) -> Option<BodyEstimate> {
        let t = time_ns as f64 / 1.0e9;
        let index = self.estimates.partition_point(|e| e.arrives_s <= t);
        let latest = self.estimates.get(index.checked_sub(1)?)?;
        let confidence = tuning.confidence((t - latest.window_end_s) as f32);
        (confidence > 0.0).then(|| BodyEstimate {
            pelvis_yaw: wrap(latest.pelvis_yaw + turn),
            chest_yaw: wrap(latest.chest_yaw + turn),
            confidence,
        })
    }
}

/// One estimate: when its window ended and reached the rig, seconds, its headings, and every
/// joint's global rotation in the room frame.
#[derive(Debug, Copy, Clone, PartialEq)]
struct TimedEstimate {
    window_end_s: f64,
    arrives_s: f64,
    pelvis_yaw: f32,
    chest_yaw: f32,
    joints: [Quat; JOINT_COUNT],
}

/// An estimate older than this is stale for the FK, as the payload's estimator judges it, seconds.
const JOINTS_STALE_S: f64 = 0.5;

#[cfg(test)]
mod tests {
    use body_estimator::EgoPoserOutput;

    use super::*;

    fn row(time_s: f64) -> EstimateRow {
        // Upright, facing the room's forward, with every joint at rest.
        let mut pose_body = [0.0f32; 126];
        for joint in pose_body.chunks_mut(6) {
            joint.copy_from_slice(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        }
        EstimateRow {
            time_s,
            output: EgoPoserOutput {
                root_orient: [-1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                pose_body,
                betas: None,
            },
        }
    }

    #[test]
    fn the_game_cadence_takes_every_other_row_late() {
        let rows: Vec<EstimateRow> = (0..60).map(|i| row(i as f64 / 60.0)).collect();
        let track = EstimateTrack::from_rows(&rows, EstimateCadence::GAME);
        assert_eq!(track.estimates.len(), 30);
        let tuning = EstimateTuning::new();
        // Before the first arrives there is none; just after, a fresh one.
        assert_eq!(track.at(10_000_000, 0.0, &tuning), None);
        let first = track.at(30_000_000, 0.25, &tuning).unwrap();
        assert_eq!(first.confidence, 1.0);
        assert!((first.pelvis_yaw - 0.25).abs() < 1e-5);
        assert!((wrap(first.chest_yaw - first.pelvis_yaw)).abs() < 1e-5);
        // Long after the last, it has gone stale.
        assert_eq!(track.at(5_000_000_000, 0.0, &tuning), None);
    }
}
