//! The learned body estimator's part in the yaw: where the estimate says the pelvis and the chest
//! face, turned into a target for the legs and a pull on the chest (`docs/mod/body/rig.md`, "The
//! yaw").
//!
//! The estimate's pelvis tracks the player's body with little lag, but it also moves part of the
//! way with a head-only glance and jitters a little, so it is never fed to the legs raw:
//!
//! - It is smoothed by a critically damped spring ([`EstimateTuning::smooth_s`]), which takes out
//!   the jitter and most of a quick glance.
//! - The legs hold within a deadband of the smoothed estimate ([`EstimateTuning::deadband_deg`]):
//!   when it leaves the band, the legs are carried along at its edge, so a sustained turn of the
//!   body turns them continuously, in small adjustments, while what is left of a glance stays
//!   inside it.
//! - Once the smoothed estimate has stayed still for a while, the legs settle the rest of the way
//!   onto it slowly ([`EstimateTuning::centre_deg_s`]), the weight shift of a player who has come to
//!   rest, so the band does not leave them bladed.
//!
//! The chest's target is pulled toward the estimate's chest by [`EstimateTuning::chest_weight`].
//!
//! The estimate's say is a weight between none and all ([`EstimateFollow::weight`]): it eases in
//! over [`EstimateTuning::blend_s`] while an estimate arrives and out again once none does, scaled
//! by the estimate's own confidence, and eases toward none while the stick walks the body, where
//! the head's moving turn decides where Rico faces. The caller blends the legs' change between the
//! head's rules and the estimate's by it, so nothing snaps as the estimate comes and goes.

use crate::{angle::wrap, smooth::smoothstep, yaw::smooth_damp};

/// The learned estimator's latest reading of the body, in the room frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BodyEstimate {
    /// The pelvis's heading.
    pub pelvis_yaw: f32,
    /// The upper chest's heading.
    pub chest_yaw: f32,
    /// How much the estimate counts, `0..=1`: the caller lowers it as the estimate ages.
    pub confidence: f32,
}

/// How the estimate drives the yaw.
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default)
)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct EstimateTuning {
    /// How long the estimate takes to ease in when it appears and out when it stops, seconds.
    pub blend_s: f32,
    /// The smoothing time of the critically damped spring that follows the estimate's pelvis and
    /// chest, seconds.
    pub smooth_s: f32,
    /// How far the smoothed estimate may be from the legs before it carries them, degrees.
    pub deadband_deg: f32,
    /// The fastest the estimate turns the legs, degrees per second.
    pub slew_deg_s: f32,
    /// How fast the legs settle onto a still estimate, degrees per second.
    pub centre_deg_s: f32,
    /// The smoothed estimate counts as still while it turns slower than this, degrees per second.
    pub still_deg_s: f32,
    /// How long the estimate must stay still before the settle is at full rate, seconds; it eases
    /// in over this time.
    pub still_s: f32,
    /// How far the chest's target moves from the head's (and the hands') toward the estimate's
    /// chest, as a share of the angle between them.
    pub chest_weight: f32,
    /// How long the stick's walk takes to hand the legs back to the head's moving turn, seconds,
    /// and the estimate back once it stops.
    pub stick_blend_s: f32,
    /// An estimate younger than this counts fully, seconds...
    pub fresh_s: f32,
    /// ...and one older than this not at all, with a linear fade between ([`Self::confidence`]).
    pub stale_s: f32,
}

impl EstimateTuning {
    pub const fn new() -> Self {
        Self {
            blend_s: 0.5,
            smooth_s: 0.15,
            deadband_deg: 20.0,
            slew_deg_s: 180.0,
            centre_deg_s: 20.0,
            still_deg_s: 15.0,
            still_s: 0.6,
            chest_weight: 0.4,
            stick_blend_s: 0.3,
            fresh_s: 0.15,
            stale_s: 0.5,
        }
    }

