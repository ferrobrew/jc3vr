//! Checks for the physical grapple.

use crate::{
    config::validate::{Invalid, nonnegative, positive, pulse, range},
    grapple::physical::PhysicalGrappleConfig,
};

/// Check the physical grapple's settings. Paths are relative to `controllers.grapple`.
pub(super) fn grapple(c: &PhysicalGrappleConfig) -> Result<(), Invalid> {
    nonnegative("grapple.pull.speed_hold_s", c.pull.speed_hold_s, 60.0)?;
    nonnegative("grapple.pull.relax_mps", c.pull.relax_mps, 100.0)?;
    positive("grapple.reel.min_scale", c.reel.min_scale, 10.0)?;
    positive("grapple.reel.max_scale", c.reel.max_scale, 10.0)?;
    if c.reel.max_scale < c.reel.min_scale {
        return Err(Invalid(
            "grapple.reel.max_scale must not be below grapple.reel.min_scale".into(),
        ));
    }
    positive("grapple.reel.full_speed_mps", c.reel.full_speed_mps, 100.0)?;
    nonnegative("grapple.reel.boost_s", c.reel.boost_s, 60.0)?;
    nonnegative("grapple.tether.deadzone_m", c.tether.deadzone_m, 10.0)?;
    positive("grapple.tether.full_m", c.tether.full_m, 10.0)?;
    if c.tether.full_m <= c.tether.deadzone_m {
        return Err(Invalid(
            "grapple.tether.full_m must exceed grapple.tether.deadzone_m".into(),
        ));
    }
    nonnegative("grapple.swing.deadzone_mps", c.swing.deadzone_mps, 100.0)?;
    positive("grapple.swing.full_mps", c.swing.full_mps, 100.0)?;
    if c.swing.full_mps <= c.swing.deadzone_mps {
        return Err(Invalid(
            "grapple.swing.full_mps must exceed grapple.swing.deadzone_mps".into(),
        ));
    }
    range("grapple.swing.scale", c.swing.scale, -10.0, 10.0)?;
    nonnegative("grapple.swing.max_scale", c.swing.max_scale, 10.0)?;
    nonnegative("grapple.swing.smoothing_s", c.swing.smoothing_s, 10.0)?;
    range(
        "grapple.haptics.tension_amplitude",
        c.haptics.tension_amplitude,
        0.0,
        1.0,
    )?;
    range(
        "grapple.haptics.reel_amplitude",
        c.haptics.reel_amplitude,
        0.0,
        1.0,
    )?;
    positive("grapple.haptics.pulse_s", c.haptics.pulse_s, 1.0)?;
    pulse("grapple.haptics.retract", &c.haptics.retract)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_pass_and_inverted_spans_fail() {
        let defaults = PhysicalGrappleConfig::new();
        assert!(grapple(&defaults).is_ok());
        let mut reel = defaults;
        reel.reel.max_scale = reel.reel.min_scale * 0.5;
        assert!(grapple(&reel).is_err());
        let mut tether = defaults;
        tether.tether.full_m = tether.tether.deadzone_m;
        assert!(grapple(&tether).is_err());
        let mut swing = defaults;
        swing.swing.full_mps = 0.0;
        assert!(grapple(&swing).is_err());
    }
}
