//! The draw-path detours' own cost, for a benchmark run that asks for it.
//!
//! Each draw entry detour opens a [`DrawDetourScope`] and each forwarding call into the runtime goes
//! through [`forward`]. While armed, the outermost scope on a thread adds its whole length to the
//! detour total and [`forward`] adds the forwarding call's length to the forward total, so the
//! difference is what the detours spend around the draws: the mod's own bookkeeping and the state
//! changes it makes for a draw (the viewport splits, the pinned constants, the bound variants). The
//! clock reads count against that difference; [`clock_pair_ns`] measures what a pair costs so a
//! reader can take it out. Disarmed, a scope and a forward each cost one relaxed atomic load.

use std::{
    cell::Cell,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Instant,
};

/// Open a draw detour's scope. Hold it for the detour's whole body.
#[must_use = "the detour is timed only while the scope is held"]
pub(crate) fn enter() -> DrawDetourScope {
    if !ARMED.load(Ordering::Relaxed) {
        return DrawDetourScope(None);
    }
    let outermost = DEPTH.with(|depth| {
        let outer = depth.get();
        depth.set(outer + 1);
        outer == 0
    });
    DrawDetourScope(Some(outermost.then(Instant::now)))
}

/// A draw detour's open scope: `None` while disarmed, `Some(None)` when nested inside another.
pub(crate) struct DrawDetourScope(Option<Option<Instant>>);

impl Drop for DrawDetourScope {
    fn drop(&mut self) {
        let Some(start) = self.0 else {
            return;
        };
        DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
        if let Some(start) = start {
            DETOUR_NS.fetch_add(elapsed_ns(start), Ordering::Relaxed);
            DETOUR_CALLS.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Run a forwarding call into the runtime, timing it while armed.
pub fn forward(call: impl FnOnce()) {
    if !ARMED.load(Ordering::Relaxed) {
        call();
        return;
    }
    let start = Instant::now();
    call();
    FORWARD_NS.fetch_add(elapsed_ns(start), Ordering::Relaxed);
    FORWARD_CALLS.fetch_add(1, Ordering::Relaxed);
}

/// The totals since the payload loaded, never reset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DetourTotals {
    pub detour_ns: u64,
    pub detour_calls: u64,
    pub forward_ns: u64,
    pub forward_calls: u64,
}

pub fn totals() -> DetourTotals {
    DetourTotals {
        detour_ns: DETOUR_NS.load(Ordering::Relaxed),
        detour_calls: DETOUR_CALLS.load(Ordering::Relaxed),
        forward_ns: FORWARD_NS.load(Ordering::Relaxed),
        forward_calls: FORWARD_CALLS.load(Ordering::Relaxed),
    }
}

pub fn set_armed(armed: bool) {
    ARMED.store(armed, Ordering::Relaxed);
}

/// The mean cost of two back-to-back clock reads and the subtraction between them, in nanoseconds,
/// over `samples` pairs.
pub fn clock_pair_ns(samples: u32) -> f64 {
    let start = Instant::now();
    let mut sink = 0u64;
    for _ in 0..samples {
        let a = Instant::now();
        sink = sink.wrapping_add(elapsed_ns(a));
    }
    std::hint::black_box(sink);
    elapsed_ns(start) as f64 / f64::from(samples.max(1))
}

static ARMED: AtomicBool = AtomicBool::new(false);
static DETOUR_NS: AtomicU64 = AtomicU64::new(0);
static DETOUR_CALLS: AtomicU64 = AtomicU64::new(0);
static FORWARD_NS: AtomicU64 = AtomicU64::new(0);
static FORWARD_CALLS: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// How many armed draw detour scopes are open on this thread.
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

fn elapsed_ns(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_scopes_count_once_and_forwards_are_timed() {
        // The only test that arms the timing, so it has the totals to itself.
        std::thread::spawn(|| {
            let before = totals();
            set_armed(true);
            {
                let _outer = enter();
                let _inner = enter();
                forward(|| std::thread::sleep(std::time::Duration::from_millis(2)));
            }
            set_armed(false);
            let _disarmed = enter();
            forward(|| ());
            let after = totals();
            assert_eq!(after.detour_calls - before.detour_calls, 1);
            assert_eq!(after.forward_calls - before.forward_calls, 1);
            let forward_ns = after.forward_ns - before.forward_ns;
            assert!(forward_ns >= 2_000_000, "{forward_ns}");
            assert!(after.detour_ns - before.detour_ns >= forward_ns);
        })
        .join()
        .unwrap();
    }
}
