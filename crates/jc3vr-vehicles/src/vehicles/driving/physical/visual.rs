//! The virtual aircraft controls: each handle's line and grip, and a lever's full travel rail.
//! Drawn whether or not the vehicle models the control, so the input's motion is always visible.

use std::time::Duration;

use glam::Vec3;
use jc3vr_body::controllers::grab::cockpit::ControlKind;
use jc3vr_input::frame::Hand;
use jc3vr_view::controllers::{
    cockpit::CockpitTransform,
    overlay::{
        primitive::{Anchor, DepthMode, Facing, Primitive, Rgba, Shape, Source, SpriteSize},
        store,
    },
};

use crate::vehicles::driving::physical::{ControlState, Placement, yoke::YokeFrame};

pub(crate) fn submit(
    placement: &Placement,
    state: &ControlState,
    kinds: &[ControlKind],
    xf: &CockpitTransform,
) {
    let mut primitives = Vec::new();
    if xf.is_usable() {
        for &kind in kinds {
            let held = state.holds.iter().flatten().any(|hold| hold.kind == kind);
            let color = if held { HELD } else { HANDLE };
            if kind == ControlKind::Yoke {
                if let Some(frame) = placement.yoke() {
                    yoke(&mut primitives, xf, &frame, state, color);
                }
                continue;
            }
            let Some(control) = geometry(placement, state, kind) else {
                continue;
            };
            let mut line = |from, to, width, color| {
                primitives.push(Primitive::Line {
                    from: Anchor::Head(xf.point_from_head(from)),
                    to: Anchor::Head(xf.point_from_head(to)),
                    width: width * xf.scale(),
                    color,
                    depth: DEPTH,
                });
            };
            line(control.base, control.handle, 0.018, SHAFT);
            line(
                control.handle - control.grip_half,
                control.handle + control.grip_half,
                0.042,
                color,
            );
            if let Some((low, high)) = control.rail {
                line(low, high, 0.012, SHAFT);
                for stop in [low, high] {
                    line(
                        stop - Vec3::X * 0.025,
                        stop + Vec3::X * 0.025,
                        0.012,
                        HANDLE,
                    );
                }
                if kind == ControlKind::Throttle {
                    let (back, front) = placement.lever_range(kind);
                    let share = |value: f32| (value - back) / (front - back);
                    let at = low.lerp(high, share(0.0));
                    line(at - Vec3::X * 0.035, at + Vec3::X * 0.035, 0.018, HANDLE);
                    if let Some(gate) = placement.afterburner_mark() {
                        let at = low.lerp(high, share(gate));
                        line(at - Vec3::X * 0.025, at + Vec3::X * 0.025, 0.015, GATE);
                    }
                }
            }
            for at in [
                control.handle - control.grip_half,
                control.handle + control.grip_half,
            ] {
                primitives.push(Primitive::Sprite {
                    at: Anchor::Head(xf.point_from_head(at)),
                    size: SpriteSize::World(0.042 * xf.scale()),
                    facing: Facing::Billboard,
                    shape: Shape::Disc,
                    color,
                    depth: DEPTH,
                });
            }
        }
    }
    store::submit(Source::AircraftControls, &mut primitives, TTL);
}

/// The yoke's column, its wheel's bar, and its horns, turned and pulled as it is.
fn yoke(
    primitives: &mut Vec<Primitive>,
    xf: &CockpitTransform,
    frame: &YokeFrame,
    state: &ControlState,
    color: Rgba,
) {
    let (roll, pitch) = (state.yoke.roll, state.yoke.pitch);
    let hub = frame.hub_at(pitch);
    let up = frame.turn(roll) * frame.up();
    let mut line = |from: Vec3, to: Vec3, width: f32, color| {
        primitives.push(Primitive::Line {
            from: Anchor::Head(xf.point_from_head(from)),
            to: Anchor::Head(xf.point_from_head(to)),
            width: width * xf.scale(),
            color,
            depth: DEPTH,
        });
    };
    line(hub - frame.axis * YOKE_COLUMN_M, hub, 0.018, SHAFT);
    let [left, right] = Hand::BOTH.map(|hand| frame.horn(hand, roll, pitch));
    let foot = up * frame.grip_height;
    line(left - foot, right - foot, 0.018, SHAFT);
    for horn in [left, right] {
        line(horn - foot, horn + up * 0.035, 0.032, color);
    }
    for horn in [left, right] {
        primitives.push(Primitive::Sprite {
            at: Anchor::Head(xf.point_from_head(horn)),
            size: SpriteSize::World(0.036 * xf.scale()),
            facing: Facing::Billboard,
            shape: Shape::Disc,
            color,
            depth: DEPTH,
        });
    }
}

