//! The palms' simulation tick: the probes, the pushes, the shoves, and the pickups, folded into the
//! readout.
//!
//! The tick is due after the player aim control's post-simulation update, which the game world's
//! update queue may run on one of its worker threads, so the update only notes that a tick is due
//! ([`after_aim`]), and the tick runs on the game thread at the start of the frame's render update
//! ([`main_thread_tick`]): after the simulation the aim update belongs to, and outside the world
//! update and the physics step. "The game thread" is the thread the game's frame runs on, as
//! [`thread_role`] marks it; the input tick is no proxy for it, since the game's input pre-update
//! runs on a CPU fragment worker during the simulation (`docs/engine/gameplay/input.md`).

use std::{num::NonZeroUsize, time::Instant};

use body_rig::rotation::level;
use glam::{Mat4, Quat, Vec2, Vec3};
use jc3vr_body::controllers::{
    aim::raycast::CastOptions,
    body,
    clock::TimeScale,
    facts::{mounted, seat::rides_in_seat},
    fingers::{
        grip,
        grip::{
            Grip, GripSource,
            catalogue::GripPose,
            contact as finger_contact,
            contact::{CastHit, FitTarget},
        },
        tuning::FINGERS,
    },
    hands,
    hands::tiers,
    haptics, held,
    held::HeldItem,
    mode,
    mode::{AirborneVariant, Detection, Mode, OnFootVariant},
    physics::local_player::LocalPlayer,
    scheme,
};
use jc3vr_core::{
    game_tick::GameTick,
    thread_role::{self, ThreadRole},
};
use jc3vr_input::{
    frame::{Hand, Pose},
    haptics::cues::{Event, Sustain},
};
use jc3vr_view::{controllers::cockpit::CockpitTransform, headpose::teleport};
use parking_lot::Mutex;
use windows::Win32::System::Threading::GetCurrentThreadId;

use crate::controllers::physical_hands::{
    climb,
    config::PHYSICAL_HANDS_CONFIG,
    contact::{HandContact, PointContact, PointProbe, Surface, SurfaceHit},
    doors,
    doors::{acquisition::PanelReach, config::TEARING},
    engine, grab_view, grasp,
    grasp::PalmMotion,
    hold,
    hold::{BodySpace, PlayerKeepOut},
    impacts,
    palm::{PROBE_POINTS, PalmBox},
    physics, pickup,
    pickup::PICKUP,
    push, query, seated,
    shove::SHOVE,
    state::{STATE, VELOCITY_SMOOTHING_S, stand_down},
    status::{HandStatus, Idle},
    touch::{ShoveInput, Touch, pressure_haptics},
    tuning::PALMS,
};

/// Note that a palms' tick is due, `dt` seconds of simulation after the last: called from the
/// player aim control's post-simulation update, on whichever thread runs it. Ticks noted before the
/// next [`main_thread_tick`] are run as one, over their summed time.
pub fn after_aim(dt: f32) {
    let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
    let mut pending = PENDING.lock();
    *pending = Some(pending.unwrap_or(0.0) + dt);
}

/// Run the palms' tick the aim update left due, if any, in a game tick outside the world update
/// and the physics step.
pub fn main_thread_tick(game: &GameTick<'_>) {
    let Some(dt) = PENDING.lock().take() else {
        return;
    };
    let Some(owner) = LocalPlayer::get(game) else {
        return;
    };
    run(&owner, dt);
}

/// The palms' simulation time not yet ticked, while a tick is due.
static PENDING: Mutex<Option<f32>> = Mutex::new(None);

