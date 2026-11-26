//! Take markers: a timestamped line per hotkey press, so a take can be found inside a long
//! recording without scrubbing.
//!
//! A marker carries the wall clock, the engine's game-run time, and a label. While the video
//! recorder runs, it goes into the recording's per-frame sidecar before the next rendered frame
//! ([`crate::recorder::mark`]), so post finds the take by frame and wall clock. Otherwise it is appended to
//! `markers.jsonl` in the session folder, one JSON object per line, where the wall clock is what
//! lines it up against a host-side recording. Either way, while a tracking recording or the
//! lookback collects, the same marker is also noted as a `take_marker` event on the tracking
//! timeline, joinable to the other copy by its index and stamp.

use std::io::Write as _;

use jc3gi::clock::Clock;
use parking_lot::Mutex;
use serde::Serialize;
use tracking_format::event::{Label, TakeMarker};

use crate::{
    recorder::tracking::events::{self, EventKind},
    session::SessionFile,
};

/// One take marker.
#[derive(Serialize, Clone)]
pub struct Marker {
    /// A session-local ordinal, from 1.
    pub index: u32,
    /// The local wall-clock stamp (`session::stamp` format).
    pub stamp: String,
    /// Milliseconds since the Unix epoch, for lining up against a host recording.
    pub unix_ms: i64,
    /// The engine's game-run time in microseconds, or `None` before the clock is up.
    pub game_run_time_us: Option<u64>,
    pub label: String,
}

/// Record a marker with `label` (empty for an unlabelled take marker).
pub fn mark(label: &str) {
    let mut state = STATE.lock();
    state.count += 1;
    // SAFETY: a read of the clock singleton's counters; main-thread data read from the main thread
    // (the hotkey and the debug tab both run there).
    let game_run_time_us = unsafe { Clock::get().map(|c| c.m_GameRunTimeMicro) };
    let marker = Marker {
        index: state.count,
        stamp: crate::session::stamp(),
        unix_ms: jiff::Timestamp::now().as_millisecond(),
        game_run_time_us,
        label: label.trim().to_owned(),
    };
    tracing::info!(
        "director: take marker {} at {} ({})",
        marker.index,
        marker.stamp,
        if marker.label.is_empty() {
            "unlabelled"
        } else {
            marker.label.as_str()
        }
    );
    state.recent.push(marker.clone());
    if state.recent.len() > RECENT_SHOWN {
        state.recent.remove(0);
    }
    drop(state);
    events::note(EventKind::TakeMarker(Box::new(TakeMarker {
        index: marker.index,
        stamp: Label::Owned(marker.stamp.clone()),
        unix_ms: marker.unix_ms,
        game_run_time_us: marker.game_run_time_us,
        label: Label::Owned(marker.label.clone()),
    })));
    if let Err(marker) = crate::recorder::mark(marker) {
        write_line(&marker);
    }
}

/// The most recent markers, newest last, for the debug tab.
pub fn recent() -> Vec<Marker> {
    STATE.lock().recent.clone()
}

/// How many recent markers the debug tab lists.
pub const RECENT_SHOWN: usize = 8;

struct State {
    count: u32,
    recent: Vec<Marker>,
}

static STATE: Mutex<State> = Mutex::new(State {
    count: 0,
    recent: Vec::new(),
});

fn write_line(marker: &Marker) {
    let path = match crate::session::file(SessionFile::Markers) {
        Ok(path) => path,
        Err(e) => {
            tracing::warn!("director: markers: {e}");
            return;
        }
    };
    let line = match serde_json::to_string(marker) {
        Ok(line) => line,
        Err(e) => {
            tracing::warn!("director: could not serialize a marker: {e}");
            return;
        }
    };
    let result = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| writeln!(file, "{line}"));
    if let Err(e) = result {
        tracing::warn!(
            "director: could not append a marker to {}: {e}",
            path.display()
        );
    }
}
