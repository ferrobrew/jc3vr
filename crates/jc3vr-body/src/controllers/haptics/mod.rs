//! Controller haptics: one vocabulary for every vibration the mod sends (`docs/mod/input/haptics.md`).
//!
//! - [`vocabulary`]: the named primitives (tick, click, bump, thud, buzz, hum, and tension) and the
//!   shape each one plays with;
//! - [`cues`]: every source, as a one-off [`Event`] or a [`Sustain`] that follows a level, with the
//!   primitive, strength, and category it plays;
//! - [`mixer`]: what each hand plays when several sources reach it at once;
//! - [`rumble`]: the game's own rumble, forwarded to the hands;
//! - [`shot`]: the shot in progress on the thread, which the weapon handling marks so that the
//!   weapon's fire rumble goes to the firing hand.
//!
//! A source calls [`play`], [`play_hands`], or [`play_scaled`] for a one-off cue and [`sustain`]
//! each tick for a level. Any thread may call them: the cue is shaped from the settings, checked
//! against the master switch, the controller scheme, and its category's switch, and handed to the
//! mixer, a leaf lock.
//! The VR frame loop calls [`mix`] once per rendered frame and plays what it returns.
//!
//! [`vocabulary`]: jc3vr_input::haptics::vocabulary
//! [`cues`]: jc3vr_input::haptics::cues
//! [`mixer`]: jc3vr_input::haptics::mixer

pub mod rumble;
pub mod shot;

use std::time::{Duration, Instant};

use jc3vr_input::{
    frame::Hand,
    haptics::{
        cues::{Category, Event, Sustain},
        mixer::{MixStats, Sustained, Transient},
        output,
        vocabulary::Primitive,
    },
};
use jc3vr_view::{
    tuning::HAPTICS,
    vr::input::config::{HAPTICS_CONFIG, HapticsConfig},
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{scheme, telemetry};

/// The haptics' hooks, aggregated by `controllers::hook_library`.
pub fn hook_library() -> HookLibrary {
    rumble::hook_library()
}

/// Play `event`'s cue on `hand`.
#[inline]
pub fn play(hand: Hand, event: Event) {
    play_scaled(hand, event, 1.0);
}

/// Play `event`'s cue on each of `hands`.
pub fn play_hands(hands: impl IntoIterator<Item = Hand>, event: Event) {
    for hand in hands {
        play(hand, event);
    }
}

/// Play `event`'s cue on `hand` at `scale` of its strength, `0..=1`: a source whose cue follows how
/// hard something happened. A scale of zero plays nothing.
pub fn play_scaled(hand: Hand, event: Event, scale: f32) {
    if scale.is_nan() || scale <= 0.0 {
        return;
    }
    let tuning = HAPTICS.get();
    let cue = tuning.event(event);
    if !acting(cue.category) {
        return;
    }
    let shape = tuning.shape(cue.primitive);
    let transient = Transient {
        amplitude: shape.amplitude(cue.strength * scale.min(1.0)),
        duration: seconds(shape.duration_s * tuning.duration_factor(cue.length)),
        frequency_hz: frequency(cue.primitive),
        priority: cue.primitive.priority(),
    };
    queue(hand, event, transient);
}

/// Send `source`'s level on `hand` for this tick, `0..=1` of its cue's strength. A source sends
/// every tick it lasts; a level of zero ends it at once, and one not sent again ends when its
/// primitive's hold runs out.
pub fn sustain(hand: Hand, source: Sustain, level: f32) {
    let tuning = HAPTICS.get();
    let cue = tuning.sustain(source);
    if !acting(cue.category) {
        return;
    }
    let shape = tuning.shape(cue.primitive);
    let level = if level.is_finite() {
        level.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let sustained = Sustained {
        source,
        amplitude: shape.amplitude(cue.strength * level),
        frequency_hz: frequency(cue.primitive),
        hold: seconds(shape.duration_s),
    };
    output::with_mixer(|mixer| mixer.sustain(hand, sustained, Instant::now()));
}

pub fn cancel_sustain(hand: Hand, source: Sustain) {
    output::with_mixer(|mixer| mixer.cancel_sustain(hand, source));
}

/// Play a weapon's fire rumble on `hand` as [`Event::Shot`], at the amplitude and for the length
/// the game's rumble sets.
pub(crate) fn play_shot(hand: Hand, amplitude: f32, duration_s: f32) {
    let cue = HAPTICS.get().event(Event::Shot);
    if !acting(cue.category) {
        return;
    }
    let transient = Transient {
        amplitude: amplitude.clamp(0.0, 1.0),
        duration: seconds(duration_s),
        frequency_hz: frequency(cue.primitive),
        priority: cue.primitive.priority(),
    };
    queue(hand, Event::Shot, transient);
}

/// Play `primitive` on `hand` at full strength, for the debug UI, whatever the switches say; the
/// master switch still applies when the frame loop plays it.
pub fn test(hand: Hand, primitive: Primitive) {
    let shape = HAPTICS.get().shape(primitive);
    // A sustained primitive tests as a one-off a little longer than its hold, so it is felt alone.
    let duration_s = if primitive.is_sustained() {
        TEST_SUSTAIN_S
    } else {
        shape.duration_s
    };
    let transient = Transient {
        amplitude: shape.amplitude(1.0),
        duration: seconds(duration_s),
        frequency_hz: frequency(primitive),
        priority: primitive.priority(),
    };
    output::with_mixer(|mixer| mixer.transient(hand, transient));
}

/// One one-off cue sent, for the readout.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct CueRecord {
    pub event: Event,
    pub hand: Hand,
    pub primitive: Primitive,
    pub amplitude: f32,
    pub duration_s: f32,
}

/// The haptics readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HapticsStatus {
    /// The most recent one-off cues, newest last.
    pub recent: Vec<CueRecord>,
    /// The one-off cues sent, per category, indexed by [`Category::index`].
    pub sent: [u64; Category::ALL.len()],
    /// What the mixer did.
    pub mix: MixStats,
    /// The amplitude each hand was last sent, if the last flush sent it anything.
    pub last_output: [Option<f32>; 2],
}

