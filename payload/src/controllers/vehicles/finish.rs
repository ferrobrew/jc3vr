//! The second half of the vehicle handling's input tick: act on the grabs, move the held controls,
//! write their axes over the tables', add the arm pose to the move stick in the wingsuit, and
//! steer with the held handles under the parachute.

use jc3gi::input::input_action_map::Action::{TURN_LEFT, TURN_RIGHT};

use crate::controllers::{
    bindings::{controls::Controls, grab::GrabVolume, hold::Desired},
    frame::Hand,
    hands::{self, tiers},
    haptics::{self, Event},
    mode::Mode,
    vehicles::{
        arm_tick::arm_tick,
        axis_output::{axis_wanted, lever_axes_wanted, stick_axes_wanted},
        config::PhysicalControlsConfig,
        control_haptics,
        handle_tick::handles_tick,
        mounted_grip::{self, MountedGripEvent},
        mounted_recoil, parachute, parachute_rig,
        physical::{self, ControlEvent, ControlKind, ControlState, buttons::ButtonEvent},
        state::{ARM_PANEL, STATE, log_handle_events, push_events},
        steering, wheel_render, write_outputs,
    },
};

/// The second half of the vehicle handling's input tick: act on the grabs, move the held controls,
/// write their axes over the tables', add the arm pose to the move stick in the wingsuit, and steer
/// with the held handles under the parachute.
pub(crate) fn finish_tick(
    mode: Mode,
    grabs: [Option<GrabVolume>; 2],
    controls: &Controls,
    desired: &mut Vec<Desired>,
    dt: f32,
) {
    let Some(active) = hands::active_hands_raw() else {
        return;
    };
    let config = active.config;
    let grips = active.hands.map(|h| h.map(|h| h.grip.position));
    let orientations = active.hands.map(|h| h.map(|h| h.grip.orientation));
    let held = Hand::BOTH.map(|hand| controls.hand(hand).grip.held());
    let handle_grabs = grabs.map(|g| matches!(g, Some(GrabVolume::ParachuteHandle(_))));
    let mounted_grabs = grabs.map(|g| matches!(g, Some(GrabVolume::MountedHandle(_))));
    let grabs = grabs.map(|g| match g {
        Some(GrabVolume::VehicleControl(kind)) => Some(kind),
        _ => None,
    });

    let mut events = Vec::new();
    let mut handle_events = Vec::new();
    let mut state = STATE.lock();
    let mut mounted_events = Vec::new();
    let mounted_handles = state.mounted_handle_points;
    mounted_grip::step(
        &mut state.mounted_grip,
        &mounted_grip::GripStep {
            config: &config.vehicles.mounted,
            handles: mounted_handles,
            grips,
            held,
            grabs: mounted_grabs,
            dt,
            blend_s: config.hands.anchor.blend_s,
        },
        &mut mounted_events,
    );
    mounted_recoil::set_holds(
        Hand::BOTH.map(|h| state.mounted_grip.shares(&config.vehicles.mounted)[h.index()] > 0.0),
    );
    state.status.mounted_holds = state.mounted_grip.holds;
    if !mounted_events.is_empty() {
        state.status.mounted_grip_events += mounted_events.len() as u64;
        log_mounted_events(&mounted_events);
    }
    let Some(placement) = state.placement else {
        return;
    };
    let offered = state.status.offered;
    let outputs = physical::step(
        &mut state.controls,
        &placement,
        &physical::StepInputs {
            kinds: offered,
            grips,
            orientations,
            held,
            grabs,
            stick_steers: axis_wanted(desired, TURN_LEFT, TURN_RIGHT),
            stick_moves: stick_axes_wanted(desired),
            lever_moves: lever_axes_wanted(desired),
            dt,
        },
        &mut events,
    );
    write_outputs(&outputs, desired);
    let holds_now = state.controls.holds.map(|h| h.map(|h| h.kind));
    let afterburner_was = std::mem::replace(&mut state.afterburner, outputs.afterburner);
    let rendered = state.render.map(|target| {
        let angle = rendered_angle(&state.controls, &placement.config, state.steering_turn);
        (target, angle)
    });
    wheel_render::publish(rendered);
    state.status.rendered_angle = rendered.map(|(_, angle)| angle);
    let handles_config = state.handles_config;
    let pulls = state.handles_on.then(|| {
        parachute::step(
            &mut state.handles,
            &handles_config,
            &parachute::StepInputs {
                grips,
                held,
                grabs: handle_grabs,
            },
            &mut handle_events,
        )
    });
    // The rig's handles the hands do not hold are drawn on their cords, not in the hands.
    parachute_rig::publish(
        state
            .handles_on
            .then(|| state.handles.holds.map(|hold| hold.is_some())),
    );
    let blend = config.hands.anchor.blend_s;
    for hand in Hand::BOTH {
        let i = hand.index();
        let hold = state.controls.holds[i];
        let handle = state.handles.holds[i];
        if hold.is_some() {
            state.last_holds[i] = hold;
            state.last_handles[i] = None;
        } else if handle.is_some() {
            state.last_handles[i] = handle;
            state.last_holds[i] = None;
        }
        let target = if hold.is_some() || handle.is_some() {
            1.0
        } else {
            0.0
        };
        state.anchor_weights[i] =
            tiers::step_weight(state.anchor_weights[i], target, dt, blend, blend);
        if state.anchor_weights[i] <= 0.0 {
            state.last_holds[i] = None;
            state.last_handles[i] = None;
        }
    }
    let engaged = state.status.arms.is_some_and(|a| a.arms.is_some());
    let arms = arm_tick(mode, controls, &active, desired, engaged);
    if arms.is_some_and(|a| a.arms.is_some()) != engaged {
        tracing::debug!(
            target: "controllers",
            engaged = !engaged,
            pose = ?arms.map(|a| a.pose),
            "wingsuit arm steering"
        );
    }
    let handles =
        pulls.map(|pulls| handles_tick(&state.handles, pulls, controls, desired, &handles_config));
    state.status.holds = state.controls.holds.map(|h| h.map(|h| h.kind));
    state.status.controls = state.controls;
    state.status.outputs = outputs;
    if let Some(cue) = outputs.stick_cue.or(outputs.lever_cue) {
        state.status.last_cue = Some(cue);
        state.status.cues += 1;
    }
    state.status.afterburner_lights += u64::from(outputs.afterburner && !afterburner_was);
    state.status.arms = arms;
    state.status.handles = handles;
    let button_events: Vec<ButtonEvent> = events
        .iter()
        .filter_map(|e| match e {
            ControlEvent::Button(b) => Some(*b),
            _ => None,
        })
        .collect();
    state.status.button_presses += button_events.iter().filter(|e| e.engages()).count() as u64;
    push_events(&mut state.status, events);
    drop(state);
    *ARM_PANEL.lock() = arms.filter(|_| config.vehicles.arm_steering.show_readout);
    log_handle_events(&handle_events);
    control_haptics::pulse(&outputs, holds_now, &button_events);
    for event in button_events.iter().filter(|e| e.engages()) {
        tracing::info!(
            target: "controllers",
            hand = event.hand().label(),
            "the hand worked the {}",
            event.what()
        );
    }
    if outputs.afterburner != afterburner_was {
        tracing::info!(
            target: "controllers",
            lit = outputs.afterburner,
            "the throttle {} its afterburner gate",
            if outputs.afterburner { "went through" } else { "came back out of" }
        );
    }
}

