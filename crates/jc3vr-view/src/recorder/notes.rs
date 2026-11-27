//! The recording's notes that the tracking recorder takes on its own behalf: the settings in
//! effect and the frame-time breakdown. Like every event, nothing is built unless a recording or
//! the lookback collects. The frame-time breakdown reaches the recorder through the frame timing's
//! report sink ([`register_frame_timing`]), so `frame_timing` does not name the recorder.

use jc3vr_core::{
    frame_timing,
    frame_timing::{Report, Slot, ns_to_ms},
};
use tracking_format::event::{FrameTiming, Label, Settings, SlotTiming};

use crate::recorder::{events, events::EventKind};

/// Note the settings file's text, as a save writes it or a recording starts with it.
pub fn note_settings(toml: &str) {
    events::note_with(|| {
        EventKind::Settings(Box::new(Settings {
            toml: Label::Owned(toml.to_owned()),
        }))
    });
}

/// Hand every closed frame-timing window to [`note_frame_timing`]. Call once, before the first
/// window closes.
pub fn register_frame_timing() {
    frame_timing::set_report_sink(note_frame_timing);
}

/// Note a closed reporting window of the frame-time breakdown.
fn note_frame_timing(report: &Report) {
    events::note_with(|| {
        let totals = &report.totals;
        let slots = Slot::ALL
            .iter()
            .zip(&totals.slots)
            .filter(|(_, slot)| slot.calls > 0)
            .map(|(slot, totals_of)| SlotTiming {
                name: Label::Borrowed(slot.name()),
                self_ms: totals.per_frame_ms(totals_of.self_ns) as f32,
                max_ms: ns_to_ms(totals_of.max_ns) as f32,
                calls: u32::try_from(totals_of.calls).unwrap_or(u32::MAX),
            })
            .collect();
        EventKind::FrameTiming(Box::new(FrameTiming {
            window_s: report.window_s as f32,
            frames: u32::try_from(totals.frames).unwrap_or(u32::MAX),
            sim_ticks: u32::try_from(totals.sim_ticks).unwrap_or(u32::MAX),
            frame_ms: totals.mean_frame_ms() as f32,
            frame_max_ms: ns_to_ms(totals.frame_max_ns) as f32,
            slots,
        }))
    });
}
