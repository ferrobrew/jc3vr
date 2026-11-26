//! The IK replay's summary, accumulated frame by frame: the steps and their reasons, the reach,
//! the twist, and the glances.

use body_rig::{
    contact::{ContactFrame, FootPose, Release},
    math::{Side, angle::wrap},
};
use glam::{Vec2, Vec3};

use crate::rig_eval::summary::percentile;

/// What the IK replay found.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IkSummary {
    /// Seconds the IK body would have owned the standing body.
    pub standing_s: f64,
    /// Spells of the standing body.
    pub spells: u64,
    pub steps: u64,
    /// Steps per minute of the standing body.
    pub steps_per_minute: f64,
    /// The steps by the rule that released them, in [`RELEASES`]' order.
    pub reasons: [u64; 6],
    /// Hand-overs to the walk.
    pub handovers: u64,
    /// Seconds a planted foot stood out of its leg's reach even with the pelvis lowered by the
    /// most the IK body lowers it: where the drawn foot would leave its plant.
    pub reach_over_s: f64,
    /// Times a planted foot went out of that reach.
    pub reach_violations: u64,
    /// How far the pelvis would have to come down for the planted feet to stay in reach: the
    /// ninetieth percentile and the largest, metres (the IK body lowers it by at most its
    /// `max_pelvis_drop_m`).
    pub drop_p90_m: f32,
    pub max_drop_m: f32,
    /// The pelvis's twist over the planted feet while both are planted: the median, the ninetieth,
    /// and the ninety-ninth percentiles, degrees.
    pub twist_p50_deg: f32,
    pub twist_p90_deg: f32,
    pub twist_p99_deg: f32,
    /// The share of the time both feet were planted that the twist stood past the twist rule's
    /// threshold.
    pub twist_over_share: f64,
    /// Glances of the head while standing: quick turns of at least [`GLANCE_MIN_DEG`].
    pub glances: u64,
    /// Steps released during a glance or within [`GLANCE_AFTER_S`] after it.
    pub glance_steps: u64,
    pub steps_per_glance: f64,
}

/// The release rules in the order [`IkSummary::reasons`] counts them.
pub const RELEASES: [Release; 6] = [
    Release::Reach,
    Release::CentreOfMass,
    Release::RootMoved,
    Release::Twist,
    Release::Tidy,
    Release::FollowUp,
];

/// A glance starts once the head turns faster than this, degrees per second...
pub const GLANCE_START_DEG_S: f32 = 60.0;

/// ...ends once it turns slower than this...
pub const GLANCE_END_DEG_S: f32 = 20.0;

/// ...and counts once it has turned at least this far from where it started, degrees.
pub const GLANCE_MIN_DEG: f32 = 15.0;

/// A step this soon after a glance ends is the glance's, seconds: the twist's hold and the swing.
pub const GLANCE_AFTER_S: f64 = 1.0;

/// How far the hip at `hip` must come down for the ankle at `ankle` to stand within the leg's
/// reach, as the IK body's pelvis drop measures it (a little short of straight, so the knee keeps a
/// bend to solve with), metres; infinite for an ankle further across the floor than the leg.
fn needed_drop(hip: Vec3, ankle: Vec3, leg_m: f32) -> f32 {
    let reach = leg_m * DROP_REACH_SHARE;
    let offset = hip - ankle;
    let across = Vec2::new(offset.x, offset.z).length();
    if !(across.is_finite() && across < reach) {
        return f32::INFINITY;
    }
    (offset.y - (reach * reach - across * across).sqrt()).max(0.0)
}

/// The IK body's pelvis drop keeps a planted foot within this share of the leg's length.
const DROP_REACH_SHARE: f32 = 0.995;

/// A drop past this is recorded as this, for the percentiles, metres.
const MAX_RECORDED_DROP_M: f32 = 1.0;

/// The running sums behind an [`IkSummary`].
#[derive(Debug, Clone, Default)]
pub(crate) struct IkAccumulator {
    summary: IkSummary,
    twist_deg: f32,
    max_drop_m: f32,
    drops: Vec<f32>,
    twists: Vec<f32>,
    twist_over_s: f64,
    both_planted_s: f64,
    in_spell: bool,
    over_reach: bool,
    head: Option<f32>,
    glance: Option<Glance>,
    /// When the last counted glance ended.
    glance_end: Option<f64>,
}

/// A glance under way.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Glance {
    from_yaw: f32,
    furthest_deg: f32,
    standing: bool,
}

impl IkAccumulator {
    /// A summary whose twist share counts past `twist_deg` and whose reach counts past a pelvis
    /// drop of `max_drop_m`.
    pub(crate) fn new(twist_deg: f32, max_drop_m: f32) -> Self {
        Self {
            twist_deg,
            max_drop_m,
            ..Self::default()
        }
    }

