use std::{num::NonZeroUsize, time::Instant};

use body_rig::rotation::level;
use glam::{Vec2, Vec3};
use jc3gi::character::character::Character;
use jc3vr_body::controllers::{
    aim::raycast::CastOptions,
    body,
    clock::TimeScale,
    grab::world_reach::{Reach, TargetKind},
    hands, haptics,
};
use jc3vr_input::{
    frame::{Hand, Pose},
    haptics::cues::Event,
};
use jc3vr_view::{controllers::cockpit::CockpitTransform, headpose::teleport};

use crate::controllers::physical_hands::{
    config::PHYSICAL_HANDS_CONFIG,
    contact::{HandContact, PointContact, Surface, SurfaceHit},
    doors,
    doors::{
        acquisition::PanelReach,
        config::{TEARING, TearTuning},
    },
    draw, grab_view,
    grab_view::model::{RayOutcome, ReachRay},
    grasp,
    grasp::PalmMotion,
    palm::{PROBE_POINTS, PalmBox},
    physics, pickup,
    pickup::{
        HeldKind, PICKUP, PickupState,
        diagnostics::{ConsumerResult, scan::Scan},
    },
    query,
    state::{STATE, VELOCITY_SMOOTHING_S, stand_down},
    status::{HandStatus, Idle},
    tick::{declare_grips, free_hands},
    touch::pressure_haptics,
    tuning::PALMS,
};

