//! The view crate's tuning files (`docs/mod/tuning.md`): [`HAPTICS`], the haptic output's
//! primitives, mix, and cues, whose types the input crate declares
//! ([`jc3vr_input::haptics::tuning`]) and which this crate loads because the input crate depends on
//! no layer crate.

use std::ops::Deref;

use jc3vr_core::{
    config::engine::{Invalid, nonnegative, positive, range},
    tuning::Tuning,
};
use jc3vr_input::haptics::{
    cues::{Event, Sustain},
    tuning::{HapticsTuning, file_key},
    vocabulary::Primitive,
};
use serde::Deserialize;

jc3vr_core::tuning_file! {
    /// The haptics' tuning file.
    pub static HAPTICS: HapticsFile = "haptics.toml";
}

/// The haptics' tuning, as the file holds it: [`HapticsTuning`], to which it derefs.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(transparent)]
pub struct HapticsFile(pub HapticsTuning);

impl Deref for HapticsFile {
    type Target = HapticsTuning;

    fn deref(&self) -> &HapticsTuning {
        &self.0
    }
}

impl Tuning for HapticsFile {
    fn validate(&self) -> Result<(), Invalid> {
        let tuning = &self.0;
        for primitive in Primitive::ALL {
            let shape = tuning.shape(primitive);
            let name = |field: &str| format!("vocabulary.{}.{field}", primitive.label());
            nonnegative(&name("soft"), shape.soft, 1.0)?;
            range(&name("full"), shape.full, shape.soft, 1.0)?;
            positive(&name("duration_s"), shape.duration_s, 10.0)?;
            nonnegative(&name("frequency_hz"), shape.frequency_hz, 1000.0)?;
            if primitive.is_sustained() && shape.soft != 0.0 {
                return Err(Invalid(format!(
                    "{} must be zero for a sustained primitive, so that a level of zero is still",
                    name("soft")
                )));
            }
        }

        let mix = &tuning.mix;
        nonnegative("mix.sustain_ceiling", mix.sustain_ceiling, 1.0)?;
        range(
            "mix.sustain_knee",
            mix.sustain_knee,
            0.0,
            mix.sustain_ceiling,
        )?;
        nonnegative("mix.knee", mix.knee, 1.0)?;
        nonnegative("mix.stack_share", mix.stack_share, 1.0)?;
        nonnegative("mix.duck_share", mix.duck_share, 1.0)?;
        positive("mix.refresh_s", mix.refresh_s, 1.0)?;
        positive("long_factor", tuning.long_factor, 10.0)?;

        for event in Event::ALL {
            let cue = tuning.events.get(event);
            let name = format!("events.{}", file_key(&event));
            nonnegative(&format!("{name}.strength"), cue.strength, 1.0)?;
            if cue.primitive.is_sustained() {
                return Err(Invalid(format!(
                    "{name}.primitive must be a one-off primitive, got {}",
                    cue.primitive.label()
                )));
            }
        }
        for source in Sustain::ALL {
            let cue = tuning.sustains.get(source);
            let name = format!("sustains.{}", file_key(&source));
            nonnegative(&format!("{name}.strength"), cue.strength, 1.0)?;
            if !cue.primitive.is_sustained() {
                return Err(Invalid(format!(
                    "{name}.primitive must be a sustained primitive, got {}",
                    cue.primitive.label()
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_shipped_haptics_file_loads_and_validates() {
        assert_eq!(HAPTICS.get().validate(), Ok(()));
    }
}
