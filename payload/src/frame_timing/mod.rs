//! Always-on frame timing: where the game thread's frame goes, split between the mod's hooks, the
//! engine, the OpenXR runtime's calls, and waits, at a cost low enough to leave on in every build.
//!
//! The profiler (`crate::profiler`, behind the `profiler` feature) answers the same question in
//! detail but only while it collects, and a capture is something a player has to think to take. This
//! is the coarse version that is always running: a fixed set of [`Slot`]s, each a region of the frame
//! timed with a [`scope`], accumulated in static atomic counters, and reported every
//! [`LOG_WINDOW`] as one `perf: frame breakdown` log line, next to the VR frame-loop health line.
//! The debug UI's Performance tab shows the same breakdown over the last [`UI_WINDOW`] from
//! [`latest`].
//!
//! Scopes nest, and each records its *self* time: a scope's time minus the time of the scopes that
//! closed inside it on the same thread. So the engine's `Game::Update` scope measures the engine's own
//! work with the mod's hooks inside it taken out, and the self times on one thread add up to the
//! thread's covered wall time without double counting. Each self time is also added to a bucket by
//! the thread it ran on ([`ThreadClass`], from [`crate::thread_role`]) and by the slot's [`Kind`];
//! the game thread's buckets against the frame interval are the headline numbers. Whatever the game
//! thread spends outside every scope is reported as untracked.
//!
//! The cost is two clock reads, a few thread-local accesses, and four relaxed atomic adds per scope,
//! with a few dozen scopes a frame; the clock is `QueryPerformanceCounter` through [`Instant`]. Only
//! the game thread's once-a-frame [`frame_tick`] touches a lock, and only once a second.
//!
//! Threading: [`scope`] may be used on any thread. [`frame_tick`] must be called once per frame from
//! the game thread, and [`latest`] is read from the game thread's debug UI. [`frame_percentiles`],
//! the spread of the recent frame intervals, may be read from any thread.

mod recent;
mod report;
mod slot;

