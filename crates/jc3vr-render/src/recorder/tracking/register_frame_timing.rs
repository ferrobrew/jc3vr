//! Registers the frame-time breakdown's tracking event as the frame timing's report sink, so each
//! closed debug-UI window lands in a recording ([`note_frame_timing`]) without `frame_timing`
//! naming the recorder.

use jc3vr_core::frame_timing;

use crate::recorder::tracking::notes::note_frame_timing;

/// Hand every closed frame-timing window to [`note_frame_timing`]. Call once, before the first
/// window closes.
pub fn register_frame_timing() {
    frame_timing::set_report_sink(note_frame_timing);
}