impl HapticsStatus {
    pub fn total_sent(&self) -> u64 {
        self.sent.iter().sum()
    }
}

pub fn status() -> HapticsStatus {
    let mut status = STATUS.lock().clone();
    status.mix = output::with_mixer(|mixer| mixer.stats());
    status.last_output = output::last_output();
    status
}

/// How many one-off cues the readout keeps.
const MAX_RECENT: usize = 12;
/// How long a sustained primitive plays when tested, seconds.
const TEST_SUSTAIN_S: f32 = 0.4;

static STATUS: Mutex<HapticsStatus> = Mutex::new(HapticsStatus {
    recent: Vec::new(),
    sent: [0; Category::ALL.len()],
    mix: MixStats {
        played: 0,
        stacked: 0,
        masked: 0,
        limited: 0,
    },
    last_output: [None; 2],
});

/// Whether the haptics act and `category` plays.
fn acting(category: Category) -> bool {
    let enabled = scheme::enabled();
    let haptics = *HAPTICS_CONFIG.lock();
    acts(enabled, &haptics) && haptics.category(category)
}

/// Whether the haptics act at all: the controllers (`enabled`) and the haptics on, under the
/// controller scheme.
fn acts(enabled: bool, haptics: &HapticsConfig) -> bool {
    enabled && haptics.enabled && scheme::is_controllers()
}

fn frequency(primitive: Primitive) -> Option<f32> {
    let hz = HAPTICS.get().shape(primitive).frequency_hz;
    (hz > 0.0 && hz.is_finite()).then_some(hz)
}

fn seconds(s: f32) -> Duration {
    Duration::try_from_secs_f32(s.max(0.0)).unwrap_or(Duration::ZERO)
}

fn queue(hand: Hand, event: Event, transient: Transient) {
    output::with_mixer(|mixer| mixer.transient(hand, transient));
    let cue = HAPTICS.get().event(event);
    telemetry::note_haptic(telemetry::Cue {
        hand,
        event: event.label(),
        primitive: cue.primitive.label(),
        amplitude: transient.amplitude,
        duration_s: transient.duration.as_secs_f32(),
        frequency_hz: transient.frequency_hz,
    });
    let mut status = STATUS.lock();
    status.sent[cue.category.index()] += 1;
    status.recent.push(CueRecord {
        event,
        hand,
        primitive: cue.primitive,
        amplitude: transient.amplitude,
        duration_s: transient.duration.as_secs_f32(),
    });
    let excess = status.recent.len().saturating_sub(MAX_RECENT);
    status.recent.drain(..excess);
}
