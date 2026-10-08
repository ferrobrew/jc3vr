//! Tests for the plane's flight stick and throttle.

use super::*;

fn plane() -> Placement {
    placement(|c| {
        c.flight_stick.enabled = true;
        c.throttle.enabled = true;
    })
}

const KINDS: [ControlKind; 2] = [ControlKind::FlightStick, ControlKind::Throttle];

/// A tick with the given hands' grips and orientations, every tracked hand's grip held.
fn step_oriented(
    state: &mut ControlState,
    placement: &Placement,
    grips: [Option<Vec3>; 2],
    orientations: [Option<Quat>; 2],
    grabs: [Option<ControlKind>; 2],
) -> Outputs {
    let mut events = Vec::new();
    step(
        state,
        placement,
        &StepInputs {
            kinds: &KINDS,
            grips,
            orientations,
            held: grips.map(|g| g.is_some()),
            grabs,
            stick_steers: false,
            stick_moves: false,
            lever_moves: false,
            dt: DT,
        },
        &mut events,
    )
}

#[test]
fn taking_a_returning_stick_does_not_jump_it() {
    let p = plane();
    let top = p.config.flight_stick.top_m;
    let mut state = ControlState::default();
    let grip = top + Vec3::new(0.02, 0.03, 0.0);
    let grabs = [None, Some(ControlKind::FlightStick)];
    step_with(&mut state, &p, &KINDS, [None, Some(grip)], grabs);
    let pushed = grip + Vec3::new(0.06, 0.0, 0.0);
    step_with(&mut state, &p, &KINDS, [None, Some(pushed)], [None; 2]);
    step_held(&mut state, &p, &KINDS, [None; 2], [None; 2], [false; 2]);
    let returning = state.stick;
    assert!(returning.x > 0.0 && returning.x < 0.06, "{returning}");
    // Taken again away from where it was let go, the stick stays where the spring had it.
    let again = top + Vec3::new(-0.03, 0.0, 0.02);
    let (out, _) = step_with(&mut state, &p, &KINDS, [None, Some(again)], grabs);
    assert!(
        (state.stick - returning).length() < 1e-5,
        "{} {returning}",
        state.stick
    );
    assert!(out.flight_stick.is_some_and(|s| s.x > 0.0));
}

#[test]
fn the_thumbstick_takes_over_from_a_returning_stick() {
    let p = plane();
    let mut state = ControlState {
        stick: Vec2::new(0.05, 0.0),
        ..ControlState::default()
    };
    let mut events = Vec::new();
    let mut inputs = StepInputs {
        kinds: &KINDS,
        grips: [None; 2],
        orientations: [None; 2],
        held: [false; 2],
        grabs: [None; 2],
        stick_steers: false,
        stick_moves: false,
        lever_moves: false,
        dt: DT,
    };
    let out = step(&mut state, &p, &inputs, &mut events);
    assert!(out.flight_stick.is_some());
    inputs.stick_moves = true;
    let out = step(&mut state, &p, &inputs, &mut events);
    assert_eq!(out.flight_stick, None);
    assert_eq!(state.stick, Vec2::ZERO);
}

#[test]
fn twisting_the_held_stick_works_the_rudder_from_where_it_was_taken() {
    let p = plane();
    let top = p.config.flight_stick.top_m;
    let mut state = ControlState::default();
    // A wrist already turned when it takes the stick starts from no rudder.
    let taken = Quat::from_rotation_y(0.5);
    let out = step_oriented(
        &mut state,
        &p,
        [None, Some(top)],
        [None, Some(taken)],
        [None, Some(ControlKind::FlightStick)],
    );
    assert_eq!(out.rudder, Some(0.0));
    let right = Quat::from_rotation_y(-p.config.flight.twist_full_deg.to_radians()) * taken;
    let out = step_oriented(
        &mut state,
        &p,
        [None, Some(top)],
        [None, Some(right)],
        [None; 2],
    );
    assert!(
        out.rudder.is_some_and(|r| (r - 1.0).abs() < 1e-4),
        "{out:?}"
    );

    let mut off = plane();
    off.config.flight.twist_rudder = false;
    let out = step_oriented(
        &mut state,
        &off,
        [None, Some(top)],
        [None, Some(right)],
        [None; 2],
    );
    assert_eq!(out.rudder, None);
}

