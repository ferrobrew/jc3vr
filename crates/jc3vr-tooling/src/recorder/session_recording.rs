//! Session recording: the video recorder and a tracking recording started and stopped as one, under
//! one stamp, so a long play session is captured whole and its two halves share one folder
//! (`recordings/<stamp>/`). Both stamp their frames with the OpenXR predicted display time, which is
//! what lines them up afterwards.
//!
//! The take-marker action ([`mark`]) belongs with it: a marker lands in both recordings while they
//! run, and with no tracking recording running it also saves the lookback, so a moment is kept
//! whether or not a session recording was started.

use std::time::{Duration, Instant};

use jc3vr_view::{recorder as tracking, recorder::lookback};
use parking_lot::Mutex;

use crate::{
    director::markers,
    recorder::{self, HostState, RECORDER_CONFIG},
    tuning::TOOLING,
};

/// The pair's state for the debug UI.
#[derive(Debug, Clone)]
pub struct SessionStatus {
    pub video: bool,
    pub tracking: bool,
    /// The stamp of the current or last session recording.
    pub stamp: Option<String>,
    /// Why the tracking recording did not start or stopped early, if it did.
    pub tracking_error: Option<String>,
    /// Why the video half is not running, if it degraded to tracking only.
    pub video_error: Option<String>,
}

/// Start both recorders under a fresh stamp, or stop both when either runs. Any thread.
pub fn toggle() {
    if recorder::is_recording() || tracking::is_recording() {
        recorder::stop();
        tracking::request_stop();
        *STARTED_AT.lock() = None;
        tracing::info!("recorder: session recording: stopping");
    } else {
        let stamp = jc3vr_core::session::stamp();
        *STAMP.lock() = Some(stamp.clone());
        *STARTED_AT.lock() = Some(Instant::now());
        *VIDEO_ERROR.lock() = None;
        recorder::start_stamped(&stamp);
        tracking::request_start_stamped(stamp.clone());
        tracing::info!("recorder: session recording: starting as {stamp}");
    }
}

/// Drop an unlabelled take marker, and save the lookback when no tracking recording runs to carry
/// the marker. Any thread.
pub fn mark() {
    markers::mark("");
    if !tracking::is_recording() {
        lookback::request_save();
    }
}

/// The pair's state, without collecting completed writers.
pub fn status() -> SessionStatus {
    poll();
    SessionStatus {
        video: recorder::is_recording(),
        tracking: tracking::is_recording(),
        stamp: STAMP.lock().clone(),
        tracking_error: tracking::peek_status().error,
        video_error: VIDEO_ERROR.lock().clone(),
    }
}

/// Degrade a session whose capture host has not connected to tracking only, and keep the video
/// error visible. Called from the render frame and from the UI.
pub(crate) fn poll() {
    let Some(started) = *STARTED_AT.lock() else {
        return;
    };
    if !recorder::is_recording() {
        return;
    }
    let status = recorder::status();
    if status.host == Some(HostState::Closed) {
        if let Some(error) = status.error {
            *VIDEO_ERROR.lock() = Some(error);
        }
        *STARTED_AT.lock() = None;
        return;
    }
    let grace = Duration::from_millis(TOOLING.get().recording.capture_host_grace_ms);
    if !should_degrade(status.host.as_ref(), started.elapsed(), grace) {
        return;
    }
    drop(status);
    let (host, port) = RECORDER_CONFIG.query(|c| (c.host().to_owned(), c.port));
    recorder::stop();
    *VIDEO_ERROR.lock() = Some(format!(
        "capture host is not running at {host}:{port}; recording tracking only"
    ));
    *STARTED_AT.lock() = None;
    tracing::warn!(
        "recorder: capture host is not running at {host}:{port}; session recording is tracking only"
    );
}

/// Whether a video take that has not connected yet should give up and leave the tracking half
/// running, once it has waited `grace`.
fn should_degrade(host: Option<&HostState>, elapsed: Duration, grace: Duration) -> bool {
    host == Some(&HostState::Waiting) && elapsed >= grace
}

static STAMP: Mutex<Option<String>> = Mutex::new(None);
static STARTED_AT: Mutex<Option<Instant>> = Mutex::new(None);
static VIDEO_ERROR: Mutex<Option<String>> = Mutex::new(None);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_waiting_host_degrades_only_after_the_grace_period() {
        let grace = Duration::from_millis(TOOLING.get().recording.capture_host_grace_ms);
        assert!(should_degrade(Some(&HostState::Waiting), grace, grace));
        assert!(!should_degrade(
            Some(&HostState::Waiting),
            grace - Duration::from_millis(1),
            grace
        ));
        for host in [HostState::Starting, HostState::Finishing, HostState::Closed] {
            assert!(!should_degrade(Some(&host), Duration::from_secs(10), grace));
        }
        assert!(!should_degrade(None, Duration::from_secs(10), grace));
    }
}
