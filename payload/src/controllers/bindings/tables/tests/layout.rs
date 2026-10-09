//! The shipped default layout against the one the raw-input tables encoded: whatever a player
//! could reach before stays reachable, but for the actions the new layout drops on purpose.

use std::collections::BTreeSet;

use super::*;
use crate::controllers::bindings::tables::{
    coverage::{all_contexts, all_modes, injected, injected_with},
    tests::equivalence::legacy_layout,
};

/// The game actions the new layout leaves without a button: grenades come from their pouch and
/// C4 from its chest pouch, and the grenade throw stays as an unbound action for a player who
/// binds it. The gamepad's B (releasing the tethers, cancelling, and stepping off a ledge) is the
/// grapple button's double tap ([`crate::controllers::bindings::grapple_button`]), a gesture the
/// tables do not declare.
const DROPPED: [Action; 6] = [
    THROW_GRENADE,
    PLANT_EXPLOSIVE,
    RELEASE_GRAPPLE,
    CANCEL,
    BYPASS_FALL_PREVENTION,
    BTN_B,
];

#[test]
fn every_action_reachable_before_stays_reachable() {
    let mut added = BTreeSet::new();
    for mode in all_modes() {
        for (dominant, ctx) in all_contexts() {
            let before = injected_with(mode, dominant, ctx, legacy_layout);
            let now = injected(mode, dominant, ctx);
            let lost: Vec<_> = before
                .difference(&now)
                .filter(|a| !DROPPED.contains(a))
                .collect();
            assert!(
                lost.is_empty(),
                "{mode:?}, {dominant:?}-handed, {ctx:?}: {lost:?} no longer reachable"
            );
            added.extend(now.difference(&before).copied());
        }
    }
    // The new layout adds the weapon slot select on the off-hand trackpad, and the cutscene skip
    // on the right B.
    let select = BTreeSet::from([
        SELECT_DUEL_WIELD,
        SELECT_TWO_HANDED,
        SELECT_TWO_HANDED_SPECIAL,
        SELECT_EXPLOSIVES,
        SKIP_CUTSCENE,
    ]);
    assert_eq!(added, select);
}

#[test]
fn the_dominant_upper_button_enters_and_exits_vehicles() {
    for dominant in Hand::BOTH {
        let c = controls(dominant, |dom, _| dom.b = down());
        let step = Bindings::default().sim_step(ON_FOOT, &c, DT, &CONFIG);
        assert_eq!(player(&step, ENTER_VEHICLE), Some(Want::Hold(1.0)));
        assert_eq!(player(&step, RELOAD), None);
        for class in [
            VehicleClass::Car,
            VehicleClass::Helicopter,
            VehicleClass::Plane,
        ] {
            let seat = Mode::Vehicle(VehicleSeat {
                class,
                driver: true,
            });
            let step = Bindings::default().sim_step(seat, &c, DT, &CONFIG);
            assert_eq!(
                player(&step, EXIT_VEHICLE),
                Some(Want::Hold(1.0)),
                "{class:?}"
            );
            assert_eq!(player(&step, USE_VEHICLE_MOD), None, "{class:?}");
        }
    }
}

#[test]
fn the_off_hand_trackpad_selects_weapon_slots_by_quadrant() {
    for (pad, action) in [
        (Vec2::new(0.0, 0.8), SELECT_DUEL_WIELD),
        (Vec2::new(0.8, 0.1), SELECT_TWO_HANDED),
        (Vec2::new(-0.8, 0.0), SELECT_TWO_HANDED_SPECIAL),
        (Vec2::new(0.1, -0.8), SELECT_EXPLOSIVES),
    ] {
        let mut b = Bindings::default();
        let c = controls(Hand::Right, |_, off| {
            off.trackpad_left = down();
            off.trackpad = pad;
        });
        let step = b.sim_step(ON_FOOT, &c, DT, &CONFIG);
        let selected: Vec<_> = step
            .desired
            .iter()
            .map(|d| d.target.action)
            .filter(|a| {
                matches!(
                    a,
                    SELECT_DUEL_WIELD
                        | SELECT_TWO_HANDED
                        | SELECT_TWO_HANDED_SPECIAL
                        | SELECT_EXPLOSIVES
                )
            })
            .collect();
        assert_eq!(selected, [action], "{pad:?}");
        // The quadrant holds while the thumb slides, until the release.
        let slid = controls(Hand::Right, |_, off| {
            off.trackpad_left = ButtonControl {
                down: true,
                ..Default::default()
            };
            off.trackpad = -pad;
        });
        let step = b.sim_step(ON_FOOT, &slid, DT, &CONFIG);
        assert_eq!(player(&step, action), Some(Want::Hold(1.0)), "{pad:?}");
    }
    // A press in the centre selects nothing.
    let c = controls(Hand::Right, |_, off| {
        off.trackpad_left = down();
        off.trackpad = Vec2::new(0.05, 0.1);
    });
    let step = Bindings::default().sim_step(ON_FOOT, &c, DT, &CONFIG);
    assert!(step.desired.is_empty(), "{:?}", step.desired);
}
