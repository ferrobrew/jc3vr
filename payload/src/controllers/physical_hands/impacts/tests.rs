//! Publication, event-lease, and queued-delivery integration through the production planner.

use std::{
    num::NonZeroUsize,
    time::{Duration, Instant},
};

use glam::Vec3;
use jc3gi::physics::{hknpBody, hknpWorld};

use crate::controllers::physical_hands::impacts::{
    policy::Motion,
    state::{Contact, Enabled, FlagCall, Held, Identity, Observation, State},
};

fn active() -> Enabled {
    Enabled {
        controllers: true,
        hands: true,
        pickup: true,
        feature: true,
        gameplay: true,
        running: true,
    }
}

fn observed(native: bool) -> Observation {
    Observation {
        identity: Identity {
            world: 1,
            body: 2,
            owner: NonZeroUsize::new(3).unwrap(),
            instance: 4,
        },
        flags: if native {
            hknpBody::RAISE_CONTACT_IMPULSE_EVENTS
        } else {
            0
        },
        motion: Motion {
            centre: Vec3::ZERO,
            linear: Vec3::X * 3.0,
            angular: Vec3::ZERO,
            mass_kg: 10.0,
        },
    }
}

fn held(observed: Observation) -> Option<Held> {
    Some((observed.identity.body, observed.identity.owner))
}

fn acquire(holders: [bool; 2], native: bool) -> (State, Observation, Instant) {
    let mut state = State::new();
    let observation = observed(native);
    let now = Instant::now();
    state.publish(
        holders.map(|h| if h { held(observation) } else { None }),
        now,
    );
    let changes = state.plan_step(now, active(), &[observation]);
    assert_eq!(changes.len(), usize::from(!native));
    if let Some(call) = changes.first() {
        assert_eq!(
            *call,
            flag_call(
                observation.identity,
                true,
                hknpBody::RAISE_CONTACT_IMPULSE_EVENTS,
                hknpWorld::REBUILD_COLLISION_CACHES
            )
        );
    }
    let lease = state.leases[0].unwrap();
    assert!(!lease.armed);
    assert!(state.plan_step(now, active(), &[observation]).is_empty());
    assert!(state.leases[0].unwrap().armed);
    (state, observation, now)
}

fn contact(state: &State) -> Contact {
    state.leases[0]
        .unwrap()
        .contact(Vec3::ZERO, Vec3::X, 12.0, false)
}

fn flag_call(identity: Identity, enable: bool, flags: u32, mode: i32) -> FlagCall {
    FlagCall {
        world: identity.world,
        body: identity.body,
        flags,
        mode,
        enable,
    }
}

#[test]
fn a_queued_contact_expires_without_another_step() {
    let (mut state, observation, now) = acquire([true, false], false);
    state.queue(contact(&state), now);
    assert!(
        state
            .deliver(
                now + Duration::from_millis(251),
                active(),
                &[observation.identity]
            )
            .is_empty()
    );
    assert_eq!(state.status.suppressed, 1);
}

#[test]
fn a_fresh_identical_publication_does_not_keep_an_old_contact_alive() {
    let (mut state, observation, now) = acquire([true, false], false);
    state.queue(contact(&state), now);
    state.publish([held(observation), None], now + Duration::from_millis(200));
    assert!(
        state
            .deliver(
                now + Duration::from_millis(251),
                active(),
                &[observation.identity]
            )
            .is_empty()
    );
}

#[test]
fn drop_and_reacquire_the_same_body_does_not_resurrect_feedback() {
    let (mut state, observation, now) = acquire([true, false], false);
    state.queue(contact(&state), now);
    state.publish([None; 2], now + Duration::from_millis(10));
    state.publish([held(observation), None], now + Duration::from_millis(20));
    state.plan_step(now + Duration::from_millis(20), active(), &[observation]);
    assert!(
        state
            .deliver(
                now + Duration::from_millis(30),
                active(),
                &[observation.identity]
            )
            .is_empty()
    );
}

#[test]
fn a_released_hand_is_removed_from_a_queued_shared_contact() {
    let (mut state, observation, now) = acquire([true; 2], false);
    state.queue(contact(&state), now);
    state.publish([None, held(observation)], now + Duration::from_millis(10));
    let feedback = state.deliver(
        now + Duration::from_millis(20),
        active(),
        &[observation.identity],
    );
    assert_eq!(feedback.len(), 1);
    assert_eq!(feedback[0].hands, [false, true]);
    assert_eq!(state.status.feedback, 1);
}

#[test]
fn a_reacquired_hand_does_not_join_the_other_hands_old_contact() {
    let (mut state, observation, now) = acquire([true; 2], false);
    state.queue(contact(&state), now);
    state.publish([None, held(observation)], now + Duration::from_millis(10));
    state.publish([held(observation); 2], now + Duration::from_millis(20));
    assert_eq!(
        state.deliver(
            now + Duration::from_millis(30),
            active(),
            &[observation.identity]
        )[0]
        .hands,
        [false, true]
    );
}

