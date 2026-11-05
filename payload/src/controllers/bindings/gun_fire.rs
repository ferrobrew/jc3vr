//! Each hand's fire press for the gun it holds.
//!
//! On foot, falling, in the wingsuit, and out of a vehicle's driver's seat, a gun fires from its
//! hand's trigger. In a vehicle a gun also takes over its hand's vehicle weapon action
//! ([`vehicle_weapon_actions`]), the upper face button by default, so the binding that fires the
//! vehicle's weapon from a hand fires that hand's gun instead, and a player who rebinds the one
//! rebinds the other. In the driver's seat the triggers are the pedals, so there the vehicle
//! weapon action alone fires the gun. The tables keep a gun hand's vehicle weapon action from the
//! vehicle's weapon ([`crate::controllers::bindings::tables::sink::Gate::HandUnarmed`]).

use crate::controllers::{
    bindings::{
        controls::ButtonControl,
        intents::SemanticInput,
        tables::{declare::vehicle_weapon_actions, merge_buttons},
    },
    frame::Hand,
    mode::Mode,
};

/// Each hand's fire press, by [`Hand::index`]: `triggers` where the triggers fire the hand's gun in
/// `mode`, merged in a vehicle with the hand's vehicle weapon actions in `input`. A hand in
/// `unavailable` presses nothing.
pub(super) fn presses(
    mode: Mode,
    input: &SemanticInput,
    triggers: [ButtonControl; 2],
    unavailable: [bool; 2],
) -> [ButtonControl; 2] {
    Hand::BOTH.map(|hand| {
        let i = hand.index();
        if unavailable[i] {
            return ButtonControl::default();
        }
        let trigger = if triggers_fire_guns(mode) {
            triggers[i]
        } else {
            ButtonControl::default()
        };
        let Mode::Vehicle(seat) = mode else {
            return trigger;
        };
        vehicle_weapon_actions(seat.class)
            .into_iter()
            .filter_map(|id| input.get(id))
            .map(|action| action.hands[i].button)
            .fold(trigger, merge_buttons)
    })
}

/// Whether a gun fires from its hand's trigger in `mode`: everywhere but a driver's seat, whose
/// triggers are the pedals, and the menus, where no gun fires.
const fn triggers_fire_guns(mode: Mode) -> bool {
    match mode {
        Mode::Vehicle(seat) => !seat.driver,
        Mode::Ui => false,
        Mode::OnFoot(_) | Mode::Airborne(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use test_case::test_case;

    use super::*;
    use crate::{
        controllers::{
            bindings::{controls::Controls, intents::simulate::simulate},
            mode::{AirborneVariant, OnFootVariant, VehicleClass, VehicleSeat},
        },
        vr::input::semantic::{SemanticSet, defaults::default_binding},
    };

    const HELD: ButtonControl = ButtonControl {
        down: true,
        pressed: true,
        released: false,
    };

    fn seat(class: VehicleClass, driver: bool) -> Mode {
        Mode::Vehicle(VehicleSeat { class, driver })
    }

    /// The default layout's semantic input with each hand's upper face button held as `faces`.
    fn faces(mode: Mode, dominant: Hand, faces: [bool; 2]) -> SemanticInput {
        let mut controls = Controls {
            dominant,
            ..Default::default()
        };
        for hand in Hand::BOTH {
            if faces[hand.index()] {
                controls.hands[hand.index()].b = HELD;
            }
        }
        simulate(
            &controls,
            SemanticSet::of(mode.action_set()),
            default_binding,
        )
    }

    #[test_case(VehicleClass::Car)]
    #[test_case(VehicleClass::Motorcycle)]
    #[test_case(VehicleClass::Sea)]
    #[test_case(VehicleClass::Helicopter)]
    #[test_case(VehicleClass::Plane)]
    fn the_upper_face_fires_its_hands_gun_in_every_seat(class: VehicleClass) {
        for driver in [false, true] {
            let mode = seat(class, driver);
            for dominant in Hand::BOTH {
                for hand in Hand::BOTH {
                    let input = faces(mode, dominant, Hand::BOTH.map(|h| h == hand));
                    let fire = presses(mode, &input, [ButtonControl::default(); 2], [false; 2]);
                    assert_eq!(
                        fire.map(|b| b.held()),
                        Hand::BOTH.map(|h| h == hand),
                        "{mode:?} {dominant:?} {hand:?}"
                    );
                }
            }
        }
    }

    #[test_case(VehicleClass::Car)]
    #[test_case(VehicleClass::Helicopter)]
    #[test_case(VehicleClass::Plane)]
    fn the_trigger_fires_the_gun_out_of_the_drivers_seat_only(class: VehicleClass) {
        for driver in [false, true] {
            let mode = seat(class, driver);
            let input = faces(mode, Hand::Right, [false; 2]);
            let fire = presses(mode, &input, [HELD; 2], [false; 2]);
            assert_eq!(fire.map(|b| b.held()), [!driver; 2], "{mode:?}");
        }
    }

    #[test_case(Mode::OnFoot(OnFootVariant::Ground))]
    #[test_case(Mode::Airborne(AirborneVariant::Falling))]
    #[test_case(Mode::Airborne(AirborneVariant::Wingsuit))]
    fn out_of_a_vehicle_the_trigger_alone_fires(mode: Mode) {
        let input = faces(mode, Hand::Right, [true; 2]);
        let idle = presses(mode, &input, [ButtonControl::default(); 2], [false; 2]);
        assert_eq!(idle.map(|b| b.held()), [false; 2]);
        let pulled = presses(mode, &input, [HELD; 2], [false; 2]);
        assert_eq!(pulled.map(|b| b.held()), [true; 2]);
    }

    #[test]
    fn an_unavailable_hand_and_the_menus_fire_nothing() {
        let mode = seat(VehicleClass::Car, false);
        let input = faces(mode, Hand::Right, [true; 2]);
        let fire = presses(mode, &input, [HELD; 2], [true, false]);
        assert_eq!(fire.map(|b| b.held()), [false, true]);
        let menu = presses(
            Mode::Ui,
            &faces(Mode::Ui, Hand::Right, [false; 2]),
            [HELD; 2],
            [false; 2],
        );
        assert_eq!(menu.map(|b| b.held()), [false; 2]);
    }
}
