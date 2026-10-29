//! Stereo draw-sequence diff: the feasibility probe for single-pass stereo (halving the per-frame
//! draw submission, the measured bottleneck -- ~20k draw calls/frame at ~41% GPU utilisation).
//!
//! Any technique that renders both eyes from one CPU walk requires the two eyes' draw sequences to
//! be identical -- same passes, same draw counts, in the same order. The mod already pins the
//! render lists, constant-buffer ring, and frame counters identical between eyes, so they *should*
//! be; this measures whether they actually are. During a traced stereo frame it records, per render
//! pass, the number of GPU ops (`Draw*` + `Dispatch`) each eye issues, then diffs the two sequences
//! and records the result into the render trace as a [`TraceEvent::StereoDrawDiff`]: which passes
//! match (replayable from one walk) and which diverge (the per-eye special-casing burden --
//! expected: the screen-space passes that read per-eye history, SSAO / GI / SSR / post). The
//! fraction of ops in matching passes is the feasibility verdict.
//!
//! Gated on `stereo.diagnose_stereo_draw_diff` and a collecting trace; it records nothing
//! otherwise. When the trace ends it logs one summary line, so the log keeps a hint of the run
//! even when the trace file is never read. Reads the live per-eye draw counters
//! ([`crate::hooks::draw_count::DRAW_COUNTS`]) around each `DoDraw`, so it adds only two atomic
//! loads per pass while recording.

use std::{
    collections::BTreeSet,
    sync::atomic::{AtomicBool, Ordering},
};

use jc3gi::graphics_engine::render_engine::GetRenderPassName_ADDRESS;
use parking_lot::Mutex;

use crate::{
    debug::trace::{TraceEvent, TraceState, tracing_active},
    hooks::draw_count::DRAW_COUNTS,
};

/// One traced stereo frame's draw-sequence diff, aligned **by pass id**, not position: eye 1
/// legitimately runs fewer passes than eye 0 when the view-independent prepasses (reflections,
/// shadow atlas, water sim) are shared -- it skips the ones eye 0 already rendered -- so a
/// positional diff mis-aligns at the first skipped pass. The per-eye scene work is the passes
/// *both* eyes run; that set's op-count agreement is the real single-pass-stereo verdict. Passes
/// only one eye runs are reported separately (eye-0-only = already-shared prepasses; eye-1-only =
/// unexpected).
#[derive(serde::Serialize, serde::Deserialize, Debug, Default, PartialEq)]
pub struct DrawDiff {
    /// Passes both eyes run.
    pub shared_passes: u32,
    /// Eye 0's GPU ops in the shared passes.
    pub shared_ops: u32,
    /// Of [`Self::shared_ops`], the ops in passes whose op counts agree between the eyes.
    pub matched_ops: u32,
    /// Passes only eye 0 runs (the shared prepasses).
    pub eye0_only_passes: u32,
    /// Eye 0's GPU ops in [`Self::eye0_only_passes`].
    pub eye0_only_ops: u32,
    /// Whether the shared passes keep the same relative order in both eyes (single-pass needs the
    /// one walk to visit them in an order valid for both).
    pub order_consistent: bool,
    /// The shared passes whose op counts differ between the eyes, in eye 0's order.
    pub divergent: Vec<DivergentPass>,
    /// The passes only eye 1 runs, which should not exist: eye 1 should be a subset of eye 0.
    pub eye1_only: Vec<NamedPass>,
}

/// A shared pass whose op counts differ between the eyes.
#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
pub struct DivergentPass {
    pub pass: i16,
    pub name: String,
    pub eye0_ops: u32,
    pub eye1_ops: u32,
}

/// A render pass by id and engine debug name.
#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
pub struct NamedPass {
    pub pass: i16,
    pub name: String,
}

impl DrawDiff {
    /// Whether the frame breaks single-pass replay anywhere: a divergent shared pass, a pass only
    /// eye 1 runs, or an inconsistent shared order.
    pub fn has_mismatch(&self) -> bool {
        !self.divergent.is_empty() || !self.eye1_only.is_empty() || !self.order_consistent
    }

