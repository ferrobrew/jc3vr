//! The lookback: a rolling window of the last [`WINDOW`] of capture batches, kept while motion
//! controllers are enabled, so that the moments before something went wrong can be saved as a
//! tracking recording after the fact ([`request_save`]).
//!
//! Each batch is the frame, its input records, and its events, encoded once on the capture path
//! exactly as a recording writes them, with times measured from the window's epoch (when the
//! lookback started). The window is bounded by time and, as a backstop for a runaway frame rate or
//! hand-joint frames, by bytes ([`MAX_BYTES`]). A save copies only the batches' shared handles and
//! leaves encoding nothing; the file is written on a writer thread like any recording's.
//!
//! A saved file's times therefore start where the window starts, not at zero; a replay measures
//! from its first frame. Batches evicted from the front take their input history with them, so
//! the save opens with an input discontinuity counting them.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// How far back the lookback reaches.
pub(crate) const WINDOW: Duration = Duration::from_secs(10);

/// Whether the lookback is collecting. Any thread.
pub(crate) fn is_running() -> bool {
    RUNNING.load(Ordering::Relaxed)
}

/// Ask for the window to be saved at the top of the next frame. Any thread.
pub(crate) fn request_save() {
    SAVE_REQUESTED.store(true, Ordering::Relaxed);
}

/// The lookback's state for the debug UI.
#[derive(Debug, Clone, Default)]
pub(crate) struct LookbackStatus {
    pub(crate) running: bool,
    pub(crate) frames: usize,
    pub(crate) seconds: f32,
    pub(crate) bytes: usize,
    /// The file of the last save.
    pub(crate) path: Option<std::path::PathBuf>,
    /// Why the last save failed.
    pub(crate) error: Option<String>,
}

/// The window's capture batches, oldest first.
pub(super) struct Ring {
    entries: VecDeque<Entry>,
    bytes: usize,
    /// Input records carried by evicted batches since the window started.
    evicted_inputs: u64,
    evicted: bool,
    max_bytes: usize,
}

/// One captured frame's batch.
pub(super) struct Entry {
    /// Nanoseconds since the window's epoch.
    pub(super) time_ns: u64,
    pub(super) bytes: Arc<[u8]>,
    /// The input records the batch carries, for the discontinuity a save opens with.
    pub(super) input_records: u64,
    /// The player's world matrix on the frame, for a save's start marker.
    pub(super) transform: Option<[f32; 16]>,
}

/// What a save takes from the window.
pub(super) struct Snapshot {
    pub(super) batches: Vec<Arc<[u8]>>,
    /// The oldest batch's time, for the opening discontinuity.
    pub(super) first_time_ns: u64,
    /// The player's world matrix on the oldest frame that knew it.
    pub(super) transform: Option<[f32; 16]>,
    /// Input records lost off the front, when anything was evicted.
    pub(super) evicted_inputs: Option<u64>,
}

impl Ring {
    pub(super) const fn new() -> Self {
        Self::bounded(MAX_BYTES)
    }

    const fn bounded(max_bytes: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            bytes: 0,
            evicted_inputs: 0,
            evicted: false,
            max_bytes,
        }
    }

    /// Append the newest batch and evict what has fallen out of the window or past the byte bound.
    /// The newest batch always stays.
    pub(super) fn push(&mut self, entry: Entry) {
        self.bytes += entry.bytes.len();
        let newest = entry.time_ns;
        self.entries.push_back(entry);
        let window = u64::try_from(WINDOW.as_nanos()).unwrap_or(u64::MAX);
        while self.entries.len() > 1
            && let Some(front) = self.entries.front()
            && (newest.saturating_sub(front.time_ns) > window || self.bytes > self.max_bytes)
        {
            let front = self.entries.pop_front().expect("a front entry");
            self.bytes -= front.bytes.len();
            self.evicted_inputs = self.evicted_inputs.saturating_add(front.input_records);
            self.evicted = true;
        }
    }

    pub(super) fn snapshot(&self) -> Option<Snapshot> {
        let first = self.entries.front()?;
        Some(Snapshot {
            batches: self.entries.iter().map(|e| Arc::clone(&e.bytes)).collect(),
            first_time_ns: first.time_ns,
            transform: self.entries.iter().find_map(|e| e.transform),
            evicted_inputs: self.evicted.then_some(self.evicted_inputs),
        })
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn bytes(&self) -> usize {
        self.bytes
    }

    /// The span between the oldest and newest batch.
    pub(super) fn span(&self) -> Duration {
        match (self.entries.front(), self.entries.back()) {
            (Some(front), Some(back)) => {
                Duration::from_nanos(back.time_ns.saturating_sub(front.time_ns))
            }
            _ => Duration::ZERO,
        }
    }
}

