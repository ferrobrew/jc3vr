//! The palms' simulation tick, after the player aim control's post-simulation update: the probes,
//! the pushes, the shoves, and the pickups, folded into the readout.

use std::{sync::atomic::Ordering, time::Instant};

use glam::{Mat4, Quat, Vec2, Vec3};
use jc3gi::character::character::Character;
use windows::Win32::System::Threading::GetCurrentThreadId;

use crate::{
    config::Config,
    controllers::{
        aim::{
            ray::Ray,
            raycast::{self, CastOptions},
        },
        body,
        fingers::grip::{
            self, Grip, GripSource,
            catalogue::GripPose,
            contact::{self as finger_contact, CastHit, FitTarget},
        },
        frame::{Hand, Pose},
        hands::{self, pose as hand_pose, reach, tiers},
        mode::{self, AirborneVariant, Detection, Mode, OnFootVariant},
        physical_hands::{
            PhysicalHandsConfig, climb,
            contact::{HandContact, PointContact, PointProbe, Surface, SurfaceHit},
            doors, draw, engine, grab_view, grasp,
            grasp::PalmMotion,
            hold::{self, BodySpace, PlayerKeepOut},
            palm::{PROBE_POINTS, PalmBox},
            physics, pickup, push,
            shove::ShoveTuning,
            state::{MAIN_THREAD, STATE, VELOCITY_SMOOTHING_S, stand_down},
            status::{HandStatus, Idle},
            touch::{ShoveInput, Touch, pressure_haptics},
        },
        roomscale::teleport,
        scheme,
        weapons::{self, c4, foregrip, grenade},
    },
    headpose, hooks,
};