    /// The percentage of shared-pass ops in passes whose counts agree (100 when no shared pass ran
    /// any op).
    pub fn replayable_pct(&self) -> f64 {
        if self.shared_ops == 0 {
            100.0
        } else {
            100.0 * f64::from(self.matched_ops) / f64::from(self.shared_ops)
        }
    }
}

/// Whether the probe would record were a stereo frame to begin now: the toggle is set and a trace
/// is collecting.
pub fn is_active() -> bool {
    tracing_active() && crate::config::Config::lock_query(|c| c.stereo.diagnose_stereo_draw_diff)
}

/// Opens a stereo frame for the probe. `drained_inline` is whether every dispatch of this frame
/// drains on the calling thread before [`finish_frame`]: a deferred tail drains the last dispatch
/// later, so its pass records would be incomplete at the diff. Clears any stale records and arms
/// recording for this frame when the probe is active; arming at the frame boundary keeps a trace
/// started mid-frame from diffing a partial sequence.
pub fn begin_frame(drained_inline: bool) {
    let mut state = STATE.lock();
    state.eye0.clear();
    state.eye1.clear();
    RECORDING.store(drained_inline && is_active(), Ordering::Relaxed);
}

/// Whether this frame's passes are being recorded (armed by [`begin_frame`]). Lock-free, for the
/// `DoDraw` hot path.
pub fn recording() -> bool {
    RECORDING.load(Ordering::Relaxed)
}

/// The running total of GPU ops issued this dispatch (draws + indexed draws + compute dispatches).
/// `DoDraw` deltas of this attribute ops to a pass. The counters are cleared per dispatch by the
/// draw driver, so the delta across one `DoDraw` is that pass's op count.
pub fn op_total() -> u32 {
    let c = &DRAW_COUNTS;
    (c.draw.load(Ordering::Relaxed)
        + c.draw_indexed.load(Ordering::Relaxed)
        + c.dispatch.load(Ordering::Relaxed)) as u32
}

/// Records one pass's op count for `eye` (`0` or `1`). Appended in draw order, so the two eyes'
/// sequences can be compared position-by-position.
pub fn record_pass(pass_id: i16, eye: usize, ops: u32) {
    let mut state = STATE.lock();
    let seq = if eye == 0 {
        &mut state.eye0
    } else {
        &mut state.eye1
    };
    seq.push((pass_id, ops));
}

/// Closes the stereo frame opened by [`begin_frame`]: diffs the two eyes' recorded sequences,
/// records the diff into the trace, and folds it into the run summary. Call after both eyes'
/// dispatches have drained and before the trace's frame tick, so the diff lands in its own frame. A
/// no-op unless the frame was armed and both eyes recorded passes.
pub fn finish_frame() {
    if !RECORDING.swap(false, Ordering::Relaxed) {
        return;
    }
    let mut state = STATE.lock();
    let eye0 = std::mem::take(&mut state.eye0);
    let eye1 = std::mem::take(&mut state.eye1);
    if eye0.is_empty() || eye1.is_empty() {
        return;
    }
    let diff = diff_sequences(&eye0, &eye1, |id| pass_name(id).to_owned());
    state.summary.add(&diff);
    drop(state);
    TraceState::record(TraceEvent::StereoDrawDiff(diff));
}

/// Resets the run summary at the start of a trace.
pub(crate) fn begin_run() {
    STATE.lock().summary = RunSummary::default();
}

/// Logs the run summary at the end of a trace, if the probe diffed any frame.
pub(crate) fn finish_run() {
    let summary = std::mem::take(&mut STATE.lock().summary);
    if summary.frames > 0 {
        tracing::info!(target: "stereo_diff", "{}", summary.line());
    }
}

