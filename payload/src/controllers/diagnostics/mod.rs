//! The controller subsystems' derived state as a tracking-recording event, for diagnosing tracking
//! and hand-control problems after the fact.
//!
//! The game-thread hook calls [`record`] after the engine's render update, so the late hand
//! snapshot and the frame's sub-tick fraction it is evaluated at are the ones that frame rendered
//! with. While a tracking recording or the lookback collects
//! ([`crate::recorder::tracking::events::collecting`]), each call notes one
//! [`ControllerState`](tracking_format::event::ControllerState) event, which the recorder stamps on
//! the frame timeline and writes beside the next frame. The event combines independently read
//! subsystem snapshots; it does not claim an atomic cross-subsystem instant.

mod sample;

use crate::recorder::tracking::events::{self, EventKind};

/// Note the controller subsystems' state when a recording or the lookback collects it.
pub fn record() {
    if crate::is_shutting_down() || !events::collecting() {
        return;
    }
    events::note(EventKind::ControllerState(Box::new(sample::snapshot())));
}