pub(super) unsafe fn run(owner: &Character, dt: f32) {
    let Some(vehicle) = NonZeroUsize::new(unsafe { owner.GetVehiclePtr() } as usize) else {
        stand_down(Idle::Mode);
        return;
    };
    let Some(active) = hands::active_hands_raw() else {
        stand_down(Idle::NoHands);
        return;
    };
    let config = *PHYSICAL_HANDS_CONFIG.lock();
    let time_scale = unsafe { TimeScale::current() };
    let xf = CockpitTransform::current(active.head.position, active.world_scale);
    let shoulders = body::shoulder_positions(
        active.head.position,
        level(active.head.orientation),
        &body::player(),
    )
    .map(|point| xf.point_to_world(point));
    let free = free_hands();
    let options = CastOptions {
        material: true,
        ..unsafe { CastOptions::ignoring_player(0) }
    };
    let mut casts = 0;
    let mut error = None;
    let mut cast = |from: Vec3, to: Vec3| match unsafe { query::cast(from, to, &options) } {
        Ok(hit) => {
            casts += 1;
            hit
        }
        Err(why) => {
            casts += 1;
            error = Some(why.to_string());
            None
        }
    };
    let mut state = STATE.lock();
    let epoch = teleport::epoch();
    if state.epoch != epoch || state.seated_vehicle != Some(vehicle) {
        state.epoch = epoch;
        state.reset_probes();
        state.pickup.drop_all();
        state.seated_vehicle = Some(vehicle);
    }
    scope(&mut state.pickup, vehicle);
    let enabled = config.pickup.enabled && config.doors.enabled;
    if !enabled {
        state.pickup.drop_all();
    }
    let now = Instant::now();
    let viewing = grab_view::recording(&config);
    let mut palms = [None; 2];
    let mut wrists = [None; 2];
    let mut hand_status = [HandStatus::default(); 2];
    let mut clamp = [Vec3::ZERO; 2];
    let mut scans = [Scan::new(); 2];
    let notes = [Vec::new(), Vec::new()];
    let mut rays = [Vec::new(), Vec::new()];
    let handles = [Vec::new(), Vec::new()];
    for hand in Hand::BOTH {
        let i = hand.index();
        let speed = state.hand_velocity(hand).length();
        let Some(cockpit) = active.hands[i].filter(|_| free[i]) else {
            state.motion[i].0.reset();
            state.motion[i].1.reset();
            state.door_press[i] = doors::pressure::Press::new();
            continue;
        };
        let wrist = xf.pose_to_world(cockpit.wrist);
        wrists[i] = Some(wrist);
        let palm = PalmBox::from_wrist(wrist, active.world_scale, &PALMS);
        let motion = PalmMotion {
            pose: Pose {
                position: palm.centre,
                orientation: palm.orientation,
            },
            velocity: state.motion[i].0.update(
                palm.centre,
                dt,
                time_scale.game_s(VELOCITY_SMOOTHING_S),
            ),
            angular_velocity: state.motion[i].1.update(
                palm.orientation,
                dt,
                time_scale.game_s(VELOCITY_SMOOTHING_S),
            ),
        };
        palms[i] = Some(motion);
        hand_status[i] = HandStatus {
            acting: enabled,
            palm: Some(palm),
            speed,
            ..HandStatus::default()
        };
        if !enabled || state.pickup.holding(hand) {
            state.door_press[i] = doors::pressure::Press::new();
            continue;
        }
        let mut contacts = [PointContact::Clear; PROBE_POINTS];
        for (k, point) in palm.probe_points().into_iter().enumerate() {
            state.probes[i][k].reset();
            contacts[k] = state.probes[i][k].step(point, shoulders[i], &mut cast);
        }
        let contact = HandContact::fold(&contacts);
        let contact_hit = contacts
            .iter()
            .filter_map(|contact| match contact {
                PointContact::Pressed { hit, .. }
                    if palm.centre.distance(hit.position)
                        <= doors::pressure::REACH_M * active.world_scale =>
                {
                    Some(*hit)
                }
                _ => None,
            })
            .min_by(|a, b| {
                palm.centre
                    .distance_squared(a.position)
                    .total_cmp(&palm.centre.distance_squared(b.position))
            });
        let pressure_hit = match unsafe { doors::pressure::probe(shoulders[i], &palm, &options) } {
            Ok(probe) => state.door_press[i].observe_probe(probe),
            Err(why) => {
                state.door_press[i].reset();
                state.status.pickup.last_error = Some(why.to_string());
                None
            }
        };
        let pressure_panel = pressure_hit.and_then(|hit| unsafe {
            panel_reach_with(
                vehicle,
                hit,
                &palm,
                PanelReach::uniform(doors::pressure::REACH_M * active.world_scale),
                &config.doors,
                &mut scans[i],
            )
        });
        let panel = contact_hit.and_then(|hit| unsafe {
            panel_reach(
                vehicle,
                hit,
                &palm,
                active.world_scale,
                &config.doors,
                &mut scans[i],
            )
        });
        match unsafe {
            state.door_press[i].step(doors::pressure::Input {
                reach: pressure_panel,
                hit: pressure_hit,
                palm: palm.centre,
                dt,
                world_scale: active.world_scale,
                options: &options,
            })
        } {
            Ok(Some(slot)) => {
                state.status.pickup.unlatches += 1;
                state.status.pickup.last_door = Some(slot);
                haptics::play(hand, Event::Unlatch);
            }
            Ok(None) => {}
            Err(why) => state.status.pickup.last_error = Some(why.to_string()),
        }
        if state.door_press[i].take_contact_cue() {
            haptics::play(hand, Event::DoorPressureContact);
        }
        let mut reaches: Vec<Reach> = panel.into_iter().collect();
        let origin = palm.point(Vec3::Y);
        let length = PICKUP.reach_m * active.world_scale + 2.0 * palm.half_extents.y;
        scans[i].ray_ran = true;
        for direction in pickup::REACH_DIRECTIONS {
            let to = origin + (palm.orientation * direction).normalize_or_zero() * length;
            let hit = cast(origin, to);
            let reached = hit.and_then(|hit| unsafe {
                panel_reach(
                    vehicle,
                    hit,
                    &palm,
                    active.world_scale,
                    &config.doors,
                    &mut scans[i],
                )
            });
            reaches.extend(reached);
            if viewing {
                rays[i].push(ReachRay {
                    from: origin,
                    to,
                    hit: hit.map(|hit| hit.position),
                    outcome: RayOutcome::of(hit.is_some(), reached.is_some()),
                });
            }
        }
        let reach = grasp::nearest(reaches);
        scans[i].selected = reach;
        hand_status[i].reach = reach;
        if panel.is_some() {
            hand_status[i].press = contact.press.map(|(_, depth)| (depth, true));
            clamp[i] = xf.offset_to_cockpit(contact.clamp_offset());
        }
    }
    let mut refused = [None, None];
    if enabled {
        let strength = if config.bodies.strength.is_finite() {
            config.bodies.strength.max(0.0)
        } else {
            0.0
        };
        let tuning = pickup::PickupTuning {
            hand_force_n: PICKUP.hand_force_n * strength,
            hand_torque_nm: PICKUP.hand_torque_nm * strength,
            ..PICKUP.configured(&config.pickup)
        };
        let inputs = pickup::TickInputs {
            palms,
            space: None,
            joints: pickup::JointInputs::default(),
            dt,
            time_scale: time_scale.get(),
            unlatch_pull_m: doors::UNLATCH_PULL_M * active.world_scale,
            tearing: TearTuning {
                enabled: config.doors.tear,
                ..TEARING
            },
            world_scale: active.world_scale,
            characters: config.characters,
            reaches: hand_status.map(|hand| hand.reach),
            carrier_velocity: Some(Vec3::from_array(owner.m_MeasuredWorldVelocity.data)),
            carrier_pose: Some(Pose {
                position: xf.point_to_world(Vec3::ZERO),
                orientation: xf.body,
            }),
        };
        let mut outcome = unsafe { pickup::tick(&mut state.pickup, inputs, &tuning) };
        scope(&mut state.pickup, vehicle);
        for hand in Hand::BOTH {
            let i = hand.index();
            if state.pickup.held[i].is_none() {
                outcome.reports[i] = None;
            }
            if let Some(palm) = palms[i] {
                let target = outcome.pins[i]
                    .filter(|_| state.pickup.latches[i].is_some())
                    .or_else(|| {
                        outcome.reports[i]
                            .map(|held| palm.pose.position + held.anchor - held.target)
                    });
                if let Some(target) = target {
                    clamp[i] = xf.offset_to_cockpit(
                        (target - palm.pose.position).clamp_length_max(PICKUP.tear_distance_m),
                    );
                }
            }
            hand_status[i].latched = state.pickup.latched(hand);
        }
        refused.clone_from(&outcome.refused);
        state.record_pickup(outcome, &mut hand_status);
    }
    declare_grips(&state.pickup, &hand_status, wrists, &mut cast);
    pressure_haptics(&hand_status);
    state.status.idle = None;
    state.status.hands = hand_status;
    state.status.push = Vec2::ZERO;
    state.status.casts = casts;
    if let Some(error) = error {
        state.status.last_error = Some(error);
    }
    state.pickup.scans = scans;
    state.reach_notes = notes;
    state.clamp = clamp;
    state.at = Some(now);
    physics::publish_holds([None; 2]);
    physics::publish_pins([None; 2]);
    drop(state);
    draw::submit(&hand_status, &config);
    for hand in Hand::BOTH {
        if let Some((at, why)) = &refused[hand.index()] {
            grab_view::refused(hand, Some(*at), why);
        }
    }
    if viewing {
        grab_view::record(grab_view::Recorded {
            head: xf.head_world,
            rays,
            handles,
        });
    }
}