use std::{
    cell::{Cell, RefCell},
    marker::PhantomData,
    sync::{
        OnceLock,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use parking_lot::Mutex;

pub(crate) use crate::frame_timing::{
    recent::FramePercentiles,
    report::{Report, TickLoad, Totals, ns_to_ms},
    slot::{Kind, Slot, ThreadClass},
};
use crate::{
    frame_timing::{recent::RecentFrames, report::SlotTotals},
    thread_role::{self, ThreadRole},
};

/// How often the breakdown is logged.
pub(crate) const LOG_WINDOW: Duration = Duration::from_secs(10);

/// How often the breakdown the debug UI shows is refreshed.
pub(crate) const UI_WINDOW: Duration = Duration::from_secs(1);

/// Time `slot` until the returned scope drops. Scopes on one thread must drop in the reverse order
/// they were opened, which holding them as locals guarantees.
#[must_use = "the region is timed only while the scope is held"]
pub(crate) fn scope(slot: Slot) -> Scope {
    let outer_child_ns = CHILD_NS.with(|c| c.replace(0));
    Scope {
        slot,
        start: Instant::now(),
        outer_child_ns,
        _thread_bound: PhantomData,
    }
}

/// A timed region opened by [`scope`]. Bound to its thread, since its nesting is per thread.
pub(crate) struct Scope {
    slot: Slot,
    start: Instant,
    /// The enclosing scope's child time so far, restored (plus this scope's time) on drop.
    outer_child_ns: u64,
    _thread_bound: PhantomData<*const ()>,
}

impl Drop for Scope {
    fn drop(&mut self) {
        let elapsed = duration_ns(self.start.elapsed());
        let children = CHILD_NS.with(|c| c.replace(self.outer_child_ns.saturating_add(elapsed)));
        let self_ns = elapsed.saturating_sub(children);

        let counters = &SLOTS[self.slot.index()];
        counters.self_ns.fetch_add(self_ns, Ordering::Relaxed);
        counters.calls.fetch_add(1, Ordering::Relaxed);
        counters.max_ns.fetch_max(elapsed, Ordering::Relaxed);
        BUCKETS[thread_class().index()][self.slot.kind().index()]
            .fetch_add(self_ns, Ordering::Relaxed);
    }
}

/// Count a sim tick against the current game frame, so the breakdown can split the frames by how
/// many ticks they ran. Call at the start of each `Game::UpdateGame`, on the game thread.
pub(crate) fn note_sim_tick() {
    TICKS_THIS_FRAME.fetch_add(1, Ordering::Relaxed);
}

/// Mark the start of a game frame: records the interval since the previous call, and closes the
/// reporting windows that have run their length, publishing the debug UI's breakdown and logging
/// the long one. Call once per frame from the game thread, at the top of `Game::Update`.
pub(crate) fn frame_tick() {
    let now = duration_ns(epoch().elapsed());
    // A stall averages into its ten-second window as a frame time a few hundred milliseconds long,
    // which hides that it was one frame, so a frame past the threshold is also logged on its own;
    // the line's timestamp is when the stalled frame ended.
    let interval = LAST_TICK_NS
        .with(|last| last.replace(Some(now)))
        .map(|last| now.saturating_sub(last));
    if let Some(interval) = interval.filter(|&ns| ns >= STALL_LOG_NS) {
        tracing::warn!(
            target: "perf",
            frame_s = ns_to_ms(interval) / 1000.0,
            "perf: a game frame stalled",
        );
    }
    // The ticks counted since the last call belong to the frame that ends here.
    let ticks = TICKS_THIS_FRAME.swap(0, Ordering::Relaxed);
    let (ui, log) = WINDOWS.with_borrow_mut(|windows| windows.tick(now, ticks, drain));
    let percentiles = RECENT.with_borrow_mut(|recent| {
        if let Some(interval) = interval {
            recent.push(interval);
        }
        ui.is_some().then(|| recent.percentiles()).flatten()
    });
    if let Some(percentiles) = percentiles {
        *PERCENTILES.lock() = Some(percentiles);
    }
    if let Some(ui) = ui {
        *LATEST.lock() = Some(ui);
    }
    if let Some(log) = log {
        tracing::info!(target: "perf", "{log}");
    }
}

/// The breakdown over the last complete [`UI_WINDOW`], or `None` before the first one closes.
pub(crate) fn latest() -> Option<Report> {
    LATEST.lock().clone()
}

/// The median and 95th percentile of the recent game frame intervals, refreshed every
/// [`UI_WINDOW`], or `None` before the first window closes. A leaf lock, so any thread may read it.
pub(crate) fn frame_percentiles() -> Option<FramePercentiles> {
    *PERCENTILES.lock()
}

/// One slot's live counters.
struct SlotCounters {
    self_ns: AtomicU64,
    calls: AtomicU64,
    max_ns: AtomicU64,
}

impl SlotCounters {
    const fn new() -> Self {
        Self {
            self_ns: AtomicU64::new(0),
            calls: AtomicU64::new(0),
            max_ns: AtomicU64::new(0),
        }
    }
}

static SLOTS: [SlotCounters; Slot::COUNT] = [const { SlotCounters::new() }; Slot::COUNT];

static BUCKETS: [[AtomicU64; Kind::COUNT]; ThreadClass::COUNT] =
    [const { [const { AtomicU64::new(0) }; Kind::COUNT] }; ThreadClass::COUNT];

/// The sim ticks the current game frame has run, counted by [`note_sim_tick`] and taken by
/// [`frame_tick`].
static TICKS_THIS_FRAME: AtomicU32 = AtomicU32::new(0);

/// The frame interval past which [`frame_tick`] logs the frame on its own.
const STALL_LOG_NS: u64 = 1_000_000_000;

thread_local! {
    /// When the game thread last ticked, on the frame clock.
    static LAST_TICK_NS: Cell<Option<u64>> = const { Cell::new(None) };
    /// The time of the scopes that have closed inside the innermost open scope on this thread.
    static CHILD_NS: Cell<u64> = const { Cell::new(0) };
    /// The reporting windows. Only the game thread ticks them.
    static WINDOWS: RefCell<Windows> = const { RefCell::new(Windows::new()) };
    /// The recent frame intervals. Only the game thread records them.
    static RECENT: RefCell<RecentFrames> = const { RefCell::new(RecentFrames::new()) };
}

/// The percentiles [`frame_percentiles`] returns.
static PERCENTILES: Mutex<Option<FramePercentiles>> = Mutex::new(None);

/// The breakdown over the last complete [`UI_WINDOW`].
static LATEST: Mutex<Option<Report>> = Mutex::new(None);

fn duration_ns(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

/// The calling thread's class, from its role.
fn thread_class() -> ThreadClass {
    match thread_role::current() {
        ThreadRole::Game | ThreadRole::GameFrame | ThreadRole::GameDebugUi => ThreadClass::Game,
        ThreadRole::FrameTail => ThreadClass::Tail,
        ThreadRole::Unmarked => ThreadClass::Other,
    }
}

/// The instant the frame clock counts from.
fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// Take the live counters' totals and reset them. A scope closing on another thread while this
/// runs lands in this window or the next; nothing is lost.
fn drain() -> Totals {
    let mut totals = Totals::default();
    for (slot, counters) in totals.slots.iter_mut().zip(&SLOTS) {
        *slot = SlotTotals {
            self_ns: counters.self_ns.swap(0, Ordering::Relaxed),
            calls: counters.calls.swap(0, Ordering::Relaxed),
            max_ns: counters.max_ns.swap(0, Ordering::Relaxed),
        };
    }
    for (bucket, live) in totals.buckets.iter_mut().zip(&BUCKETS) {
        for (ns, live) in bucket.iter_mut().zip(live) {
            *ns = live.swap(0, Ordering::Relaxed);
        }
    }
    totals
}

/// The frame intervals and the two reporting windows, advanced once per frame. Times are
/// nanoseconds on the frame clock.
struct Windows {
    last_frame_ns: Option<u64>,
    /// The frame intervals since the UI window opened; the scope totals are drained into it when
    /// it closes.
    pending: Totals,
    ui_start_ns: Option<u64>,
    log: Totals,
    log_start_ns: Option<u64>,
}

impl Windows {
    const fn new() -> Self {
        Self {
            last_frame_ns: None,
            pending: Totals::EMPTY,
            ui_start_ns: None,
            log: Totals::EMPTY,
            log_start_ns: None,
        }
    }

    /// Record a frame starting at `now`, the frame before it having run `ticks` sim ticks, and
    /// return the UI window's report and the log window's report for any window that closed.
    /// `drain` takes the scope totals since the last drain.
    fn tick(
        &mut self,
        now: u64,
        ticks: u32,
        drain: impl FnOnce() -> Totals,
    ) -> (Option<Report>, Option<Report>) {
        if let Some(last) = self.last_frame_ns.replace(now) {
            self.pending.add_frame(now.saturating_sub(last), ticks);
        }
        let ui_start = *self.ui_start_ns.get_or_insert(now);
        let log_start = *self.log_start_ns.get_or_insert(now);
        if now.saturating_sub(ui_start) < duration_ns(UI_WINDOW) {
            return (None, None);
        }

        let mut window = std::mem::take(&mut self.pending);
        window.absorb(&drain());
        self.log.absorb(&window);
        self.ui_start_ns = Some(now);
        let ui = Report {
            window_s: (now - ui_start) as f64 / 1e9,
            totals: window,
        };

        if now.saturating_sub(log_start) < duration_ns(LOG_WINDOW) {
            return (Some(ui), None);
        }
        self.log_start_ns = Some(now);
        let log = Report {
            window_s: (now - log_start) as f64 / 1e9,
            totals: std::mem::take(&mut self.log),
        };
        (Some(ui), Some(log))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    fn drained_with(slot: Slot, self_ns: u64) -> Totals {
        let mut t = Totals::default();
        t.slots[slot.index()] = SlotTotals {
            self_ns,
            calls: 1,
            max_ns: self_ns,
        };
        t
    }

    #[test]
    fn the_first_frame_has_no_interval() {
        let mut w = Windows::new();
        assert_eq!(
            w.tick(5 * MS, 0, || panic!("not drained yet")),
            (None, None)
        );
        assert_eq!(w.pending.frames, 0);
        w.tick(19 * MS, 1, || panic!("not drained yet"));
        assert_eq!(w.pending.frames, 1);
        assert_eq!(w.pending.frame_ns, 14 * MS);
        // The ticks the first frame ran belong to the interval that ends with the second.
        assert_eq!(w.pending.sim_ticks, 1);
        assert_eq!(w.pending.tick_load(TickLoad::One).frames, 1);
    }

    #[test]
    fn the_ui_window_closes_each_second_with_the_drained_scopes() {
        let mut w = Windows::new();
        let mut now = 0;
        let mut ui_reports = Vec::new();
        // 14 ms frames for a little over two seconds.
        while now <= 2_100 * MS {
            let (ui, log) = w.tick(now, 0, || drained_with(Slot::DebugUi, MS));
            assert!(log.is_none());
            ui_reports.extend(ui);
            now += 14 * MS;
        }
        assert_eq!(ui_reports.len(), 2);
        let first = &ui_reports[0];
        assert!((first.window_s - 1.008).abs() < 1e-9, "{}", first.window_s);
        assert_eq!(first.totals.frames, 72);
        assert_eq!(first.totals.frame_ns, 72 * 14 * MS);
        assert_eq!(first.totals.slot(Slot::DebugUi).self_ns, MS);
    }

    #[test]
    fn the_log_window_sums_the_ui_windows_it_spans() {
        let mut w = Windows::new();
        let mut now = 0;
        let mut ui_frames = 0;
        let mut drains = 0;
        let log = loop {
            let (ui, log) = w.tick(now, 1, || {
                drains += 1;
                drained_with(Slot::VrWaitFrame, 2 * MS)
            });
            ui_frames += ui.map_or(0, |r| r.totals.frames);
            if let Some(log) = log {
                break log;
            }
            now += 10 * MS;
        };
        assert_eq!(log.totals.frames, ui_frames);
        assert_eq!(log.totals.sim_ticks, ui_frames);
        assert_eq!(log.totals.tick_load(TickLoad::One).frames, ui_frames);
        assert_eq!(log.totals.frame_ns, 1000 * 10 * MS);
        assert_eq!(log.totals.slot(Slot::VrWaitFrame).self_ns, drains * 2 * MS);
        assert_eq!(log.totals.slot(Slot::VrWaitFrame).calls, drains);
        assert!((log.window_s - 10.0).abs() < 1e-9, "{}", log.window_s);
        // The next log window starts empty.
        assert_eq!(w.log, Totals::default());
    }

    #[test]
    fn nested_scopes_record_self_time() {
        // The only test that opens scopes, so it has these two slots' counters to itself.
        std::thread::spawn(|| {
            {
                let _outer = scope(Slot::Tail);
                std::thread::sleep(Duration::from_millis(2));
                let _inner = scope(Slot::TailDrain);
                std::thread::sleep(Duration::from_millis(20));
            }
            let outer = &SLOTS[Slot::Tail.index()];
            let inner = &SLOTS[Slot::TailDrain.index()];
            assert_eq!(outer.calls.load(Ordering::Relaxed), 1);
            assert_eq!(inner.calls.load(Ordering::Relaxed), 1);
            let outer_self = outer.self_ns.load(Ordering::Relaxed);
            let inner_self = inner.self_ns.load(Ordering::Relaxed);
            assert!(inner_self >= 20 * MS, "{inner_self}");
            assert!((2 * MS..15 * MS).contains(&outer_self), "{outer_self}");
            // The outer scope's maximum is its whole length, the inner one included.
            assert!(outer.max_ns.load(Ordering::Relaxed) >= outer_self + inner_self);
            // An unmarked thread's time lands in the other-threads buckets by the slots' kinds.
            let other = &BUCKETS[ThreadClass::Other.index()];
            assert!(other[Kind::Wait.index()].load(Ordering::Relaxed) >= inner_self);
            assert!(other[Kind::Mod.index()].load(Ordering::Relaxed) >= outer_self);
        })
        .join()
        .unwrap();
    }
}