/// The palms' tick, after the player aim control's post-simulation update.
///
/// # Safety
///
/// `owner` must be the live local player character, called from its aim control's update after
/// the original has run.
pub(crate) unsafe fn after_aim(owner: &Character, dt: f32) {
    physics::observe(dt);
    let config = Config::lock_query(|c| c.controllers.physical_hands);
    let idle = if !config.enabled || !scheme::is_controllers() || !hooks::in_gameplay() {
        Some(Idle::Disabled)
    } else if MAIN_THREAD.load(Ordering::Relaxed) != unsafe { GetCurrentThreadId() } {
        Some(Idle::OffThread)
    } else {
        (!mode_acts(mode::current())).then_some(Idle::Mode)
    };
    if let Some(idle) = idle {
        stand_down(idle);
        return;
    }
    let Some(active) = hands::active_hands_raw() else {
        stand_down(Idle::NoHands);
        return;
    };
    let head = headpose::query();
    let body_rotation = headpose::xr::body_rotation();
    let compose = |pose: Pose| {
        hand_pose::compose_world(
            head.position,
            body_rotation,
            active.head.position,
            pose,
            active.world_scale,
        )
    };
    let at = |point: Vec3| {
        compose(Pose {
            position: point,
            orientation: Quat::IDENTITY,
        })
        .position
    };
    let shoulders = body::shoulder_positions(
        active.head.position,
        reach::yaw_only(active.head.orientation),
        &body::player(),
    )
    .map(at);
    let free = free_hands();
    let to_world = Mat4::from(owner.m_WorldMatrixT1);
    let space = (config.pickup.keep_out_radius_m > 0.0).then(|| {
        body::procedural::torso_keep_out(to_world).map_or_else(
            || {
                PlayerKeepOut::Cylinder(BodySpace {
                    base: to_world.w_axis.truncate(),
                    height_m: body::player().height * active.world_scale,
                    radius_m: config.pickup.keep_out_radius_m * active.world_scale,
                    forward: body_rotation * Vec3::NEG_Z,
                })
            },
            PlayerKeepOut::Torso,
        )
    });
    let shove_tuning = ShoveTuning::of(&config.shove);
    // SAFETY: forwarded from the caller.
    let melee_impulse = unsafe { engine::melee_impulse(owner) };
    // SAFETY: forwarded from the caller.
    let standing_on = unsafe { engine::ground_body(owner) };

    let mut state = STATE.lock();
    let now = Instant::now();
    if teleport::epoch() != state.epoch {
        state.epoch = teleport::epoch();
        state.reset_probes();
        state.pickup.drop_all();
    }
    let pickup_on = config.pickup.enabled;
    if !pickup_on {
        state.pickup.drop_all();
    }
    // The palms pass through what a hand holds, so that a free palm never pushes off it; the reach
    // rays do not, so that the other hand can take hold of it too.
    let held_objects = state.pickup.held_objects();
    let probe_options = CastOptions {
        ignore: [
            owner as *const Character as usize,
            held_objects.into_iter().find(|&o| o != 0).unwrap_or(0),
        ],
        see_through: false,
        // The impact sounds play against the surface's material.
        material: config.sounds.enabled,
    };
    let reach_options = CastOptions {
        ignore: [owner as *const Character as usize, 0],
        see_through: false,
        material: false,
    };
    let mut casts = 0;
    let mut error = None;
    let mut cast = |from: Vec3, to: Vec3, options: &CastOptions| -> Option<SurfaceHit> {
        casts += 1;
        let ray = Ray::new(from, to - from)?;
        // SAFETY: the simulation phase outside the physics step; the ignored objects are the live
        // local character and the objects the hands hold, which this tick's pickup checks.
        match unsafe { raycast::cast(&ray, (to - from).length(), options) } {
            Ok(hit) => hit.map(|hit| unsafe { engine::surface(&hit) }),
            Err(e) => {
                error = Some(e.to_string());
                None
            }
        }
    };

    let reach_tuning = pickup::ReachTuning {
        world_scale: active.world_scale,
        pickup: &config.pickup,
        doors: &config.doors,
        characters: &config.characters,
        hand_bodies: physics::status().hands.map(|h| h.body_id),
    };
    // The vehicles whose doors the free palms might reach, offered by their handles.
    let door_vehicles = if pickup_on && config.doors.enabled {
        let near: Vec<Vec3> = Hand::BOTH
            .into_iter()
            .filter(|h| free[h.index()])
            .filter_map(|h| active.hands[h.index()].map(|c| compose(c.wrist).position))
            .collect();
        // SAFETY: the simulation phase outside the physics step, on the aim update's thread.
        unsafe { doors::scan::near(&near, active.world_scale) }
    } else {
        Vec::new()
    };
    let climb_on = climb::enabled(&config);
    let mut climb_palms = [None; 2];
    let mut climb_pressing = [false; 2];
    let mut climb_reaches = [None; 2];
    let mut touches = Vec::new();
    let mut reach_notes = [Vec::new(), Vec::new()];
    // The grab view's record of the reach, only while it is on.
    let viewing = grab_view::recording(&config);
    let mut rays = [Vec::new(), Vec::new()];
    let mut handles = [Vec::new(), Vec::new()];
    let near_hands = Hand::BOTH.map(|hand| {
        active.hands[hand.index()]
            .filter(|_| viewing)
            .map(|c| compose(c.wrist).position)
    });
    let mut hand_status = [HandStatus::default(); 2];
    let mut clamp = [Vec3::ZERO; 2];
    let mut palms = [None; 2];
    let mut wrists = [None; 2];
    for hand in Hand::BOTH {
        let i = hand.index();
        let velocity = state.hand_velocity(hand);
        let speed = velocity.length();
        let Some(cockpit) = active.hands[i].filter(|_| free[i]) else {
            state.probes[i].iter_mut().for_each(PointProbe::reset);
            state.pressed[i] = false;
            state.gives[i].reset();
            state.motion[i].0.reset();
            state.motion[i].1.reset();
            continue;
        };
        let palm = PalmBox::from_wrist(compose(cockpit.wrist), active.world_scale, &config);
        let motion = PalmMotion {
            pose: Pose {
                position: palm.centre,
                orientation: palm.orientation,
            },
            velocity: state.motion[i]
                .0
                .update(palm.centre, dt, VELOCITY_SMOOTHING_S),
            angular_velocity: state.motion[i]
                .1
                .update(palm.orientation, dt, VELOCITY_SMOOTHING_S),
        };
        palms[i] = Some(motion);
        wrists[i] = Some(compose(cockpit.wrist));
        if state.pickup.holding(hand) {
            // A holding hand neither probes nor pushes: its palm is around the object.
            state.probes[i].iter_mut().for_each(PointProbe::reset);
            state.pressed[i] = false;
            state.gives[i].reset();
            hand_status[i] = HandStatus {
                acting: true,
                palm: Some(palm),
                speed,
                ..HandStatus::default()
            };
            continue;
        }
        if physics::drives(hand) {
            // A hand its body drives does not probe, push, or shove: the body meets the world
            // itself. It still reaches for what its grip would take, from where its body is
            // rather than the controller, so that it reaches what the body touches: a body held
            // at a wall is drawn there while the controller is through it.
            state.probes[i].iter_mut().for_each(PointProbe::reset);
            state.pressed[i] = false;
            state.gives[i].reset();
            let body_palm =
                physics::body_pose(hand).map_or(palm, |(centre, orientation)| PalmBox {
                    centre,
                    orientation,
                    half_extents: palm.half_extents,
                });
            let reach = if pickup_on {
                // SAFETY: the simulation phase outside the physics step.
                unsafe {
                    pickup::reach(
                        &body_palm,
                        None,
                        standing_on,
                        &reach_tuning,
                        &mut |from, to| cast(from, to, &reach_options),
                        &mut reach_notes[i],
                        viewing.then(|| &mut rays[i]),
                    )
                }
            } else {
                None
            };
            // SAFETY: the simulation phase outside the physics step; the vehicles are this tick's.
            let door = unsafe {
                doors::scan::handle_reach(
                    body_palm.centre,
                    &door_vehicles,
                    active.world_scale,
                    &config.doors,
                    &mut |from, to| cast(from, to, &reach_options),
                    &mut reach_notes[i],
                    viewing.then(|| &mut handles[i]),
                )
            };
            let reach = grasp::nearest(reach.into_iter().chain(door));
            if climb_on {
                // The palm in the rig: its tracking-space place turned and scaled as the world
                // palm is, but not carried by the head's world position, which the climb moves.
                let rig = PalmBox::from_wrist(cockpit.wrist, 1.0, &config).centre;
                climb_palms[i] = Some(body_rotation * rig * active.world_scale);
                climb_pressing[i] = physics::pressing(hand);
                // A loose object in reach wins over the world it rests on.
                climb_reaches[i] = if reach.is_none() {
                    // SAFETY: the simulation phase outside the physics step.
                    unsafe {
                        climb::reach(
                            &body_palm,
                            config.climb.reach_m * active.world_scale,
                            &config.climb,
                            &mut |from, to| cast(from, to, &probe_options),
                        )
                    }
                } else {
                    None
                };
            }
            hand_status[i] = HandStatus {
                palm: Some(palm),
                speed,
                reach,
                ..HandStatus::default()
            };
            continue;
        }
        let points = palm.probe_points();
        let mut contacts = [PointContact::Clear; PROBE_POINTS];
        for (k, point) in points.into_iter().enumerate() {
            contacts[k] = state.probes[i][k].step(point, shoulders[i], &mut |from, to| {
                cast(from, to, &probe_options)
            });
        }
        let contact = HandContact::fold(&contacts);
        let world_velocity = body_rotation * velocity;
        let pressed = contact.press.is_some();
        if let Some((hit, _)) = contact.press
            && !state.pressed[i]
        {
            touches.push(Touch {
                hand,
                hit,
                velocity: world_velocity,
            });
        }
        state.pressed[i] = pressed;
        let shove = match contact.press {
            Some((hit, depth)) if config.shove.enabled => {
                // SAFETY: the simulation phase outside the physics step; the hit was found by
                // this tick's casts.
                unsafe {
                    state.shove(
                        hand,
                        ShoveInput {
                            hit,
                            depth,
                            hand_velocity: motion.velocity,
                            dt,
                        },
                        &shove_tuning,
                        &config,
                    )
                }
            }
            _ => {
                state.gives[i].reset();
                None
            }
        };
        let resistance = shove.map_or(1.0, |s| s.resistance);
        let hand_push = contact.press.map_or(Vec2::ZERO, |(hit, depth)| {
            push::hand_push(hit.normal, depth, &config) * resistance
        });
        let offset = contact.clamp_offset();
        clamp[i] = (body_rotation.inverse() * offset) / active.world_scale.max(1e-3);
        let reach = if pickup_on {
            // SAFETY: the simulation phase outside the physics step.
            unsafe {
                pickup::reach(
                    &palm,
                    contact.press.map(|(hit, _)| hit),
                    standing_on,
                    &reach_tuning,
                    &mut |from, to| cast(from, to, &reach_options),
                    &mut reach_notes[i],
                    viewing.then(|| &mut rays[i]),
                )
            }
        } else {
            None
        };
        // SAFETY: the simulation phase outside the physics step; the vehicles are this tick's.
        let door = unsafe {
            doors::scan::handle_reach(
                palm.centre,
                &door_vehicles,
                active.world_scale,
                &config.doors,
                &mut |from, to| cast(from, to, &reach_options),
                &mut reach_notes[i],
                viewing.then(|| &mut handles[i]),
            )
        };
        let reach = grasp::nearest(reach.into_iter().chain(door));
        hand_status[i] = HandStatus {
            acting: true,
            palm: Some(palm),
            press: contact.press.map(|(hit, depth)| {
                (
                    depth,
                    matches!(
                        hit.surface,
                        Surface::Solid {
                            object: Some(_),
                            ..
                        }
                    ),
                )
            }),
            push: if config.push { hand_push } else { Vec2::ZERO },
            speed,
            reach,
            held: None,
            shove,
            latched: false,
        };
    }

    let mut joined = [None; 2];
    let mut pins = [None; 2];
    let mut refused = [None, None];
    if pickup_on {
        let inputs = pickup::TickInputs {
            palms,
            space,
            joints: joint_inputs(&config),
            dt,
            unlatch_pull_m: config.doors.unlatch_pull_m * active.world_scale,
            world_scale: active.world_scale,
            characters: config.characters,
            reaches: hand_status.map(|h| h.reach),
        };
        // SAFETY: the simulation phase outside the physics step.
        let outcome = unsafe { pickup::tick(&mut state.pickup, inputs, &config.pickup) };
        joined = outcome.joints;
        pins = outcome.pins;
        refused.clone_from(&outcome.refused);
        state.record_pickup(outcome, &mut hand_status);
        for hand in Hand::BOTH {
            let i = hand.index();
            hand_status[i].latched = state.pickup.latched(hand);
            // A hand on a latched door stays at its grab point; a driven hand's body is held
            // there, and a probed one is drawn there.
            if let (Some(pin), Some(palm)) = (pins[i], palms[i])
                && !physics::drives(hand)
            {
                let offset =
                    (pin - palm.pose.position).clamp_length_max(config.pickup.break_distance_m);
                clamp[i] = (body_rotation.inverse() * offset) / active.world_scale.max(1e-3);
            }
        }
        for hand in Hand::BOTH {
            let i = hand.index();
            let Some(held) = hand_status[i].held.filter(|h| !h.joined) else {
                continue;
            };
            // The rendered hand stays on the grab point, wherever the body is.
            let offset =
                (held.anchor - held.target).clamp_length_max(config.pickup.break_distance_m);
            clamp[i] = (body_rotation.inverse() * offset) / active.world_scale.max(1e-3);
            if config.push && config.pickup.resist {
                hand_status[i].push =
                    hold::resist_push(held.anchor, held.target, held.load, &config);
            }
        }
    }
    declare_grips(&state.pickup, &hand_status, wrists, &mut |from, to| {
        cast(from, to, &reach_options)
    });
    pressure_haptics(&hand_status);
    let push = push::combine(hand_status.map(|h| h.push), &config);
    for touch in touches {
        // SAFETY: forwarded from the caller; the touches' objects were found by this tick's casts.
        unsafe { state.touch(touch, melee_impulse, &config) };
    }
    if let Some(error) = error {
        state.status.last_error = Some(error);
    }
    state.status.idle = None;
    state.status.hands = hand_status;
    state.status.push = push;
    state.status.casts = casts;
    state.reach_notes = reach_notes;
    state.clamp = clamp;
    state.at = Some(now);
    physics::publish_holds(joined);
    physics::publish_pins(pins);
    let snapshot = state.status.hands;
    drop(state);
    climb::observe(
        climb::Observed {
            palms: climb_palms,
            reaches: climb_reaches,
            pressing: climb_pressing,
            body_rotation,
        },
        &config,
    );
    draw::submit(&snapshot, &config);
    for hand in Hand::BOTH {
        if let Some((at, why)) = &refused[hand.index()] {
            grab_view::refused(hand, Some(*at), why);
        }
    }
    if viewing {
        grab_view::record(grab_view::Recorded {
            head: head.position,
            rays,
            handles,
        });
        // SAFETY: forwarded from the caller: the simulation phase outside the physics step, on the
        // main thread.
        unsafe { grab_view::scan_candidates(near_hands, standing_on, &config) };
    }
}