fn scope(state: &mut PickupState, vehicle: NonZeroUsize) {
    state.seizes = [None; 2];
    state.regrips = [None; 2];
    state.hijack_clicks = [false; 2];
    for hand in Hand::BOTH {
        let i = hand.index();
        if state.requests[i].is_some_and(|request| {
            request
                .grab()
                .is_some_and(|(reach, _)| !in_scope(reach, vehicle))
        }) {
            state.replace_request(hand, None, ConsumerResult::Cleared);
        }
        if state.latches[i].is_some_and(|latch| latch.door.vehicle != vehicle) {
            state.latches[i] = None;
        }
        let attached = state.doors[i].is_some_and(|grip| grip.door.vehicle == vehicle);
        if state.held[i].is_some_and(|held| held.joined || held.kind != HeldKind::Door || !attached)
        {
            state.set_held(i, None);
        }
        if state.held[i].is_none() {
            state.doors[i] = None;
        }
        if state.held[i].is_none() && state.latches[i].is_none() {
            state.evidence_holds[i] = None;
        }
    }
}

fn in_scope(reach: Reach, vehicle: NonZeroUsize) -> bool {
    matches!(reach.kind, TargetKind::Door(door) if door.vehicle == vehicle)
}

unsafe fn panel_reach(
    vehicle: NonZeroUsize,
    hit: SurfaceHit,
    palm: &PalmBox,
    world_scale: f32,
    config: &doors::DoorsConfig,
    scan: &mut Scan,
) -> Option<Reach> {
    unsafe {
        panel_reach_with(
            vehicle,
            hit,
            palm,
            PanelReach {
                box_m: doors::REACH_M * world_scale,
                handle_m: doors::HANDLE_REACH_M * world_scale,
            },
            config,
            scan,
        )
    }
}

unsafe fn panel_reach_with(
    vehicle: NonZeroUsize,
    hit: SurfaceHit,
    palm: &PalmBox,
    reach: PanelReach,
    config: &doors::DoorsConfig,
    scan: &mut Scan,
) -> Option<Reach> {
    let Surface::Solid {
        object: Some(object),
        body,
    } = hit.surface
    else {
        return None;
    };
    unsafe { doors::observed_reach(object, body, &hit, palm.centre, reach, config, scan) }
        .filter(|reach| in_scope(*reach, vehicle))
}

#[cfg(test)]
mod tests {
    use glam::Quat;
    use jc3gi::vehicle::vehicle::DoorSlot;
    use jc3vr_body::controllers::{grab::world_reach::DoorTarget, physics::rigid::BodyPose};