    /// How much an estimate `age_s` seconds old counts, `0..=1`: fully until [`Self::fresh_s`],
    /// not at all from [`Self::stale_s`], and linearly between. An age that is not a number counts
    /// for nothing.
    pub fn confidence(&self, age_s: f32) -> f32 {
        if age_s.is_nan() {
            return 0.0;
        }
        if age_s <= self.fresh_s {
            return 1.0;
        }
        if age_s >= self.stale_s || self.stale_s <= self.fresh_s {
            return 0.0;
        }
        1.0 - (age_s - self.fresh_s) / (self.stale_s - self.fresh_s)
    }
}

impl Default for EstimateTuning {
    fn default() -> Self {
        Self::new()
    }
}

/// The estimate's state in the yaw.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct EstimateFollow {
    /// The estimate's say before the stick and the confidence, `0..=1`.
    presence: f32,
    /// How fully the stick's walk has taken the legs back, `0..=1`.
    stick: f32,
    /// The latest estimate's confidence.
    confidence: f32,
    /// The smoothed pelvis and its rate, once an estimate has arrived.
    pelvis: Option<f32>,
    pelvis_rate: f32,
    /// The smoothed chest and its rate, once an estimate has arrived.
    chest: Option<f32>,
    chest_rate: f32,
    /// How long the smoothed pelvis has stayed still, seconds.
    still: f32,
}

/// One frame's result of the estimate's legs rule.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct EstimateStep {
    /// The legs' heading the estimate alone would give.
    pub legs: f32,
    /// The estimate's say, `0..=1`.
    pub weight: f32,
}

impl EstimateFollow {
    pub const fn new() -> Self {
        Self {
            presence: 0.0,
            stick: 0.0,
            confidence: 0.0,
            pelvis: None,
            pelvis_rate: 0.0,
            chest: None,
            chest_rate: 0.0,
            still: 0.0,
        }
    }

    /// The estimate's say over the legs and the chest, `0..=1`.
    pub fn weight(&self) -> f32 {
        (self.presence * self.confidence * (1.0 - self.stick)).clamp(0.0, 1.0)
    }

    /// The smoothed estimate of the pelvis's heading, while the estimate has a say.
    pub fn pelvis(&self) -> Option<f32> {
        self.pelvis.filter(|_| self.presence > 0.0)
    }

    /// The smoothed estimate of the chest's heading, while the estimate has a say.
    pub fn chest(&self) -> Option<f32> {
        self.chest.filter(|_| self.presence > 0.0)
    }

    /// Whether the estimate plays any part this frame: it has arrived, or is still easing out.
    pub(crate) fn active(&self, estimate: Option<&BodyEstimate>) -> bool {
        estimate.is_some() || self.presence > 0.0
    }

    /// Turn the frame the headings are measured in by `turn` radians (new frame from old).
    pub(crate) fn rotate(&mut self, turn: f32) {
        self.pelvis = self.pelvis.map(|yaw| wrap(yaw + turn));
        self.chest = self.chest.map(|yaw| wrap(yaw + turn));
    }

