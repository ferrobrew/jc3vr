//! The first half of the vehicle handling's input tick: read the mounted gun, fit the wheel and
//! the handlebars, and provide the vehicle handling's grip targets ([`jc3vr_body::controllers::grab`]):
//! the driver's controls, the cockpit's buttons, the parachute's handles, the ripcord, and the
//! mounted gun's handles.

use std::{sync::atomic::Ordering, time::Instant};

use glam::Vec3;
use jc3vr_body::controllers::{
    body,
    body::BodyDimensions,
    fingers::{
        grip,
        grip::{Grip, GripSource},
    },
    grab,
    grab::{
        GrabVolume, GripTarget, Mark, MarkAt, MarkStyle, Shown, Tier,
        cockpit::{Button, ControlKind},
    },
    hands,
    hands::tiers,
    mode::{Mode, VehicleSeat},
};
use jc3vr_core::game_tick::GameTick;
use jc3vr_input::frame::Hand;
use jc3vr_view::{controllers::cockpit::CockpitTransform, headpose};
use jc3vr_weapons::controllers::loadout::live;
use parking_lot::Mutex;

use crate::controllers::vehicles::{
    arm_tick::arms_act,
    config::VehicleSettings,
    driving_view,
    entry::{VehicleEntry, VehicleEntryIdentity},
    fit,
    fit::{FitInputs, VehicleLatch},
    handle_tick::handles_act,
    mounted_grip,
    mounted_tick::{MountedTick, mounted_tick, offered},
    parachute, parachute_rig, physical,
    physical::{
        ControlEvent, Placement,
        buttons::{ButtonPlacement, PushPhase},
    },
    ripcord_tick,
    state::{
        MOUNTED_WEAPON, MountedHandWeight, STATE, State, log_handle_events, mounted_hand_weight,
        push_events,
    },
    tuning::{PARACHUTE_HANDLES, PhysicalControlsTuning},
    types::TickVolumes,
    wing_guide,
};

