//! The diagnostic notes a recording carries: refusals and other moments a playtest needs
//! explained, the forces the mod applies to bodies, the actions it injects, the haptic cues it
//! plays, the settings in effect, and the frame-time breakdown. Each producer calls in where it
//! acts; nothing is built unless a recording or the lookback collects.

use std::panic::Location;

use glam::Vec3;
use tracking_format::event::{
    Diagnostic, Force, ForceKind, FrameTiming, Haptic, Injected, InjectedWrite, Label, Settings,
    SlotTiming, WorldMotion,
};

use crate::{
    controllers::frame::Hand,
    frame_timing::{Report, Slot, ns_to_ms},
    recorder::tracking::{
        events::{self, EventKind},
        format::hand_to_file,
    },
};

/// Note a refusal or another diagnostic moment from `source`, the same fact its log line reports.
/// `detail` is built only while a recording or the lookback collects.
pub(crate) fn note_diagnostic(
    source: &'static str,
    hand: Option<Hand>,
    what: &'static str,
    detail: impl FnOnce() -> Option<String>,
) {
    if !events::collecting() {
        return;
    }
    events::note(EventKind::Diagnostic(Box::new(Diagnostic {
        source: Label::Borrowed(source),
        hand: hand.map(hand_to_file),
        what: Label::Borrowed(what),
        detail: detail().map(Label::Owned),
    })));
}

/// A body's state before a force, as the writer read it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct BodyBefore {
    pub object: Option<usize>,
    pub mass_kg: Option<f32>,
    pub linear: Vec3,
    pub angular: Vec3,
}

/// What a force did to a body.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) enum Applied {
    Velocity { linear: Vec3, angular: Vec3 },
    Impulse { impulse: Vec3, at: Vec3 },
}

/// Note a velocity write or an impulse on `body`, attributed to the caller's source line.
#[track_caller]
pub(crate) fn note_force(body: u32, applied: Applied, before: Option<BodyBefore>) {
    if !events::collecting() {
        return;
    }
    let caller = Location::caller();
    events::note(EventKind::Force(Box::new(Force {
        source: Label::Borrowed(caller.file()),
        line: caller.line(),
        kind: match applied {
            Applied::Velocity { linear, angular } => ForceKind::Velocity { linear, angular },
            Applied::Impulse { impulse, at } => ForceKind::Impulse { impulse, at },
        },
        body,
        object: before.and_then(|b| b.object).map(|object| object as u64),
        mass_kg: before.and_then(|b| b.mass_kg),
        before: before.map(|b| WorldMotion {
            linear: b.linear,
            angular: b.angular,
        }),
    })));
}

/// One action write the bindings landed, for [`note_injected`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Write {
    pub map: String,
    pub action: String,
    pub value: f32,
    pub phase: &'static str,
    pub held_s: Option<f32>,
    pub outcome: &'static str,
}

/// Note the action writes landed on one input tick, when there were any. `writes` is built only
/// while a recording or the lookback collects.
pub(crate) fn note_injected(writes: impl FnOnce() -> Vec<Write>) {
    if !events::collecting() {
        return;
    }
    let writes = writes();
    if writes.is_empty() {
        return;
    }
    events::note(EventKind::Injected(Box::new(Injected {
        writes: writes
            .into_iter()
            .map(|w| InjectedWrite {
                map: Label::Owned(w.map),
                action: Label::Owned(w.action),
                value: w.value,
                phase: Label::Borrowed(w.phase),
                held_s: w.held_s,
                outcome: Label::Borrowed(w.outcome),
            })
            .collect(),
    })));
}

/// A haptic cue as the mixer took it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Cue {
    pub hand: Hand,
    pub event: &'static str,
    pub primitive: &'static str,
    pub amplitude: f32,
    pub duration_s: f32,
    pub frequency_hz: Option<f32>,
}

/// Note a one-off haptic cue.
pub(crate) fn note_haptic(cue: Cue) {
    if !events::collecting() {
        return;
    }
    events::note(EventKind::Haptic(Box::new(Haptic {
        hand: hand_to_file(cue.hand),
        event: Label::Borrowed(cue.event),
        primitive: Label::Borrowed(cue.primitive),
        amplitude: cue.amplitude,
        duration_s: cue.duration_s,
        frequency_hz: cue.frequency_hz,
    })));
}

/// Note the settings file's text, as a save writes it or a recording starts with it.
pub(crate) fn note_settings(toml: &str) {
    if !events::collecting() {
        return;
    }
    events::note(EventKind::Settings(Box::new(Settings {
        toml: Label::Owned(toml.to_owned()),
    })));
}

/// Note a closed reporting window of the frame-time breakdown.
pub(crate) fn note_frame_timing(report: &Report) {
    if !events::collecting() {
        return;
    }
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
    events::note(EventKind::FrameTiming(Box::new(FrameTiming {
        window_s: report.window_s as f32,
        frames: u32::try_from(totals.frames).unwrap_or(u32::MAX),
        sim_ticks: u32::try_from(totals.sim_ticks).unwrap_or(u32::MAX),
        frame_ms: totals.mean_frame_ms() as f32,
        frame_max_ms: ns_to_ms(totals.frame_max_ns) as f32,
        slots,
    })));
}
