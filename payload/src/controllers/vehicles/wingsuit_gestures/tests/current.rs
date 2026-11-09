use super::*;

#[test]
fn grapple_after_leaving_the_suit_deploys_without_another_neutral_and_stops_retrying_in_suit() {
    for mode in [FALLING, GROUND] {
        let mut h = History::new(SUIT);
        h.hold(FLIGHT, 6);
        h.mode = mode;
        h.airborne = mode == FALLING;
        h.deploy_grapple = true;
        let commands = h.hold(FLIGHT, 6);
        assert_eq!(deploys(&commands), 1);
        assert_eq!(h.state.phase(), GesturePhase::AwaitingFlight);
        assert_eq!(deploys(&h.hold(FLIGHT, 10)), 1);
        h.mode = SUIT;
        h.deploy_grapple = false;
        assert!(h.hold(FLIGHT, 30).iter().all(|c| c.transition.is_none()));
    }
}

#[test]
fn active_grapple_still_defers_to_canopy_grip_slot_and_mechanical_intent() {
    for denial in 0..7 {
        let mut h = History::new(FALLING);
        h.deploy_grapple = true;
        if denial == 0 {
            h.canopy = true;
        } else {
            let claim = &mut h.claims[(denial - 1) / 3];
            match (denial - 1) % 3 {
                0 => claim.grip = true,
                1 => claim.slot = true,
                _ => claim.mechanical = true,
            }
        }
        assert_eq!(deploys(&h.hold(FLIGHT, 20)), 0);
        h.canopy = false;
        h.claims = [HandClaims::default(); 2];
        assert_eq!(deploys(&h.hold(FLIGHT, 6)), 1);
    }
}

#[test]
fn grapple_release_during_pending_deployment_preserves_retries_without_lowering_arms() {
    let mut h = History::new(FALLING);
    h.deploy_grapple = true;
    assert_eq!(deploys(&h.hold(FLIGHT, 6)), 1);
    h.deploy_grapple = false;
    assert_eq!(deploys(&h.hold(FLIGHT, 10)), 1);
    assert!(h.state.deploy_since_ns.is_some());
}

#[test]
fn current_availability_persists_without_a_command_and_across_transition_pulses() {
    for (mode, availability) in [
        (FALLING, GestureAvailability::Deploy),
        (SUIT, GestureAvailability::CloseAndBrake),
    ] {
        let mut h = History::new(mode);
        for _ in 0..20 {
            assert_eq!(h.observe(Some(FOLDED)), GestureCommands::default());
            assert_eq!(h.state.current(), availability);
        }
        h.hold(FOLDED, 5);
        let commands = h.hold(FLIGHT, 6);
        assert_eq!(h.state.current(), availability);
        if mode == FALLING {
            assert!(commands.iter().any(|command| command.transition.is_some()));
        } else {
            assert!(commands.iter().all(|command| command.transition.is_none()));
            let commands = h.hold(FOLDED, 9);
            assert!(commands.iter().any(|command| command.transition.is_some()));
            assert_eq!(h.state.current(), availability);
        }
        h.state.reset();
        assert_eq!(h.state.current(), GestureAvailability::Unavailable);
    }
}

#[test]
fn current_availability_uses_the_recognizers_whole_input_gate() {
    for denial in 0..15 {
        let mut h = History::new(SUIT);
        h.observe(Some(FLIGHT));
        let mut pose = Some(FLIGHT);
        match denial {
            0 => h.gameplay = false,
            1 => h.canopy = true,
            2 | 3 => h.claims[denial - 2].grapple = true,
            4..=11 => {
                let claim = &mut h.claims[(denial - 4) / 4];
                match (denial - 4) % 4 {
                    0 => claim.grip = true,
                    1 => claim.slot = true,
                    2 => claim.mechanical = true,
                    _ => claim.grapple = true,
                }
            }
            12 => pose = None,
            13 => pose.as_mut().unwrap().lateral[0] = f32::NAN,
            _ => h.mode = CANOPY,
        }
        assert_eq!(h.observe(pose), GestureCommands::default());
        assert_eq!(
            h.state.current(),
            GestureAvailability::Unavailable,
            "{denial}"
        );
    }
}

#[test]
fn backward_samples_and_large_gaps_remove_current_availability() {
    for backward in [false, true] {
        let mut h = History::new(SUIT);
        h.observe(Some(FLIGHT));
        let mut sample = h.sample;
        if backward {
            sample.sequence -= 1;
            sample.time_ns -= 1;
        } else {
            sample.sequence += 1;
            sample.time_ns += 10_000_000_000;
        }
        let commands = h.state.step(
            GestureInput {
                sample,
                mode: SUIT,
                airborne: false,
                deploy_grapple: false,
                gameplay: true,
                canopy_requested: false,
                pose: Some(FLIGHT),
                claims: [HandClaims::default(); 2],
            },
            &CONFIG,
        );
        assert_eq!(commands, GestureCommands::default());
        assert_eq!(h.state.current(), GestureAvailability::Unavailable);
    }
}
