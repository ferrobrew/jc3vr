//! The tether retract by pulling: with tethers attached, pulling the grapple hand back drives the
//! game's own retract input (`RETRACT_GRAPPLE`) as an analog value, and the game squeezes the
//! tethers as it does for the gamepad's trigger. Pure.
//!
//! The game squeezes by the cube of the retract value
//! ([`GrapplingHook::UpdateDualTetherHandling`]), which suits a trigger's travel. The hand's pull
//! is turned into the value whose cube is the pull's fraction ([`retract_value`]), so that the
//! squeeze follows the pull's depth in proportion. The game's own ramp of the squeeze over the
//! retract's hold time still applies on top.
//!
//! [`GrapplingHook::UpdateDualTetherHandling`]: jc3gi::equipment::grappling_hook::GrapplingHook::UpdateDualTetherHandling

use jc3vr_body::controllers::mode::{AirborneVariant, Detection, Mode, OnFootVariant};

use crate::grapple::physical::config::TetherTuning;

/// What the tether retract needs to know of the tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TetherTick {
    /// The dual tethers the hook holds.
    pub tethers: u32,
    /// Whether the grapple has a line of its own: a line to an object pulls it on the retract
    /// input instead, and a line still being placed belongs to the tether that is not formed yet.
    pub active_wire: bool,
    /// The grapple button is held, independently of the throttle and weapon triggers.
    pub button_held: bool,
    pub detection: Detection,
}

/// Whether the pull may retract the tethers: tethers are attached, the grapple
/// has no line of its own, nothing carries the character along a line, and the character is on
/// the ground, falling, or seated in a vehicle, with the grapple button held.
/// The wingsuit and the parachute are left out: the retract input boosts
/// them too, and their arm steering and handles already use the hands.
pub fn armed(tick: &TetherTick) -> bool {
    tick.button_held
        && tick.tethers > 0
        && !tick.active_wire
        && tick.detection.traversal.is_none()
        && matches!(
            tick.detection.mode,
            Mode::OnFoot(OnFootVariant::Ground)
                | Mode::Airborne(AirborneVariant::Falling)
                | Mode::Vehicle(_)
        )
}

/// The retract value for a pull `depth_m` metres deep: zero inside the deadzone, then the cube
/// root of the pull's fraction of the span to [`TetherTuning::full_m`], so that the game's cube
/// of it is the fraction itself.
pub fn retract_value(depth_m: f32, config: &TetherTuning) -> f32 {
    if !depth_m.is_finite() || depth_m <= config.deadzone_m {
        return 0.0;
    }
    let span = (config.full_m - config.deadzone_m).max(1e-3);
    ((depth_m - config.deadzone_m) / span)
        .clamp(0.0, 1.0)
        .cbrt()
}

#[cfg(test)]
mod tests {
    use glam::Vec3;
    use jc3vr_body::controllers::{
        intent::yank::HandLine,
        mode::{Traversal, VehicleClass, VehicleSeat},
    };

    use super::*;
    use crate::grapple::physical::{config::GRAPPLE, pull::LinePull};

    fn tick(tethers: u32) -> TetherTick {
        TetherTick {
            tethers,
            active_wire: false,
            button_held: true,
            detection: Detection {
                mode: Mode::OnFoot(OnFootVariant::Ground),
                ..Default::default()
            },
        }
    }

    #[test]
    fn the_retract_requires_free_tethers_and_an_allowed_mode() {
        assert!(armed(&tick(1)));
        assert!(!armed(&tick(0)));
        assert!(!armed(&TetherTick {
            active_wire: true,
            ..tick(2)
        }));
        let mut falling = tick(1);
        falling.detection.mode = Mode::Airborne(AirborneVariant::Falling);
        assert!(armed(&falling));
        for mode in [
            Mode::Airborne(AirborneVariant::Wingsuit),
            Mode::Airborne(AirborneVariant::Parachute),
            Mode::OnFoot(OnFootVariant::Swimming),
            Mode::Ui,
        ] {
            let mut t = tick(1);
            t.detection.mode = mode;
            assert!(!armed(&t), "{mode:?}");
        }
        let mut reeling = tick(1);
        reeling.detection.traversal = Some(Traversal::ReelingIn);
        assert!(!armed(&reeling));
    }

    #[test]
    fn vehicle_pulls_require_the_grapple_button_and_rebase_after_release() {
        for class in [
            VehicleClass::Car,
            VehicleClass::Motorcycle,
            VehicleClass::Sea,
            VehicleClass::Helicopter,
            VehicleClass::Plane,
            VehicleClass::Mech,
            VehicleClass::Unknown,
        ] {
            for driver in [false, true] {
                let mut tick = tick(2);
                tick.detection.mode = Mode::Vehicle(VehicleSeat { class, driver });
                let mut pull = LinePull::new();
                let mut sample = |held, z| {
                    tick.button_held = held;
                    let reading = pull.step(
                        1.0 / 30.0,
                        Some(HandLine {
                            grip: Vec3::new(0.0, 0.0, z),
                            aim: Vec3::NEG_Z,
                        }),
                        armed(&tick),
                        &GRAPPLE.pull,
                    );
                    retract_value(reading.depth_m, &GRAPPLE.tether)
                };
                assert_eq!(sample(false, -0.6), 0.0);
                assert_eq!(sample(false, -0.3), 0.0);
                assert_eq!(sample(true, -0.6), 0.0);
                assert!(sample(true, -0.3) > 0.0, "{class:?}, driver={driver}");
                assert_eq!(sample(false, -0.3), 0.0);
                assert_eq!(sample(true, -0.3), 0.0);
                tick.active_wire = true;
                assert!(!armed(&tick));
                tick.active_wire = false;
                tick.detection.traversal = Some(Traversal::ReelingIn);
                assert!(!armed(&tick));
            }
        }
    }

    #[test]
    fn the_squeeze_follows_the_pull_in_proportion() {
        let config = GRAPPLE.tether;
        assert_eq!(retract_value(0.0, &config), 0.0);
        assert_eq!(retract_value(config.deadzone_m, &config), 0.0);
        assert_eq!(retract_value(f32::NAN, &config), 0.0);
        let half = (config.deadzone_m + config.full_m) / 2.0;
        let squeeze = retract_value(half, &config).powi(3);
        assert!((squeeze - 0.5).abs() < 1e-4, "{squeeze}");
        assert_eq!(retract_value(10.0, &config), 1.0);
    }
}
