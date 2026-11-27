//! The take-marker request: code below the recorder asks for a marker, and the recorder, which
//! knows whether a recording runs to carry it, registers what a request does ([`set_mark_handler`]).

use std::sync::OnceLock;

/// Ask for an unlabelled take marker. Any thread. Without a registered handler the request is
/// dropped with a warning.
pub fn request_mark() {
    match MARK_HANDLER.get() {
        Some(handler) => handler(),
        None => tracing::warn!("session: a take marker was requested with no marker handler"),
    }
}

/// Register what a take-marker request does. Only one handler is held: the first registration wins
/// and a later one is refused with a warning.
pub fn set_mark_handler(handler: fn()) {
    if MARK_HANDLER.set(handler).is_err() {
        tracing::warn!("session: a marker handler is already registered; the new one is ignored");
    }
}

/// Whether a take-marker handler is registered, for the install's registration check.
pub fn mark_handler_registered() -> bool {
    MARK_HANDLER.get().is_some()
}

static MARK_HANDLER: OnceLock<fn()> = OnceLock::new();
