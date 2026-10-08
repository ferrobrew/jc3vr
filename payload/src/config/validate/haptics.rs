//! Checks for the controller haptics: the strength, the shot's tuning, the vocabulary's shapes, and
//! the mixer's rules.

use crate::{
    config::validate::{Invalid, nonnegative, positive, range},
    controllers::haptics::{
        HapticsConfig,
        mixer::MixConfig,
        vocabulary::{Primitive, Shape},
    },
};

pub(super) fn haptics(h: &HapticsConfig) -> Result<(), Invalid> {
    range("haptics.intensity", h.intensity, 0.0, 1.0)?;
    nonnegative(
        "haptics.kick_amplitude_per_rad",
        h.kick_amplitude_per_rad,
        100.0,
    )?;
    nonnegative("haptics.max_shot_s", h.max_shot_s, 10.0)?;
    for primitive in Primitive::ALL {
        shape(
            &format!("haptics.vocabulary.{}", primitive.label()),
            h.vocabulary.shape(primitive),
        )?;
    }
    mix(&h.mix)
}

fn shape(name: &str, shape: &Shape) -> Result<(), Invalid> {
    range(&format!("{name}.soft"), shape.soft, 0.0, 1.0)?;
    range(&format!("{name}.full"), shape.full, 0.0, 1.0)?;
    if shape.soft > shape.full {
        return Err(Invalid(format!("{name}.soft exceeds full")));
    }
    positive(&format!("{name}.duration_s"), shape.duration_s, 2.0)?;
    range(
        &format!("{name}.frequency_hz"),
        shape.frequency_hz,
        0.0,
        1000.0,
    )
}

fn mix(m: &MixConfig) -> Result<(), Invalid> {
    range("haptics.mix.sustain_ceiling", m.sustain_ceiling, 0.0, 1.0)?;
    range("haptics.mix.sustain_knee", m.sustain_knee, 0.0, 1.0)?;
    if m.sustain_knee > m.sustain_ceiling {
        return Err(Invalid(
            "haptics.mix.sustain_knee exceeds sustain_ceiling".into(),
        ));
    }
    range("haptics.mix.knee", m.knee, 0.0, 1.0)?;
    nonnegative("haptics.mix.stack_share", m.stack_share, 2.0)?;
    nonnegative("haptics.mix.duck_share", m.duck_share, 2.0)?;
    positive("haptics.mix.refresh_s", m.refresh_s, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_pass() {
        assert_eq!(haptics(&HapticsConfig::new()), Ok(()));
    }

    #[test]
    fn a_soft_end_above_the_full_one_names_the_primitive() {
        let mut config = HapticsConfig::new();
        config.vocabulary.thud.soft = 1.0;
        config.vocabulary.thud.full = 0.5;
        assert_eq!(
            haptics(&config),
            Err(Invalid("haptics.vocabulary.thud.soft exceeds full".into()))
        );
    }

    #[test]
    fn a_knee_above_its_ceiling_is_refused() {
        let mut config = HapticsConfig::new();
        config.mix.sustain_knee = 0.9;
        assert!(haptics(&config).is_err());
    }
}