/// Log the hands taking and letting go of the mounted gun's handles, and pulse a taking hand.
fn log_mounted_events(events: &[MountedGripEvent]) {
    for event in events {
        match *event {
            MountedGripEvent::Grabbed(hand) => {
                tracing::info!(target: "controllers", hand = hand.label(), "the hand took the mounted gun's handle");
                haptics::play(hand, Event::HandleGrab);
            }
            MountedGripEvent::Released(hand) => {
                tracing::info!(target: "controllers", hand = hand.label(), "the hand let go of the mounted gun's handle");
            }
        }
    }
}

/// The angle to turn the rendered wheel to, radians clockwise: the held or returning wheel's, or
/// otherwise the wheel angle that would give the vehicle's own steering, so the stick turns it too.
pub(super) fn rendered_angle(
    controls: &ControlState,
    config: &PhysicalControlsConfig,
    steering_turn: Option<f32>,
) -> f32 {
    let held = controls
        .holds
        .iter()
        .flatten()
        .any(|h| h.kind == ControlKind::Wheel);
    if held || controls.wheel_angle != 0.0 {
        return controls.wheel_angle;
    }
    steering_turn.map_or(0.0, |turn| {
        steering::angle_for_steer(turn.clamp(-1.0, 1.0), &config.wheel)
    })
}
