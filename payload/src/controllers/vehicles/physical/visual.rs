use std::time::Duration;

use glam::Vec3;

use crate::controllers::{
    cockpit::CockpitTransform,
    frame::Hand,
    overlay::{
        primitive::{Anchor, DepthMode, Facing, Primitive, Rgba, Shape, Source, SpriteSize},
        store,
    },
    vehicles::physical::{ControlKind, ControlState, Placement},
};

pub(crate) fn submit(
    placement: &Placement,
    state: &ControlState,
    kinds: &[ControlKind],
    xf: &CockpitTransform,
) {
    let mut primitives = Vec::new();
    if xf.is_usable() {
        for &kind in kinds {
            let Some(control) = geometry(placement, state, kind) else {
                continue;
            };
            let held = state.holds.iter().flatten().any(|hold| hold.kind == kind);
            let color = if held { HELD } else { HANDLE };
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
                if let Some(gate) = placement.gate(kind) {
                    let at = low.lerp(high, (gate + 1.0) * 0.5);
                    line(at - Vec3::X * 0.025, at + Vec3::X * 0.025, 0.015, GATE);
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
            base: placement.mirror(spec.top_m) - Vec3::Y * 0.24,
            rail: None,
        }
    } else {
        let (_, spec) = placement.lever(kind)?;
        let rest = placement.mirror(spec.rest_m);
        let support = match kind {
            ControlKind::Collective => Vec3::NEG_Z * 0.18,
            _ => Vec3::NEG_Y * 0.08,
        };
        let half = spec.axis.direction() * spec.travel_m * 0.5;
        Geometry {
            handle,
            grip_half: match kind {
                ControlKind::Collective => Vec3::Z * 0.05,
                _ => Vec3::X * 0.05,
            },
            base: handle + support,
            rail: Some((rest - half + support, rest + half + support)),
        }
    };
    (control.handle.is_finite() && control.base.is_finite()).then_some(control)
}

const TTL: Duration = Duration::from_millis(150);
const DEPTH: DepthMode = DepthMode::XRay {
    occluded_alpha: 0.3,
};
const SHAFT: Rgba = [0.24, 0.29, 0.34, 1.0];
const HANDLE: Rgba = [0.7, 0.83, 0.95, 1.0];
const HELD: Rgba = [0.35, 1.0, 0.6, 1.0];
const GATE: Rgba = [1.0, 0.55, 0.15, 1.0];

#[cfg(test)]
mod tests {
    use glam::Vec2;

    use super::*;
    use crate::controllers::vehicles::{
        physical::{Hold, StepInputs, anchor_point, step},
        tuning::PHYSICAL,
    };

    #[test]
    fn moving_handles_share_the_grab_centres_and_hand_anchors_for_both_sides() {
        for dominant in Hand::BOTH {
            let placement = Placement::new(PHYSICAL, dominant);
            for kind in [
                ControlKind::Cyclic,
                ControlKind::FlightStick,
                ControlKind::Collective,
                ControlKind::Throttle,
            ] {
                for value in [-1.0, 0.0, 1.0] {
                    let mut state = ControlState {
                        stick: Vec2::new(0.06 * value, 0.08 * value),
                        levers: [value; 2],
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
                    for candidate in crate::controllers::vehicles::physical::candidates(
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
            let placement = Placement::new(PHYSICAL, Hand::Right);
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
    fn sticks_keep_their_base_and_levers_reach_the_drawn_stops() {
        let placement = Placement::new(PHYSICAL, Hand::Left);
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
            for (value, stop) in [(-1.0, low), (1.0, high)] {
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
