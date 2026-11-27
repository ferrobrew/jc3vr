//! Fixtures the crate's tests share.

use glam::Vec3;
use jc3vr_body::controllers::grab::{Candidate, GrabVolume, GripTarget, Mark, MarkAt, resolve};
use jc3vr_input::frame::Hand;

/// The candidate `hand`, at `point`, takes among `candidates`, by the grip model's own resolver:
/// each candidate placed as its target, and the hand's reach among them chosen.
pub(crate) fn grab_volume(point: Vec3, hand: Hand, candidates: &[Candidate]) -> Option<GrabVolume> {
    let targets: Vec<GripTarget> = candidates
        .iter()
        .map(|c| c.target(Mark::ring(MarkAt::Cockpit(c.centre), c.radius)))
        .collect();
    resolve::choose(resolve::reaches(hand, &targets, Some(point))).map(|reach| reach.volume)
}
