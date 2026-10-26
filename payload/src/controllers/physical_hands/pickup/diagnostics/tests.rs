use std::{num::NonZeroUsize, time::Instant};

use glam::{Quat, Vec3};

use super::*;
use crate::controllers::{
    frame::Pose,
    grab::{GripTarget, Mark, MarkAt, PickupKind, target::Tier},
    physical_hands::{
        characters::CharactersConfig,
        config::PickupConfig,
        doors::DoorsConfig,
        grasp::{PalmMotion, TargetKind},
        hold::{self, HandHold, HoldTuning},
        palm::PalmBox,
        pickup::{
            ReachObservations, ReachTuning,
            state::{Held, HeldKind, PickupState, Request},
            tick::{self, BodyGrab, JointInputs, TickInputs, TickOutcome, take_body},
        },
        rigid::BodyPose,
    },
};

fn target() -> Reach {
    Reach {
        object: NonZeroUsize::new(8).unwrap(),
        body: 3,
        point: Vec3::ZERO,
        distance: 0.05,
        normal: Vec3::Y,
        kind: TargetKind::Prop { mass_kg: 5.0 },
    }
}

fn input_state() -> InputState {
    InputState {
        grips: [ButtonControl {
            down: true,
            pressed: true,
            released: false,
        }; 2],
        latches: [GripLatch::Open; 2],
        grabs: [Some(GrabVolume::Pickup(PickupKind::Prop)); 2],
    }
}

fn observed_tick(count: usize) -> Tick {
    let targets = (0..count)
        .map(|i| {
            let mut target = GripTarget::measured(
                GrabVolume::Pickup(PickupKind::Prop),
                Hand::Left,
                0.05 + i as f32,
                Mark::ring(MarkAt::World(Vec3::ZERO), 0.05),
            );
            target.observed = Some(self::target());
            target
        })
        .collect();
    crate::controllers::grab::tick::resolve(targets, Default::default(), None)
}

#[test]
fn resolver_evidence_uses_real_chosen_and_loss_results_and_a_fixed_bound() {
    let tick = observed_tick(CANDIDATE_CAPACITY + 3);
    let mut hand = HandEvidence::new();
    let record = hand.input(41, &tick, input_state(), Hand::Left);
    assert_eq!(record.sequence, 41);
    assert_eq!(record.episode, 1);
    assert_eq!(
        record.chosen.unwrap().resolved,
        tick.resolved[0].chosen.unwrap()
    );
    assert_eq!(record.chosen.unwrap().observed, Some(target()));
    assert_eq!(record.candidates_omitted, 3);
    let actual_losers =
        resolve::losers(&tick.resolved[0].reaches, tick.resolved[0].chosen.unwrap());
    assert_eq!(record.candidates[1].unwrap().lost, Some(actual_losers[0].1));
    assert_eq!(record.candidates[1].unwrap().resolved.tier, Tier::Target);
    let mut continued = input_state();
    continued.grips[0].pressed = false;
    assert_eq!(hand.input(42, &tick, continued, Hand::Left).episode, 1);
    continued.grips[0].down = false;
    hand.input(43, &tick, continued, Hand::Left);
    continued.grips[0].down = true;
    assert_eq!(hand.input(44, &tick, continued, Hand::Left).episode, 2);
    assert_eq!(hand.last_press.unwrap().sequence, 41);
}

#[test]
fn a_missing_provider_identity_is_not_reported_as_a_missing_candidate() {
    let mut tick = observed_tick(1);
    tick.targets[0].observed = None;
    let mut hand = HandEvidence::new();
    let record = hand.input(50, &tick, input_state(), Hand::Left);
    assert!(record.chosen.is_some());
    assert!(record.chosen.unwrap().observed.is_none());
    let empty = crate::controllers::grab::tick::resolve(Vec::new(), Default::default(), None);
    assert!(
        hand.input(51, &empty, input_state(), Hand::Left)
            .chosen
            .is_none()
    );
}

