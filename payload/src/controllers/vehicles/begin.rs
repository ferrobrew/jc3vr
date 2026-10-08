//! The first half of the vehicle handling's input tick: read the mounted gun, fit the wheel and
//! the handlebars, and place the physical controls' and the parachute handles' grab volumes.

use std::{sync::atomic::Ordering, time::Instant};

use glam::Vec3;

use crate::{
    controllers::{
        bindings::grab::{self, GrabVolume},
        body::{self, BodyDimensions, alignment},
        config::ControllersConfig,
        fingers::grip::{self, Grip, GripSource},
        frame::Hand,
        hands::{self, tiers},
        mode::Mode,
        vehicles::{
            arm_tick::arms_act,
            driving_view, fit,
            fit::FitInputs,
            handle_tick::handles_act,
            highlights, mounted_grip,
            mounted_tick::{mounted_tick, offered},
            parachute,
            physical::{
                self, ControlState, Placement,
                buttons::{Button, ButtonPlacement, PushPhase},
            },
            state::{MOUNTED_WEAPON, STATE, log_handle_events, push_events},
            types::TickVolumes,
            wheel::CockpitTransform,
        },
    },
    headpose,
};

/// The first half of the vehicle handling's input tick: read the mounted gun, place the physical
/// controls, and return the control each hand is in. Called while the bindings inject, before the
/// tables.
pub(crate) fn begin_tick(mode: Mode) -> TickVolumes {
    let Some(active) = hands::active_hands_raw() else {
        crate::controllers::vehicles::stand_down();
        return TickVolumes::default();
    };
    let config = active.config;
    let seat = match mode {
        Mode::Vehicle(seat) => Some(seat),
        _ => None,
    };
    let physical_config = config.vehicles.physical;
    let xf = CockpitTransform {
        head_world: headpose::query().position,
        head_cockpit: active.head.position,
        body: headpose::xr::body_rotation(),
        world_scale: active.world_scale,
    };
    let mut latch = STATE.lock().latch.take();
    let fit = fit::fit(
        &FitInputs {
            physical: &physical_config,
            view: &config.vehicles.driving_view,
            seat,
            xf: &xf,
            anchor: headpose::anchor(),
        },
        &mut latch,
    );
    driving_view::publish(fit.view);
    let offered = offered(seat, &config, fit.fork);
    let placement = Placement {
        wheel: fit.wheel,
        bars: fit.bars,
        ..Placement::new(physical_config, config.acting_hand())
    };
    let grips = active.hands.map(|h| h.map(|h| h.grip.position));
    let mounted_config = config.vehicles.mounted;
    let aiming = STATE
        .lock()
        .mounted_grip
        .aiming(&mounted_config, grips.map(|g| g.is_some()));
    let mounted = mounted_tick(&active, aiming);
    let handles_on = handles_act(mode, &config);
    let handles_config = config.vehicles.parachute_handles;

    let mut events = Vec::new();
    let mut handle_events = Vec::new();
    let mut state = STATE.lock();
    if state.status.seat != seat {
        physical::release_all(&mut state.controls, &mut events);
        state.controls = ControlState::default();
    }
    if !handles_on {
        parachute::release_all(&mut state.handles, &mut handle_events);
        state.handles_torso = None;
    }
    let mut candidates = physical::candidates(&placement, &state.controls, offered, grips);
    candidates.extend(mounted_grip::candidates(
        &mounted_config,
        mounted.handle_points,
    ));
    let mut reach = Vec::new();
    let mut marks = None;
    if handles_on {
        let dims = body_dimensions(&config);
        // Along the torso, so that a player turned from the recenter's forward finds the handles
        // by their own shoulders; while the estimate is stale, the last heading eases toward the
        // head's.
        let now = Instant::now();
        let dt = state
            .handles_torso
            .map_or(0.0, |(_, at)| now.duration_since(at).as_secs_f32());
        let torso = parachute::torso_heading(
            alignment::estimate().map(|e| e.rotation()),
            state.handles_torso.map(|(torso, _)| torso),
            hands::reach::yaw_only(active.head.orientation),
            dt,
        );
        state.handles_torso = Some((torso, now));
        let shoulders = body::shoulder_positions(active.head.position, torso, &dims);
        let centres = parachute::handle_centres(&handles_config, shoulders, torso);
        candidates.extend(parachute::candidates(
            &handles_config,
            &state.handles,
            centres,
            grips,
        ));
        reach.extend(parachute::reach_candidates(
            &handles_config,
            &state.handles,
            centres,
        ));
        marks = handles_config
            .show_handles
            .then(|| parachute::marks(&handles_config, &state.handles, centres, grips));
    }
    let volumes = Hand::BOTH.map(|hand| {
        let i = hand.index();
        if let Some(hold) = state.controls.holds[i] {
            grip::declare(hand, Grip::new(GripSource::of_control(hold.kind)));
            return Some(GrabVolume::VehicleControl(hold.kind));
        }
        if state.handles.holds[i].is_some() {
            grip::declare(hand, Grip::new(GripSource::ParachuteHandle));
            return Some(GrabVolume::ParachuteHandle(hand));
        }
        if state.mounted_grip.holds[i] {
            return Some(GrabVolume::MountedHandle(hand));
        }
        grab::grab_volume(grips[i]?, hand, &candidates)
    });
    let reach = Hand::BOTH.map(|hand| {
        let i = hand.index();
        grab::grab_volume(grips[i]?, hand, &reach).filter(|_| volumes[i].is_none())
    });
    state.handles_on = handles_on;
    state.handles_config = handles_config;
    state.placement = Some(placement);
    state.latch = latch;
    state.render = fit.render.filter(|_| physical_config.animate_wheel);
    state.steering_turn = fit.steering_turn;
    state.status.fit = fit.readout;
    state.status.wheel = seat.is_some_and(|s| s.driver).then_some(fit.wheel);
    state.mounted_target = mounted.target;
    MOUNTED_WEAPON.store(mounted.target.map_or(0, |t| t.weapon), Ordering::Relaxed);
    state.mounted_handles = mounted.handles.or(state.mounted_handles);
    state.mounted_handle_points = mounted.handle_points;
    state.mounted_config = mounted_config;
    state.mounted_barrel = mounted.barrel;
    declare_mounted_grips(mounted.handles, state.mounted_grip.shares(&mounted_config));
    state.status.active = true;
    state.status.seat = seat;
    state.status.offered = offered;
    state.status.volumes = volumes;
    state.status.mounted = mounted.readout;
    let arms_engaged = state.status.arms.is_some_and(|a| a.arms.is_some());
    if !handles_on {
        state.status.handles = None;
    }
    push_events(&mut state.status, events);
    let controls_now = state.controls;
    drop(state);
    log_handle_events(&handle_events);
    // A hand pressing a cockpit button presses it with a flat palm.
    for hand in Hand::BOTH {
        let pressing = Button::ALL
            .into_iter()
            .any(|b| controls_now.buttons.phase(b, hand) == PushPhase::Pressed);
        if pressing {
            grip::declare(hand, Grip::new(GripSource::Surface));
        }
    }

    let shown = (physical_config.show_wheel && seat.is_some_and(|s| s.driver)).then_some(fit.wheel);
    let wings = (arms_act(mode, &config) && config.vehicles.arm_steering.show_guide)
        .then_some(arms_engaged);
    let buttons = ButtonPlacement::for_seat(offered, &physical_config.buttons, &placement.wheel);
    let button_marks = highlights::button_marks(&buttons, &controls_now.buttons);
    highlights::submit(
        &config,
        &highlights::Highlights {
            candidates: &candidates,
            volumes,
            wings,
            handles: marks,
            wheel: shown,
            buttons: &button_marks,
        },
        &xf,
    );
    TickVolumes { volumes, reach }
}

/// Declare each hand the mounted gun's handles anchor, for its finger pose, at the anchor's share
/// times the hand's hold on its handle, `shares`.
fn declare_mounted_grips(handles: Option<[Option<Vec3>; 2]>, shares: [f32; 2]) {
    let tiers = tiers::status();
    let Some(handles) = handles.filter(|_| tiers.state == Some(tiers::TierState::MountedGun))
    else {
        return;
    };
    for hand in Hand::BOTH {
        let weight = tiers.anchor_weights[hand.index()] * shares[hand.index()];
        if handles[hand.index()].is_some() && weight > 0.0 {
            grip::declare(hand, Grip::new(GripSource::MountedGun).share(weight));
        }
    }
}

/// The dimensions the hands are measured against: the character's while the hands are remapped
/// onto them, the player's otherwise.
pub(super) fn body_dimensions(config: &ControllersConfig) -> BodyDimensions {
    if config.body.remap_hands {
        BodyDimensions::CHARACTER_ESTIMATE
    } else {
        body::player()
    }
}