/// The first half of the vehicle handling's input tick: read the mounted gun, place the physical
/// controls, and return the vehicle handling's grip targets. Called while the bindings inject,
/// before the tables.
pub fn begin_tick(game: &GameTick<'_>, mode: Mode) -> TickVolumes {
    let Some(active) = hands::active_hands_raw() else {
        crate::controllers::vehicles::stand_down();
        physical::visual::clear();
        return TickVolumes::default();
    };
    let config = VehicleSettings::read();
    let seat = match mode {
        Mode::Vehicle(seat) => Some(seat),
        _ => None,
    };
    let mut physical_config = PhysicalControlsTuning::of(&config.vehicles.physical);
    let xf = CockpitTransform::current(active.head.position, active.world_scale);
    let entry = seat.and_then(|_| VehicleEntry::read(game));
    // The nitrous button is only on a vehicle with nitrous, where the game's own nitrous meter
    // shows: elsewhere it would be a marker and a press that do nothing.
    if !entry.as_ref().is_some_and(VehicleEntry::has_nitrous) {
        physical_config.buttons.nitrous_button = false;
    }
    let mut events = Vec::new();
    let mut state = STATE.lock();
    let mut latch = prepare_entry(
        &mut state,
        seat,
        entry.as_ref().map(|e| e.identity),
        &mut events,
    );
    let retired_owner = state.entry_owner.take();
    drop(state);
    drop(retired_owner);
    let view_eyes =
        headpose::anchor().map(|anchor| anchor + jc3vr_view::hooks::camera::head_to_eye_point());
    let fit = fit::fit(
        &FitInputs {
            physical: &physical_config,
            view_enabled: config.vehicles.driving_view.enabled,
            seat,
            xf: &xf,
            eyes: view_eyes,
            hands: active
                .hands
                .map(|hand| hand.map(|hand| xf.point_to_world(hand.grip.position))),
        },
        entry.as_ref(),
        &mut latch,
    );
    STATE.lock().entry_owner = entry.map(VehicleEntry::into_owner);
    let yoke = fit
        .cockpit
        .is_some_and(|fit| fit.asset.stick_kind == ControlKind::Yoke);
    let offered = offered(seat, fit.fork, yoke);
    if offered.is_empty() {
        physical::visual::clear();
    }
    let placement = Placement {
        wheel: fit.wheel,
        bars: fit.bars,
        native_stick: fit.native_stick,
        cockpit: fit.cockpit,
        ..Placement::new(physical_config, config.dominant)
    };
    let grips = active.hands.map(|h| h.map(|h| h.grip.position));
    let aiming = STATE
        .lock()
        .mounted_grip
        .aiming(grips.map(|g| g.is_some()));
    let mounted = mounted_tick(game, &active, aiming);
    // A gunner seat's view is corrected from its gun's handles, as a driver's from the wheel.
    let gunner_view = (seat.is_some_and(|s| !s.driver) && fit.view.is_none())
        .then(|| gunner_view_target(&xf, view_eyes, &mounted))
        .flatten();
    if gunner_view.is_none() {
        *GUNNER_VIEW.lock() = None;
    }
    driving_view::publish(fit.view.or(gunner_view));
    let handles_on = handles_act(mode, &config);
    let native_centres = parachute_rig::centres(handles_on, &xf);

    let mut handle_events = Vec::new();
    let mut state = STATE.lock();
    let previous_native = state.placement.and_then(|placement| placement.native_stick);
    physical::refresh_native(
        &mut state.controls,
        previous_native,
        placement.native_stick,
        &mut events,
    );
    let previous_cockpit = state.placement.and_then(|placement| placement.cockpit);
    physical::refresh_cockpit(
        &mut state.controls,
        previous_cockpit,
        placement.cockpit,
        &mut events,
    );
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
                    mounted_grip::GRAB_RADIUS_M,
                    hand,
                )
                .held(hand),
            );
        }
    }
    // A hand holding something of the table's (a weapon, a prop, a grenade, or a charge) takes no
    // new control, button, or handle: it keeps only the one it holds, so the player drives
    // one-handed or docks first.
    let holding = Hand::BOTH.map(|hand| {
        state.controls.holds[hand.index()].is_some()
            || state.mounted_grip.holds[hand.index()]
            || live::held_kind(hand).is_some()
    });
    let controls = physical::candidates(&placement, &state.controls, offered, grips);
    targets.extend(
        controls
            .iter()
            .filter(|c| c.hand.is_none_or(|h| !holding[h.index()]))
            .map(|c| {
                let mark = Mark::ring(MarkAt::Cockpit(c.centre), CONTROL_MARK_M);
                let mark = match c.volume {
                    GrabVolume::VehicleControl(kind)
                        if matches!(
                            kind,
                            ControlKind::Cyclic
                                | ControlKind::FlightStick
                                | ControlKind::Yoke
                                | ControlKind::Collective
                                | ControlKind::Throttle
                        ) =>
                    {
                        mark.labelled(kind.label()).showing(Shown::InReach)
                    }
                    _ => mark.showing(Shown::Always),
                };
                c.target(mark)
            }),
    );
    targets.extend(
        mounted_grip::candidates(mounted.handle_points)
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
        // Each button is marked only once a hand is at it: the horn is the wheel's drawn hub, and
        // the nitrous button is named by its label then.
        let label = match button {
            Button::Horn => None,
            Button::Nitrous => Some(NITROUS_LABEL),
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
        let mark = Mark::ring(MarkAt::Cockpit(at.centre), at.radius * 2.0)
            .on_surface(at.normal)
            .styled(MarkStyle::Button)
            .showing(Shown::InReach);
        let target = GripTarget::sphere(
            GrabVolume::CockpitButton(button),
            only,
            at.centre,
            at.radius,
            match label {
                Some(label) => mark.labelled(label),
                None => mark,
            },
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
        let placed = parachute::candidates(&PARACHUTE_HANDLES, &state.handles, centres, grips);
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
            parachute::reach_candidates(&state.handles, centres)
                .filter(|c| {
                    c.hand.is_some_and(|hand| {
                        !holding[hand.index()] && native_centres[hand.index()].is_some()
                    })
                })
                .map(|c| {
                    // The reach is where a press takes the handle, so it shows while a hand is
                    // in it rather than only the handle's own smaller ring.
                    c.target(
                        Mark::ring(MarkAt::Cockpit(c.centre), c.radius).showing(Shown::InReach),
                    )
                    .ranked(Tier::Fallback)
                }),
        );
    }
    targets.extend(ripcord_tick::targets(mode, &active, &config));
    let grips = grab::tick::hand_grips(&active);
    let volumes = grab::resolve::resolve(&targets, grips).map(|r| r.chosen.map(|c| c.volume));
    state.handles_on = handles_on;
    state.placement = Some(placement);
    state.latch = latch;
    state.render = fit.render;
    state.steering_turn = fit.steering_turn;
    state.status.fit = fit.readout;
    state.status.wheel = seat.is_some_and(|s| s.driver).then_some(fit.wheel);
    state.mounted_target = mounted.target;
    MOUNTED_WEAPON.store(mounted.weapon.unwrap_or(0), Ordering::Relaxed);
    state.mounted_handles = mounted.handles.or(state.mounted_handles);
    state.mounted_handle_points = mounted.handle_points;
    state.mounted_barrel = mounted.barrel;
    declare_mounted_grips(mounted.handles, state.mounted_grip.shares());
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
    wing_guide::submit(arms_act(mode, &config), arms_engaged);
    TickVolumes { targets }
}

