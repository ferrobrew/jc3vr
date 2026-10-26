use super::*;
use crate::controllers::{
    mode::OnFootVariant, summary::eligibility, weapons::reload::eligible_belt_for_test,
};

mod climb;
mod doors;
mod parachute;

fn owner_state(guns: [Option<GunOwner>; 2]) -> BeforeFinish {
    BeforeFinish {
        guns,
        reload: ReloadStatus::default(),
        foregrip: None,
        grenade: [false; 2],
        c4: None,
        parachute: [None; 2],
    }
}

#[test]
fn capture_revalidates_belt_against_physical_guns_without_per_hand_trigger_substitution() {
    let mut config = ControllersConfig::default();
    config.weapons.hold.either_hand = true;
    config.weapons.hold.per_hand_tick = false;
    let hand = config.acting_hand();
    let mode = Mode::OnFoot(OnFootVariant::Ground);
    let (lease, guns) = eligible_belt_for_test(&config);
    let before = owner_state(guns);
    let gun_triggers = config.weapons.hold.either_hand && config.weapons.hold.per_hand_tick;
    let owners = guns.map(|gun| Owner {
        mode,
        gun: gun.filter(|_| gun_triggers),
    });
    assert!(owners.iter().all(|owner| owner.gun.is_none()));
    let ready = Hand::BOTH.map(|physical| physical == hand);
    let row = Row {
        control: Control::Motion(hand),
        gesture: Gesture::Press,
        effect: Effect::Mod(ModAction::BeltReload),
    };
    let capture = |physical_guns, sequence, ready, belt_reload| {
        capture_current(
            Refresh {
                sequence,
                belt_reload,
                config: &config,
                owners,
                ready,
                trigger_blocked: [true; 2],
                chosen: [None; 2],
                selected: [None; 2],
                grabs: [None; 2],
                before: &before,
                grapple: GrapplePermissions::default(),
                yank: None,
                flight: GestureAvailability::Deploy,
                parachute: [None; 2],
            },
            owner_state(physical_guns),
            [false; 2],
            ReloadPermissions::default(),
            [false; 2],
        )
    };
    let snapshot = capture(guns, 7, ready, Some(lease));
    assert!(snapshot.mechanics.contains(&row));
    assert_eq!(snapshot.routing.triggers, [TriggerUse::Reserved; 2]);
    assert!(
        eligibility::resolve(std::slice::from_ref(&row), &snapshot)
            .context
            .usable
            .contains(&row)
    );
    assert_eq!(
        snapshot.mechanics,
        tables::describe::flight_mechanics(GestureAvailability::Deploy)
            .into_iter()
            .chain(std::iter::once(row.clone()))
            .collect::<Vec<_>>()
    );

    let mut changed = guns;
    changed[hand.index()].as_mut().unwrap().weapon = 2;
    let mut carried = guns;
    carried[hand.index()].as_mut().unwrap().role = GunHand::Carry;
    for physical in [changed, carried, [None; 2]] {
        let revoked = capture(physical, 7, ready, Some(lease));
        assert!(!revoked.mechanics.contains(&row));
        assert!(
            eligibility::resolve(std::slice::from_ref(&row), &revoked)
                .context
                .usable
                .is_empty()
        );
        assert_eq!(revoked.routing.triggers, [TriggerUse::Reserved; 2]);
    }
    for unavailable in [
        capture(guns, 8, ready, Some(lease)),
        capture(guns, 7, [false; 2], Some(lease)),
        capture(guns, 7, ready, None),
    ] {
        assert!(!unavailable.mechanics.contains(&row));
    }
}
