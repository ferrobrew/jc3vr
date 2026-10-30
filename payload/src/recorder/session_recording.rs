//! Session recording: the video recorder and a tracking recording started and stopped as one, under
//! one stamp, so a long play session is captured whole and its two halves pair by name
//! (`videos/<stamp>/` and `recordings/<stamp>.jc3trk`). Both stamp their frames with the OpenXR
//! predicted display time, which is what lines them up afterwards.
//!
//! The take-marker action ([`mark`]) belongs with it: a marker lands in both recordings while they
//! run, and with no tracking recording running it also saves the lookback, so a moment is kept
//! whether or not a session recording was started.

use parking_lot::Mutex;

use crate::{
    director::markers,
    recorder::{
        self,
        tracking::{self, lookback},
    },
};

/// The pair's state for the debug UI.
#[derive(Debug, Clone)]
pub(crate) struct SessionStatus {
    pub(crate) video: bool,
    pub(crate) tracking: bool,
    /// The stamp of the current or last session recording.
    pub(crate) stamp: Option<String>,
    /// Why the tracking recording did not start or stopped early, if it did.
    pub(crate) tracking_error: Option<String>,
}

/// Start both recorders under a fresh stamp, or stop both when either runs. Any thread.
pub(crate) fn toggle() {
    if recorder::is_recording() || tracking::is_recording() {
        recorder::stop();
        tracking::request_stop();
        tracing::info!("recorder: session recording: stopping");
    } else {
        let stamp = crate::session::stamp();
        *STAMP.lock() = Some(stamp.clone());
        recorder::start_stamped(&stamp);
        tracking::request_start_stamped(stamp.clone());
        tracing::info!("recorder: session recording: starting as {stamp}");
    }
}

/// Drop an unlabelled take marker, and save the lookback when no tracking recording runs to carry
/// the marker. Any thread.
pub(crate) fn mark() {
    markers::mark("");
    if !tracking::is_recording() {
        lookback::request_save();
    }
}

/// The pair's state, without collecting completed writers.
pub(crate) fn status() -> SessionStatus {
    SessionStatus {
        video: recorder::is_recording(),
        tracking: tracking::is_recording(),
        stamp: STAMP.lock().clone(),
        tracking_error: tracking::peek_status().error,
    }
}

static STAMP: Mutex<Option<String>> = Mutex::new(None);
