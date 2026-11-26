//! The version 8 notes: the actions the mod injected into the game, refusals and other diagnostic
//! moments, haptic cues, the settings in effect, and the frame-time breakdown.

use crate::{event::Label, model::Hand};

/// The action writes the mod landed in the game's action maps on one input tick, from version 8.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct Injected {
    pub writes: Vec<InjectedWrite>,
}

/// One action write and what became of it.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct InjectedWrite {
    /// The action map: `player` or `ui`.
    pub map: Label,
    /// The game's action, by its name.
    pub action: Label,
    /// The value the write raised the effector to.
    pub value: f32,
    /// The write's phase: `value`, `clicked`, `held`, `released`, `unclick`, or `neutral`.
    pub phase: Label,
    /// How long the action had been held, seconds, for a hold or a release.
    pub held_s: Option<f32>,
    /// What became of it: `applied`, `kept physical press`, `already moved on`, or `unresolved`.
    pub outcome: Label,
}

/// A refusal or another moment a playtest needs explained, from version 8: the same fact a log
/// line reports, at the same seam.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    /// The subsystem, such as `trigger`, `door`, `swim`, `wingsuit`, `pouch`, or `snatch`.
    pub source: Label,
    /// The hand it concerns, if one.
    pub hand: Option<Hand>,
    /// What happened, in a few words.
    pub what: Label,
    /// The evidence behind it.
    pub detail: Option<Label>,
}

/// A one-off haptic cue sent to a hand, from version 8.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct Haptic {
    pub hand: Hand,
    /// The cue's source event and the primitive it plays.
    pub event: Label,
    pub primitive: Label,
    /// `0..=1`.
    pub amplitude: f32,
    pub duration_s: f32,
    /// The vibration frequency, hertz, or `None` for the runtime's default.
    pub frequency_hz: Option<f32>,
}

/// The mod's settings, noted when a recording or a lookback save starts and whenever a change is
/// saved, from version 8.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// The settings file's TOML.
    pub toml: Label,
}

/// The frame-time breakdown over one reporting window, from version 8: the same numbers the log's
/// `perf: frame breakdown` line and the debug UI's Performance tab show.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct FrameTiming {
    pub window_s: f32,
    pub frames: u32,
    pub sim_ticks: u32,
    /// The mean and longest game frame interval, milliseconds.
    pub frame_ms: f32,
    pub frame_max_ms: f32,
    /// Every slot that ran in the window.
    pub slots: Vec<SlotTiming>,
}

/// One timed slot of the frame.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct SlotTiming {
    pub name: Label,
    /// Its own time per frame, and its longest call, milliseconds.
    pub self_ms: f32,
    pub max_ms: f32,
    pub calls: u32,
}
