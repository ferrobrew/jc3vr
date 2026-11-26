//! The capture tooling's arming flags, which keep the render hooks' probe tables off the shipped
//! path.
//!
//! The single-pass draw path ([`crate::stereo::single_pass::probes`]) and the graphics-engine
//! pipeline hooks ([`crate::hooks::graphics_engine::probes`]) call into tables the capture side
//! registers at install. Each probe is inert until the capture side receives a request, so the hooks
//! read these flags with one relaxed load and make no call into a table while nothing has asked for
//! one. A flag rises at the first request of its kind and stays up for the session: a probe that
//! brackets work (a guard taken before a submission and dropped after it, or a scope's two halves)
//! then never sees its flag fall between the two.
//!
//! A benchmark arms only the probes it reads, so a run measures the draw path a capture-free session
//! takes.

use std::sync::atomic::{AtomicU8, Ordering};

/// What a request arms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Arming {
    /// A screenshot, snapshot, or visual-control request: every probe.
    Capture,
    /// A benchmark request: the probes the benchmark reads (the draw-task scope and the render-block
    /// type switch).
    Benchmark,
}

/// Raise `arming`'s flag. It stays raised.
pub(crate) fn arm(arming: Arming) {
    let bit = match arming {
        Arming::Capture => CAPTURE,
        Arming::Benchmark => BENCHMARK,
    };
    ARMED.fetch_or(bit, Ordering::Relaxed);
}

/// Whether a capture request has armed every probe.
#[inline]
pub(crate) fn capture_armed() -> bool {
    ARMED.load(Ordering::Relaxed) & CAPTURE != 0
}

/// Whether a capture or a benchmark request has armed the probes the benchmark reads.
#[inline]
pub(crate) fn capture_or_benchmark_armed() -> bool {
    ARMED.load(Ordering::Relaxed) != 0
}

const CAPTURE: u8 = 1 << 0;
const BENCHMARK: u8 = 1 << 1;

/// The raised flags.
static ARMED: AtomicU8 = AtomicU8::new(0);
