//! The haptics' tuning: the primitives' shapes, the mixer's rules, and the primitive and strength
//! of every source's cue.
//!
//! The values are the file `crates/jc3vr-view/tuning/haptics.toml` holds (`docs/mod/tuning.md`).
//! This crate depends on no layer crate, so it declares the types and takes the values as
//! arguments: the view crate, which runs the haptic output, owns the file, loads and checks it
//! (`jc3vr_view::tuning::HAPTICS`), and the sources and the frame loop read it from there.

use std::{
    collections::HashMap,
    fmt::{Debug, Write},
    hash::Hash,
};

use serde::Deserialize;

use crate::haptics::{
    cues::{Cue, Event, Length, Sustain},
    mixer::MixTuning,
    vocabulary::{Primitive, Shape, Vocabulary},
};

/// The haptics tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HapticsTuning {
    pub vocabulary: Vocabulary,
    pub mix: MixTuning,
    /// How many times its primitive's duration a [`Length::Long`] cue plays.
    pub long_factor: f32,
    /// Every one-off source's cue.
    pub events: EventCues,
    /// Every sustained source's cue.
    pub sustains: SustainCues,
}

impl HapticsTuning {
    /// The cue `event` plays.
    pub fn event(&self, event: Event) -> Cue {
        let spec = self.events.get(event);
        Cue {
            primitive: spec.primitive,
            strength: spec.strength,
            category: event.category(),
            length: spec.length,
        }
    }

    /// The cue `source` plays at a level of one.
    pub fn sustain(&self, source: Sustain) -> Cue {
        let spec = self.sustains.get(source);
        Cue {
            primitive: spec.primitive,
            strength: spec.strength,
            category: source.category(),
            length: Length::Normal,
        }
    }

    /// The shape of `primitive`.
    pub fn shape(&self, primitive: Primitive) -> &Shape {
        self.vocabulary.shape(primitive)
    }

    /// How many times its primitive's duration a cue of `length` plays.
    pub fn duration_factor(&self, length: Length) -> f32 {
        match length {
            Length::Normal => 1.0,
            Length::Long => self.long_factor,
        }
    }
}

/// A one-off source's cue, as the file gives it.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventCue {
    /// A one-off primitive.
    pub primitive: Primitive,
    /// Where in the primitive's range the cue sits at full scale, `0..=1`.
    pub strength: f32,
    pub length: Length,
}

/// A sustained source's cue, as the file gives it.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SustainCue {
    /// A sustained primitive.
    pub primitive: Primitive,
    /// Where in the primitive's range the cue sits at a level of one, `0..=1`.
    pub strength: f32,
}

/// Every one-off source's cue: a table keyed by the event's name in the file, which must name each
/// event once.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(try_from = "HashMap<Event, EventCue>")]
pub struct EventCues([EventCue; Event::COUNT]);

impl EventCues {
    pub fn get(&self, event: Event) -> &EventCue {
        &self.0[event as usize]
    }
}

impl TryFrom<HashMap<Event, EventCue>> for EventCues {
    type Error = String;

    fn try_from(mut cues: HashMap<Event, EventCue>) -> Result<Self, Self::Error> {
        let table = complete(&Event::ALL, &mut cues, "events")?;
        Ok(Self(table))
    }
}

/// Every sustained source's cue: a table keyed by the source's name in the file, which must name
/// each source once.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(try_from = "HashMap<Sustain, SustainCue>")]
pub struct SustainCues([SustainCue; Sustain::COUNT]);

impl SustainCues {
    pub fn get(&self, source: Sustain) -> &SustainCue {
        &self.0[source as usize]
    }
}

impl TryFrom<HashMap<Sustain, SustainCue>> for SustainCues {
    type Error = String;

    fn try_from(mut cues: HashMap<Sustain, SustainCue>) -> Result<Self, Self::Error> {
        let table = complete(&Sustain::ALL, &mut cues, "sustains")?;
        Ok(Self(table))
    }
}

/// A source as the file names it: `palm_touch` for [`Event::PalmTouch`].
pub fn file_key(source: &impl Debug) -> String {
    let name = format!("{source:?}");
    let mut out = String::with_capacity(name.len() + 4);
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// Takes the cue of every one of `all` out of `cues`, in order, naming the ones missing. A key
/// named twice, or one that names no source, is the TOML parser's to refuse.
fn complete<K, V, const N: usize>(
    all: &[K; N],
    cues: &mut HashMap<K, V>,
    table: &str,
) -> Result<[V; N], String>
where
    K: Copy + Eq + Hash + Debug,
{
    let mut missing = String::new();
    let mut found = Vec::with_capacity(N);
    for key in all {
        match cues.remove(key) {
            Some(cue) => found.push(cue),
            None => {
                let separator = if missing.is_empty() { "" } else { ", " };
                let _ = write!(missing, "{separator}{}", file_key(key));
            }
        }
    }
    if !missing.is_empty() {
        return Err(format!("{table}: missing the cue of {missing}"));
    }
    found
        .try_into()
        .map_err(|_| format!("{table}: the table does not hold every source"))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::LazyLock;

    use super::*;

    /// The tuning the repository ships, parsed from the view crate's file, which this crate's tests
    /// read without the view crate's loader.
    pub(crate) fn shipped() -> &'static HapticsTuning {
        static SHIPPED: LazyLock<HapticsTuning> = LazyLock::new(|| {
            toml::from_str(include_str!("../../../jc3vr-view/tuning/haptics.toml"))
                .unwrap_or_else(|error| panic!("haptics.toml: {error}"))
        });
        &SHIPPED
    }

    #[test]
    fn a_table_missing_a_source_names_it() {
        let text = include_str!("../../../jc3vr-view/tuning/haptics.toml")
            .replace("\npalm_touch = ", "\n# palm_touch = ");
        let error = toml::from_str::<HapticsTuning>(&text).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("events: missing the cue of palm_touch"),
            "{error}"
        );
    }
}