#[test]
fn the_throttle_lights_the_afterburner_only_through_its_gate_and_cues_on_the_way() {
    let p = plane();
    let spec = p.config.throttle;
    let rest = p.mirror(spec.rest_m);
    let forward = spec.axis.direction() * (spec.travel_m * 0.5);
    let mut state = ControlState::default();
    let grabs = [Some(ControlKind::Throttle), None];
    step_with(&mut state, &p, &KINDS, [Some(rest), None], grabs);
    let gate = p.config.flight.gate_at;

    let (out, _) = step_with(
        &mut state,
        &p,
        &KINDS,
        [Some(rest + forward * (gate - 0.05)), None],
        [None; 2],
    );
    let dry = out.throttle.unwrap();
    assert!(dry < AFTERBURNER && dry > 0.7, "{dry}");
    assert!(!out.afterburner);

    let (out, _) = step_with(
        &mut state,
        &p,
        &KINDS,
        [Some(rest + forward * (gate + 0.05)), None],
        [None; 2],
    );
    assert_eq!(out.throttle, Some(1.0));
    assert!(out.afterburner);
    assert_eq!(out.lever_cue, Some(ControlCue::Gate));

    let (out, _) = step_with(
        &mut state,
        &p,
        &KINDS,
        [Some(rest + forward * 1.2), None],
        [None; 2],
    );
    assert_eq!(out.lever_cue, Some(ControlCue::Stop));

    // Back past idle into the brake.
    step_with(&mut state, &p, &KINDS, [Some(rest), None], [None; 2]);
    let (out, _) = step_with(
        &mut state,
        &p,
        &KINDS,
        [Some(rest - forward * 0.5), None],
        [None; 2],
    );
    assert_eq!(out.lever_cue, Some(ControlCue::Neutral));
    assert!(out.throttle.is_some_and(|t| t < 0.0));

    let mut off = plane();
    off.config.flight.afterburner_gate = false;
    let mut state = ControlState::default();
    step_with(&mut state, &off, &KINDS, [Some(rest), None], grabs);
    let (out, _) = step_with(
        &mut state,
        &off,
        &KINDS,
        [Some(rest + forward * 0.95), None],
        [None; 2],
    );
    assert!(out.throttle.is_some_and(|t| t > AFTERBURNER), "{out:?}");
    assert!(!out.afterburner);
}

#[test]
fn a_parked_throttle_gives_way_to_the_trigger() {
    let p = plane();
    let mut state = ControlState {
        levers: [0.0, 0.5],
        ..ControlState::default()
    };
    let mut events = Vec::new();
    let mut inputs = StepInputs {
        kinds: &KINDS,
        grips: [None; 2],
        orientations: [None; 2],
        held: [false; 2],
        grabs: [None; 2],
        stick_steers: false,
        stick_moves: false,
        lever_moves: false,
        dt: DT,
    };
    assert!(
        step(&mut state, &p, &inputs, &mut events)
            .throttle
            .is_some()
    );
    inputs.lever_moves = true;
    assert_eq!(step(&mut state, &p, &inputs, &mut events).throttle, None);
    // The lever stays where it was, and takes over again when the trigger is let go.
    assert_eq!(state.levers[1], 0.5);
    inputs.lever_moves = false;
    assert!(
        step(&mut state, &p, &inputs, &mut events)
            .throttle
            .is_some()
    );
}

const AFTERBURNER: f32 = jc3gi::vehicle::vehicle::AirPlane::NITROUS_THRUST_THRESHOLD;
