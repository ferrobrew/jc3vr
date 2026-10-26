//! The first half of the vehicle handling's input tick: read the mounted gun, fit the wheel and
//! the handlebars, and provide the vehicle handling's grip targets ([`crate::controllers::grab`]):
//! the driver's controls, the cockpit's buttons, the parachute's handles, and the mounted gun's
//! handles.

use std::sync::atomic::Ordering;

use glam::Vec3;

use crate::{
    controllers::{
        body::{self, BodyDimensions},
        config::ControllersConfig,
        fingers::grip::{self, Grip, GripSource},
        frame::Hand,
        grab::{self, GrabVolume, GripTarget, Mark, MarkAt, MarkStyle, Shown, Tier},
        hands::{self, tiers},
        loadout::live,
        mode::Mode,
        vehicles::{
            arm_tick::arms_act,
            driving_view, fit,
            fit::FitInputs,
            handle_tick::handles_act,
            mounted_grip,
            mounted_tick::{mounted_tick, offered},
            parachute, parachute_rig,
            physical::{
                self, ControlState, Placement,
                buttons::{Button, ButtonPlacement, PushPhase},
            },
            state::{MOUNTED_WEAPON, STATE, log_handle_events, push_events},
            types::TickVolumes,
            wheel::CockpitTransform,
            wing_guide,
        },
    },
    headpose,
};

