//! What the GPU timestamp lane ([`crate::profiler::gpu`]) reports of each resolved dispatch to a
//! consumer that records runs, and the switch that consumer arms the lane with.
//!
//! The consumer (the capture tooling's benchmark) registers a `GpuDispatchSink` at startup and arms
//! the lane with [`arm`] for a run; the lane, which exists only with the `profiler` feature, then
//! issues its queries while armed and hands each resolved dispatch to the sink. The sample and the
//! seam names are compiled in every build, so the consumer's records keep their shape without the
//! feature.

#[cfg(feature = "profiler")]
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// The GPU seams each dispatch record carries a time for, in the order of
/// `crate::profiler::gpu::GpuSeam::ALL`.
pub(crate) const GPU_SEAM_NAMES: [&str; 7] = [
    "pre_draw",
    "gbuffer",
    "scene",
    "post_effects",
    "post_draw",
    "fsr",
    "mod_post_draw",
];

/// One resolved GPU dispatch, from the profiler's timestamp read-back.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GpuDispatchSample {
    /// The dispatch's eye (0 or 1) or 2 for a far-field dispatch.
    pub(crate) lane: u8,
    /// The GPU's execution window for the dispatch, idle inside it included.
    pub(crate) span_ns: i64,
    /// The GPU's idle gap since the previous dispatch ended, when measurable.
    pub(crate) idle_before_ns: Option<i64>,
    /// The wall time the draw thread spent recording the dispatch.
    pub(crate) submit_ns: i64,
    /// Each seam's GPU time, by [`GPU_SEAM_NAMES`], when it ran.
    pub(crate) seams_ns: [Option<i64>; GPU_SEAM_NAMES.len()],
}

/// Where the lane hands its resolved dispatches while armed.
#[cfg(feature = "profiler")]
#[derive(Clone, Copy)]
pub(crate) struct GpuDispatchSink {
    /// Files one resolved dispatch.
    pub(crate) record: fn(GpuDispatchSample),
    /// Counts a dispatch whose timestamps were dropped unread.
    pub(crate) dropped: fn(),
}

/// Registers the dispatch sink. Once, at startup; a second registration is refused with an error
/// log.
#[cfg(feature = "profiler")]
pub(crate) fn register_dispatch_sink(sink: GpuDispatchSink) {
    if SINK.set(sink).is_err() {
        tracing::error!("gpu samples: the dispatch sink is already registered; ignoring another");
    }
}

/// Arms or disarms the lane's dispatch timing for a recording run.
pub(crate) fn arm(armed: bool) {
    ARMED.store(armed, Ordering::Relaxed);
}

/// Whether the lane should bracket dispatches for a recording run.
#[cfg(feature = "profiler")]
pub(crate) fn armed() -> bool {
    ARMED.load(Ordering::Relaxed)
}

/// Hands one resolved dispatch to the sink, if one is registered.
#[cfg(feature = "profiler")]
pub(crate) fn record(sample: GpuDispatchSample) {
    if let Some(sink) = SINK.get() {
        (sink.record)(sample);
    }
}

/// Tells the sink, if one is registered, that a dispatch's timestamps were dropped unread.
#[cfg(feature = "profiler")]
pub(crate) fn note_dropped() {
    if let Some(sink) = SINK.get() {
        (sink.dropped)();
    }
}

static ARMED: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "profiler")]
static SINK: OnceLock<GpuDispatchSink> = OnceLock::new();