    /// Fold this frame's estimate in and return what it makes of the legs at `legs`. `follow` is
    /// the share of the estimate's change the head's pitch lets through, as the chest's.
    pub(crate) fn step(
        &mut self,
        estimate: Option<&BodyEstimate>,
        stick: bool,
        legs: f32,
        follow: f32,
        dt: f32,
        tuning: &EstimateTuning,
    ) -> EstimateStep {
        let blend = |dt: f32, over: f32| {
            if over.is_finite() && over > 0.0 {
                dt / over
            } else {
                1.0
            }
        };
        let present = estimate.filter(|e| e.pelvis_yaw.is_finite() && e.chest_yaw.is_finite());
        let towards = if present.is_some() { 1.0 } else { 0.0 };
        let step = blend(dt, tuning.blend_s);
        self.presence += (towards - self.presence).clamp(-step, step);
        let stick_step = blend(dt, tuning.stick_blend_s);
        self.stick += (if stick { 1.0 } else { 0.0 } - self.stick).clamp(-stick_step, stick_step);
        if let Some(estimate) = present {
            self.confidence = if estimate.confidence.is_finite() {
                estimate.confidence.clamp(0.0, 1.0)
            } else {
                0.0
            };
            self.follow_estimate(estimate, dt, tuning);
        }
        let weight = self.weight();
        if self.presence <= 0.0 {
            // Gone: the next estimate starts the smoothing afresh.
            *self = Self {
                stick: self.stick,
                ..Self::new()
            };
            return EstimateStep { legs, weight: 0.0 };
        }
        let Some(pelvis) = self.pelvis else {
            return EstimateStep { legs, weight };
        };
        let off = wrap(pelvis - legs);
        let band = tuning.deadband_deg.max(0.0).to_radians();
        let carry = off.signum() * (off.abs() - band).max(0.0);
        let settle = smoothstep(self.still / tuning.still_s.max(f32::EPSILON))
            * tuning.centre_deg_s.max(0.0).to_radians()
            * dt;
        let centre = (off - carry).clamp(-settle, settle);
        let slew = tuning.slew_deg_s.max(0.0).to_radians() * dt;
        let change = ((carry + centre) * follow.clamp(0.0, 1.0)).clamp(-slew, slew);
        EstimateStep {
            legs: wrap(legs + change),
            weight,
        }
    }

    /// Move the smoothed pelvis and chest toward `estimate`'s, and time how long the pelvis stays
    /// still.
    fn follow_estimate(&mut self, estimate: &BodyEstimate, dt: f32, tuning: &EstimateTuning) {
        let smooth = |state: Option<f32>, rate: f32, to: f32| match state {
            Some(from) => smooth_damp(from, wrap(to), rate, tuning.smooth_s, dt),
            None => (wrap(to), 0.0),
        };
        let (pelvis, pelvis_rate) = smooth(self.pelvis, self.pelvis_rate, estimate.pelvis_yaw);
        let (chest, chest_rate) = smooth(self.chest, self.chest_rate, estimate.chest_yaw);
        self.pelvis = Some(pelvis);
        self.pelvis_rate = if pelvis_rate.is_finite() {
            pelvis_rate
        } else {
            0.0
        };
        self.chest = Some(chest);
        self.chest_rate = if chest_rate.is_finite() {
            chest_rate
        } else {
            0.0
        };
        if self.pelvis_rate.abs() < tuning.still_deg_s.max(0.0).to_radians() {
            self.still += dt;
        } else {
            self.still = 0.0;
        }
    }

    /// The chest's target `wanted` pulled toward the smoothed estimate's chest by the tuning's
    /// share and the estimate's say.
    pub(crate) fn chest_target(&self, wanted: f32, tuning: &EstimateTuning) -> f32 {
        match self.chest() {
            Some(chest) => wrap(
                wanted + wrap(chest - wanted) * tuning.chest_weight.clamp(0.0, 1.0) * self.weight(),
            ),
            None => wanted,
        }
    }
}

