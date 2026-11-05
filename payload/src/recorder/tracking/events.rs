//! The mod-side events a tracking recording carries beside its frames ([`EventKind`]): noted by
//! the subsystems that make the decisions, held here until the next captured frame, and written
//! before it, stamped on the recording's frame timeline.
//!
//! Noting is a no-op unless a recording or the lookback ([`crate::recorder::tracking::lookback`])
//! is collecting, so a producer builds its event lazily through [`note_with`]. The pending list
//! is bounded; an event beyond the bound is counted and dropped rather than growing the list when
//! no frame is captured to drain it (a session without a headset, for one).

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use parking_lot::Mutex;
use tracking_format::event::Event;
pub(crate) use tracking_format::event::EventKind;

/// Whether noted events are kept: a tracking recording runs, or the lookback does.
pub(crate) fn collecting() -> bool {
    crate::recorder::tracking::is_recording() || crate::recorder::tracking::lookback::is_running()
}

/// Note an event for the next captured frame. Any thread; takes only this module's leaf lock.
pub(crate) fn note(kind: EventKind) {
    if !collecting() {
        return;
    }
    let at = Instant::now();
    let mut pending = PENDING.lock();
    if pending.len() >= MAX_PENDING {
        DROPPED.fetch_add(1, Ordering::Relaxed);
        return;
    }
    pending.push(Noted { at, kind });
}

/// Note the event `build` makes, building it only while a recording or the lookback collects.
pub(crate) fn note_with(build: impl FnOnce() -> EventKind) {
    if collecting() {
        note(build());
    }
}

/// Events dropped because the pending list was full, since the payload loaded.
pub(crate) fn dropped() -> u64 {
    DROPPED.load(Ordering::Relaxed)
}

/// An event and the instant it was noted.
#[derive(Debug, Clone)]
pub(super) struct Noted {
    pub(super) at: Instant,
    pub(super) kind: EventKind,
}

/// Drain the pending events. Called once per captured frame, even while idle, so a recording that
/// starts cannot inherit events noted before it.
pub(super) fn take() -> Vec<Noted> {
    std::mem::take(&mut *PENDING.lock())
}

/// The events noted at or after `origin`, stamped as nanoseconds since it.
pub(super) fn stamp(noted: &[Noted], origin: Instant) -> Vec<Event> {
    noted
        .iter()
        .filter(|noted| noted.at >= origin)
        .map(|noted| Event {
            time_ns: crate::recorder::tracking::nanos_since(noted.at, origin),
            kind: noted.kind.clone(),
        })
        .collect()
}

/// How many events wait for a frame at most. A game frame notes one controller state, and each
/// simulation step its bodies, two poses, the held bodies, the injected actions, and the forces
/// the mod applied; the bound covers many frames without a capture.
const MAX_PENDING: usize = 1024;

static PENDING: Mutex<Vec<Noted>> = Mutex::new(Vec::new());
static DROPPED: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tracking_format::event::{GunRelease, Label};

    use super::*;

    fn release() -> EventKind {
        EventKind::GunRelease(Box::new(GunRelease {
            hand: tracking_format::model::Hand::Left,
            attempt: 1,
            weapon: None,
            route: Label::Borrowed("Unresolved"),
            max_speed_mps: 1.0,
            refusal: None,
            before_disarm: false,
            disarm_returned: false,
            slot_clear_returned: false,
            reconcile_returned: false,
            controller: None,
            drop: None,
        }))
    }

    #[test]
    fn stamping_drops_events_before_the_origin_and_measures_from_it() {
        let origin = Instant::now();
        let noted = [
            Noted {
                at: origin - Duration::from_nanos(5),
                kind: release(),
            },
            Noted {
                at: origin + Duration::from_nanos(7),
                kind: release(),
            },
        ];
        let events = stamp(&noted, origin);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].time_ns, 7);
    }
}
