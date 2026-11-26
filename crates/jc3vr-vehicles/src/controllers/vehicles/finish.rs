//! The second half of the vehicle handling's input tick: act on the grabs, move the held controls,
//! write their axes over the tables', add the arm pose to the move stick in the wingsuit, and
//! steer with the held handles under the parachute.

use jc3gi::input::input_action_map::Action::{
    HELI_TURN_LEFT, HELI_TURN_RIGHT, TURN_LEFT, TURN_RIGHT,
};
use jc3vr_body::controllers::{
    grab::{GrabVolume, cockpit::ControlKind},
    hands,
    hands::{anchor::ANCHOR_BLEND_S, tiers},
    haptics,
    intent::{
        controls::Controls,
        hold::{Desired, MapKind, Target, Want},
    },
    locomotion::axis_output::axis_wanted,
    mode::{Mode, VehicleClass},
};
use jc3vr_core::game_tick::GameTick;
use jc3vr_input::{frame::Hand, haptics::cues::Event};
use jc3vr_view::controllers::cockpit::CockpitTransform;

use crate::controllers::vehicles::{
    arm_tick::arm_tick,
    axis_output::{lever_axes_wanted, stick_axes_wanted},
    begin::body_dimensions,
    config::VehicleSettings,
    control_haptics,
    handle_tick::handles_tick,
    mounted_grip,
    mounted_grip::MountedGripEvent,
    mounted_recoil, parachute, parachute_rig, physical,
    physical::{ControlEvent, ControlState, buttons::ButtonEvent},
    state::{STATE, log_handle_events, publish_mounted_holds, push_events},
    steering,
    tuning::PhysicalControlsTuning,
    wheel_render, wingsuit_evade, write_outputs,
};

