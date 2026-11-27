//! The sticks' motion: a stick's top follows the holding hand across the horizontal plane within
//! its travel, and let go, a centring spring returns it.
//!
//! The grab is relative: the hand's offset from the top is taken when it grabs, wherever the top is
//! then, so a hand that takes the stick off-centre, or takes it again on its way back to centre,
//! moves it only by moving. The flight stick and the cyclic also work the yaw by their twist about
//! the shaft, measured from the hand's orientation when it took the stick, so a wrist held at any
//! angle starts from no rudder.

use body_rig::math::{
    axis::{past_deadzone, radial_past_deadzone},
    rotation::twist_angle,
};
use glam::{Quat, Vec2, Vec3};
use jc3vr_body::controllers::grab::cockpit::ControlKind;
use jc3vr_input::frame::Hand;

use crate::controllers::vehicles::{
    physical::{
        ControlState, Holding, Placement, cues,
        cues::{ControlCue, TravelCues},
    },
    tuning::{FlightControlsTuning, StickSpec, VEHICLES},
};

/// A stick's deflection (+X right, +Y back) for its top pulled to `top_now`, limited to the full
/// deflection.
pub fn stick_deflection(spec: &StickSpec, top: Vec3, top_now: Vec3) -> Vec2 {
    let d = top_now - top;
    let flat = Vec2::new(d.x, d.z);
    if !flat.is_finite() {
        return Vec2::ZERO;
    }
    flat.clamp_length_max(spec.max_deflection_m.max(0.0))
}

/// What a stick deflected by `deflection` outputs (+X rolls right, +Y pushes forward): its share of
/// the full deflection past the deadzone, rescaled so that it still reaches one.
pub(crate) fn stick_value(spec: &StickSpec, deflection: Vec2) -> Vec2 {
    let max = spec.max_deflection_m.max(1e-3);
    let share = Vec2::new(deflection.x, -deflection.y) / max;
    radial_past_deadzone(share, spec.deadzone.clamp(0.0, 0.9), 1.0)
}

/// The released stick's deflection after `dt` seconds of its centring spring.
pub(crate) fn spring_back(spec: &StickSpec, deflection: Vec2, dt: f32) -> Vec2 {
    if spec.return_s <= 0.0 || !deflection.is_finite() {
        return Vec2::ZERO;
    }
    let next = deflection * (-dt.max(0.0) / spec.return_s).exp();
    if next.length() < VEHICLES.get().controls.stick_settled_m {
        Vec2::ZERO
    } else {
        next
    }
}

/// The yaw, `-1..=1` with positive right, for a hand that took a stick at orientation `grab` and
/// holds it at `now`: its twist about the shaft (the cockpit frame's vertical), turning the top to
/// the right for a right yaw, past the deadzone and fully at the full twist.
pub(crate) fn twist_rudder(config: &FlightControlsTuning, grab: Quat, now: Quat) -> f32 {
    twist_rudder_about_axis(config, grab, now, Vec3::Y)
}

pub(super) fn deflection3(d: Vec2) -> Vec3 {
    Vec3::new(d.x, 0.0, d.y)
}

/// What a stick produced this tick.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(super) struct StickStep {
    pub(super) value: Option<Vec2>,
    pub(super) rudder: Option<f32>,
    pub(super) cue: Option<ControlCue>,
}

/// Move `kind`'s stick with the hand holding it, or let go, spring it back to centre, still writing
/// its value on the way so the vehicle comes back with it. A tick on which the tables write the
/// stick's axes (`tables_move`) snaps a returning stick to centre, so the thumbstick flies at once.
pub(super) fn step_stick(
    state: &mut ControlState,
    placement: &Placement,
    kind: ControlKind,
    holding: Holding,
    (orientations, tables_move): ([Option<Quat>; 2], bool),
    dt: f32,
) -> StickStep {
    let Some(spec) = placement.stick(kind) else {
        return StickStep::default();
    };
    let held = Hand::BOTH
        .into_iter()
        .find_map(|hand| holding[hand.index()].map(|(hold, point)| (hand, hold, point)));
    let Some((hand, hold, point)) = held else {
        if state.stick == Vec2::ZERO {
            return StickStep::default();
        }
        if tables_move {
            state.stick = Vec2::ZERO;
            state.stick_cues = TravelCues::new();
            return StickStep::default();
        }
        state.stick = spring_back(&spec, state.stick, dt);
        return StickStep {
            value: Some(stick_value(&spec, state.stick)),
            ..StickStep::default()
        };
    };
    state.stick = placement.stick_deflection(kind, &spec, point - hold.offset);
    let share = state.stick.length() / spec.max_deflection_m.max(1e-3);
    let flight = &placement.config.flight;
    let twistable = matches!(kind, ControlKind::FlightStick | ControlKind::Cyclic);
    let rudder = twistable
        .then(|| hold.orientation.zip(orientations[hand.index()]))
        .flatten()
        .map(|(grab, now)| {
            placement
                .cockpit
                .filter(|fit| fit.asset.stick_kind == kind)
                .map_or_else(
                    || twist_rudder(flight, grab, now),
                    |fit| twist_rudder_about_axis(flight, grab, now, fit.direction(Vec3::Y)),
                )
        });
    StickStep {
        value: Some(stick_value(&spec, state.stick)),
        rudder,
        cue: cues::stick_cue(&mut state.stick_cues, share),
    }
}

