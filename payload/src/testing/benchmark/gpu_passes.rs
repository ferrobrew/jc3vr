//! Per-pass GPU totals for a benchmark run that asks for them (`gpu_passes` in the request).
//!
//! The profiler's GPU layer brackets each render pass a dispatch draws with a timestamp pair, and a
//! few stretches of work that are not render passes (each post effect, the mod's foveation passes)
//! with a pair of their own (`crate::profiler::gpu::pass_interval` and `detail_interval`). While a
//! run records with this switch on, every resolved interval is added to a total keyed by the
//! dispatch's lane and the interval's label, and the acknowledgement carries the totals: what each
//! pass cost over the recording, which divided by the frames is its cost per frame. The totals are
//! kept rather than every interval, since a dispatch draws about a hundred passes.
//!
//! The holes between the timed intervals are totalled too, each named by where it lies (before a
//! seam or a pass, after a seam's last pass, or after the dispatch's last seam): the clears, copies,
//! and resolves the engine records outside a pass's draw walk land there.
//!
//! A pass's interval is its GPU execution window, so idle inside it counts (see "Reading the GPU
//! lane" in `docs/mod/performance/profiler.md`). The detail intervals can nest inside a pass or
//! overlap one another's stretch of the frame only where the code they bracket does, which the
//! labels say; render passes never nest.

use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
};

use parking_lot::Mutex;
use serde::Serialize;

/// What a GPU interval timed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum GpuPassLabel {
    /// A render pass, by the engine's debug name for it.
    RenderPass(&'static str),
    /// A stretch of work that is not a render pass, by a name the payload gives it.
    Detail(&'static str),
    /// A hole between the timed intervals, by where it lies (`crate::profiler::gpu::GapAt`).
    Gap(&'static str),
}

/// Whether the GPU layer should time every pass for the recording run.
pub(crate) fn armed() -> bool {
    ARMED.load(Ordering::Relaxed)
}

/// Add one resolved dispatch's pass intervals to the run's totals, while a run records them.
pub(crate) fn record(lane: u8, intervals: impl Iterator<Item = (GpuPassLabel, i64)>) {
    if !RECORDING.load(Ordering::Relaxed) {
        return;
    }
    let mut totals = TOTALS.lock();
    let totals = totals.get_or_insert_with(HashMap::new);
    for (label, ns) in intervals {
        let total = totals.entry((lane, label)).or_default();
        total.ns += ns;
        total.intervals += 1;
    }
}

/// Arm the pass timestamps for a run's warm-up and recording, or disarm them.
pub(super) fn set_armed(armed: bool) {
    ARMED.store(armed, Ordering::Relaxed);
}

/// Start or stop adding resolved intervals to the totals. Starting clears them.
pub(super) fn set_recording(recording: bool) {
    if recording {
        *TOTALS.lock() = None;
    }
    RECORDING.store(recording, Ordering::Relaxed);
}

/// The totals the recording gathered, largest first, and clear them.
pub(super) fn take() -> Vec<GpuPassTotal> {
    let totals = TOTALS.lock().take().unwrap_or_default();
    let mut totals: Vec<GpuPassTotal> = totals
        .into_iter()
        .map(|((lane, label), total)| {
            let (kind, name) = match label {
                GpuPassLabel::RenderPass(name) => ("render_pass", name),
                GpuPassLabel::Detail(name) => ("detail", name),
                GpuPassLabel::Gap(name) => ("gap", name),
            };
            GpuPassTotal {
                lane,
                kind,
                name,
                total_us: (total.ns + 500).div_euclid(1000),
                intervals: total.intervals,
            }
        })
        .collect();
    totals.sort_by(|a, b| b.total_us.cmp(&a.total_us).then(a.name.cmp(b.name)));
    totals
}

/// One label's GPU time over a recording, on one dispatch lane.
#[derive(Debug, Serialize)]
pub(super) struct GpuPassTotal {
    /// The dispatch's eye (0 or 1) or 2 for a far-field dispatch.
    lane: u8,
    /// `render_pass`, `detail`, or `gap`.
    kind: &'static str,
    name: &'static str,
    /// The summed GPU time of the label's intervals, in microseconds.
    total_us: i64,
    /// How many intervals were summed.
    intervals: u64,
}

#[derive(Default)]
struct Total {
    ns: i64,
    intervals: u64,
}

static ARMED: AtomicBool = AtomicBool::new(false);
static RECORDING: AtomicBool = AtomicBool::new(false);
static TOTALS: Mutex<Option<HashMap<(u8, GpuPassLabel), Total>>> = Mutex::new(None);