#[test]
fn each_delivery_gate_rejects_a_contact_queued_before_disable() {
    let disabled = [
        Enabled {
            controllers: false,
            ..active()
        },
        Enabled {
            hands: false,
            ..active()
        },
        Enabled {
            pickup: false,
            ..active()
        },
        Enabled {
            feature: false,
            ..active()
        },
        Enabled {
            gameplay: false,
            ..active()
        },
        Enabled {
            running: false,
            ..active()
        },
    ];
    for enabled in disabled {
        let (mut state, observation, now) = acquire([true, false], false);
        state.queue(contact(&state), now);
        assert!(
            state
                .deliver(now, enabled, &[observation.identity])
                .is_empty()
        );
        assert!(
            state
                .deliver(now, active(), &[observation.identity])
                .is_empty()
        );
    }
}

#[test]
fn repeated_identical_publication_keeps_a_legitimate_contact() {
    let (mut state, observation, now) = acquire([true, false], false);
    state.queue(contact(&state), now);
    for millis in [10, 20, 30] {
        state.publish(
            [held(observation), None],
            now + Duration::from_millis(millis),
        );
    }
    let feedback = state.deliver(
        now + Duration::from_millis(40),
        active(),
        &[observation.identity],
    );
    assert_eq!(feedback.len(), 1);
    assert_eq!(feedback[0].hands, [true, false]);
}

#[test]
fn identical_publication_after_a_stale_gap_cannot_refresh_an_old_hold_stamp() {
    let (mut state, observation, now) = acquire([true, false], false);
    let old = contact(&state);
    state.publish([held(observation), None], now + Duration::from_millis(300));
    state.queue(old, now + Duration::from_millis(300));
    assert!(
        state
            .deliver(
                now + Duration::from_millis(301),
                active(),
                &[observation.identity]
            )
            .is_empty()
    );
}

#[test]
fn acquisition_and_release_change_only_the_leased_flag_when_needed() {
    for native in [false, true] {
        let (mut state, observation, now) = acquire([true; 2], native);
        state.publish([None, held(observation)], now);
        assert!(state.plan_step(now, active(), &[observation]).is_empty());
        state.publish([None; 2], now);
        let changes = state.plan_step(now, active(), &[observation]);
        assert_eq!(changes.len(), usize::from(!native));
        if !native {
            assert_eq!(
                changes[0],
                flag_call(
                    observation.identity,
                    false,
                    hknpBody::RAISE_CONTACT_IMPULSE_EVENTS,
                    hknpWorld::REBUILD_COLLISION_CACHES
                )
            );
        }
        assert!(state.leases.iter().all(Option::is_none));
    }
}

#[test]
fn expired_and_disabled_holds_plan_restoration() {
    for (delay, enabled) in [
        (251, active()),
        (
            0,
            Enabled {
                pickup: false,
                ..active()
            },
        ),
    ] {
        let (mut state, observation, now) = acquire([true, false], false);
        let changes = state.plan_step(now + Duration::from_millis(delay), enabled, &[observation]);
        assert_eq!(
            changes,
            vec![flag_call(
                observation.identity,
                false,
                hknpBody::RAISE_CONTACT_IMPULSE_EVENTS,
                hknpWorld::REBUILD_COLLISION_CACHES
            )]
        );
    }
}

#[test]
fn identity_mismatch_never_restores_or_delivers_to_a_reused_body() {
    for identity in [
        Identity {
            world: 10,
            ..observed(false).identity
        },
        Identity {
            instance: 11,
            ..observed(false).identity
        },
        Identity {
            owner: NonZeroUsize::new(12).unwrap(),
            ..observed(false).identity
        },
    ] {
        let (mut state, observation, now) = acquire([true, false], false);
        state.queue(contact(&state), now);
        state.publish([None; 2], now);
        assert!(
            state
                .plan_step(
                    now,
                    active(),
                    &[Observation {
                        identity,
                        ..observation
                    }]
                )
                .is_empty()
        );
        assert!(state.deliver(now, active(), &[identity]).is_empty());
    }
}

#[test]
fn native_flag_changes_preserve_initial_state_mixed_masks_and_cache_mode() {
    let unrelated = hknpBody::IS_KEYFRAMED;
    for initial in [false, true] {
        for requested in [false, true] {
            let (mut state, observation, now) = acquire([true, false], initial);
            let call = flag_call(
                observation.identity,
                requested,
                hknpBody::RAISE_CONTACT_IMPULSE_EVENTS | unrelated,
                7,
            );
            let intercepted = state.intercept_flags(call, Some(observation.identity));
            assert_eq!(
                intercepted,
                FlagCall {
                    flags: if requested { call.flags } else { unrelated },
                    ..call
                }
            );
            state.publish([None; 2], now);
            let changes = state.plan_step(now, active(), &[observation]);
            assert_eq!(changes.len(), usize::from(!requested));
            if !requested {
                assert_eq!(
                    changes[0],
                    flag_call(
                        observation.identity,
                        false,
                        hknpBody::RAISE_CONTACT_IMPULSE_EVENTS,
                        hknpWorld::REBUILD_COLLISION_CACHES
                    )
                );
            }
        }
    }
}

