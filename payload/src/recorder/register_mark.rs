//! The recorder's handling of take-marker requests ([`crate::session::set_mark_handler`]): a
//! request drops an unlabelled marker, and saves the lookback when no tracking recording runs to
//! carry it ([`crate::recorder::session_recording::mark`]).

pub(crate) fn register() {
    crate::session::set_mark_handler(crate::recorder::session_recording::mark);
}
