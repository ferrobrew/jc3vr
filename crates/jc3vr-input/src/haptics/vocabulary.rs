//! The haptic vocabulary: the few named primitives every cue is built from, and the shape (the
//! amplitude range, the duration, and the frequency hint) each one has.
//!
//! A cue picks a primitive and a strength within it, so a tick is always faint and short and a thud
//! always heavy and low, whatever raised it. The shapes are tuned once
//! ([`HapticsTuning::vocabulary`](crate::haptics::tuning::HapticsTuning::vocabulary)), rather than
//! per cue.

use serde::{Deserialize, Serialize};

/// A named haptic primitive.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Primitive {
    /// The faintest, shortest cue: a hand crossing into a volume, a detent, a selection moving.
    Tick,
    /// A crisp, light confirmation: a button, a catch, a round, a release.
    Click,
    /// A firmer, rounder knock: a grip taken, a hand meeting a surface, a stop reached.
    Bump,
    /// The heaviest cue: a shot, a hook taking hold, a body caught, a detonation.
    Thud,
    /// A long, rough cue that says no: a refused grab, a press that does nothing.
    Buzz,
    /// A sustained low vibration that follows a level: the game's rumble, the reel's speed.
    Hum,
    /// A sustained, tighter vibration that follows a load: a pull, a press, a hang.
    Tension,
}

impl Primitive {
    pub const ALL: [Primitive; 7] = [
        Primitive::Tick,
        Primitive::Click,
        Primitive::Bump,
        Primitive::Thud,
        Primitive::Buzz,
        Primitive::Hum,
        Primitive::Tension,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Primitive::Tick => "tick",
            Primitive::Click => "click",
            Primitive::Bump => "bump",
            Primitive::Thud => "thud",
            Primitive::Buzz => "buzz",
            Primitive::Hum => "hum",
            Primitive::Tension => "tension",
        }
    }

    /// Whether the primitive follows a level each tick rather than playing once.
    pub const fn is_sustained(self) -> bool {
        matches!(self, Primitive::Hum | Primitive::Tension)
    }

    /// How hard a one-off primitive holds the hand against another arriving while it plays: a
    /// weaker cue of a lower priority is masked rather than cutting it short.
    pub const fn priority(self) -> u8 {
        match self {
            Primitive::Tick | Primitive::Hum | Primitive::Tension => 0,
            Primitive::Click => 1,
            Primitive::Bump => 2,
            Primitive::Buzz => 3,
            Primitive::Thud => 4,
        }
    }
}

/// How one primitive plays.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shape {
    /// The amplitude at strength zero, `0..=1`. Zero for a sustained primitive, so that a level of
    /// zero is still.
    pub soft: f32,
    /// The amplitude at full strength, `0..=1`.
    pub full: f32,
    /// How long a one-off primitive plays, seconds; for a sustained one, how long a level holds
    /// without being refreshed.
    pub duration_s: f32,
    /// The frequency hint, hertz. Zero leaves the runtime's default, and a runtime may ignore it.
    pub frequency_hz: f32,
}

impl Shape {
    /// The amplitude at `strength`, `0..=1`, between [`Shape::soft`] and [`Shape::full`].
    pub fn amplitude(&self, strength: f32) -> f32 {
        let strength = if strength.is_finite() {
            strength.clamp(0.0, 1.0)
        } else {
            0.0
        };
        (self.soft + (self.full - self.soft) * strength).clamp(0.0, 1.0)
    }
}

/// The shape of every primitive.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vocabulary {
    pub tick: Shape,
    pub click: Shape,
    pub bump: Shape,
    pub thud: Shape,
    pub buzz: Shape,
    pub hum: Shape,
    pub tension: Shape,
}

impl Vocabulary {
    pub const fn shape(&self, primitive: Primitive) -> &Shape {
        match primitive {
            Primitive::Tick => &self.tick,
            Primitive::Click => &self.click,
            Primitive::Bump => &self.bump,
            Primitive::Thud => &self.thud,
            Primitive::Buzz => &self.buzz,
            Primitive::Hum => &self.hum,
            Primitive::Tension => &self.tension,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::haptics::tuning::tests::shipped;

    #[test]
    fn strength_spans_the_shape() {
        let shape = shipped().vocabulary.bump;
        assert_eq!(shape.amplitude(0.0), shape.soft);
        assert_eq!(shape.amplitude(1.0), shape.full);
        assert_eq!(shape.amplitude(5.0), shape.full);
        assert_eq!(shape.amplitude(f32::NAN), shape.soft);
    }

    #[test]
    fn the_default_primitives_rise_in_weight() {
        let v = shipped().vocabulary;
        let weights = [v.tick, v.click, v.bump, v.thud].map(|s| (s.full, s.duration_s));
        for pair in weights.windows(2) {
            assert!(pair[0].0 < pair[1].0 && pair[0].1 < pair[1].1);
        }
        assert!(v.buzz.duration_s > v.thud.duration_s);
        assert_eq!(v.hum.soft, 0.0);
        assert_eq!(v.tension.soft, 0.0);
    }

    #[test]
    fn only_hum_and_tension_are_sustained() {
        for primitive in Primitive::ALL {
            assert_eq!(
                primitive.is_sustained(),
                matches!(primitive, Primitive::Hum | Primitive::Tension)
            );
        }
    }
}