#[test]
fn nonmatching_native_flag_calls_are_unchanged_and_do_not_change_restoration() {
    for kind in 0..5 {
        let (mut state, observation, now) = acquire([true, false], true);
        let call = flag_call(
            observation.identity,
            false,
            hknpBody::RAISE_CONTACT_IMPULSE_EVENTS,
            9,
        );
        let (call, identity) = match kind {
            0 => (
                FlagCall {
                    flags: hknpBody::IS_KEYFRAMED,
                    ..call
                },
                Some(observation.identity),
            ),
            1 => (FlagCall { world: 8, ..call }, Some(observation.identity)),
            2 => (FlagCall { body: 9, ..call }, Some(observation.identity)),
            3 => (
                call,
                Some(Identity {
                    instance: 10,
                    ..observation.identity
                }),
            ),
            _ => (call, None),
        };
        assert_eq!(state.intercept_flags(call, identity), call);
        state.publish([None; 2], now);
        assert!(state.plan_step(now, active(), &[observation]).is_empty());
    }
}

#[test]
fn a_recent_contact_cannot_deliver_after_the_publication_expires() {
    let (mut state, observation, now) = acquire([true, false], false);
    state.queue(contact(&state), now + Duration::from_millis(240));
    assert!(
        state
            .deliver(
                now + Duration::from_millis(251),
                active(),
                &[observation.identity]
            )
            .is_empty()
    );
    assert_eq!(state.status.suppressed, 1);
}

#[test]
fn the_latest_native_flag_request_controls_release_after_several_changes() {
    for final_request in [false, true] {
        let (mut state, observation, now) = acquire([true, false], true);
        for requested in [false, true, final_request] {
            let call = flag_call(
                observation.identity,
                requested,
                hknpBody::RAISE_CONTACT_IMPULSE_EVENTS | hknpBody::IS_KEYFRAMED,
                11,
            );
            let intercepted = state.intercept_flags(call, Some(observation.identity));
            assert_eq!(
                intercepted.flags,
                if requested {
                    call.flags
                } else {
                    hknpBody::IS_KEYFRAMED
                }
            );
            assert_eq!(intercepted.mode, call.mode);
        }
        state.publish([None; 2], now);
        let changes = state.plan_step(now, active(), &[observation]);
        assert_eq!(changes.len(), usize::from(!final_request));
    }
}

#[test]
fn delivery_rejects_changed_world_owner_and_wrapper_even_with_a_current_hold() {
    for identity in [
        Identity {
            world: 10,
            ..observed(false).identity
        },
        Identity {
            instance: 11,
            ..observed(false).identity
        },
        Identity {
            owner: NonZeroUsize::new(12).unwrap(),
            ..observed(false).identity
        },
    ] {
        let (mut state, _, now) = acquire([true, false], false);
        state.queue(contact(&state), now);
        assert!(state.deliver(now, active(), &[identity]).is_empty());
    }
}

#[test]
fn flag_calls_without_a_lease_pass_through_unchanged() {
    let mut state = State::new();
    let identity = observed(false).identity;
    for enable in [false, true] {
        let call = flag_call(
            identity,
            enable,
            hknpBody::RAISE_CONTACT_IMPULSE_EVENTS | hknpBody::IS_KEYFRAMED,
            13,
        );
        assert_eq!(state.intercept_flags(call, Some(identity)), call);
    }
}

#[test]
fn a_disabled_step_invalidates_old_contacts_before_same_identity_reacquisition() {
    let (mut state, observation, now) = acquire([true, false], false);
    let old = contact(&state);
    state.queue(old, now);
    let disabled_at = now + Duration::from_millis(10);
    let changes = state.plan_step(
        disabled_at,
        Enabled {
            feature: false,
            ..active()
        },
        &[observation],
    );
    assert_eq!(
        changes,
        vec![flag_call(
            observation.identity,
            false,
            hknpBody::RAISE_CONTACT_IMPULSE_EVENTS,
            hknpWorld::REBUILD_COLLISION_CACHES
        )]
    );
    assert!(state.leases.iter().all(Option::is_none));

    let enabled_at = now + Duration::from_millis(20);
    state.publish([held(observation), None], enabled_at);
    assert_eq!(
        state.plan_step(enabled_at, active(), &[observation]).len(),
        1
    );
    assert!(
        state
            .plan_step(enabled_at, active(), &[observation])
            .is_empty()
    );
    assert!(
        state
            .deliver(enabled_at, active(), &[observation.identity])
            .is_empty()
    );

    state.queue(old, enabled_at);
    assert!(
        state
            .deliver(enabled_at, active(), &[observation.identity])
            .is_empty()
    );
    state.queue(contact(&state), enabled_at);
    assert_eq!(
        state.deliver(enabled_at, active(), &[observation.identity])[0].hands,
        [true, false]
    );
}

#[test]
fn disabled_clear_drops_queue_and_restores_the_valid_lease() {
    let (mut state, observation, now) = acquire([true, false], false);
    state.queue(contact(&state), now);
    state.clear(now);
    assert_eq!(state.plan_step(now, active(), &[observation]).len(), 1);
    assert!(
        state
            .deliver(now, active(), &[observation.identity])
            .is_empty()
    );
}