/// How far the legs move toward a blend of the head's rules' heading `by_head` and the estimate's
/// `by_estimate` from `legs`, with the estimate's say `weight`.
pub(crate) fn blend_legs(legs: f32, by_head: f32, by_estimate: f32, weight: f32) -> f32 {
    let weight = weight.clamp(0.0, 1.0);
    wrap(legs + wrap(by_head - legs) * (1.0 - weight) + wrap(by_estimate - legs) * weight)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::{
        tuning::shipped,
        yaw::{YawFollow, YawInput, YawTuning},
    };

    const TUNING: YawTuning = YawTuning::new();
    const DT: f32 = 1.0 / 90.0;

    fn look(head_deg: f32) -> YawInput {
        YawInput {
            head_yaw: head_deg.to_radians(),
            head_pitch: 0.0,
            hands: None,
            moving: false,
            dt: DT,
        }
    }

    fn estimate(pelvis_deg: f32, chest_deg: f32) -> BodyEstimate {
        BodyEstimate {
            pelvis_yaw: pelvis_deg.to_radians(),
            chest_yaw: chest_deg.to_radians(),
            confidence: 1.0,
        }
    }

    /// Run `seconds` of frames with the head and the estimate at `at(t)`, returning the legs'
    /// heading in degrees at every frame.
    fn run(
        follow: &mut YawFollow,
        seconds: f32,
        at: impl Fn(f32) -> (f32, Option<BodyEstimate>),
    ) -> Vec<f32> {
        (0..(seconds / DT) as usize)
            .map(|i| {
                let (head, estimate) = at(i as f32 * DT);
                follow
                    .update_with(
                        &look(head),
                        estimate.as_ref(),
                        false,
                        &TUNING,
                        &shipped().yaw,
                    )
                    .to_degrees()
            })
            .collect()
    }

    /// A rig that has stood facing zero with the estimate at zero long enough to take it in fully.
    fn settled() -> YawFollow {
        let mut follow = YawFollow::new();
        follow.snap(0.0);
        run(&mut follow, 2.0, |_| (0.0, Some(estimate(0.0, 0.0))));
        assert!((follow.estimate().weight() - 1.0).abs() < 1e-6);
        follow
    }

    #[test]
    fn a_glance_moves_the_legs_less_than_the_estimate_and_they_settle_back() {
        let mut follow = settled();
        // A glance 70 degrees to the side and back over 0.6 s, which the estimate follows 60% of
        // the way, as the network does.
        let glance = |t: f32| {
            let head = 70.0 * (std::f32::consts::PI * (t / 0.6).min(1.0)).sin();
            (head, Some(estimate(0.6 * head, 0.8 * head)))
        };
        let legs = run(&mut follow, 4.0, glance);
        // The legs take well under the estimate's own share of the glance, where the head's rules
        // would have turned them all the way for a turn past the turn angle...
        let furthest = legs.iter().fold(0.0f32, |m, l| m.max(l.abs()));
        assert!(furthest < 0.5 * 70.0, "{furthest}");
        // ...and settle back under the body once it has stayed still.
        let last = legs.last().unwrap().abs();
        assert!(last < TUNING.estimate.deadband_deg * 0.5, "{last}");
    }

    #[test]
    fn a_sustained_body_turn_turns_the_legs_promptly_and_continuously() {
        let mut follow = settled();
        // The body turns a quarter in half a second and stays.
        let turn = |t: f32| {
            let body = 90.0 * (t / 0.5).min(1.0);
            (body, Some(estimate(body, body)))
        };
        let legs = run(&mut follow, 4.0, turn);
        // Under way within half a second, never faster than the slew, and onto the estimate.
        let half = (0.5 / DT) as usize;
        assert!(legs[half] > 30.0, "{}", legs[half]);
        for pair in legs.windows(2) {
            assert!(pair[1] - pair[0] <= TUNING.estimate.slew_deg_s * DT + 1e-3);
            assert!(pair[1] >= pair[0] - 1e-4, "the legs swing back: {pair:?}");
        }
        let last = *legs.last().unwrap();
        assert!((last - 90.0).abs() < 2.0, "{last}");
    }

    #[test]
    fn a_body_bladed_from_the_head_keeps_the_legs_on_the_body() {
        let mut follow = settled();
        // The head turns 50 degrees for good while the body stays: the head's rules alone would
        // turn the legs after the sustain time.
        let legs = run(&mut follow, 4.0, |_| (50.0, Some(estimate(0.0, 10.0))));
        assert!(legs.iter().all(|l| l.abs() < 1.0), "{:?}", legs.last());
        // The chest lies between the head's and the estimate's.
        let chest = follow.chest().unwrap().to_degrees();
        assert!(chest > 15.0 && chest < 45.0, "{chest}");
    }

    #[test]
    fn the_estimate_eases_in_and_out_without_a_snap() {
        let mut follow = YawFollow::new();
        follow.snap(0.0);
        // The head's rules alone hold the legs at zero with the head at 30; the estimate says 30.
        let legs = run(&mut follow, 3.0, |t| {
            (30.0, (t < 1.5).then(|| estimate(30.0, 30.0)))
        });
        for pair in legs.windows(2) {
            assert!((pair[1] - pair[0]).abs() <= TUNING.estimate.slew_deg_s * DT + 1e-3);
        }
        // Gone for longer than the blend, the estimate has no say left.
        assert_eq!(follow.estimate().weight(), 0.0);
        assert_eq!(follow.estimate().pelvis(), None);
    }

    #[test]
    fn the_say_ramps_with_presence_confidence_and_the_stick() {
        let mut follow = YawFollow::new();
        follow.snap(0.0);
        let mut half = estimate(0.0, 0.0);
        half.confidence = 0.5;
        follow.update_with(&look(0.0), Some(&half), false, &TUNING, &shipped().yaw);
        let first = follow.estimate().weight();
        assert!(first > 0.0 && first < 0.5 * DT / TUNING.estimate.blend_s + 1e-6);
        for _ in 0..200 {
            follow.update_with(&look(0.0), Some(&half), false, &TUNING, &shipped().yaw);
        }
        assert!((follow.estimate().weight() - 0.5).abs() < 1e-5);
        for _ in 0..200 {
            follow.update_with(&look(0.0), Some(&half), true, &TUNING, &shipped().yaw);
        }
        assert_eq!(follow.estimate().weight(), 0.0);
    }

    #[test]
    fn rotating_the_frame_carries_the_smoothed_estimate() {
        let mut follow = settled();
        follow.rotate(0.5);
        assert!((follow.estimate().pelvis().unwrap() - 0.5).abs() < 1e-5);
        // The estimate now arrives in the turned frame: nothing moves.
        let legs = run(&mut follow, 1.0, |_| {
            (
                0.5f32.to_degrees(),
                Some(estimate(0.5f32.to_degrees(), 0.5f32.to_degrees())),
            )
        });
        assert!(legs.iter().all(|l| (l - 0.5f32.to_degrees()).abs() < 1e-3));
    }

    #[test]
    fn confidence_fades_with_age() {
        let tuning = EstimateTuning::new();
        assert_eq!(tuning.confidence(0.0), 1.0);
        assert_eq!(tuning.confidence(tuning.fresh_s), 1.0);
        let middle = (tuning.fresh_s + tuning.stale_s) * 0.5;
        assert!((tuning.confidence(middle) - 0.5).abs() < 1e-5);
        assert_eq!(tuning.confidence(tuning.stale_s), 0.0);
        assert_eq!(tuning.confidence(f32::NAN), 0.0);
        assert_eq!(tuning.confidence(f32::INFINITY), 0.0);
    }

    proptest! {
        #[test]
        fn without_an_estimate_the_yaw_is_unchanged(
            heads in proptest::collection::vec((-180.0f32..180.0, -80.0f32..30.0, any::<bool>()), 1..300),
        ) {
            let mut plain = YawFollow::new();
            let mut with = YawFollow::new();
            for (head, pitch, moving) in heads {
                let input = YawInput {
                    head_pitch: pitch.to_radians(),
                    moving,
                    ..look(head)
                };
                let a = plain.update(&input, &TUNING);
                let b = with.update_with(&input, None, moving, &TUNING, &shipped().yaw);
                prop_assert_eq!(a.to_bits(), b.to_bits());
                prop_assert_eq!(plain.chest().map(f32::to_bits), with.chest().map(f32::to_bits));
                prop_assert_eq!(plain.turn_cause(), with.turn_cause());
            }
        }
    }
}
