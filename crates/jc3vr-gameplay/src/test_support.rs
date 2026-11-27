//! The serialization the crate's tests share over the view crate's process-wide input history and
//! flight packet: a test that captures the history or relies on an empty flight packet holds
//! [`HISTORY_LOCK`], and the policy runtime holds it while it records an evaluation, so a capture
//! sees only its own test's records.

use jc3vr_view::vr::input::flight_pose;
use parking_lot::{ReentrantMutex, ReentrantMutexGuard};

/// Held by each test that records into, captures, or resets the input history or the flight
/// packet, and by the policy runtime's records under test. Reentrant, so a test that holds it
/// still records.
pub(crate) static HISTORY_LOCK: ReentrantMutex<()> = ReentrantMutex::new(());

/// Hold [`HISTORY_LOCK`] with the flight packet empty, and leave it empty when the guard drops,
/// unwinding included.
pub(crate) fn flight_guard() -> FlightGuard {
    let guard = FlightGuard {
        _serial: HISTORY_LOCK.lock(),
    };
    flight_pose::unavailable();
    guard
}

pub(crate) struct FlightGuard {
    _serial: ReentrantMutexGuard<'static, ()>,
}

impl Drop for FlightGuard {
    fn drop(&mut self) {
        flight_pose::unavailable();
    }
}