/// The palms' tick, for the local player `owner`.
fn run(player: &LocalPlayer<'_>, dt: f32) {
    let owner = player.character();
    physics::observe(dt);
    // SAFETY: the game tick: the game runs.
    let time_scale = unsafe { TimeScale::current() };
    let config = *PHYSICAL_HANDS_CONFIG.lock();
    let gate = Gate {
        active: scheme::is_controllers() && jc3vr_core::hooks::in_gameplay(),
        role: thread_role::current(),
    };
    let seated_vehicle =
        (gate.active && gate.role == ThreadRole::Game && unsafe { rides_in_seat(owner) })
            .then(|| NonZeroUsize::new(unsafe { owner.GetVehiclePtr() } as usize))
            .flatten();
    {
        let mut state = STATE.lock();
        if state.seated_vehicle != seated_vehicle {
            state.reset_probes();
            state.pickup.drop_all();
            state.pickup.input_at = None;
            state.clamp = [Vec3::ZERO; 2];
            state.at = None;
            state.status.hands = [HandStatus::default(); 2];
            state.seated_vehicle = seated_vehicle;
        }
    }
    if gate.role == ThreadRole::Game {
        let held: Vec<u32> = STATE
            .lock()
            .pickup
            .held
            .iter()
            .flatten()
            .map(|h| h.body)
            .collect();
        unsafe { doors::hinge::tick(dt, &held, gate.active, PICKUP.gravity_mps2) };
    }
    if seated_vehicle.is_some() {
        unsafe { seated::run(owner, dt) };
        return;
    }
    let idle = stand_down_reason(gate, || mode_acts(mode::current()));
    if idle == Some(Idle::OffThread) && STATE.lock().status.idle != Some(Idle::OffThread) {
        tracing::warn!(
            target: "controllers",
            role = gate.role.describe(),
            // SAFETY: reads the calling thread's own id.
            thread = unsafe { GetCurrentThreadId() },
            "palms: the tick runs off the game thread; the palms stand down"
        );
    }
    if let Some(idle) = idle {
        stand_down(idle);
        return;
    }
    let Some(active) = hands::active_hands_raw() else {
        stand_down(Idle::NoHands);
        return;
    };
    let xf = CockpitTransform::current(active.head.position, active.world_scale);
    let body_rotation = xf.body;
    let compose = |pose: Pose| xf.pose_to_world(pose);
    let at = |point: Vec3| {
        compose(Pose {
            position: point,
            orientation: Quat::IDENTITY,
        })
        .position
    };
    let shoulders = body::shoulder_positions(
        active.head.position,
        level(active.head.orientation),
        &body::player(),
    )
    .map(at);
    let free = free_hands();
    let to_world = Mat4::from(owner.m_WorldMatrixT1);
    let space = (PICKUP.keep_out_radius_m > 0.0).then(|| {
        body::procedural::torso_keep_out(to_world).map_or_else(
            || {
                PlayerKeepOut::Cylinder(BodySpace {
                    base: to_world.w_axis.truncate(),
                    height_m: body::player().height * active.world_scale,
                    radius_m: PICKUP.keep_out_radius_m * active.world_scale,
                    forward: body_rotation * Vec3::NEG_Z,
                })
            },
            PlayerKeepOut::Torso,
        )
    });
    // SAFETY: the local player, in the game tick.
    let melee_impulse = unsafe { engine::melee_impulse(owner) };
    // SAFETY: as above.
    let standing_on = unsafe { engine::ground_body(owner) };
    let floor_hands: Vec<Vec3> = Hand::BOTH
        .into_iter()
        .filter(|hand| free[hand.index()])
        .filter_map(|hand| active.hands[hand.index()])
        .map(|hand| PalmBox::from_wrist(compose(hand.wrist), active.world_scale, &PALMS).centre)
        .collect();
    let floor_radius =
        (PICKUP.reach_m + PALMS.palm_half_extents_m.abs().length()) * active.world_scale;
    // SAFETY: this input phase runs outside the world step, before holding the hand-state lock.
    let floor_weapons = unsafe { pickup::floor_weapons(&floor_hands, floor_radius) };

    let mut state = STATE.lock();
    let now = Instant::now();
    if teleport::epoch() != state.epoch {
        state.epoch = teleport::epoch();
        state.reset_probes();
        state.pickup.drop_all();
    }
    // The palms pass through what a hand holds, so that a free palm never pushes off it; the reach
    // rays do not, so that the other hand can take hold of it too.
    let held_objects = state.pickup.held_objects();
    let held = held_objects.into_iter().find(|&o| o != 0).unwrap_or(0);
    let probe_options = CastOptions {
        // The impact sounds play against the surface's material.
        material: true,
        ..CastOptions::ignoring(player, held)
    };
    let reach_options = CastOptions {
        material: true,
        ..CastOptions::ignoring(player, 0)
    };
    let mut casts = 0;
    let mut error = None;
    let mut cast = |from: Vec3, to: Vec3, solid: bool| -> Option<SurfaceHit> {
        casts += 1;
        let result = if solid {
            unsafe { query::cast_solid(from, to, &probe_options) }
        } else {
            unsafe { query::cast(from, to, &reach_options) }
        };
        match result {
            Ok(hit) => hit,
            Err(e) => {
                error = Some(e.to_string());
                None
            }
        }
    };

    let reach_tuning = pickup::ReachTuning {
        floor_weapons: &floor_weapons,
        world_scale: active.world_scale,
        pickup: &PICKUP,
        hand_bodies: physics::status().hands.map(|h| h.body_id),
    };
    // The vehicles whose doors the free palms might reach, offered by their handles; each hand adds
    // the vehicles its own rays hit.
    let door_vehicles = {
        let near: Vec<Vec3> = Hand::BOTH
            .into_iter()
            .filter(|h| free[h.index()])
            .filter_map(|h| active.hands[h.index()].map(|c| compose(c.wrist).position))
            .collect();
        // SAFETY: the simulation phase outside the physics step, on the aim update's thread.
        unsafe { doors::scan::near(&near, active.world_scale) }
    };
    let mut climb_palms = [None; 2];
    let mut climb_pressing = [false; 2];
    let mut climb_reaches = [None; 2];
    let mut touches = Vec::new();
    let mut reach_notes = [Vec::new(), Vec::new()];
    let mut interaction_scans = [pickup::diagnostics::scan::Scan::new(); 2];
    // The grab view's record of the reach, only while it is on.
    let viewing = config.grab_view.enabled;
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
            state.door_press[i].reset();
            state.probes[i].iter_mut().for_each(PointProbe::reset);
            state.pressed[i] = false;
            state.gives[i].reset();
            state.motion[i].0.reset();
            state.motion[i].1.reset();
            continue;
        };
        let palm = PalmBox::from_wrist(compose(cockpit.wrist), active.world_scale, &PALMS);
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
        wrists[i] = Some(compose(cockpit.wrist));
        if state.pickup.holding(hand) {
            state.door_press[i].reset();
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
        let pressure_hit =
            match unsafe { doors::pressure::probe(shoulders[i], &palm, &reach_options) } {
                Ok(probe) => state.door_press[i].observe_probe(probe),
                Err(why) => {
                    state.door_press[i].reset();
                    state.status.last_error = Some(why.to_string());
                    None
                }
            };
        let pressure_reach = pressure_hit.and_then(|hit| {
            let Surface::Solid {
                object: Some(object),
                body,
            } = hit.surface
            else {
                return None;
            };
            unsafe {
                doors::reach(
                    object,
                    body,
                    &hit,
                    palm.centre,
                    PanelReach::uniform(doors::pressure::REACH_M * active.world_scale),
                )
            }
        });
        let pressure_result = unsafe {
            state.door_press[i].step(doors::pressure::Input {
                reach: pressure_reach,
                hit: pressure_hit,
                palm: palm.centre,
                dt,
                world_scale: active.world_scale,
                options: &reach_options,
            })
        };
        if state.door_press[i].take_contact_cue() {
            haptics::play(hand, Event::DoorPressureContact);
        }
        match pressure_result {
            Ok(Some(slot)) => {
                state.status.pickup.unlatches += 1;
                state.status.pickup.last_door = Some(slot);
                haptics::play(hand, Event::Unlatch);
            }
            Err(error) => state.status.last_error = Some(error.to_string()),
            Ok(None) => {}
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
            let mut hit_vehicles = Vec::new();
            // SAFETY: the simulation phase outside the physics step.
            let reach = unsafe {
                pickup::reach(
                    &body_palm,
                    None,
                    standing_on,
                    &reach_tuning,
                    &mut |from, to| cast(from, to, false),
                    pickup::ReachObservations {
                        notes: &mut reach_notes[i],
                        rays: viewing.then(|| &mut rays[i]),
                        scan: &mut interaction_scans[i],
                        vehicles: &mut hit_vehicles,
                    },
                )
            };
            // SAFETY: the simulation phase outside the physics step; the vehicles are this tick's.
            let door = unsafe {
                doors::scan::handle_reach(
                    body_palm.centre,
                    &with_hit_vehicles(&door_vehicles, &hit_vehicles),
                    active.world_scale,
                    &mut |from, to| cast(from, to, false),
                    doors::scan::HandleObservations {
                        notes: &mut reach_notes[i],
                        handles: viewing.then(|| &mut handles[i]),
                        scan: &mut interaction_scans[i],
                    },
                )
            };
            let reach = grasp::nearest(reach.into_iter().chain(door));
            interaction_scans[i].selected = reach;
            // The palm in the rig: its tracking-space place turned and scaled as the world
            // palm is, but not carried by the head's world position, which the climb moves.
            let rig = PalmBox::from_wrist(cockpit.wrist, 1.0, &PALMS).centre;
            climb_palms[i] = Some(xf.offset_to_world(rig));
            climb_pressing[i] = physics::pressing(hand);
            // A loose object in reach wins over the world it rests on.
            climb_reaches[i] = if reach.is_none() {
                // SAFETY: the simulation phase outside the physics step.
                unsafe {
                    climb::reach(
                        &body_palm,
                        climb::config::CLIMB.reach_m * active.world_scale,
                        &mut |from, to| cast(from, to, true),
                    )
                }
            } else {
                None
            };
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
            contacts[k] =
                state.probes[i][k].step(point, shoulders[i], &mut |from, to| cast(from, to, true));
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
            Some((hit, depth)) => {
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
                        &SHOVE,
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
            push::hand_push(hit.normal, depth, &PALMS) * resistance
        });
        let offset = contact.clamp_offset();
        clamp[i] = xf.offset_to_cockpit(offset);
        let mut hit_vehicles = Vec::new();
        // SAFETY: the simulation phase outside the physics step.
        let reach = unsafe {
            pickup::reach(
                &palm,
                contact.press.map(|(hit, _)| hit),
                standing_on,
                &reach_tuning,
                &mut |from, to| cast(from, to, false),
                pickup::ReachObservations {
                    notes: &mut reach_notes[i],
                    rays: viewing.then(|| &mut rays[i]),
                    scan: &mut interaction_scans[i],
                    vehicles: &mut hit_vehicles,
                },
            )
        };
        // SAFETY: the simulation phase outside the physics step; the vehicles are this tick's.
        let door = unsafe {
            doors::scan::handle_reach(
                palm.centre,
                &with_hit_vehicles(&door_vehicles, &hit_vehicles),
                active.world_scale,
                &mut |from, to| cast(from, to, false),
                doors::scan::HandleObservations {
                    notes: &mut reach_notes[i],
                    handles: viewing.then(|| &mut handles[i]),
                    scan: &mut interaction_scans[i],
                },
            )
        };
        let reach = grasp::nearest(reach.into_iter().chain(door));
        interaction_scans[i].selected = reach;
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
            push: hand_push,
            speed,
            reach,
            held: None,
            shove,
            latched: false,
        };
    }

    let strength = if config.bodies.strength.is_finite() {
        config.bodies.strength.max(0.0)
    } else {
        0.0
    };
    let pickup_tuning = pickup::PickupTuning {
        hand_force_n: PICKUP.hand_force_n * strength,
        hand_torque_nm: PICKUP.hand_torque_nm * strength,
        ..PICKUP
    };
    let inputs = pickup::TickInputs {
        palms,
        space,
        joints: joint_inputs(),
        dt,
        // SAFETY: the clock singleton is read on the game thread.
        time_scale: time_scale.get(),
        unlatch_pull_m: doors::UNLATCH_PULL_M * active.world_scale,
        tearing: TEARING,
        world_scale: active.world_scale,
        reaches: hand_status.map(|h| h.reach),
        carrier_velocity: Some(Vec3::from_array(owner.m_MeasuredWorldVelocity.data)),
        carrier_pose: Some(Pose {
            position: xf.point_to_world(Vec3::ZERO),
            orientation: body_rotation,
        }),
    };
    // SAFETY: the simulation phase outside the physics step.
    let outcome = unsafe { pickup::tick(&mut state.pickup, inputs, &pickup_tuning) };
    let joined = outcome.joints;
    let pins = outcome.pins;
    let refused = outcome.refused.clone();
    state.record_pickup(outcome, &mut hand_status);
    for hand in Hand::BOTH {
        let i = hand.index();
        hand_status[i].latched = state.pickup.latched(hand);
        // A hand on a latched door stays at its grab point; a driven hand's body is held
        // there, and a probed one is drawn there.
        if let (Some(pin), Some(palm)) = (pins[i], palms[i])
            && !physics::drives(hand)
        {
            let offset = (pin - palm.pose.position).clamp_length_max(PICKUP.tear_distance_m);
            clamp[i] = xf.offset_to_cockpit(offset);
        }
    }
    for hand in Hand::BOTH {
        let i = hand.index();
        let Some(held) = hand_status[i].held.filter(|h| !h.joined) else {
            continue;
        };
        // Physical hand bodies follow carry pins; only nonphysical hands need this offset.
        if !physics::drives(hand) {
            let offset = (held.anchor - held.target).clamp_length_max(PICKUP.tear_distance_m);
            clamp[i] = xf.offset_to_cockpit(offset);
        }
        let deadband = hold::resist_deadband_m(state.pickup.props()[i].is_some(), &PALMS);
        let lag = state.pickup.held[i].map_or(Vec3::ZERO, |body| {
            impacts::obstruction(body.body, body.object, held.anchor - held.target)
        });
        hand_status[i].push = hold::resist_push(lag, Vec3::ZERO, held.load, deadband, &PALMS);
    }
    declare_grips(&state.pickup, &hand_status, wrists, &mut |from, to| {
        cast(from, to, false)
    });
    let mut pressure_status = hand_status;
    for hand in Hand::BOTH {
        if state.door_press[hand.index()].engaged() {
            haptics::cancel_sustain(hand, Sustain::BodyPressure);
            haptics::cancel_sustain(hand, Sustain::PalmPressure);
            pressure_status[hand.index()].shove = None;
        }
    }
    pressure_haptics(&pressure_status);
    let push = push::combine(hand_status.map(|h| h.push), &PALMS);
    for touch in touches {
        // SAFETY: the game tick; the touches' objects were found by this tick's casts.
        unsafe { state.touch(touch, melee_impulse) };
    }
    if let Some(error) = error {
        state.status.last_error = Some(error);
    }
    state.status.idle = None;
    state.status.hands = hand_status;
    state.pickup.scans = interaction_scans;
    state.status.push = push;
    state.status.casts = casts;
    state.reach_notes = reach_notes;
    state.clamp = clamp;
    state.at = Some(now);
    physics::publish_holds(joined);
    physics::publish_pins(pins);
    drop(state);
    climb::observe(climb::Observed {
        palms: climb_palms,
        reaches: climb_reaches,
        pressing: climb_pressing,
        body_rotation,
    });
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
        // SAFETY: the game tick, outside the physics step.
        unsafe { grab_view::scan_candidates(near_hands, standing_on) };
    }
}

