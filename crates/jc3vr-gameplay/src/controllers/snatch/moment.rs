//! The snatch's phase changes as tracking-recording events: what the step saw on its way from one
//! phase to the next, gathered while it runs and noted once it has settled.

use tracking_format::event::{Label, Snatch};

use crate::{
    controllers::frame::Hand,
    recorder::tracking::{
        events::{self, EventKind},
        format::hand_to_file,
    },
};

/// What a step saw beyond the phases it went between.
#[derive(Debug, Default)]
pub(super) struct Moment {
    /// How the snatch ended, and why.
    outcome: Option<&'static str>,
    reason: Option<String>,
    /// The offered target's kind and distance, on arming and firing.
    target: Option<(&'static str, f32)>,
    /// The yank that fired: its distance and speed.
    yank: Option<(f32, f32)>,
}

impl Moment {
    pub(super) const fn new() -> Self {
        Self {
            outcome: None,
            reason: None,
            target: None,
            yank: None,
        }
    }

    pub(super) fn ended(&mut self, outcome: &'static str, reason: String) {
        self.outcome = Some(outcome);
        self.reason = Some(reason);
    }

    pub(super) fn target(&mut self, kind: &'static str, distance_m: f32) {
        self.target = Some((kind, distance_m));
    }

    pub(super) fn yank(&mut self, distance_m: f32, speed_mps: f32) {
        self.yank = Some((distance_m, speed_mps));
    }

    /// Note the step from the phase `from` to `to` of `hand`'s snatch, when the phase changed or
    /// the snatch ended, and start afresh.
    pub(super) fn note(&mut self, hand: Option<Hand>, from: &'static str, to: &'static str) {
        let moment = std::mem::take(self);
        if from == to && moment.outcome.is_none() {
            return;
        }
        events::note_with(|| {
            EventKind::Snatch(Box::new(Snatch {
                hand: hand.map(hand_to_file),
                from: Label::Borrowed(from),
                to: Label::Borrowed(to),
                outcome: moment.outcome.map(Label::Borrowed),
                reason: moment.reason.map(Label::Owned),
                target: moment.target.map(|(kind, _)| Label::Borrowed(kind)),
                distance_m: moment.target.map(|(_, distance)| distance),
                yank_m: moment.yank.map(|(distance, _)| distance),
                yank_mps: moment.yank.map(|(_, speed)| speed),
            }))
        });
    }
}
