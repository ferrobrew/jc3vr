//! The two clocks a publication is stamped on, and the windows that say how long a stamp stays
//! current.
//!
//! [`Stamp`] is the mod's one wall clock: nanoseconds since the process epoch, the instant the
//! first reading was taken, read through [`Instant`] (`QueryPerformanceCounter` on Windows). The
//! frame timing counts from the same epoch, so a stamp and a frame-timing time are comparable.
//! [`Step`] counts the physics steps. A publication read inside the physics step measures its age
//! in steps, so a hitch that stalls the whole simulation ages nothing; one read from the frame or
//! the input tick measures it in wall time.

use std::{
    fmt::Debug,
    ops::Add,
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// A clock, named by the type of its readings: [`Stamp`] for wall time, [`Step`] for physics
/// steps.
pub trait Clock: Copy + Ord + Debug + Send + Sync + 'static {
    /// How long a reading stays current, on this clock.
    type Window: Copy + Debug + Send + Sync + 'static;

    /// The clock's current reading.
    fn now() -> Self;

    /// Whether a value stamped `self` is still current at `now` under `window`. A stamp ahead of
    /// `now` (a reader that took its time before the writer published) is current.
    fn within(self, now: Self, window: Self::Window) -> bool;
}

/// A reading of the wall clock: nanoseconds since the process epoch.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Stamp(u64);

impl Stamp {
    /// The epoch itself.
    pub const ZERO: Self = Self(0);

    /// The current reading.
    #[inline]
    pub fn now() -> Self {
        Self(duration_ns(epoch().elapsed()))
    }

    pub const fn from_nanos(ns: u64) -> Self {
        Self(ns)
    }

    /// The reading at `instant`, for a time taken with [`Instant`] by a crate below this one (the
    /// input frame's publication times). An instant before the epoch reads as the epoch.
    pub fn from_instant(instant: Instant) -> Self {
        Self(duration_ns(instant.saturating_duration_since(epoch())))
    }

    pub const fn as_nanos(self) -> u64 {
        self.0
    }

    /// The time from the epoch to this reading.
    pub const fn since_epoch(self) -> Duration {
        Duration::from_nanos(self.0)
    }

    /// The time from `earlier` to this reading, zero when `earlier` is later.
    #[inline]
    pub const fn saturating_duration_since(self, earlier: Self) -> Duration {
        Duration::from_nanos(self.0.saturating_sub(earlier.0))
    }

    /// The time since this reading.
    pub fn elapsed(self) -> Duration {
        Self::now().saturating_duration_since(self)
    }
}

impl Add<Duration> for Stamp {
    type Output = Self;

    /// The reading `rhs` after this one, saturating at the clock's end.
    fn add(self, rhs: Duration) -> Self {
        Self(self.0.saturating_add(duration_ns(rhs)))
    }
}

impl Clock for Stamp {
    type Window = Window;

    #[inline]
    fn now() -> Self {
        Self::now()
    }

    #[inline]
    fn within(self, now: Self, window: Window) -> bool {
        window.holds(self, now)
    }
}

/// How long a wall-clock publication stays current: a tuning value's duration. The boundary is
/// inclusive, so a value exactly the window old is still current. Held in nanoseconds, so that a
/// check is one subtraction and one comparison of the stamps.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Window(u64);

impl Window {
    /// A window `length` long, saturating at the clock's end.
    pub const fn new(length: Duration) -> Self {
        let ns = length.as_nanos();
        Self(if ns > u64::MAX as u128 {
            u64::MAX
        } else {
            ns as u64
        })
    }

    pub const fn from_millis(ms: u64) -> Self {
        Self(ms.saturating_mul(1_000_000))
    }

    pub const fn duration(self) -> Duration {
        Duration::from_nanos(self.0)
    }

    /// Whether a value stamped `at` is still current at `now`. Takes a [`Stamp`] or an
    /// `Option<Stamp>`, where `None` (never published) is never current.
    #[inline]
    pub fn holds(self, at: impl Into<Option<Stamp>>, now: Stamp) -> bool {
        at.into()
            .is_some_and(|at| now.0.saturating_sub(at.0) <= self.0)
    }
}

impl From<Duration> for Window {
    fn from(length: Duration) -> Self {
        Self::new(length)
    }
}

/// A reading of the physics-step clock: the number of physics steps begun since the payload
/// loaded.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Step(u64);