    use super::*;
    use crate::controllers::physical_hands::{
        doors::{Latch, tearing::TearGrip},
        grasp::Grip,
        pickup::{Held, Request, diagnostics::RequestKey},
    };

    fn vehicle() -> NonZeroUsize {
        NonZeroUsize::new(64).unwrap()
    }

    fn door() -> DoorTarget {
        DoorTarget {
            vehicle: vehicle(),
            vehicle_body: Some(3),
            part_index: 1,
            slot: DoorSlot::FRONT_LEFT,
            latched: false,
            outward: Vec3::X,
        }
    }

    fn reach(kind: TargetKind) -> Reach {
        Reach {
            object: NonZeroUsize::new(128).unwrap(),
            body: 7,
            point: Vec3::ZERO,
            normal: Vec3::X,
            distance: 0.01,
            kind,
        }
    }

    fn held(kind: HeldKind, joined: bool) -> Held {
        Held {
            object: reach(TargetKind::Door(door())).object,
            body: 7,
            grip: Grip::of(
                Pose::default(),
                BodyPose {
                    centre: Vec3::ZERO,
                    orientation: Quat::IDENTITY,
                },
                Vec3::ZERO,
            ),
            mass_kg: 5.0,
            ragdoll: None,
            joined,
            kind,
        }
    }

    #[test]
    fn only_the_occupied_vehicles_doors_are_in_scope() {
        assert!(in_scope(reach(TargetKind::Door(door())), vehicle()));
        let other = DoorTarget {
            vehicle: NonZeroUsize::new(256).unwrap(),
            ..door()
        };
        assert!(!in_scope(reach(TargetKind::Door(other)), vehicle()));
        assert!(!in_scope(reach(TargetKind::Weapon), vehicle()));
    }

    #[test]
    fn scoping_rejects_pending_grabs_but_preserves_release_and_input_age() {
        let mut state = PickupState::new();
        let input_at = Instant::now();
        state.input_at = Some(input_at);
        for request in [
            Request::Grab(reach(TargetKind::Weapon)),
            Request::ObservedGrab(RequestKey {
                sequence: 1,
                episode: 1,
                request: 1,
                target: reach(TargetKind::Weapon),
            }),
            Request::Grab(reach(TargetKind::Door(DoorTarget {
                vehicle: NonZeroUsize::new(256).unwrap(),
                ..door()
            }))),
        ] {
            state.requests = [Some(request), Some(Request::Release)];
            scope(&mut state, vehicle());
            assert_eq!(state.requests, [None, Some(Request::Release)]);
            assert_eq!(state.input_at, Some(input_at));
        }
        for request in [
            Request::Grab(reach(TargetKind::Door(door()))),
            Request::ObservedGrab(RequestKey {
                sequence: 1,
                episode: 1,
                request: 1,
                target: reach(TargetKind::Door(door())),
            }),
        ] {
            state.requests[0] = Some(request);
            scope(&mut state, vehicle());
            assert_eq!(state.requests[0], Some(request));
        }
    }

    #[test]
    fn only_nonjoined_attached_door_holds_survive_scoping() {
        for (kind, joined, attached, retained) in [
            (HeldKind::Door, false, true, true),
            (HeldKind::Door, true, true, false),
            (HeldKind::Door, false, false, false),
            (HeldKind::Prop, false, true, false),
            (HeldKind::Articulated, false, true, false),
            (HeldKind::Ragdoll, false, true, false),
        ] {
            let mut state = PickupState::new();
            state.held[0] = Some(held(kind, joined));
            if attached {
                state.doors[0] = Some(TearGrip::fixture(door(), 3));
            }
            state.hijack_clicks = [true; 2];
            scope(&mut state, vehicle());
            assert_eq!(state.held[0].is_some(), retained);
            assert_eq!(state.doors[0].is_some(), retained);
            assert_eq!(state.hijack_clicks, [false; 2]);
        }
    }

    #[test]
    fn another_vehicles_holds_and_latches_are_removed() {
        let mut state = PickupState::new();
        let other = DoorTarget {
            vehicle: NonZeroUsize::new(256).unwrap(),
            ..door()
        };
        state.held[0] = Some(held(HeldKind::Door, false));
        state.doors[0] = Some(TearGrip::fixture(other, 3));
        state.latches = [door(), other].map(|door| {
            Some(Latch {
                door,
                vehicle_body: 3,
                grab: Vec3::ZERO,
                outward: Vec3::X,
                palm: Pose::default(),
                opening_at: None,
            })
        });
        scope(&mut state, vehicle());
        assert_eq!(state.held, [None; 2]);
        assert_eq!(state.doors, [None; 2]);
        assert!(state.latches[0].is_some());
        assert!(state.latches[1].is_none());
    }
}
