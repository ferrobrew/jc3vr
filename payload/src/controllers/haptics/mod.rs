//! Controller haptics: one vocabulary for every vibration the mod sends (`docs/mod/input/haptics.md`).
//!
//! - [`vocabulary`]: the named primitives (tick, click, bump, thud, buzz, hum, and tension) and the
//!   shape each one plays with;
//! - [`cues`]: every source, as a one-off [`Event`] or a [`Sustain`] that follows a level, with the
//!   primitive, strength, and category it plays;
//! - [`mixer`]: what each hand plays when several sources reach it at once;
//! - [`rumble`]: the game's own rumble, forwarded to the hands.
//!
//! A source calls [`play`] or [`play_scaled`] for a one-off cue and [`sustain`] each tick for a
//! level. Any thread may call them: the cue is shaped from the settings, checked against the master
//! switch, the controller scheme, and its category's switch, and handed to the mixer, a leaf lock.
//! The VR frame loop calls [`mix`] once per rendered frame and plays what it returns.

pub mod config;
pub mod cues;
pub mod mixer;
pub mod rumble;
pub mod vocabulary;

use std::time::{Duration, Instant};

pub use config::HapticsConfig;
pub use cues::{Category, Event, Sustain};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        config::ControllersConfig,
        frame::Hand,
        haptics::{
            mixer::{HapticPulse, MixConfig, MixStats, Mixer, Sustained, Transient},
            vocabulary::Primitive,
        },
        scheme,
    },
};

/// The haptics' hooks, aggregated by [`crate::controllers::hook_library`].
pub(crate) fn hook_library() -> HookLibrary {
    rumble::hook_library()
}

/// Play `event`'s cue on `hand`.
pub fn play(hand: Hand, event: Event) {
    play_scaled(hand, event, 1.0);
}

/// Play `event`'s cue on `hand` at `scale` of its strength, `0..=1`: a source whose cue follows how
/// hard something happened. A scale of zero plays nothing.
pub fn play_scaled(hand: Hand, event: Event, scale: f32) {
    if scale.is_nan() || scale <= 0.0 {
        return;
    }
    let cue = event.cue();
    let Some(config) = acting(cue.category) else {
        return;
    };
    let shape = config.vocabulary.shape(cue.primitive);
    let transient = Transient {
        amplitude: shape.amplitude(cue.strength * scale.min(1.0)),
        duration: seconds(shape.duration_s * cue.length.factor()),
        frequency_hz: frequency(&config, cue.primitive),
        priority: cue.primitive.priority(),
    };
    queue(hand, event, transient);
}

/// Send `source`'s level on `hand` for this tick, `0..=1` of its cue's strength. A source sends
/// every tick it lasts; a level of zero ends it at once, and one not sent again ends when its
/// primitive's hold runs out.
pub fn sustain(hand: Hand, source: Sustain, level: f32) {
    let cue = source.cue();
    let Some(config) = acting(cue.category) else {
        return;
    };
    let shape = config.vocabulary.shape(cue.primitive);
    let level = if level.is_finite() {
        level.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let sustained = Sustained {
        source,
        amplitude: shape.amplitude(cue.strength * level),
        frequency_hz: frequency(&config, cue.primitive),
        hold: seconds(shape.duration_s),
    };
    MIXER.lock().sustain(hand, sustained, Instant::now());
}

/// Play a weapon's fire rumble on `hand` as [`Event::Shot`], at the amplitude and for the length
/// the game's rumble sets.
pub(crate) fn play_shot(hand: Hand, amplitude: f32, duration_s: f32) {
    let cue = Event::Shot.cue();
    let Some(config) = acting(cue.category) else {
        return;
    };
    let transient = Transient {
        amplitude: amplitude.clamp(0.0, 1.0),
        duration: seconds(duration_s),
        frequency_hz: frequency(&config, cue.primitive),
        priority: cue.primitive.priority(),
    };
    queue(hand, Event::Shot, transient);
}

/// What each hand plays from `now`, indexed by [`Hand::index`], for the VR frame loop to send.
pub(crate) fn mix(now: Instant, config: &MixConfig) -> [Option<HapticPulse>; 2] {
    let out = MIXER.lock().mix(now, config);
    let mut status = STATUS.lock();
    for hand in Hand::BOTH {
        status.last_output[hand.index()] = out[hand.index()].map(|p| p.amplitude);
    }
    out
}

/// Forget everything the mixer holds: the controllers or the haptics are off, or the session
/// ended.
pub(crate) fn clear() {
    MIXER.lock().clear();
}

/// Play `primitive` on `hand` at full strength, for the debug UI, whatever the switches say; the
/// master switch still applies when the frame loop plays it.
pub fn test(hand: Hand, primitive: Primitive) {
    let config = Config::lock_query(|c| c.controllers.haptics);
    let shape = config.vocabulary.shape(primitive);
    // A sustained primitive tests as a one-off a little longer than its hold, so it is felt alone.
    let duration_s = if primitive.is_sustained() {
        TEST_SUSTAIN_S
    } else {
        shape.duration_s
    };
    let transient = Transient {
        amplitude: shape.amplitude(1.0),
        duration: seconds(duration_s),
        frequency_hz: frequency(&config, primitive),
        priority: primitive.priority(),
    };
    MIXER.lock().transient(hand, transient);
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
    status.mix = MIXER.lock().stats();
    status
}

/// How many one-off cues the readout keeps.
const MAX_RECENT: usize = 12;
/// How long a sustained primitive plays when tested, seconds.
const TEST_SUSTAIN_S: f32 = 0.4;

static MIXER: Mutex<Mixer> = Mutex::new(Mixer::new());

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

/// The haptics settings, when the haptics act and `category` is switched on.
fn acting(category: Category) -> Option<HapticsConfig> {
    let config = Config::lock_query(|c| c.controllers);
    (acts(&config) && config.haptics.category(category)).then_some(config.haptics)
}

/// Whether the haptics act at all: the controllers and the haptics on, under the controller
/// scheme.
fn acts(config: &ControllersConfig) -> bool {
    config.enabled && config.haptics.enabled && scheme::is_controllers()
}

fn frequency(config: &HapticsConfig, primitive: Primitive) -> Option<f32> {
    let hz = config.vocabulary.shape(primitive).frequency_hz;
    (config.frequencies && hz > 0.0 && hz.is_finite()).then_some(hz)
}

fn seconds(s: f32) -> Duration {
    Duration::try_from_secs_f32(s.max(0.0)).unwrap_or(Duration::ZERO)
}

fn queue(hand: Hand, event: Event, transient: Transient) {
    MIXER.lock().transient(hand, transient);
    let cue = event.cue();
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