/// Diffs two eyes' per-pass op sequences by pass id (see [`DrawDiff`]); `name` resolves a pass id
/// to its display name for the reported passes.
fn diff_sequences(
    eye0: &[(i16, u32)],
    eye1: &[(i16, u32)],
    name: impl Fn(i16) -> String,
) -> DrawDiff {
    let map0 = aggregate(eye0);
    let map1 = aggregate(eye1);

    let mut diff = DrawDiff {
        order_consistent: same_relative_order(eye0, eye1),
        ..DrawDiff::default()
    };
    for &(id, n0) in &map0 {
        match map1.iter().find(|&&(i, _)| i == id) {
            Some(&(_, n1)) => {
                diff.shared_passes += 1;
                diff.shared_ops += n0;
                if n0 == n1 {
                    diff.matched_ops += n0;
                } else {
                    diff.divergent.push(DivergentPass {
                        pass: id,
                        name: name(id),
                        eye0_ops: n0,
                        eye1_ops: n1,
                    });
                }
            }
            None => {
                diff.eye0_only_passes += 1;
                diff.eye0_only_ops += n0;
            }
        }
    }
    diff.eye1_only = map1
        .iter()
        .filter(|&&(id, _)| !map0.iter().any(|&(i, _)| i == id))
        .map(|&(id, _)| NamedPass {
            pass: id,
            name: name(id),
        })
        .collect();
    diff
}

/// Sums ops per pass id, preserving first-seen order.
fn aggregate(seq: &[(i16, u32)]) -> Vec<(i16, u32)> {
    let mut out: Vec<(i16, u32)> = Vec::new();
    for &(id, ops) in seq {
        match out.iter_mut().find(|(i, _)| *i == id) {
            Some((_, n)) => *n += ops,
            None => out.push((id, ops)),
        }
    }
    out
}

/// Whether the passes common to both eyes appear in the same relative order in each sequence.
fn same_relative_order(eye0: &[(i16, u32)], eye1: &[(i16, u32)]) -> bool {
    let common: Vec<i16> = eye0
        .iter()
        .map(|&(id, _)| id)
        .filter(|id| eye1.iter().any(|&(i, _)| i == *id))
        .collect();
    let eye1_common: Vec<i16> = eye1
        .iter()
        .map(|&(id, _)| id)
        .filter(|id| common.contains(id))
        .collect();
    common == eye1_common
}

/// The engine's debug name for a render-pass id, via the pass-name table. Uses a raw `i32`
/// signature because `m_Index` can hold an unnamed id that is not a valid `RenderPassId`
/// discriminant (transmuting it would be UB); the engine returns `"NONE"` for those.
fn pass_name(pass_id: i16) -> &'static str {
    // SAFETY: the lookup reads only its argument and returns a static string in the module image.
    unsafe {
        let lookup: unsafe extern "system" fn(i32) -> *const u8 =
            std::mem::transmute(GetRenderPassName_ADDRESS);
        let ptr = lookup(i32::from(pass_id));
        if ptr.is_null() {
            return "?";
        }
        std::ffi::CStr::from_ptr(ptr.cast()).to_str().unwrap_or("?")
    }
}

/// Whether the current stereo frame records its passes; see [`begin_frame`].
static RECORDING: AtomicBool = AtomicBool::new(false);

static STATE: Mutex<StereoDiff> = Mutex::new(StereoDiff::new());

struct StereoDiff {
    eye0: Vec<(i16, u32)>,
    eye1: Vec<(i16, u32)>,
    summary: RunSummary,
}

impl StereoDiff {
    const fn new() -> Self {
        Self {
            eye0: Vec::new(),
            eye1: Vec::new(),
            summary: RunSummary::new(),
        }
    }
}

/// The diffs of one trace run, condensed into the one log line the probe keeps.
#[derive(Debug, Default, PartialEq)]
struct RunSummary {
    /// Frames diffed.
    frames: u32,
    /// Frames with any mismatch ([`DrawDiff::has_mismatch`]).
    mismatched: u32,
    /// Every pass id that diverged in some frame.
    divergent_passes: BTreeSet<i16>,
    /// Frames in which some pass ran only in eye 1.
    eye1_only_frames: u32,
    /// Frames whose shared passes ran in a different order.
    order_divergent_frames: u32,
    /// The lowest per-frame replayable percentage.
    min_replayable_pct: Option<f64>,
}

impl RunSummary {
    const fn new() -> Self {
        Self {
            frames: 0,
            mismatched: 0,
            divergent_passes: BTreeSet::new(),
            eye1_only_frames: 0,
            order_divergent_frames: 0,
            min_replayable_pct: None,
        }
    }

