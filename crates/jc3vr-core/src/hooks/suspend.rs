//! Running a block with every other thread of the process suspended, tolerating threads that exit
//! while the suspension is being set up.
//!
//! [`ThreadSuspender`] snapshots the process's threads and then opens each one; a thread that exits
//! between the snapshot and its open fails the open, and the suspender gives up on the whole
//! block. The game creates and retires threads during startup and while it streams, so that race is
//! a routine event rather than a real failure: a fresh snapshot no longer lists the thread. The
//! suspension is therefore retried with a new snapshot a bounded number of times before the error
//! is reported.

use re_utilities::{Error, ThreadSuspender, UserCallbackResult, error::WindowsError};

/// Run `f` with every other thread suspended, as [`ThreadSuspender::for_block`] does, retrying the
/// suspension when a thread exited between the snapshot and its open.
pub(crate) fn for_block<T>(f: impl FnOnce() -> UserCallbackResult<T>) -> UserCallbackResult<T> {
    let suspender = retry_transient(SUSPEND_ATTEMPTS, ThreadSuspender::new, |error| {
        matches!(error, Error::Windows(WindowsError::ThreadOpenFailed { .. }))
    })
    .map_err(|error| -> Box<dyn std::error::Error + Send + Sync> {
        format!("thread suspension: {error}").into()
    })?;
    let result = f();
    drop(suspender);
    result
}

/// How many snapshots the suspension takes before a thread-open failure is reported. Each retry
/// only needs the exiting thread to finish exiting, so a handful is ample.
const SUSPEND_ATTEMPTS: usize = 8;

/// Call `open` up to `attempts` times (at least once), returning the first success, or the last
/// error once the attempts are spent or as soon as an error is not `transient`.
fn retry_transient<S, E>(
    attempts: usize,
    mut open: impl FnMut() -> Result<S, E>,
    transient: impl Fn(&E) -> bool,
) -> Result<S, E> {
    let mut remaining = attempts.max(1);
    loop {
        match open() {
            Ok(value) => return Ok(value),
            Err(error) => {
                remaining -= 1;
                if remaining == 0 || !transient(&error) {
                    return Err(error);
                }
                tracing::debug!("thread suspension: a thread exited during the snapshot; retrying");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    enum Fault {
        Vanished,
        Denied,
    }

    fn scripted(script: Vec<Result<u32, Fault>>) -> impl FnMut() -> Result<u32, Fault> {
        let mut script = script.into_iter();
        move || script.next().expect("the script covers every attempt")
    }

    fn vanished(fault: &Fault) -> bool {
        *fault == Fault::Vanished
    }

    #[test]
    fn a_vanished_thread_is_retried_until_the_snapshot_is_clean() {
        let open = scripted(vec![Err(Fault::Vanished), Err(Fault::Vanished), Ok(7)]);
        assert_eq!(retry_transient(8, open, vanished), Ok(7));
    }

    #[test]
    fn a_non_transient_error_is_reported_at_once() {
        let open = scripted(vec![Err(Fault::Denied)]);
        assert_eq!(retry_transient(8, open, vanished), Err(Fault::Denied));
    }

    #[test]
    fn the_last_error_is_reported_once_the_attempts_are_spent() {
        let open = scripted(vec![Err(Fault::Vanished), Err(Fault::Vanished)]);
        assert_eq!(retry_transient(2, open, vanished), Err(Fault::Vanished));
    }

    #[test]
    fn zero_attempts_still_tries_once() {
        let open = scripted(vec![Ok(1)]);
        assert_eq!(retry_transient(0, open, vanished), Ok(1));
    }
}