pub(crate) fn clear() {
    store::clear(Source::AircraftControls);
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Geometry {
    handle: Vec3,
    grip_half: Vec3,
    base: Vec3,
    rail: Option<(Vec3, Vec3)>,
}

fn geometry(placement: &Placement, state: &ControlState, kind: ControlKind) -> Option<Geometry> {
    let handle = placement.grab_point(state, kind, Hand::Right, Vec3::ZERO)?;
    let control = if let Some(spec) = placement.stick(kind) {
        Geometry {
            handle,
            grip_half: Vec3::Y * 0.045,
            base: placement.stick_top(kind, &spec) - Vec3::Y * STICK_SHAFT_M,
            rail: None,
        }
    } else {
        let (_, spec) = placement.lever(kind)?;
        let rest = placement.lever_rest(kind, &spec);
        let support = match kind {
            ControlKind::Collective => Vec3::NEG_Z * 0.18,
            _ => Vec3::NEG_Y * 0.08,
        };
        let half = placement.lever_direction(kind, &spec) * spec.travel_m * 0.5;
        let (back, front) = placement.lever_range(kind);
        Geometry {
            handle,
            grip_half: match kind {
                ControlKind::Collective => Vec3::Z * 0.05,
                _ => Vec3::X * 0.05,
            },
            base: handle + support,
            rail: Some((rest + half * back + support, rest + half * front + support)),
        }
    };
    (control.handle.is_finite() && control.base.is_finite()).then_some(control)
}

const TTL: Duration = Duration::from_millis(150);
/// How much of the yoke's column is drawn ahead of its hub, metres.
const YOKE_COLUMN_M: f32 = 0.2;
/// How far below a stick's top its shaft is drawn, metres.
const STICK_SHAFT_M: f32 = 0.24;
const DEPTH: DepthMode = DepthMode::XRay {
    occluded_alpha: 0.3,
};
const SHAFT: Rgba = [0.24, 0.29, 0.34, 1.0];
const HANDLE: Rgba = [0.7, 0.83, 0.95, 1.0];
const HELD: Rgba = [0.35, 1.0, 0.6, 1.0];
const GATE: Rgba = [1.0, 0.55, 0.15, 1.0];

#[cfg(test)]
mod tests {
    use glam::{Mat4, Quat, Vec2};

    use super::*;
    use crate::vehicles::{
        driving::{
            physical::{
                Hold, StepInputs, anchor_point,
                native::{CockpitFit, NativeStick, asset as native_asset, cockpit_asset},
                step,
            },
            wheel_render::RenderWheel,
        },
        tuning::PhysicalControlsTuning,
    };

    #[test]
    fn moving_handles_share_the_grab_centres_and_hand_anchors_for_both_sides() {
        for dominant in Hand::BOTH {
            let placement = Placement::new(PhysicalControlsTuning::shipped(), dominant);
            for kind in [
                ControlKind::Cyclic,
                ControlKind::FlightStick,
                ControlKind::Collective,
                ControlKind::Throttle,
            ] {
                for value in [-1.0, 0.0, 1.0] {
                    let mut state = ControlState {
                        stick: Vec2::new(0.06 * value, 0.08 * value),
                        // Within the throttle's travel, a third of which is behind its rest.
                        levers: [value, value * 0.6],
                        ..ControlState::default()
                    };
                    let control = geometry(&placement, &state, kind).unwrap();
                    let offset = Vec3::new(0.01, 0.02, -0.01);
                    let hold = Hold {
                        kind,
                        offset,
                        orientation: None,
                    };
                    state.holds[dominant.index()] = Some(hold);
                    let mut grips = [None; 2];
                    grips[dominant.index()] = Some(control.handle + offset);
                    let anchor = anchor_point(&state, &placement, hold, dominant, grips).unwrap();
                    assert!((anchor - offset - control.handle).length() < 1e-5);
                    for candidate in crate::vehicles::driving::physical::candidates(
                        &placement,
                        &state,
                        &[kind],
                        grips,
                    ) {
                        assert_eq!(candidate.centre, control.handle);
                    }
                    assert!((control.grip_half.length() * 2.0) > 0.08);
                    assert!(control.grip_half.length() < placement.config.grab_radius_m);
                }
            }
        }
    }

    #[test]
    fn grabbing_the_visible_handle_moves_it_with_the_output_and_release() {
        for kind in [
            ControlKind::FlightStick,
            ControlKind::Cyclic,
            ControlKind::Throttle,
            ControlKind::Collective,
        ] {
            let placement = Placement::new(PhysicalControlsTuning::shipped(), Hand::Right);
            let mut state = ControlState::default();
            let start = geometry(&placement, &state, kind).unwrap().handle;
            let mut inputs = StepInputs {
                kinds: &[kind],
                grips: [None, Some(start)],
                orientations: [None; 2],
                held: [false, true],
                grabs: [None, Some(kind)],
                stick_steers: false,
                stick_moves: false,
                lever_moves: false,
                dt: 1.0 / 30.0,
            };
            let mut events = Vec::new();
            step(&mut state, &placement, &inputs, &mut events);
            assert_eq!(state.holds[1].unwrap().kind, kind);
            assert_eq!(geometry(&placement, &state, kind).unwrap().handle, start);
            let movement = placement.lever(kind).map_or(Vec3::X * 0.05, |(_, spec)| {
                spec.axis.direction() * spec.travel_m * 0.25
            });
            inputs.grabs = [None; 2];
            inputs.grips[1] = Some(start + movement);
            let output = step(&mut state, &placement, &inputs, &mut events);
            let moved = geometry(&placement, &state, kind).unwrap().handle;
            assert!((moved - start - movement).length() < 1e-5);
            assert!(match kind {
                ControlKind::FlightStick => output.flight_stick.unwrap().x > 0.0,
                ControlKind::Cyclic => output.cyclic.unwrap().x > 0.0,
                ControlKind::Throttle => output.throttle.unwrap() > 0.0,
                ControlKind::Collective => output.collective.unwrap() > 0.0,
                _ => unreachable!(),
            });
            inputs.held = [false; 2];
            step(&mut state, &placement, &inputs, &mut events);
            assert_eq!(state.holds, [None; 2]);
            let released = geometry(&placement, &state, kind).unwrap().handle;
            match kind {
                ControlKind::Throttle => assert_eq!(released, moved),
                ControlKind::Collective => assert_eq!(released, start),
                _ => assert!((released - start).length() < (moved - start).length()),
            }
        }
    }

    #[test]
    fn measured_cockpits_draw_the_virtual_controls_at_their_measured_handles() {
        let utility = (
            Vec3::new(-0.800297, 0.6425335, -3.0365307),
            Vec3::new(0.9101837, 2.977942, 2.4216115),
        );
        let large = (Vec3::splat(-10.0), Vec3::splat(10.0));
        for (name, pilot, bounds) in [
            ("utility_body", Vec3::new(-0.35, 1.5, -0.92), utility),
            ("fighterjet_body", Vec3::new(0.0, 2.3, -2.6), large),
            ("fighterbomber_body", Vec3::new(0.0, 2.9, -2.762207), large),
        ] {
            let asset = cockpit_asset(name, pilot).unwrap();
            let fit = CockpitFit::new(
                RenderWheel {
                    vehicle: 1,
                    part_index: 0,
                    properties: 1,
                    hub: Vec3::ZERO,
                    axis: Vec3::Y,
                },
                Mat4::from_translation(-pilot),
                asset,
                bounds,
                &PhysicalControlsTuning::shipped(),
            );
            let mut placement = Placement::new(PhysicalControlsTuning::shipped(), Hand::Right);
            placement.cockpit = Some(fit);
            let state = ControlState::default();
            let stick = geometry(&placement, &state, asset.stick_kind).unwrap();
            assert!((stick.handle - fit.stick_top()).length() < 1e-5);
            assert!((stick.base - fit.stick_top() + Vec3::Y * STICK_SHAFT_M).length() < 1e-5);
            assert!(stick.rail.is_none());
            let lever_kind = match asset.stick_kind {
                ControlKind::Cyclic => ControlKind::Collective,
                ControlKind::FlightStick => ControlKind::Throttle,
                _ => unreachable!(),
            };
            let (_, spec) = placement.lever(lever_kind).unwrap();
            let lever = geometry(&placement, &state, lever_kind).unwrap();
            let rest = fit.lever_rest(lever_kind).unwrap();
            assert!((lever.handle - rest).length() < 1e-5);
            let (low, high) = lever.rail.unwrap();
            assert!(((high - low).length() - spec.travel_m).abs() < 1e-5);
            let (_, part_rest) = fit.fallback_lever.or(fit.asset.lever).unwrap();
            assert!(part_rest.cmpge(bounds.0).all() && part_rest.cmple(bounds.1).all());
            let moved = geometry(
                &placement,
                &ControlState {
                    stick: Vec2::new(0.04, -0.03),
                    levers: [0.5; 2],
                    ..ControlState::default()
                },
                lever_kind,
            )
            .unwrap();
            assert!((moved.rail.unwrap().0 - low).length() < 1e-5);
            assert!(
                (moved.handle
                    - lever.handle
                    - placement.lever_direction(lever_kind, &spec) * (spec.travel_m * 0.25))
                    .length()
                    < 1e-5
            );
            let pushed = geometry(
                &placement,
                &ControlState {
                    stick: Vec2::new(0.04, -0.03),
                    ..ControlState::default()
                },
                asset.stick_kind,
            )
            .unwrap();
            assert!(
                (pushed.handle
                    - stick.handle
                    - fit.direction(Vec3::X) * 0.04
                    - fit.direction(Vec3::Z) * -0.03)
                    .length()
                    < 1e-5
            );
        }
    }

    #[test]
    fn native_sticks_keep_the_virtual_overlay_on_the_measured_handle() {
        for name in ["lightassault_joystick", "smallfloatprop_controlwheel"] {
            let (kind, pivot, grip) = native_asset(name).unwrap();
            let native = NativeStick {
                kind,
                target: RenderWheel {
                    vehicle: 1,
                    part_index: 2,
                    properties: 3,
                    hub: pivot,
                    axis: Vec3::Y,
                },
                part_to_cockpit: Mat4::from_rotation_translation(
                    Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                    Vec3::new(0.3, 1.2, -0.4),
                ),
                pivot,
                grip,
            };
            let mut placement = Placement::new(PhysicalControlsTuning::shipped(), Hand::Right);
            placement.native_stick = Some(native);
            let state = ControlState {
                stick: Vec2::new(0.04, -0.03),
                ..ControlState::default()
            };
            let control = geometry(&placement, &state, kind).unwrap();
            assert!((control.handle - native.handle(state.stick)).length() < 1e-5);
            assert!((control.base - native.top() + Vec3::Y * STICK_SHAFT_M).length() < 1e-5);
            assert!(control.rail.is_none());
            let lever_kind = match kind {
                ControlKind::Cyclic => ControlKind::Collective,
                ControlKind::FlightStick => ControlKind::Throttle,
                _ => unreachable!(),
            };
            let lever = geometry(&placement, &state, lever_kind).unwrap();
            let (_, spec) = placement.lever(lever_kind).unwrap();
            let (low, high) = lever.rail.unwrap();
            assert!(((high - low).length() - spec.travel_m).abs() < 1e-5);
        }
    }

    #[test]
    fn sticks_keep_their_base_and_levers_reach_the_drawn_stops() {
        let placement = Placement::new(PhysicalControlsTuning::shipped(), Hand::Left);
        for kind in [ControlKind::Cyclic, ControlKind::FlightStick] {
            let rest = geometry(&placement, &ControlState::default(), kind).unwrap();
            let pushed = geometry(
                &placement,
                &ControlState {
                    stick: Vec2::new(0.06, 0.08),
                    ..ControlState::default()
                },
                kind,
            )
            .unwrap();
            assert_eq!(rest.base, pushed.base);
            assert!((pushed.handle - rest.handle - Vec3::new(0.06, 0.0, 0.08)).length() < 1e-5);
        }
        for kind in [ControlKind::Collective, ControlKind::Throttle] {
            let rest = geometry(&placement, &ControlState::default(), kind).unwrap();
            let (low, high) = rest.rail.unwrap();
            let (_, spec) = placement.lever(kind).unwrap();
            assert!(((high - low).length() - spec.travel_m).abs() < 1e-5);
            let (back, front) = placement.lever_range(kind);
            for (value, stop) in [(back, low), (front, high)] {
                let end = geometry(
                    &placement,
                    &ControlState {
                        levers: [value; 2],
                        ..ControlState::default()
                    },
                    kind,
                )
                .unwrap();
                assert!((end.base - stop).length() < 1e-5);
                assert!((end.handle - end.base - (rest.handle - rest.base)).length() < 1e-5);
            }
        }
        for kind in [ControlKind::Wheel, ControlKind::Handlebars] {
            assert!(geometry(&placement, &ControlState::default(), kind).is_none());
        }
    }
}