fn prepare_entry(
    state: &mut State,
    seat: Option<VehicleSeat>,
    identity: Option<VehicleEntryIdentity>,
    events: &mut Vec<ControlEvent>,
) -> Option<VehicleLatch> {
    state.observe_entry(seat, identity, events);
    state.latch.take()
}

/// The gunner seat's view correction for the manned gun of `mounted`: its handles' midpoint from
/// the character's eyes `eyes` (the anchor's, before any correction), averaged once the gunner has
/// settled ([`driving_view::Entry`]) and kept for the seat.
fn gunner_view_target(
    xf: &CockpitTransform,
    eyes: Option<Vec3>,
    mounted: &MountedTick,
) -> Option<driving_view::Target> {
    let gun = mounted.weapon?;
    let [Some(left), Some(right)] = mounted.handle_points else {
        return None;
    };
    let midpoint = xf.point_to_world((left + right) * 0.5);
    let from_eyes = eyes
        .map(|eyes| xf.offset_to_cockpit(midpoint - eyes))
        .filter(|offset| offset.is_finite());
    let mut latch = GUNNER_VIEW.lock();
    let (latched_gun, since, entry) =
        latch.get_or_insert_with(|| (gun, Instant::now(), driving_view::Entry::new()));
    if *latched_gun != gun {
        *latch = Some((gun, Instant::now(), driving_view::Entry::new()));
        return None;
    }
    if entry.observe(since.elapsed(), from_eyes) {
        tracing::info!(
            target: "controllers",
            handles_from_eyes = ?entry.mean(),
            "vehicles: the gunner seat's handles measured for the view"
        );
    }
    entry.mean().map(|mean| driving_view::Target {
        vehicle: gun,
        correction: driving_view::derive_handles(mean),
    })
}

/// The gunner seat's measurement: the gun, when the seat began, and the handles' entry.
static GUNNER_VIEW: Mutex<Option<(usize, Instant, driving_view::Entry)>> = Mutex::new(None);

#[cfg(test)]
mod tests;

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

/// The nitrous button's label.
const NITROUS_LABEL: &str = "Nitrous";

/// Declare each hand the mounted gun's handles anchor, for its finger pose, at the anchor's share
/// times the hand's hold on its handle, `shares`.
fn declare_mounted_grips(handles: Option<[Option<Vec3>; 2]>, shares: [f32; 2]) {
    let tiers = tiers::status();
    let Some(handles) = handles else {
        return;
    };
    for hand in Hand::BOTH {
        let i = hand.index();
        let inputs = MountedHandWeight {
            manned: tiers.state == Some(tiers::TierState::MountedGun),
            grapple: matches!(
                tiers.state,
                Some(tiers::TierState::GrappleReel | tiers::TierState::GrappleHang)
            ),
            anchor_weight: tiers.anchor_weights[i],
            share: shares[i],
        };
        if let Some(grip) = mounted_finger_grip(handles[i], inputs) {
            grip::declare(hand, grip);
        }
    }
}

fn mounted_finger_grip(handle: Option<Vec3>, inputs: MountedHandWeight) -> Option<Grip> {
    handle?;
    let weight = mounted_hand_weight(inputs);
    (weight > 0.0).then(|| Grip::new(GripSource::MountedGun).share(weight))
}

/// The dimensions the hands are measured against: the character's while the hands are remapped
/// onto them, the player's otherwise.
pub(super) fn body_dimensions(config: &VehicleSettings) -> BodyDimensions {
    if config.remap_hands {
        BodyDimensions::CHARACTER_ESTIMATE
    } else {
        body::player()
    }
}