/// The first half of the vehicle handling's input tick: read the mounted gun, place the physical
/// controls, and return the vehicle handling's grip targets. Called while the bindings inject,
/// before the tables.
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
    let native_centres = parachute_rig::centres(handles_on, &xf);

    let mut events = Vec::new();
    let mut handle_events = Vec::new();
    let mut state = STATE.lock();
    if state.status.seat != seat {
        physical::release_all(&mut state.controls, &mut events);
        state.controls = ControlState::default();
    }
    if !handles_on {
        parachute::release_all(&mut state.handles, &mut handle_events);
    }
    // What a hand holds goes first, so that it keeps it over anything as near.
    let mut targets = Vec::new();
    for hand in Hand::BOTH {
        let i = hand.index();
        let Some(point) = grips[i] else {
            continue;
        };
        if let Some(hold) = state.controls.holds[i] {
            grip::declare(hand, Grip::new(GripSource::of_control(hold.kind)));
            targets.push(
                control_target(
                    GrabVolume::VehicleControl(hold.kind),
                    point,
                    physical_config.grab_radius_m,
                    hand,
                )
                .held(hand),
            );
        }
        if state.mounted_grip.holds[i] {
            targets.push(
                handle_target(
                    GrabVolume::MountedHandle(hand),
                    mounted.handle_points[i].unwrap_or(point),
                    mounted_config.grab_radius_m,
                    hand,
                )
                .held(hand),
            );
        }
    }
    // A hand holding something of the table's takes no new control or handle: it keeps only the
    // one it holds.
    let holding = Hand::BOTH.map(|hand| {
        state.controls.holds[hand.index()].is_some()
            || state.mounted_grip.holds[hand.index()]
            || (config.vehicles.free_hands_only && live::held_kind(hand).is_some())
    });
    let controls = physical::candidates(&placement, &state.controls, offered, grips);
    targets.extend(
        controls
            .iter()
            .filter(|c| c.hand.is_none_or(|h| !holding[h.index()]))
            .map(|c| {
                c.target(
                    Mark::ring(MarkAt::Cockpit(c.centre), CONTROL_MARK_M).showing(Shown::Always),
                )
            }),
    );
    targets.extend(
        mounted_grip::candidates(&mounted_config, mounted.handle_points)
            .iter()
            .filter(|c| c.hand.is_none_or(|h| !holding[h.index()]))
            .map(|c| handle_target(c.volume, c.centre, c.radius, c.hand.unwrap_or(Hand::Right))),
    );
    let buttons = ButtonPlacement::for_seat(offered, &physical_config.buttons, &placement.wheel);
    for button in Button::ALL {
        let Some(at) = buttons.button(button) else {
            continue;
        };
        let pressed_by = Hand::BOTH
            .into_iter()
            .find(|&h| state.controls.buttons.phase(button, h) == PushPhase::Pressed);
        // The horn is the wheel's drawn hub, so it is marked only once a hand is at it.
        let shown = match button {
            Button::Horn => Shown::InReach,
            Button::Nitrous => Shown::Always,
        };
        // A button is pressed by a free hand, or by the one already pressing it.
        let pressers: Vec<Hand> = Hand::BOTH
            .into_iter()
            .filter(|&h| pressed_by == Some(h) || !holding[h.index()])
            .collect();
        let only = match pressers.as_slice() {
            [] => continue,
            [hand] => Some(*hand),
            _ => None,
        };
        let target = GripTarget::sphere(
            GrabVolume::CockpitButton(button),
            only,
            at.centre,
            at.radius,
            Mark::ring(MarkAt::Cockpit(at.centre), at.radius * 2.0)
                .on_surface(at.normal)
                .styled(MarkStyle::Button)
                .showing(shown),
        );
        targets.push(match pressed_by {
            Some(hand) => GripTarget {
                held_by: Some(hand),
                ..target
            },
            None => target,
        });
    }
    if handles_on {
        let centres = native_centres.map(|point| point.unwrap_or(Vec3::ZERO));
        let placed = parachute::candidates(&handles_config, &state.handles, centres, grips);
        for (hand, c) in Hand::BOTH.into_iter().zip(placed) {
            if state.handles.holds[hand.index()].is_none() && native_centres[hand.index()].is_none()
            {
                continue;
            }
            let target = handle_target(c.volume, c.centre, c.radius, hand);
            let target = GripTarget {
                mark: target.mark.showing(Shown::Always),
                ..target
            };
            if state.handles.holds[hand.index()].is_some() {
                grip::declare(hand, Grip::new(GripSource::ParachuteHandle));
                targets.insert(0, target.held(hand));
            } else if !holding[hand.index()] {
                targets.push(target);
            }
        }
        // A hand that misses a free handle by a little still takes it when nothing else is in
        // reach.
        targets.extend(
            parachute::reach_candidates(&handles_config, &state.handles, centres)
                .filter(|c| {
                    c.hand.is_some_and(|hand| {
                        !holding[hand.index()] && native_centres[hand.index()].is_some()
                    })
                })
                .map(|c| {
                    c.target(Mark::ring(MarkAt::Cockpit(c.centre), c.radius).showing(Shown::Never))
                        .ranked(Tier::Fallback)
                }),
        );
    }
    let grips = grab::tick::hand_grips(&active);
    let volumes = grab::resolve::resolve(&targets, grips).map(|r| r.chosen.map(|c| c.volume));
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
    wing_guide::submit(
        config.overlay.enabled
            && arms_act(mode, &config)
            && config.vehicles.arm_steering.show_guide,
        arms_engaged,
    );
    TickVolumes { targets }
}

/// A vehicle control's grab point for `hand`, at `point` in the cockpit frame.
fn control_target(volume: GrabVolume, point: Vec3, radius: f32, hand: Hand) -> GripTarget {
    GripTarget::sphere(
        volume,
        Some(hand),
        point,
        radius,
        Mark::ring(MarkAt::Cockpit(point), CONTROL_MARK_M),
    )
}

/// A handle to reach for: a dot, ringed once a hand is within reach.
fn handle_target(volume: GrabVolume, point: Vec3, radius: f32, hand: Hand) -> GripTarget {
    GripTarget::sphere(
        volume,
        Some(hand),
        point,
        radius,
        Mark::ring(MarkAt::Cockpit(point), radius * 2.0).styled(MarkStyle::Handle),
    )
}

/// The marker on a vehicle control's grab point, metres.
const CONTROL_MARK_M: f32 = 0.05;

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
