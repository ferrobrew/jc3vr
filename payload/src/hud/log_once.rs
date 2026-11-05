//! A HUD part's resource failures, logged once per session. A part whose GPU resources fail to
//! build or paint retries on the next frame regardless, so logging every retry would flood the log.

use std::sync::atomic::{AtomicBool, Ordering};

/// The once-per-session error log of one HUD part, held in a static.
pub(crate) struct LogOnce {
    /// The part, the context prefix of the logged error.
    context: &'static str,
    logged: AtomicBool,
}

impl LogOnce {
    pub(crate) const fn new(context: &'static str) -> Self {
        Self {
            context,
            logged: AtomicBool::new(false),
        }
    }

    /// Log `error` at the error level if this part has logged nothing yet this session.
    pub(crate) fn error(&self, error: &anyhow::Error) {
        if !self.logged.swap(true, Ordering::Relaxed) {
            tracing::error!("{}: {error:#}", self.context);
        }
    }
}