/// The second half of the vehicle handling's input tick: act on the grabs, move the held controls,
/// write their axes over the tables', add the arm pose to the move stick in the wingsuit, and steer
/// with the held handles under the parachute.
pub fn finish_tick(
    game: &GameTick<'_>,
    mode: Mode,
    grabs: [Option<GrabVolume>; 2],
    controls: &Controls,
    desired: &mut Vec<Desired>,
    dt: f32,
    grapple_held: bool,
) {
    let Some(active) = hands::active_hands_raw() else {
        return;
    };
    let config = VehicleSettings::read();
    let grips = active.hands.map(|h| h.map(|h| h.grip.position));
    let orientations = active.hands.map(|h| h.map(|h| h.grip.orientation));
    let held = Hand::BOTH.map(|hand| controls.hand(hand).hold.held());
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
            blend_s: ANCHOR_BLEND_S,
        },
        &mut mounted_events,
    );
    mounted_recoil::set_holds(
        Hand::BOTH.map(|h| state.mounted_grip.shares(&config.vehicles.mounted)[h.index()] > 0.0),
    );
    state.status.mounted_holds = state.mounted_grip.holds;
    publish_mounted_holds(state.mounted_grip.holds);
    if !mounted_events.is_empty() {
        state.status.mounted_grip_events += mounted_events.len() as u64;
        let manned = tiers::status().state == Some(tiers::TierState::MountedGun);
        log_mounted_events(&mounted_events, manned);
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
    physical::visual::submit(
        &placement,
        &state.controls,
        offered,
        &CockpitTransform::current(active.head.position, active.world_scale),
    );
    // The twist is the plane's rudder and the helicopter's yaw, which are different actions: the
    // shared axis writer takes the plane's, so a helicopter writes its own and leaves that alone.
    let helicopter = writes_helicopter_yaw(mode);
    let mut written = outputs;
    if helicopter {
        written.rudder = None;
    }
    write_outputs(&written, desired);
    if helicopter && let Some(rudder) = outputs.rudder {
        write_heli_rudder(desired, rudder);
    }
    let holds_now = state.controls.holds.map(|h| h.map(|h| h.kind));
    let afterburner_was = std::mem::replace(&mut state.afterburner, outputs.afterburner);
    let rendered = state.render.map(|target| {
        let angle = rendered_angle(&state.controls, &placement.config, state.steering_turn);
        (target, angle)
    });
    let native = placement
        .native_stick
        .filter(|native| placement.cockpit.is_none() && offered.contains(&native.kind))
        .map(|native| (native.target, native.motion(state.controls.stick)));
    wheel_render::publish_controls(rendered, native);
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
    let blend = ANCHOR_BLEND_S;
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
    let last = state.status.arms.map(|a| a.arms);
    let engaged = last.flatten().is_some();
    let was_held = state.status.arms.is_some_and(|a| a.held_for_grapple);
    let arms = arm_tick(
        mode,
        controls,
        &active,
        &config,
        desired,
        last,
        grapple_held,
    );
    wingsuit_evade::tick(
        game,
        arms.as_ref(),
        body_dimensions(&config).arm_span,
        &config.vehicles.wingsuit_evade,
        desired,
        dt,
    );
    let now_held = arms.is_some_and(|a| a.held_for_grapple);
    if now_held != was_held {
        tracing::info!(
            target: "controllers",
            held = now_held,
            arms = ?arms.and_then(|a| a.arms),
            "wingsuit arm steering: {}",
            if now_held {
                "the grapple is aimed, so the arms hold the steering they had"
            } else {
                "the grapple's button is up, so the arms steer again"
            }
        );
    }
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
/// Log the hands' grabs and releases of the mounted gun's handles, with whether the character
/// mans the gun (`manned`): off the native mounted state a holding hand is drawn on its handle by
/// its grip alone.
fn log_mounted_events(events: &[MountedGripEvent], manned: bool) {
    for event in events {
        match *event {
            MountedGripEvent::Grabbed(hand) => {
                tracing::info!(
                    target: "controllers",
                    hand = hand.label(),
                    manned,
                    "the hand took the mounted gun's handle"
                );
                haptics::play(hand, Event::HandleGrab);
            }
            MountedGripEvent::Released(hand, why) => {
                tracing::info!(
                    target: "controllers",
                    hand = hand.label(),
                    reason = why.label(),
                    "the hand let go of the mounted gun's handle"
                );
            }
        }
    }
}

/// The angle to turn the rendered wheel to, radians clockwise: the held or returning wheel's, or
/// otherwise the wheel angle that would give the vehicle's own steering, so the stick turns it too.
pub(super) fn rendered_angle(
    controls: &ControlState,
    config: &PhysicalControlsTuning,
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

/// Whether `mode` is a helicopter's driver seat, whose stick twist writes the helicopter's yaw.
fn writes_helicopter_yaw(mode: Mode) -> bool {
    matches!(mode, Mode::Vehicle(seat) if seat.driver && seat.class == VehicleClass::Helicopter)
}

/// Write the held stick's twist over the tables' helicopter yaw, replacing the half-axes the
/// tables wrote, as [`write_outputs`] does for the plane's rudder.
fn write_heli_rudder(desired: &mut Vec<Desired>, rudder: f32) {
    desired.retain(|d| {
        !(d.target.map == MapKind::Player
            && matches!(d.target.action, HELI_TURN_LEFT | HELI_TURN_RIGHT))
    });
    let rudder = if rudder.is_finite() {
        rudder.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    for (action, half) in [(HELI_TURN_LEFT, -rudder), (HELI_TURN_RIGHT, rudder)] {
        if half > 0.0 {
            desired.push(Desired {
                target: Target {
                    map: MapKind::Player,
                    action,
                },
                want: Want::Analog(half),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use jc3gi::input::input_action_map::Action::{self, ACCELERATE};
    use jc3vr_body::controllers::mode::VehicleSeat;

    use super::*;

    fn analog(desired: &[Desired], action: Action) -> Option<f32> {
        desired.iter().find_map(|d| match d.want {
            Want::Analog(v) if d.target.action == action => Some(v),
            _ => None,
        })
    }

    fn want(action: Action, value: f32) -> Desired {
        Desired {
            target: Target {
                map: MapKind::Player,
                action,
            },
            want: Want::Analog(value),
        }
    }

    #[test]
    fn only_a_helicopter_driver_writes_the_helicopter_yaw() {
        let seat = |class: VehicleClass, driver: bool| Mode::Vehicle(VehicleSeat { class, driver });
        assert!(writes_helicopter_yaw(seat(VehicleClass::Helicopter, true)));
        assert!(!writes_helicopter_yaw(seat(
            VehicleClass::Helicopter,
            false
        )));
        assert!(!writes_helicopter_yaw(seat(VehicleClass::Plane, true)));
        assert!(!writes_helicopter_yaw(Mode::Ui));
    }

    #[test]
    fn the_helicopter_twist_replaces_the_tables_yaw_and_leaves_the_rest() {
        let mut desired = vec![want(HELI_TURN_LEFT, 0.7), want(ACCELERATE, 1.0)];
        write_heli_rudder(&mut desired, 0.4);
        assert_eq!(analog(&desired, HELI_TURN_LEFT), None);
        assert_eq!(analog(&desired, HELI_TURN_RIGHT), Some(0.4));
        assert_eq!(analog(&desired, ACCELERATE), Some(1.0));
        write_heli_rudder(&mut desired, -0.5);
        assert_eq!(analog(&desired, HELI_TURN_LEFT), Some(0.5));
        assert_eq!(analog(&desired, HELI_TURN_RIGHT), None);
        write_heli_rudder(&mut desired, 0.0);
        assert_eq!(analog(&desired, HELI_TURN_LEFT), None);
        assert_eq!(analog(&desired, HELI_TURN_RIGHT), None);
    }

    #[test]
    fn a_non_finite_twist_writes_nothing() {
        let mut desired = vec![want(HELI_TURN_RIGHT, 0.7)];
        write_heli_rudder(&mut desired, f32::NAN);
        assert_eq!(analog(&desired, HELI_TURN_LEFT), None);
        assert_eq!(analog(&desired, HELI_TURN_RIGHT), None);
    }
}
