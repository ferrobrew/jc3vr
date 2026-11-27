//! The presentation layer's tuning, read from the shipped `tuning/presentation.toml`
//! ([`PRESENTATION`]): the elbow pole's rule and smoothing, and the render-time re-solve's spine
//! shares, head residual, chain passes, clavicle turn, and drawn-elbow confidence.

use jc3vr_core::{
    config::{Invalid, nonnegative, positive, range},
    tuning::Tuning,
};
use serde::Deserialize;

jc3vr_core::tuning_file! {
    /// The presentation layer's tuning file.
    pub static PRESENTATION: PresentationTuning = "presentation.toml";
}

/// The presentation layer's tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresentationTuning {
    pub elbow: ElbowTuning,
    pub solve: SolveTuning,
}

impl Tuning for PresentationTuning {
    fn validate(&self) -> Result<(), Invalid> {
        let e = &self.elbow;
        nonnegative("elbow.palm_weight", e.palm_weight, 10.0)?;
        positive("elbow.smoothing_s", e.smoothing_s, 2.0)?;
        nonnegative("elbow.pole_down", e.pole_down, 10.0)?;
        nonnegative("elbow.pole_out", e.pole_out, 10.0)?;
        nonnegative("elbow.pole_back", e.pole_back, 10.0)?;
        nonnegative("elbow.pole_raise_out", e.pole_raise_out, 10.0)?;
        nonnegative("elbow.pole_across_out", e.pole_across_out, 10.0)?;
        range("elbow.raise_from_m", e.raise_from_m, -1.0, 1.0)?;
        positive("elbow.raise_span_m", e.raise_span_m, 2.0)?;
        positive("elbow.across_min_reach_m", e.across_min_reach_m, 2.0)?;
        positive("elbow.pole_fallback_below", e.pole_fallback_below, 1.0)?;
        range(
            "elbow.palm_straight_extension",
            e.palm_straight_extension,
            0.5,
            1.0,
        )?;
        positive("elbow.palm_fade_span", e.palm_fade_span, 0.5)?;
        let s = &self.solve;
        for (i, &share) in s.spine_shares.iter().enumerate() {
            range(&format!("solve.spine_shares[{i}]"), share, 0.0, 1.0)?;
        }
        nonnegative("solve.max_head_residual_m", s.max_head_residual_m, 1.0)?;
        if !(1..=32).contains(&s.chain_iterations) {
            return Err(Invalid(format!(
                "solve.chain_iterations must be in [1, 32], got {}",
                s.chain_iterations
            )));
        }
        range(
            "solve.clavicle_max_turn_deg",
            s.clavicle_max_turn_deg,
            0.0,
            90.0,
        )?;
        range("solve.min_drawn_bend_sin", s.min_drawn_bend_sin, 0.0, 1.0)?;
        range(
            "solve.full_drawn_bend_sin",
            s.full_drawn_bend_sin,
            s.min_drawn_bend_sin + f32::EPSILON,
            1.0,
        )
    }
}

/// The elbow pole ([`elbow`](crate::controllers::presentation::elbow)). The body pole's pulls are
/// weights against each other, not lengths.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElbowTuning {
    /// How hard the little-finger side of the wrist pulls the pole, against the body pole's
    /// downward pull.
    pub(crate) palm_weight: f32,
    /// The smoothing's time constant, seconds.
    pub smoothing_s: f32,
    /// The body pole's pull down.
    pub(crate) pole_down: f32,
    /// The body pole's pull out to the arm's side.
    pub(crate) pole_out: f32,
    /// The body pole's pull back.
    pub(crate) pole_back: f32,
    /// How much further out a raised hand lifts the pole.
    pub(crate) pole_raise_out: f32,
    /// How much further out a hand reaching across the body lifts the pole.
    pub(crate) pole_across_out: f32,
    /// The hand's height above the shoulder (below it, negative) at which the raise starts, metres.
    pub(crate) raise_from_m: f32,
    /// The height over which the raise ramps from none to full, metres.
    pub(crate) raise_span_m: f32,
    /// The reach across the body is measured against the arm's reach, or this, whichever is longer,
    /// metres, so a hand near the shoulder does not swing the pole by a few centimetres.
    pub(crate) across_min_reach_m: f32,
    /// The share of the pole's length its part across the reach falls below for the pole straight
    /// back to fade in, fully at none.
    pub(crate) pole_fallback_below: f32,
    /// The arm's extension, as a share of its length, at which the little finger's pull has faded
    /// out entirely: a straight arm has no elbow for the wrist to turn.
    pub(crate) palm_straight_extension: f32,
    /// The span of extension below [`palm_straight_extension`](Self::palm_straight_extension) over
    /// which the little finger's pull fades in.
    pub(crate) palm_fade_span: f32,
}

/// The render-time re-solve ([`solve`](crate::controllers::presentation::solve)).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SolveTuning {
    /// Each spine joint's share of the bend, base first: more of it higher, as the procedural spine
    /// shares its own swing.
    pub(crate) spine_shares: [f32; 4],
    /// How far the head moves off the end of the bent spine at most, metres: what the bend cannot
    /// cover in a frame is a tick of the head's travel at most, and anything more is not the head's
    /// motion.
    pub(crate) max_head_residual_m: f32,
    /// How many times the chain's swing is shared out at most; each pass leaves a small fraction of
    /// the last pass's miss.
    pub(crate) chain_iterations: usize,
    /// The furthest a clavicle turns to bring its shoulder within the arm's reach of the hand,
    /// degrees.
    pub(crate) clavicle_max_turn_deg: f32,
    /// The sine of the drawn arm's bend below which its elbow's direction is not trusted.
    pub(crate) min_drawn_bend_sin: f32,
    /// The sine of the drawn arm's bend above which its elbow's direction is trusted fully.
    pub(crate) full_drawn_bend_sin: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(PRESENTATION.get().validate(), Ok(()));
    }
}