#[test]
fn repeated_publication_consumer_results_only_update_the_matching_input_observation() {
    let tick = observed_tick(1);
    for result in [InputConsumer::NotPickup, InputConsumer::GripOpen] {
        let mut hand = HandEvidence::new();
        let press = hand.input(60, &tick, input_state(), Hand::Left);
        hand.queued(press, target());
        let mut next = input_state();
        next.grips[0] = ButtonControl {
            down: result != InputConsumer::GripOpen,
            pressed: false,
            released: result == InputConsumer::GripOpen,
        };
        next.grabs[0] = None;
        let later = hand.input(60, &tick, next, Hand::Left);
        assert_eq!(press.sequence, later.sequence);
        assert_eq!(press.episode, later.episode);
        assert!(press.observation < later.observation);
        assert_ne!(press.grip, later.grip);
        hand.input_consumer(later, result);
        let retained = hand.last_press.unwrap();
        assert_eq!(retained.observation, press.observation);
        assert_eq!(retained.grip, press.grip);
        assert_eq!(retained.grab, press.grab);
        assert_eq!(retained.consumer, InputConsumer::Queued);
        assert_eq!(hand.latest.unwrap().consumer, result);
        hand.input_consumer(press, InputConsumer::Queued);
        assert_eq!(hand.latest.unwrap().observation, later.observation);
        assert_eq!(hand.latest.unwrap().consumer, result);
        assert_eq!(hand.last_press.unwrap().consumer, InputConsumer::Queued);
    }
}

#[test]
fn request_ring_keeps_input_identity_and_does_not_relabel_a_later_request() {
    let mut hand = HandEvidence::new();
    let input = hand.input(60, &observed_tick(1), input_state(), Hand::Left);
    let first = hand.queued(input, target());
    for _ in 0..REQUEST_CAPACITY {
        hand.queued(input, target());
    }
    assert_eq!(hand.evicted, 1);
    hand.consumed(first, ConsumerResult::NoPalm);
    assert!(
        hand.requests
            .iter()
            .flatten()
            .all(|r| r.consumer == ConsumerResult::Queued)
    );
    let latest = hand
        .requests
        .iter()
        .flatten()
        .max_by_key(|r| r.key.request)
        .unwrap();
    assert_eq!(latest.input.sequence, 60);
    assert_eq!(latest.key.target, target());
    assert_ne!(latest.key.request, first.request);
}

#[test]
fn queue_replacement_and_no_palm_consumer_preserve_the_actual_request_key() {
    let context = input(70, &observed_tick(1), input_state(), [None; 2]);
    let key = queued(Hand::Left, context.hands[0], target());
    let mut state = PickupState::new();
    state.replace_request(
        Hand::Left,
        Some(Request::ObservedGrab(key)),
        ConsumerResult::Replaced,
    );
    let (reach, carried) = state.requests[0].unwrap().grab().unwrap();
    assert_eq!(reach, target());
    assert_eq!(carried, Some(key));
    state.replace_request(Hand::Left, Some(Request::Release), ConsumerResult::Replaced);
    assert_eq!(record(key).consumer, ConsumerResult::Replaced);
    let key = queued(Hand::Left, context.hands[0], target());
    state.requests[0] = Some(Request::ObservedGrab(key));
    state.input_at = Some(Instant::now());
    // SAFETY: the no-palm branch has no body reads, writes, or engine calls.
    let outcome = unsafe {
        tick::tick(
            &mut state,
            TickInputs {
                palms: [None; 2],
                space: None,
                joints: JointInputs::default(),
                dt: 0.01,
                unlatch_pull_m: 0.05,
                world_scale: 1.0,
                characters: CharactersConfig::default(),
                reaches: [None; 2],
            },
            &PickupConfig::default(),
        )
    };
    assert_eq!(record(key).consumer, ConsumerResult::NoPalm);
    assert_eq!(outcome.taken, [false; 2]);
    assert_eq!(state.requests, [None; 2]);
    assert!(!state.holding(Hand::Left));
}

