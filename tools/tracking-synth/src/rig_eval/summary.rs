//! The evaluation's summary, accumulated frame by frame.

use body_rig::{RigOutput, StepPhase, angle};

use crate::rig_eval::{EvalSummary, HELD_LEAN_M, HELD_LEAN_S};

/// The running sums behind an [`EvalSummary`].
#[derive(Debug, Clone, Default)]
pub(crate) struct SummaryAccumulator {
    summary: EvalSummary,
    yaw_error_sum: f64,
    chest_error_sum: f64,
    offset_sum: f64,
    turn_start: Option<f32>,
    turn_sizes: Vec<f32>,
    lean_stretch: f64,
}

impl SummaryAccumulator {
    /// Count a recenter.
    pub(crate) fn recenter(&mut self) {
        self.summary.recenters += 1;
    }

    /// Fold in one evaluated frame `dt` seconds long, with the head facing `head_yaw`.
    pub(crate) fn observe(&mut self, output: &RigOutput, head_yaw: f32, dt: f32) {
        let summary = &mut self.summary;
        let yaw_error = angle::wrap(head_yaw - output.body_yaw).abs();
        summary.grounded_s += f64::from(dt);
        if output.phase.drives() {
            summary.driving_s += f64::from(dt);
        }
        match (output.turning, self.turn_start) {
            (true, None) => {
                summary.turns += 1;
                self.turn_start = Some(output.body_yaw);
            }
            (false, Some(start)) => {
                self.turn_sizes
                    .push(angle::wrap(output.body_yaw - start).abs().to_degrees());
                self.turn_start = None;
            }
            _ => {}
        }
        self.yaw_error_sum += f64::from(yaw_error.to_degrees() * dt);
        self.chest_error_sum +=
            f64::from(angle::wrap(head_yaw - output.chest_yaw).abs().to_degrees() * dt);
        let offset = output.solve.offset.length();
        self.offset_sum += f64::from(offset * dt);
        if output.phase == StepPhase::Idle
            && offset > HELD_LEAN_M
            && output.solve.crouch_m < HELD_LEAN_CROUCH_M
        {
            self.lean_stretch += f64::from(dt);
        } else {
            summary.held_lean_s += held(self.lean_stretch);
            self.lean_stretch = 0.0;
        }
        summary.max_offset_m = summary.max_offset_m.max(offset);
        summary.steps = output.steps;
    }

    pub(crate) fn finish(mut self) -> EvalSummary {
        let summary = &mut self.summary;
        if summary.grounded_s > 0.0 {
            summary.mean_yaw_error_deg = self.yaw_error_sum / summary.grounded_s;
            summary.mean_chest_error_deg = self.chest_error_sum / summary.grounded_s;
            summary.mean_offset_m = self.offset_sum / summary.grounded_s;
        }
        summary.held_lean_s += held(self.lean_stretch);
        self.turn_sizes.sort_by(f32::total_cmp);
        summary.median_turn_deg = percentile(&self.turn_sizes, 0.5);
        summary.p90_turn_deg = percentile(&self.turn_sizes, 0.9);
        self.summary
    }
}

/// A held lean has a crouch shallower than this, metres.
const HELD_LEAN_CROUCH_M: f32 = 0.05;

/// A stretch's length, if it is long enough to count as a held lean.
fn held(stretch_s: f64) -> f64 {
    if stretch_s > HELD_LEAN_S {
        stretch_s
    } else {
        0.0
    }
}

/// The value a share `p` of the sorted `values` lie below, or zero for none.
fn percentile(values: &[f32], p: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values[((values.len() - 1) as f32 * p).round() as usize]
}