/// Declare what each hand holds, for its finger pose, with the contact fit's curls for a held body
/// where the fingers would close onto it: each finger's sweep casts with `cast` against the body,
/// from the rendered wrist (`wrists`, moved as the rendered hand is to the grab point). An empty
/// palm pressed on a surface is declared flat on it.
fn declare_grips(
    pickup: &pickup::PickupState,
    hands: &[HandStatus; 2],
    wrists: [Option<Pose>; 2],
    cast: &mut dyn FnMut(Vec3, Vec3) -> Option<SurfaceHit>,
) {
    let fingers = Config::lock_query(|c| c.controllers.fingers);
    let fit_on = fingers.enabled && fingers.grip.enabled && fingers.grip.contact_fit;
    for hand in Hand::BOTH {
        let i = hand.index();
        let Some((source, body)) = pickup.grip(hand) else {
            if hands[i].press.is_some() || physics::pressing(hand) {
                grip::declare(hand, Grip::new(GripSource::Surface));
            }
            continue;
        };
        // One hand on what it cannot lift lays its palm on it, as on a crate it drags.
        let pose = match hands[i].held {
            Some(held) if source == GripSource::Pickup && !held.liftable => GripPose::FlatPalm,
            _ => source.pose(),
        };
        let fit = body
            .zip(wrists[i])
            .filter(|_| fit_on)
            .and_then(|(body, wrist)| {
                let drawn = hands[i].held.map_or(Vec3::ZERO, |h| h.anchor - h.target);
                let target = FitTarget {
                    body,
                    wrist: Pose {
                        position: wrist.position + drawn,
                        orientation: wrist.orientation,
                    },
                    pose,
                };
                finger_contact::fit_contacts(hand, target, &fingers, &mut |from, to| {
                    let hit = cast(from, to)?;
                    let (Surface::Solid { body, .. } | Surface::Character { body, .. }) =
                        hit.surface;
                    let length = from.distance(to).max(1e-6);
                    Some(CastHit {
                        fraction: from.distance(hit.position) / length,
                        body,
                    })
                })
            });
        grip::declare(hand, Grip::new(source).pose(pose).fitted(fit));
    }
}

