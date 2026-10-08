//! The grapple hand's continuous vibration: the tethers' tension while they are squeezed, and the
//! reel's speed while the line carries the character. Pure; the input tick sends it as a short
//! pulse that the next tick refreshes.

use crate::grapple::physical::config::GrappleHapticsConfig;

/// What the vibration follows on a tick.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HapticInputs {
    /// The highest tether tension fraction, `0..=1`, while the tethers are squeezed; zero
    /// otherwise.
    pub squeeze_tension: f32,
    /// The reel's speed over its top speed, while reeling in; zero otherwise.
    pub reel_fraction: f32,
}

/// The amplitude below which nothing is sent.
pub const MIN_AMPLITUDE: f32 = 0.02;

/// The grapple hand's amplitude: the stronger of the tension's and the reel's, each in proportion
/// to its input, in `0..=1`.
pub fn amplitude(inputs: HapticInputs, config: &GrappleHapticsConfig) -> f32 {
    if !config.enabled {
        return 0.0;
    }
    let part = |value: f32, gain: f32| {
        if value.is_finite() {
            value.clamp(0.0, 1.0) * gain
        } else {
            0.0
        }
    };
    part(inputs.squeeze_tension, config.tension_amplitude)
        .max(part(inputs.reel_fraction, config.reel_amplitude))
        .clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_amplitude_follows_the_stronger_input() {
        let config = GrappleHapticsConfig::new();
        assert_eq!(amplitude(HapticInputs::default(), &config), 0.0);
        let tension = amplitude(
            HapticInputs {
                squeeze_tension: 0.5,
                reel_fraction: 0.1,
            },
            &config,
        );
        assert!((tension - 0.5 * config.tension_amplitude).abs() < 1e-6);
        let reel = amplitude(
            HapticInputs {
                squeeze_tension: 0.0,
                reel_fraction: 3.0,
            },
            &config,
        );
        assert_eq!(reel, config.reel_amplitude);
        let off = GrappleHapticsConfig {
            enabled: false,
            ..config
        };
        assert_eq!(
            amplitude(
                HapticInputs {
                    squeeze_tension: 1.0,
                    reel_fraction: 1.0
                },
                &off
            ),
            0.0
        );
        assert_eq!(
            amplitude(
                HapticInputs {
                    squeeze_tension: f32::NAN,
                    reel_fraction: 0.0
                },
                &config
            ),
            0.0
        );
    }
}