#[test]
fn an_evicted_observed_request_consumes_the_same_target_as_a_remote_grab() {
    let hand = Hand::Right;
    let i = hand.index();
    let context = input(90, &observed_tick(1), input_state(), [None; 2]);
    let key = queued(hand, context.hands[i], target());
    let mut observed = PickupState::new();
    observed.requests[i] = Some(Request::ObservedGrab(key));
    observed.input_at = Some(Instant::now());
    for _ in 0..REQUEST_CAPACITY {
        queued(hand, context.hands[i], target());
    }
    assert!(
        status().hands[i]
            .requests
            .iter()
            .flatten()
            .all(|r| r.key.request != key.request)
    );
    let mut remote = observed.clone();
    remote.requests[i] = Some(Request::Grab(target()));
    assert_eq!(observed.requests[i].unwrap().grab().unwrap().0, target());
    assert_eq!(remote.requests[i].unwrap().grab().unwrap().0, target());
    let consume = |state: &mut PickupState| {
        // SAFETY: the no-palm branch has no body reads, writes, or engine calls.
        unsafe {
            tick::tick(
                state,
                TickInputs {
                    palms: [None; 2],
                    space: None,
                    joints: JointInputs::default(),
                    dt: 0.01,
                    unlatch_pull_m: 0.05,
                    world_scale: 1.0,
                    characters: CharactersConfig::default(),
                    reaches: [None; 2],
                },
                &PickupConfig::default(),
            )
        }
    };
    let observed_outcome = consume(&mut observed);
    let remote_outcome = consume(&mut remote);
    assert_eq!(observed_outcome, remote_outcome);
    assert_eq!(observed_outcome.taken, [false; 2]);
    assert_eq!(observed.requests, [None; 2]);
    assert_eq!(observed, remote);
    assert!(
        status().hands[i]
            .requests
            .iter()
            .flatten()
            .all(|r| r.consumer == ConsumerResult::Queued)
    );
}

#[test]
fn real_body_consumer_acceptance_and_refusal_match_the_state_transition() {
    let config = PickupConfig::default();
    let grab = |joined| BodyGrab {
        hand: Hand::Left,
        reach: target(),
        palm: PalmMotion {
            pose: Pose {
                position: Vec3::ZERO,
                orientation: Quat::IDENTITY,
            },
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
        },
        joined,
    };
    let body = BodyState {
        pose: BodyPose {
            centre: Vec3::ZERO,
            orientation: Quat::IDENTITY,
        },
        velocities: Velocities::default(),
        mass_kg: Some(5.0),
        inverse_inertia: Vec3::ONE,
    };
    for joined in [false, true] {
        let mut state = PickupState::new();
        let mut outcome = TickOutcome::default();
        assert_eq!(
            take_body(&mut state, grab(joined), Ok(body), &config, &mut outcome),
            ConsumerResult::BodyAccepted { joined: false }
        );
        assert!(outcome.taken[0]);
        assert!(!state.held[0].unwrap().joined);
    }
    for mass in [None, Some(config.max_mass_kg + 1.0)] {
        let mut state = PickupState::new();
        let mut outcome = TickOutcome::default();
        assert_eq!(
            take_body(
                &mut state,
                grab(false),
                Ok(BodyState {
                    mass_kg: mass,
                    ..body
                }),
                &config,
                &mut outcome
            ),
            ConsumerResult::FixedOrTooHeavy
        );
        assert!(!state.holding(Hand::Left));
        assert!(outcome.refused[0].is_some());
    }
    for error in [
        BodyError::NoPhysicsWorld,
        BodyError::NoBody,
        BodyError::NotDynamic,
        BodyError::Reowned,
        BodyError::BadMotion,
    ] {
        let mut state = PickupState::new();
        let mut outcome = TickOutcome::default();
        assert_eq!(
            take_body(&mut state, grab(false), Err(error), &config, &mut outcome),
            ConsumerResult::BodyRefused(error)
        );
        assert!(!state.holding(Hand::Left));
        assert_eq!(outcome.error, Some(error.to_string()));
    }
}