fn twist_rudder_about_axis(
    config: &FlightControlsTuning,
    grab: Quat,
    now: Quat,
    axis: Vec3,
) -> f32 {
    let twist = -twist_angle(now * grab.inverse(), axis);
    past_deadzone(
        twist,
        config.twist_deadzone_deg.to_radians(),
        config.twist_full_deg.to_radians(),
    )
}

#[cfg(test)]
mod tests {
    use glam::Mat4;

    use super::*;
    use crate::controllers::vehicles::{
        physical::{
            Hold, StepInputs,
            native::{CockpitAsset, CockpitFit, NativeStick},
            step,
        },
        tuning::PhysicalControlsTuning,
        wheel_render::RenderWheel,
    };

    fn spec() -> StickSpec {
        PhysicalControlsTuning::shipped().flight_stick
    }

    #[test]
    fn a_resting_hand_s_tremor_flies_nothing_and_full_deflection_is_full() {
        let spec = spec();
        let max = spec.max_deflection_m;
        assert_eq!(
            stick_value(&spec, Vec2::new(max * spec.deadzone * 0.5, 0.0)),
            Vec2::ZERO
        );
        let full = stick_value(&spec, Vec2::new(0.0, -max));
        assert!((full - Vec2::new(0.0, 1.0)).length() < 1e-5, "{full}");
        let half = stick_value(&spec, Vec2::new(max * 0.5, 0.0));
        assert!(half.x > 0.4 && half.x < 0.5, "{half}");
    }

    #[test]
    fn the_spring_brings_the_stick_back_to_centre() {
        let spec = spec();
        let mut d = Vec2::new(spec.max_deflection_m, 0.0);
        let after_one_tick = spring_back(&spec, d, 1.0 / 30.0);
        assert!(after_one_tick.x < d.x && after_one_tick.x > 0.0);
        for _ in 0..30 {
            d = spring_back(&spec, d, 1.0 / 30.0);
        }
        assert_eq!(d, Vec2::ZERO);
        let snap = StickSpec {
            return_s: 0.0,
            ..spec
        };
        assert_eq!(spring_back(&snap, Vec2::X, 0.01), Vec2::ZERO);
    }

    #[test]
    fn measured_stick_translation_and_rudder_follow_the_rotated_control_axes() {
        for kind in [ControlKind::FlightStick, ControlKind::Cyclic] {
            for hand in Hand::BOTH {
                for rotation in [
                    Quat::IDENTITY,
                    Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                    Quat::from_rotation_y(0.8) * Quat::from_rotation_z(1.1),
                ] {
                    let mut placement = Placement::new(PhysicalControlsTuning::shipped(), hand);
                    placement.cockpit = Some(CockpitFit::new(
                        target(),
                        Mat4::from_scale_rotation_translation(
                            Vec3::new(0.7, 1.2, 0.9),
                            rotation,
                            Vec3::new(2.0, -1.0, 4.0),
                        ),
                        CockpitAsset {
                            yoke: None,
                            seat: 0,
                            stick_kind: kind,
                            stick_grip: Vec3::new(0.0, 1.5, -3.0),
                            lever: None,
                        },
                        (Vec3::splat(-10.0), Vec3::splat(10.0)),
                        &PhysicalControlsTuning::shipped(),
                    ));
                    let spec = placement.stick(kind).unwrap();
                    let config = placement.config.flight;
                    let full = config.twist_full_deg.to_radians();
                    for (turn, expected) in [
                        (Quat::IDENTITY, 0.0),
                        (
                            Quat::from_rotation_y(-config.twist_deadzone_deg.to_radians() * 0.5),
                            0.0,
                        ),
                        (Quat::from_rotation_y(-full), 1.0),
                        (Quat::from_rotation_y(full), -1.0),
                        (Quat::from_rotation_x(0.6), 0.0),
                        (Quat::from_rotation_z(-0.6), 0.0),
                    ] {
                        let grab = Quat::from_rotation_x(0.4) * Quat::from_rotation_z(-0.3);
                        let offset = Vec3::new(0.01, -0.02, 0.03);
                        let deflection = Vec2::new(0.04, -0.03);
                        let point = placement.cockpit.unwrap().stick_handle(deflection) + offset;
                        let mut holding = [None; 2];
                        holding[hand.index()] = Some((
                            Hold {
                                kind,
                                offset,
                                orientation: Some(rotation * grab),
                            },
                            point,
                        ));
                        let mut orientations = [None; 2];
                        orientations[hand.index()] = Some(rotation * turn * grab);
                        let mut state = ControlState::default();
                        let out = step_stick(
                            &mut state,
                            &placement,
                            kind,
                            holding,
                            (orientations, false),
                            1.0 / 90.0,
                        );
                        assert!((state.stick - deflection).length() < 1e-5);
                        assert!(
                            (out.value.unwrap() - stick_value(&spec, deflection)).length() < 1e-5
                        );
                        assert!((out.rudder.unwrap() - expected).abs() < 1e-4);
                    }
                }
            }
        }
    }

