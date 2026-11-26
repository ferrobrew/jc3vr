use super::*;

#[test]
fn ownerless_geometry_keeps_feedback_and_obstruction_without_damage() {
    let (mut state, observation, now) = acquire([true; 2], false);
    let leases = state.leases;
    let mut event = leases[0].unwrap().contact(Vec3::ZERO, Vec3::X, 12.0, false);
    event.retention = 1;
    record_leased(
        [observation.identity.body, 20],
        leases,
        |_| [Some(event), None],
        |contacts| {
            for event in contacts.into_iter().flatten() {
                state.queue(event, now);
            }
        },
    );
    let feedback = state.deliver(now, active(), &[observation.identity]);
    assert_eq!(feedback.len(), 1);
    assert_eq!(feedback[0].hands, [true; 2]);
    assert_eq!(feedback[0].share, 0.5);
    assert!(feedback[0].damage_impulse.is_none());
    assert_eq!(feedback[0].native, [false; 2]);
    assert_eq!(feedback[0].contact.target, None);
    assert_eq!(
        state.obstruction(
            observation.identity.body,
            observation.identity.owner,
            Vec3::new(2.0, 3.0, 4.0),
            now,
            active(),
        ),
        Vec3::X * 2.0
    );
    state.queue(event, now);
    assert!(
        state
            .deliver(now, active(), &[observation.identity])
            .is_empty()
    );
}

#[test]
fn unknown_target_motion_keeps_feedback_without_consuming_damage_cooldown() {
    let (mut state, observation, now) = acquire([true, false], false);
    let mut event = contact(&state);
    event.target_motion = None;
    state.queue(event, now);
    let valid = [observation.identity, target()];
    let feedback = state.deliver(now, active(), &valid);
    assert_eq!(feedback.len(), 1);
    assert_eq!(feedback[0].hands, [true, false]);
    assert_eq!(feedback[0].share, 0.5);
    assert!(feedback[0].damage_impulse.is_none());
    event.target_motion = Some(Motion::STILL);
    state.queue(event, now);
    let feedback = state.deliver(now, active(), &valid);
    assert_eq!(feedback.len(), 1);
    assert_eq!(feedback[0].hands, [false; 2]);
    assert_eq!(feedback[0].damage_impulse, Some(12.0));
    state.queue(event, now);
    assert!(state.deliver(now, active(), &valid).is_empty());
}

#[test]
fn damage_requires_a_retained_target_even_with_known_motion_and_valid_identities() {
    for missing in 0..2 {
        let (mut state, observation, now) = acquire([true, false], false);
        let mut event = contact(&state);
        if missing == 0 {
            event.target = None;
        } else {
            event.retention = 0;
        }
        state.queue(event, now);
        let feedback = state.deliver(now, active(), &[observation.identity, target()]);
        assert_eq!(feedback.len(), 1);
        assert_eq!(feedback[0].hands, [true, false]);
        assert!(feedback[0].damage_impulse.is_none());
    }
}

#[test]
fn ownerless_contacts_still_require_the_original_live_hold() {
    for invalid in 0..4 {
        let (mut state, observation, now) = acquire([true, false], false);
        let mut event = state.leases[0]
            .unwrap()
            .contact(Vec3::ZERO, Vec3::X, 12.0, false);
        event.retention = 1;
        state.queue(event, now);
        let mut valid = vec![observation.identity];
        let mut deliver_at = now;
        match invalid {
            0 => valid.clear(),
            1 => {
                valid[0].control += 1;
            }
            2 => {
                state.publish([None; 2], now);
                state.publish([held(observation), None], now);
                state.plan_step(now, active(), &[observation]);
            }
            3 => deliver_at += Duration::from_millis(251),
            _ => unreachable!(),
        }
        assert!(state.deliver(deliver_at, active(), &valid).is_empty());
    }
}