/// Whether the hands act in the mode `detection`: on foot on the ground, or falling, so that a
/// hand flung or jumped at a ledge can catch it; not swimming, in a vehicle, in the wingsuit or
/// under the parachute, nor while the grapple carries the player.
pub(crate) fn mode_acts(detection: Detection) -> bool {
    matches!(
        detection.mode,
        Mode::OnFoot(OnFootVariant::Ground) | Mode::Airborne(AirborneVariant::Falling)
    ) && detection.traversal.is_none()
}

/// What the hand bodies tell the pickups about the joints: which hands take what they grab through
/// a joint, which lost theirs, and each body's load.
fn joint_inputs(config: &PhysicalHandsConfig) -> pickup::JointInputs {
    let lost = physics::take_joint_losses();
    let joining = physics::enabled(config) && config.bodies.joint_grabs;
    let bodies = physics::status();
    pickup::JointInputs {
        join: Hand::BOTH.map(|hand| joining && physics::drives(hand)),
        lost,
        loads: bodies.hands.map(|h| (h.force_share, h.torque_share)),
    }
}

/// Which hands hold nothing: no gun, not the foregrip, no grenade or charge being thrown, and the
/// controller drives the hand.
pub(super) fn free_hands() -> [bool; 2] {
    let wielded = weapons::status()
        .wielded
        .map_or([false; 2], |w| w.weapon.wielded.hands);
    let foregrip = foregrip::status()
        .hold
        .filter(|h| h.engaged)
        .map(|h| h.holder.other());
    let throwing = [grenade::throwing_hand(), c4::throwing_hand()];
    let weights = tiers::weights();
    Hand::BOTH.map(|hand| {
        !wielded[hand.index()]
            && foregrip != Some(hand)
            && !throwing.contains(&Some(hand))
            && weights[hand.index()] > 0.0
    })
}