    fn add(&mut self, diff: &DrawDiff) {
        self.frames += 1;
        self.mismatched += u32::from(diff.has_mismatch());
        self.divergent_passes
            .extend(diff.divergent.iter().map(|p| p.pass));
        self.eye1_only_frames += u32::from(!diff.eye1_only.is_empty());
        self.order_divergent_frames += u32::from(!diff.order_consistent);
        let pct = diff.replayable_pct();
        self.min_replayable_pct = Some(self.min_replayable_pct.map_or(pct, |m| m.min(pct)));
    }

    fn line(&self) -> String {
        format!(
            "stereo draw diff: {} of {} frames mismatched ({} distinct divergent passes, {} frames \
             with eye-1-only passes, {} with divergent order); min replayable {:.1}%; details in \
             the trace",
            self.mismatched,
            self.frames,
            self.divergent_passes.len(),
            self.eye1_only_frames,
            self.order_divergent_frames,
            self.min_replayable_pct.unwrap_or(100.0),
        )
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn name(id: i16) -> String {
        format!("P{id}")
    }

    #[test]
    fn identical_sequences_are_fully_replayable() {
        let seq = [(1, 10), (2, 5), (1, 3)];
        let diff = diff_sequences(&seq, &seq, name);
        assert_eq!(
            diff,
            DrawDiff {
                shared_passes: 2,
                shared_ops: 18,
                matched_ops: 18,
                order_consistent: true,
                ..DrawDiff::default()
            }
        );
        assert!(!diff.has_mismatch());
        assert_eq!(diff.replayable_pct(), 100.0);
    }

    #[test]
    fn eye0_only_prepasses_are_not_mismatches() {
        let eye0 = [(7, 40), (1, 10), (2, 5)];
        let eye1 = [(1, 10), (2, 5)];
        let diff = diff_sequences(&eye0, &eye1, name);
        assert_eq!(diff.eye0_only_passes, 1);
        assert_eq!(diff.eye0_only_ops, 40);
        assert_eq!(diff.shared_ops, 15);
        assert!(!diff.has_mismatch());
    }

    #[test]
    fn divergent_and_eye1_only_passes_are_reported() {
        let eye0 = [(1, 10), (2, 6)];
        let eye1 = [(1, 10), (2, 4), (9, 1)];
        let diff = diff_sequences(&eye0, &eye1, name);
        assert_eq!(
            diff.divergent,
            vec![DivergentPass {
                pass: 2,
                name: "P2".into(),
                eye0_ops: 6,
                eye1_ops: 4,
            }]
        );
        assert_eq!(
            diff.eye1_only,
            vec![NamedPass {
                pass: 9,
                name: "P9".into(),
            }]
        );
        assert_eq!(diff.matched_ops, 10);
        assert!(diff.has_mismatch());
        assert!((diff.replayable_pct() - 62.5).abs() < 1e-9);
    }

    #[test]
    fn reordered_shared_passes_break_order() {
        let eye0 = [(1, 1), (2, 1)];
        let eye1 = [(2, 1), (1, 1)];
        let diff = diff_sequences(&eye0, &eye1, name);
        assert!(!diff.order_consistent);
        assert!(diff.divergent.is_empty());
        assert!(diff.has_mismatch());
    }

    #[test]
    fn run_summary_counts_mismatched_frames_and_distinct_passes() {
        let clean = [(1, 4), (2, 4)];
        let skewed = [(1, 4), (2, 2)];
        let mut summary = RunSummary::default();
        summary.add(&diff_sequences(&clean, &clean, name));
        summary.add(&diff_sequences(&clean, &skewed, name));
        summary.add(&diff_sequences(&clean, &skewed, name));
        assert_eq!(summary.frames, 3);
        assert_eq!(summary.mismatched, 2);
        assert_eq!(summary.divergent_passes, BTreeSet::from([2]));
        assert_eq!(summary.min_replayable_pct, Some(50.0));
        assert!(
            summary
                .line()
                .starts_with("stereo draw diff: 2 of 3 frames mismatched")
        );
    }
}