    /// Fold in the head's heading `yaw` at `time_s`, `dt` seconds after the last, with `standing`
    /// whether the IK body owns the standing body.
    pub(crate) fn observe_head(&mut self, time_s: f64, yaw: Option<f32>, dt: f32, standing: bool) {
        let Some(yaw) = yaw else {
            return;
        };
        let Some(last) = self.head.replace(yaw) else {
            return;
        };
        if dt <= 0.0 {
            return;
        }
        let rate = (wrap(yaw - last) / dt).to_degrees().abs();
        match self.glance.as_mut() {
            None if rate > GLANCE_START_DEG_S => {
                self.glance = Some(Glance {
                    from_yaw: last,
                    furthest_deg: wrap(yaw - last).abs().to_degrees(),
                    standing,
                });
            }
            None => {}
            Some(glance) => {
                glance.furthest_deg = glance
                    .furthest_deg
                    .max(wrap(yaw - glance.from_yaw).abs().to_degrees());
                glance.standing &= standing;
                if rate < GLANCE_END_DEG_S {
                    if glance.standing && glance.furthest_deg >= GLANCE_MIN_DEG {
                        self.summary.glances += 1;
                        self.glance_end = Some(time_s);
                    }
                    self.glance = None;
                }
            }
        }
    }

    /// Count a step released at `time_s` by `reason`.
    pub(crate) fn step(&mut self, time_s: f64, reason: Release) {
        self.summary.steps += 1;
        if let Some(slot) = RELEASES.iter().position(|r| *r == reason) {
            self.summary.reasons[slot] += 1;
        }
        let in_glance = self.glance.is_some_and(|g| g.standing);
        let after_glance = self
            .glance_end
            .is_some_and(|end| time_s - end <= GLANCE_AFTER_S);
        if in_glance || after_glance {
            self.summary.glance_steps += 1;
        }
    }

    pub(crate) fn handover(&mut self) {
        self.summary.handovers += 1;
    }

    /// Fold in a standing frame `dt` seconds long, with the planted feet `planted`.
    pub(crate) fn observe_standing(
        &mut self,
        frame: &ContactFrame,
        planted: [Option<FootPose>; 2],
        dt: f32,
    ) {
        if !self.in_spell {
            self.in_spell = true;
            self.summary.spells += 1;
        }
        let dt = f64::from(dt);
        self.summary.standing_s += dt;
        let drop = Side::BOTH
            .into_iter()
            .filter_map(|side| {
                planted[side.index()]
                    .map(|pose| needed_drop(frame.hips[side.index()], pose.ankle, frame.leg_m))
            })
            .fold(0.0f32, f32::max);
        self.drops.push(drop.min(MAX_RECORDED_DROP_M));
        let over = drop > self.max_drop_m;
        if over {
            self.summary.reach_over_s += dt;
            if !self.over_reach {
                self.summary.reach_violations += 1;
            }
        }
        self.over_reach = over;
        if let [Some(left), Some(right)] = planted {
            let toe_out = frame.stance.toe_out_deg.to_radians();
            let a = wrap(left.yaw + toe_out * Side::Left.across());
            let b = wrap(right.yaw + toe_out * Side::Right.across());
            let feet = wrap(a + wrap(b - a) * 0.5);
            let twist = wrap(frame.pelvis_yaw - feet).abs().to_degrees();
            self.twists.push(twist);
            self.both_planted_s += dt;
            if twist > self.twist_deg {
                self.twist_over_s += dt;
            }
        }
    }

    /// The standing body's spell ended.
    pub(crate) fn end_spell(&mut self) {
        self.in_spell = false;
        self.over_reach = false;
    }

    pub(crate) fn finish(mut self) -> IkSummary {
        let s = &mut self.summary;
        s.steps_per_minute = if s.standing_s > 0.0 {
            s.steps as f64 * 60.0 / s.standing_s
        } else {
            0.0
        };
        s.steps_per_glance = if s.glances > 0 {
            s.glance_steps as f64 / s.glances as f64
        } else {
            0.0
        };
        self.drops.sort_by(f32::total_cmp);
        s.drop_p90_m = percentile(&self.drops, 0.9);
        s.max_drop_m = self.drops.last().copied().unwrap_or(0.0);
        self.twists.sort_by(f32::total_cmp);
        s.twist_p50_deg = percentile(&self.twists, 0.5);
        s.twist_p90_deg = percentile(&self.twists, 0.9);
        s.twist_p99_deg = percentile(&self.twists, 0.99);
        s.twist_over_share = if self.both_planted_s > 0.0 {
            self.twist_over_s / self.both_planted_s
        } else {
            0.0
        };
        self.summary
    }
}