/// The lookback's live state, held in the recorder's state beside the active recording.
pub(super) struct Lookback {
    pub(super) epoch: Instant,
    pub(super) ring: Ring,
    pub(super) hand_joints: bool,
    /// The player's world matrix this frame, read by the frame tick for the next batch.
    pub(super) transform: Option<[f32; 16]>,
}

impl Lookback {
    pub(super) fn new(hand_joints: bool) -> Self {
        Self {
            epoch: Instant::now(),
            ring: Ring::new(),
            hand_joints,
            transform: None,
        }
    }
}

/// Mark whether the lookback runs, for [`is_running`].
pub(super) fn set_running(running: bool) {
    RUNNING.store(running, Ordering::Relaxed);
}

/// Take a pending save request.
pub(super) fn take_save_request() -> bool {
    SAVE_REQUESTED.swap(false, Ordering::Relaxed)
}

/// The most memory the window holds. Ten seconds of frames with hand joints at a high refresh
/// rate stay well below it; it only bounds a pathological case.
const MAX_BYTES: usize = 64 * 1024 * 1024;

static RUNNING: AtomicBool = AtomicBool::new(false);
static SAVE_REQUESTED: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    fn entry(time_ns: u64, len: usize, inputs: u64) -> Entry {
        Entry {
            time_ns,
            bytes: vec![0; len].into(),
            input_records: inputs,
            transform: time_ns.is_multiple_of(2).then_some([time_ns as f32; 16]),
        }
    }

    #[test]
    fn the_window_keeps_only_the_last_ten_seconds() {
        let mut ring = Ring::new();
        for frame in 0..=1200 {
            ring.push(entry(frame * 10 * MS, 4, 1));
        }
        // Frames 200..=1200 are within ten seconds of the newest.
        assert_eq!(ring.len(), 1001);
        assert_eq!(ring.span(), WINDOW);
        assert_eq!(ring.bytes(), 1001 * 4);
        let snapshot = ring.snapshot().unwrap();
        assert_eq!(snapshot.first_time_ns, 200 * 10 * MS);
        assert_eq!(snapshot.batches.len(), 1001);
        assert_eq!(snapshot.evicted_inputs, Some(200));
        assert_eq!(snapshot.transform, Some([(2000 * MS) as f32; 16]));
    }

    #[test]
    fn a_fresh_window_reports_no_eviction_and_an_empty_one_no_snapshot() {
        let mut ring = Ring::new();
        assert!(ring.snapshot().is_none());
        ring.push(entry(1, 4, 3));
        ring.push(entry(3 * MS + 1, 4, 3));
        let snapshot = ring.snapshot().unwrap();
        assert_eq!(snapshot.evicted_inputs, None);
        assert_eq!(snapshot.transform, None);
        assert_eq!(snapshot.first_time_ns, 1);
    }

    #[test]
    fn the_byte_bound_evicts_but_keeps_the_newest_batch() {
        let mut ring = Ring::bounded(100);
        ring.push(entry(0, 50, 0));
        ring.push(entry(MS, 50, 0));
        assert_eq!(ring.len(), 2);
        ring.push(entry(2 * MS, 50, 0));
        assert_eq!(ring.len(), 2);
        assert!(ring.bytes() <= 100);
        ring.push(entry(3 * MS, 101, 0));
        assert_eq!(ring.len(), 1);
        assert_eq!(ring.snapshot().unwrap().first_time_ns, 3 * MS);
    }
}