/// The vehicles the door scan looks at for one hand: those the target system keeps near the palms,
/// and those the hand's rays hit this tick, each once.
fn with_hit_vehicles(near: &[NonZeroUsize], hit: &[NonZeroUsize]) -> Vec<NonZeroUsize> {
    let mut vehicles = near.to_vec();
    for &vehicle in hit {
        if !vehicles.contains(&vehicle) {
            vehicles.push(vehicle);
        }
    }
    vehicles
}

/// Declare what each hand holds, for its finger pose, with the contact fit's curls for a held body
/// where the fingers would close onto it: each finger's sweep casts with `cast` against the body,
/// from the rendered wrist (`wrists`, moved as the rendered hand is to the grab point). An empty
/// palm pressed on a surface is declared flat on it.
pub(super) fn declare_grips(
    pickup: &pickup::PickupState,
    hands: &[HandStatus; 2],
    wrists: [Option<Pose>; 2],
    cast: &mut dyn FnMut(Vec3, Vec3) -> Option<SurfaceHit>,
) {
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
        let fit = body.zip(wrists[i]).and_then(|(body, wrist)| {
            let drawn = hands[i].held.map_or(Vec3::ZERO, |h| h.anchor - h.target);
            let target = FitTarget {
                body,
                wrist: Pose {
                    position: wrist.position + drawn,
                    orientation: wrist.orientation,
                },
                pose,
            };
            finger_contact::fit_contacts(hand, target, &FINGERS, &mut |from, to| {
                let hit = cast(from, to)?;
                let (Surface::Solid { body, .. } | Surface::Character { body, .. }) = hit.surface;
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

/// What decides whether the palms act at all this tick, before they look at the hands.
#[derive(Debug, Copy, Clone)]
struct Gate {
    /// The physical hands are on, the controller scheme is in effect, and the game is in gameplay.
    active: bool,
    /// The calling thread's role.
    role: ThreadRole,
}

/// Why the palms stand down this tick, if they do. The palms change the main thread's engine
/// state, so they act only on the game thread outside its frame scopes ([`ThreadRole::Game`]); the
/// mode is read only once the rest allows a tick.
fn stand_down_reason(gate: Gate, mode_acts: impl FnOnce() -> bool) -> Option<Idle> {
    if !gate.active {
        Some(Idle::Disabled)
    } else if gate.role != ThreadRole::Game {
        Some(Idle::OffThread)
    } else {
        (!mode_acts()).then_some(Idle::Mode)
    }
}

/// Whether the hands act in the mode `detection`: on foot on the ground, or falling, so that a
/// hand flung or jumped at a ledge can catch it; not swimming, in a vehicle, in the wingsuit or
/// under the parachute, nor while the grapple carries the player.
pub fn mode_acts(detection: Detection) -> bool {
    matches!(
        detection.mode,
        Mode::OnFoot(OnFootVariant::Ground) | Mode::Airborne(AirborneVariant::Falling)
    ) && detection.traversal.is_none()
}

/// What the hand bodies tell the pickups about the joints: which hands take what they grab through
/// a joint, which lost theirs, and each body's load.
fn joint_inputs() -> pickup::JointInputs {
    let lost = physics::take_joint_losses();
    let bodies = physics::status();
    pickup::JointInputs {
        join: Hand::BOTH.map(physics::drives),
        lost,
        loads: bodies.hands.map(|h| (h.force_share, h.torque_share)),
    }
}

/// Which hands hold nothing a palm would get in the way of: no gun, not the foregrip, no grenade,
/// charge, or detonator, nothing of the loadout table's but a prop the pickup carries, no mounted
/// gun's handle, and the controller drives the hand. A hand on a handle is drawn there, and a hand
/// body left at its controller would fight the gun it holds.
#[inline]
pub fn free_hands() -> [bool; 2] {
    let wielded = held::last_wielded_hands();
    let foregrip = held::two_hand_holder().map(Hand::other);
    let charge = held::throwing_hand();
    let detonator = held::detonator_holder();
    let weights = tiers::weights();
    let mounted = mounted::holds();
    Hand::BOTH.map(|hand| {
        let holds_item = held::item(hand).is_some_and(|kind| kind != HeldItem::Prop);
        !wielded[hand.index()]
            && !holds_item
            && foregrip != Some(hand)
            && !held::grenade_in(hand)
            && charge != Some(hand)
            && detonator != Some(hand)
            && !mounted[hand.index()]
            && weights[hand.index()] > 0.0
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate(active: bool, role: ThreadRole) -> Gate {
        Gate { active, role }
    }

    #[test]
    fn the_palms_act_only_on_the_game_thread_outside_its_frame_scopes() {
        assert_eq!(
            stand_down_reason(gate(true, ThreadRole::Game), || true),
            None
        );
        for role in [
            ThreadRole::Unmarked,
            ThreadRole::GameFrame,
            ThreadRole::GameDebugUi,
            ThreadRole::FrameTail,
        ] {
            assert_eq!(
                stand_down_reason(gate(true, role), || true),
                Some(Idle::OffThread),
                "{role:?}"
            );
        }
    }

    #[test]
    fn an_inactive_gate_reads_neither_the_thread_nor_the_mode() {
        let reason = stand_down_reason(gate(false, ThreadRole::Unmarked), || {
            panic!("the mode is read only for a tick that may act")
        });
        assert_eq!(reason, Some(Idle::Disabled));
    }

    #[test]
    fn the_mode_decides_last() {
        assert_eq!(
            stand_down_reason(gate(true, ThreadRole::Game), || false),
            Some(Idle::Mode)
        );
        let reason = stand_down_reason(gate(true, ThreadRole::Unmarked), || {
            panic!("an off-thread tick does not read the mode")
        });
        assert_eq!(reason, Some(Idle::OffThread));
    }
}
