use super::*;
use crate::controllers::{
    grab::{self, target::TargetReach},
    physical_hands::{DoorProvider, grasp::TargetKind},
};

fn row(hand: Hand) -> Row {
    Row {
        control: Control::Grip(hand),
        gesture: Gesture::Press,
        effect: Effect::Mod(ModAction::OpenVehicleDoor),
    }
}

fn capture_door(
    provider: &DoorProvider,
    tick: &grab::tick::Tick,
    released: [bool; 2],
    ready: [bool; 2],
    grabs: [Option<GrabVolume>; 2],
) -> Eligibility {
    capture_door_with_owner(
        provider,
        tick,
        released,
        ready,
        grabs,
        owner_state([None; 2]),
    )
}

fn capture_door_with_owner(
    provider: &DoorProvider,
    tick: &grab::tick::Tick,
    released: [bool; 2],
    ready: [bool; 2],
    grabs: [Option<GrabVolume>; 2],
    current: BeforeFinish,
) -> Eligibility {
    let config = ControllersConfig {
        physical_hands: provider.config,
        ..Default::default()
    };
    let before = owner_state([None; 2]);
    let selected = Hand::BOTH
        .map(|hand| tick.selected_for_summary()[hand.index()].filter(|_| !released[hand.index()]));
    let doors = Hand::BOTH
        .map(|hand| selected[hand.index()].is_some_and(|target| provider.current(hand, target)));
    capture_current(
        Refresh {
            sequence: 7,
            belt_reload: None,
            config: &config,
            owners: [Owner {
                mode: Mode::OnFoot(OnFootVariant::Ground),
                gun: None,
            }; 2],
            ready,
            grapple_routes: Default::default(),
            trigger_blocked: [false; 2],
            chosen: Hand::BOTH
                .map(|hand| tick.chosen()[hand.index()].filter(|_| !released[hand.index()])),
            selected,
            grabs,
            before: &before,
            grapple: GrapplePermissions::default(),
            yank: None,
            flight: GestureAvailability::Unavailable,
            parachute: [None; 2],
        },
        current,
        [false; 2],
        ReloadPermissions::default(),
        doors,
    )
}

fn select(targets: Vec<GripTarget>) -> grab::tick::Tick {
    grab::tick::resolve(targets, [Vec::new(), Vec::new()], None)
}

fn has_door(snapshot: &Eligibility, hand: Hand) -> bool {
    let expected = row(hand);
    let report = eligibility::resolve(&[], snapshot);
    assert_eq!(
        snapshot.mechanics.contains(&expected),
        report.context.usable.contains(&expected)
    );
    report.context.usable.contains(&expected)
}

#[test]
fn actual_provider_selection_captures_only_the_chosen_unheld_latched_door() {
    for hand in Hand::BOTH {
        let mut provider = DoorProvider::new();
        let latched = provider.door(hand, true);
        let tick = select(vec![latched]);
        assert_eq!(tick.selected_for_summary()[hand.index()], Some(latched));
        let snapshot = capture_door(&provider, &tick, [false; 2], [true; 2], [None; 2]);
        assert!(has_door(&snapshot, hand));
        assert_eq!(snapshot.mechanics, vec![row(hand)]);
        assert!(!has_door(&snapshot, hand.other()));

        for other in 0..3 {
            let mut competitor = DoorProvider::new();
            let mut target = match other {
                0 => competitor.door(hand, false),
                1 => competitor.door(hand, true).held(hand),
                _ => competitor.target(hand, TargetKind::Prop { mass_kg: 1.0 }),
            };
            target.reach = TargetReach::Measured { distance: 0.0 };
            let tick = select(vec![latched, target]);
            assert_eq!(tick.resolved[hand.index()].chosen.unwrap().index, 1);
            assert!(!has_door(
                &capture_door(&provider, &tick, [false; 2], [true; 2], [None; 2]),
                hand,
            ));
        }
        assert!(!has_door(
            &capture_door(
                &provider,
                &select(Vec::new()),
                [false; 2],
                [true; 2],
                [None; 2]
            ),
            hand,
        ));
        let diagnostic_only = GripTarget {
            door_eligibility: None,
            ..latched
        };
        assert!(diagnostic_only.observed.is_some());
        assert!(!has_door(
            &capture_door(
                &provider,
                &select(vec![diagnostic_only]),
                [false; 2],
                [true; 2],
                [None; 2]
            ),
            hand,
        ));
    }
}

#[test]
fn final_capture_revokes_door_offers_after_release_consumption_and_unready_hands() {
    for hand in Hand::BOTH {
        let mut provider = DoorProvider::new();
        let tick = select(vec![provider.door(hand, true)]);
        let mut released = [false; 2];
        released[hand.index()] = true;
        let mut ready = [true; 2];
        ready[hand.index()] = false;
        let mut grabs = [None; 2];
        grabs[hand.index()] = Some(GrabVolume::Pickup(PickupKind::Door));
        for snapshot in [
            capture_door(&provider, &tick, released, [true; 2], [None; 2]),
            capture_door(&provider, &tick, [false; 2], ready, [None; 2]),
            capture_door(&provider, &tick, [false; 2], [true; 2], grabs),
        ] {
            assert!(!has_door(&snapshot, hand));
        }
        let changed = owner_state(Hand::BOTH.map(|h| {
            (h == hand).then_some(GunOwner {
                weapon: 3,
                role: GunHand::Grip,
            })
        }));
        assert!(!has_door(
            &capture_door_with_owner(&provider, &tick, [false; 2], [true; 2], [None; 2], changed),
            hand,
        ));
        let mut other_unready = [true; 2];
        other_unready[hand.other().index()] = false;
        assert!(has_door(
            &capture_door(&provider, &tick, [false; 2], other_unready, [None; 2]),
            hand,
        ));
    }
}

#[test]
fn final_capture_does_not_reuse_a_selected_door_after_source_or_claim_changes() {
    for hand in Hand::BOTH {
        for change in 0..12 {
            let mut provider = DoorProvider::new();
            let tick = select(vec![provider.door(hand, true)]);
            match change {
                0 => provider.expire(),
                1 => provider.replace_source(),
                2 => provider.replace_sample(),
                3 => provider.remove_reach(hand),
                4 => provider.claim(hand),
                5 => provider.hold(hand),
                6 => provider.latch(hand),
                7 => provider.config.enabled = false,
                8 => provider.config.pickup.enabled = false,
                9 => provider.config.doors.enabled = false,
                10 => provider.replace_door(hand),
                _ => provider.claim_same_door(hand),
            }
            assert!(
                !has_door(
                    &capture_door(&provider, &tick, [false; 2], [true; 2], [None; 2]),
                    hand,
                ),
                "change {change}"
            );
        }
    }
}