    #[test]
    fn a_twisted_cyclic_and_flight_stick_both_yaw_through_the_step() {
        for kind in [ControlKind::FlightStick, ControlKind::Cyclic] {
            let placement = Placement::new(PhysicalControlsTuning::shipped(), Hand::Right);
            let spec = placement.stick(kind).unwrap();
            let top = placement.stick_top(kind, &spec);
            let grab = Quat::from_rotation_x(0.4);
            let turned = Quat::from_rotation_y(
                -PhysicalControlsTuning::shipped()
                    .flight
                    .twist_full_deg
                    .to_radians(),
            ) * grab;
            let mut state = ControlState::default();
            let mut events = Vec::new();
            let kinds = [kind];
            let inputs = StepInputs {
                kinds: &kinds,
                grips: [None, Some(top)],
                orientations: [None, Some(grab)],
                held: [false, true],
                grabs: [None, Some(kind)],
                stick_steers: false,
                stick_moves: false,
                lever_moves: false,
                dt: 1.0 / 90.0,
            };
            let out = step(&mut state, &placement, &inputs, &mut events);
            assert_eq!(out.rudder, Some(0.0));
            let inputs = StepInputs {
                orientations: [None, Some(turned)],
                grabs: [None; 2],
                ..inputs
            };
            let out = step(&mut state, &placement, &inputs, &mut events);
            assert!(
                out.rudder.is_some_and(|r| (r - 1.0).abs() < 1e-4),
                "{kind:?}: {out:?}"
            );
            match kind {
                ControlKind::Cyclic => assert!(out.cyclic.is_some()),
                ControlKind::FlightStick => assert!(out.flight_stick.is_some()),
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn fallback_and_native_sticks_keep_the_cockpit_vertical_twist_axis() {
        for kind in [ControlKind::FlightStick, ControlKind::Cyclic] {
            for native_stick in [
                None,
                Some(NativeStick {
                    kind,
                    target: target(),
                    part_to_cockpit: Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2),
                    pivot: Vec3::ZERO,
                    grip: Vec3::Y,
                }),
            ] {
                let mut placement = Placement::new(PhysicalControlsTuning::shipped(), Hand::Right);
                placement.native_stick = native_stick;
                let spec = placement.stick(kind).unwrap();
                let grab = Quat::from_rotation_x(0.4);
                let now = Quat::from_rotation_y(
                    -PhysicalControlsTuning::shipped()
                        .flight
                        .twist_full_deg
                        .to_radians(),
                ) * grab;
                let holding = [
                    None,
                    Some((
                        Hold {
                            kind,
                            offset: Vec3::ZERO,
                            orientation: Some(grab),
                        },
                        placement.stick_top(kind, &spec),
                    )),
                ];
                let out = step_stick(
                    &mut ControlState::default(),
                    &placement,
                    kind,
                    holding,
                    ([None, Some(now)], false),
                    1.0 / 90.0,
                );
                assert!((out.rudder.unwrap() - 1.0).abs() < 1e-4);
            }
        }
    }

    fn target() -> RenderWheel {
        RenderWheel {
            vehicle: 1,
            part_index: 2,
            properties: 3,
            hub: Vec3::ZERO,
            axis: Vec3::Y,
        }
    }

    #[test]
    fn twisting_the_top_right_is_right_rudder_past_the_deadzone() {
        let config = PhysicalControlsTuning::shipped().flight;
        let grab = Quat::from_rotation_x(0.4);
        let small = Quat::from_rotation_y(-(config.twist_deadzone_deg * 0.5).to_radians()) * grab;
        assert_eq!(twist_rudder(&config, grab, small), 0.0);
        // Turning the top clockwise seen from above, to the right.
        let right = Quat::from_rotation_y(-config.twist_full_deg.to_radians()) * grab;
        assert!((twist_rudder(&config, grab, right) - 1.0).abs() < 1e-4);
        let left = Quat::from_rotation_y(0.3) * grab;
        assert!(twist_rudder(&config, grab, left) < 0.0);
        // Tilting the wrist about the other axes is no rudder.
        let tilted = Quat::from_rotation_x(0.5) * grab;
        assert!(twist_rudder(&config, grab, tilted).abs() < 1e-4);
    }
}