impl Step {
    /// Before the first step.
    pub const ZERO: Self = Self(0);

    /// The step in progress, or the last one begun.
    #[inline]
    pub fn now() -> Self {
        Self(STEPS.load(Ordering::Acquire))
    }

    /// Begin a physics step, returning its reading. Called once at the start of each physics step
    /// by the one callback that runs there; a second caller would make the clock run fast.
    pub fn begin() -> Self {
        Self(STEPS.fetch_add(1, Ordering::AcqRel) + 1)
    }

    pub const fn from_count(steps: u64) -> Self {
        Self(steps)
    }

    pub const fn count(self) -> u64 {
        self.0
    }

    /// The steps from `earlier` to this reading, zero when `earlier` is later.
    pub const fn steps_since(self, earlier: Self) -> u64 {
        self.0.saturating_sub(earlier.0)
    }
}

impl Clock for Step {
    type Window = StepWindow;

    #[inline]
    fn now() -> Self {
        Self::now()
    }

    #[inline]
    fn within(self, now: Self, window: StepWindow) -> bool {
        window.holds(self, now)
    }
}

/// How many physics steps a step-stamped publication stays current. Inclusive, like [`Window`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StepWindow(u64);

impl StepWindow {
    pub const fn new(steps: u64) -> Self {
        Self(steps)
    }

    pub const fn steps(self) -> u64 {
        self.0
    }

    /// Whether a value stamped at step `at` is still current at step `now`. Takes a [`Step`] or an
    /// `Option<Step>`, where `None` is never current.
    #[inline]
    pub fn holds(self, at: impl Into<Option<Step>>, now: Step) -> bool {
        at.into().is_some_and(|at| now.steps_since(at) <= self.0)
    }
}

/// The physics steps begun since the payload loaded, advanced by [`Step::begin`].
static STEPS: AtomicU64 = AtomicU64::new(0);

/// The instant the wall clock counts from: the first reading.
fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

fn duration_ns(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    #[test]
    fn a_window_is_inclusive_and_admits_a_stamp_from_the_future() {
        let window = Window::from_millis(250);
        let at = Stamp::from_nanos(1_000 * MS);
        assert!(window.holds(at, Stamp::from_nanos(1_250 * MS)));
        assert!(!window.holds(at, Stamp::from_nanos(1_250 * MS + 1)));
        // A reader that took its time before the writer published sees the value as new.
        assert!(window.holds(at, Stamp::from_nanos(900 * MS)));
        assert!(!window.holds(None, Stamp::from_nanos(1_000 * MS)));
        assert!(window.holds(Some(at), Stamp::from_nanos(1_100 * MS)));
    }

    #[test]
    fn a_window_keeps_its_length() {
        assert_eq!(
            Window::new(Duration::from_millis(250)),
            Window::from_millis(250)
        );
        assert_eq!(
            Window::from(Duration::from_secs(2)).duration(),
            Duration::from_secs(2)
        );
        assert_eq!(
            Window::new(Duration::MAX).duration().as_nanos(),
            u64::MAX as u128
        );
    }

    #[test]
    fn an_instant_maps_onto_the_epoch() {
        let before = Stamp::now();
        let at = Instant::now();
        let stamp = Stamp::from_instant(at);
        assert_eq!(
            Stamp::from_instant(at + Duration::from_nanos(5)).saturating_duration_since(stamp),
            Duration::from_nanos(5)
        );
        assert!(before <= stamp && stamp <= Stamp::now());
    }

    #[test]
    fn a_step_window_counts_steps_and_not_time() {
        let window = StepWindow::new(3);
        let at = Step::from_count(40);
        assert!(window.holds(at, Step::from_count(43)));
        assert!(!window.holds(at, Step::from_count(44)));
        assert!(window.holds(at, Step::from_count(39)));
        assert!(!window.holds(None, Step::from_count(40)));
    }

    #[test]
    fn the_wall_clock_runs_forward_from_one_epoch() {
        let first = Stamp::now();
        std::thread::sleep(Duration::from_millis(2));
        let second = Stamp::now();
        assert!(second.saturating_duration_since(first) >= Duration::from_millis(2));
        assert_eq!(first.saturating_duration_since(second), Duration::ZERO);
        assert!(first.elapsed() >= Duration::from_millis(2));
        assert_eq!(
            (first + Duration::from_millis(5)).as_nanos(),
            first.as_nanos() + 5 * MS
        );
    }
}
