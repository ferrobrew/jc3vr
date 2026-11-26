//! The body's animation state as a tracking-recording event: the body rule system's current state
//! and the procedural body's hand-over case, noted when either changes.
//!
//! A note also repeats, unchanged, once the tuning's
//! [`repeat_after_s`](crate::tuning::BodyStateTuning::repeat_after_s) has passed since the last one, so that a
//! stretch of a recording that starts between changes (a lookback save, or a recording started
//! while the lookback ran) still opens with the state it started in.

use std::time::{Duration, Instant};

use jc3vr_body::controllers::body::procedural::authority::Case;
use jc3vr_view::recorder::tracking::{events, events::EventKind};
use parking_lot::Mutex;
use tracking_format::event::{BodyState, Label};

use crate::{diagnostics::state_names, tuning::TOOLING};

/// Note the body's state when it changed since the last note, or when the last note is old.
pub(super) fn note(rule_state: Option<u32>, case: Case, now: Instant) {
    let repeat_after = Duration::from_secs_f32(TOOLING.get().body_state.repeat_after_s);
    let Some(repeat) = TRACKER
        .lock()
        .observe((rule_state, case), now, repeat_after)
    else {
        return;
    };
    events::note(EventKind::BodyState(Box::new(BodyState {
        rule_state,
        rule_state_name: rule_state.and_then(state_names::name).map(Label::Borrowed),
        case: Label::Borrowed(case.label()),
        repeat,
    })));
}

/// Forget the last note, so that the next collection starts with one.
pub(super) fn forget() {
    *TRACKER.lock() = Tracker::new();
}

static TRACKER: Mutex<Tracker> = Mutex::new(Tracker::new());

type Observed = (Option<u32>, Case);

/// The last noted state and when it was noted.
#[derive(Debug)]
struct Tracker {
    last: Option<(Observed, Instant)>,
}

impl Tracker {
    const fn new() -> Self {
        Self { last: None }
    }

    /// Whether `state` is to be noted at `now`: `Some(false)` for a change (or the first
    /// observation), `Some(true)` for a repeat of a state unchanged for `repeat_after`, and `None`
    /// otherwise.
    fn observe(&mut self, state: Observed, now: Instant, repeat_after: Duration) -> Option<bool> {
        let repeat = match self.last {
            None => false,
            Some((last, _)) if last != state => false,
            Some((_, at)) if now.saturating_duration_since(at) >= repeat_after => true,
            Some(_) => return None,
        };
        self.last = Some((state, now));
        Some(repeat)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPEAT: Duration = Duration::from_secs(5);

    #[test]
    fn a_change_notes_at_once_and_an_unchanged_state_repeats_after_the_interval() {
        let start = Instant::now();
        let mut tracker = Tracker::new();
        let idle = (Some(1), Case::OnFoot);
        assert_eq!(tracker.observe(idle, start, REPEAT), Some(false));
        assert_eq!(
            tracker.observe(idle, start + Duration::from_secs(1), REPEAT),
            None
        );
        let jump = (Some(2), Case::OnFoot);
        assert_eq!(
            tracker.observe(jump, start + Duration::from_secs(2), REPEAT),
            Some(false)
        );
        let case = (Some(2), Case::Jump);
        assert_eq!(
            tracker.observe(case, start + Duration::from_secs(2), REPEAT),
            Some(false)
        );
        assert_eq!(
            tracker.observe(case, start + Duration::from_secs(6), REPEAT),
            None
        );
        assert_eq!(
            tracker.observe(case, start + Duration::from_secs(7), REPEAT),
            Some(true)
        );
        assert_eq!(
            tracker.observe(case, start + Duration::from_secs(8), REPEAT),
            None
        );
        let lost = (None, Case::Jump);
        assert_eq!(
            tracker.observe(lost, start + Duration::from_secs(8), REPEAT),
            Some(false)
        );
    }
}