#[test]
fn actual_prop_consumer_acquires_both_barrel_sides_and_only_assists_that_body() {
    let config = PickupConfig::new();
    let mut state = PickupState::new();
    let mut outcome = TickOutcome::default();
    let body = BodyState {
        pose: BodyPose {
            centre: Vec3::ZERO,
            orientation: Quat::IDENTITY,
        },
        velocities: Velocities::default(),
        mass_kg: Some(100.0),
        inverse_inertia: Vec3::splat(0.1),
    };
    let anchors = [Vec3::NEG_X * 0.3, Vec3::X * 0.3];
    for hand in Hand::BOTH {
        let point = anchors[hand.index()];
        let reach = Reach {
            point,
            kind: TargetKind::Prop { mass_kg: 100.0 },
            ..target()
        };
        let palm = PalmMotion {
            pose: Pose {
                position: point,
                orientation: Quat::IDENTITY,
            },
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
        };
        assert_eq!(
            take_body(
                &mut state,
                BodyGrab {
                    hand,
                    reach,
                    palm,
                    joined: true
                },
                Ok(body),
                &config,
                &mut outcome
            ),
            ConsumerResult::BodyAccepted { joined: false }
        );
    }
    assert_eq!(outcome.taken, [true; 2]);
    let held = state.held[0].unwrap();
    assert!(held.same(&state.held[1].unwrap()));
    let holds = [0, 1].map(|i| {
        let grip = state.held[i].unwrap().grip;
        Some(HandHold {
            anchor: grip.anchor(body.pose),
            target: grip.target(Pose {
                position: anchors[i] + Vec3::Y * 0.25,
                orientation: Quat::IDENTITY,
            }),
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
        })
    });
    let assisted = tick::carry_tuning(&held, &holds, &config);
    assert!(assisted.hand_force_n > config.hand_force_n);
    let driven = hold::solve(&body.rigid(), holds, 1.0 / 60.0, &assisted);
    assert!(driven.velocities.linear.y > config.gravity_mps2 / 60.0);
    for kind in [HeldKind::Articulated, HeldKind::Door, HeldKind::Ragdoll] {
        assert_eq!(
            tick::carry_tuning(&Held { kind, ..held }, &holds, &config),
            HoldTuning::of(&config)
        );
        assert!(kind.joined(true));
    }
    state.held[0] = None;
    assert_eq!(
        tick::carry_tuning(&held, &[None, holds[1]], &config),
        HoldTuning::of(&config)
    );
    state.drop_all();
    assert_eq!(state.held, [None; 2]);
    assert_eq!(state.requests, [None; 2]);
}

#[test]
fn the_actual_ray_scan_records_misses_without_engine_reads() {
    let mut scan = scan::Scan::new();
    let pickup = PickupConfig::default();
    let doors = DoorsConfig::default();
    let characters = CharactersConfig::default();
    let tuning = ReachTuning {
        floor_weapons: &[],
        world_scale: 1.0,
        pickup: &pickup,
        doors: &doors,
        characters: &characters,
        hand_bodies: [None; 2],
    };
    let mut casts = 0;
    // SAFETY: every cast misses, so the production scan does not read engine objects.
    let reached = unsafe {
        crate::controllers::physical_hands::pickup::reach(
            &PalmBox {
                centre: Vec3::ZERO,
                orientation: Quat::IDENTITY,
                half_extents: Vec3::splat(0.05),
            },
            None,
            None,
            &tuning,
            &mut |_, _| {
                casts += 1;
                None
            },
            ReachObservations {
                notes: &mut Vec::new(),
                rays: None,
                scan: &mut scan,
            },
        )
    };
    assert!(reached.is_none());
    assert_eq!(casts, 5);
    assert!(scan.ray_ran);
    assert_eq!(scan.entries.iter().flatten().count(), casts);
    assert!(
        scan.entries
            .iter()
            .flatten()
            .all(|entry| entry.result == scan::ScanResult::Missed)
    );
    assert!(scan.ray_selected.is_none());
}

#[test]
fn scan_is_bounded_and_the_input_retains_the_providers_copy() {
    let mut scan = scan::Scan::new();
    let entry = scan::ScanEntry {
        branch: scan::Branch::DoorHandle,
        object: Some(target().object),
        body: Some(target().body),
        door: None,
        point: Some(Vec3::ZERO),
        distance: Some(0.05),
        mass_kg: None,
        result: scan::ScanResult::NoBody,
    };
    for _ in 0..scan::SCAN_CAPACITY + 2 {
        scan.push(entry);
    }
    assert_eq!(scan.omitted, 2);
    scan.selected = Some(target());
    let captured = input(80, &observed_tick(1), input_state(), [None, Some(scan)]);
    scan.selected = None;
    assert_eq!(captured.hands[1].scan.unwrap().selected, Some(target()));
    assert_eq!(captured.hands[1].scan.unwrap().entries[0], Some(entry));
    assert!(scan.selected.is_none());
}

fn record(key: RequestKey) -> RequestRecord {
    status().hands[0]
        .requests
        .into_iter()
        .flatten()
        .find(|record| record.key.request == key.request)
        .unwrap()
}
