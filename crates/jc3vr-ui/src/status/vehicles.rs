//! The status rows for the vehicles: the plane's flight stick and throttle, the cockpit
//! buttons, the passenger's aim, and the mounted gun.

use jc3vr_body::controllers::{
    grab::cockpit::ControlKind,
    mode::{VehicleClass, VehicleSeat},
};
use jc3vr_vehicles::controllers::{
    vehicles,
    vehicles::{VehiclesStatus, mounted_recoil, passenger, passenger::PassengerStatus},
};
use smol_str::{SmolStr, format_smolstr};

use crate::status::{Line, Row};

pub(crate) const FLIGHT_CONTROLS: Row = Row::new("Flight controls", flight_controls);
pub(crate) const COCKPIT_BUTTONS: Row = Row::new("Cockpit buttons", cockpit_buttons);
pub(crate) const PASSENGER_AIM: Row = Row::new("Passenger aim", passenger_aim);
pub(crate) const MOUNTED_GUN: Row = Row::new("Mounted gun", mounted_gun);

fn flight_controls() -> Line {
    flight_controls_line(&vehicles::status())
}

fn mounted_gun() -> Line {
    let status = vehicles::status();
    mounted_gun_line(status.mounted.is_some(), &status, mounted_recoil::shots())
}

fn mounted_gun_line(manned: bool, summary: &VehiclesStatus, shots: u64) -> Line {
    if !manned {
        return Line::waiting("Not on a mounted gun");
    }
    let hands = format_smolstr!(
        "{}",
        match summary.mounted_holds {
            [true, true] => "Both hands on",
            [true, false] => "Left hand on",
            [false, true] => "Right hand on",
            [false, false] => "Hands off, gun held",
        }
    );
    Line::ok(format_smolstr!(
        "{}, {} grabs and releases, {} kicks",
        hands,
        summary.mounted_grip_events,
        shots
    ))
}

fn passenger_aim() -> Line {
    let seat = vehicles::status().seat;
    passenger_aim_line(seat, passenger::status())
}

fn passenger_aim_line(seat: Option<VehicleSeat>, status: PassengerStatus) -> Line {
    match (seat.is_some_and(|s| !s.driver), status.aiming) {
        (true, true) => Line::ok(format_smolstr!(
            "Following the hand, {} starts",
            status.starts
        )),
        (true, false) => Line::waiting("No gun hand aiming"),
        (false, _) if status.starts > 0 => {
            Line::ok(format_smolstr!("Not a passenger, {} starts", status.starts))
        }
        (false, _) => Line::waiting("Not a passenger"),
    }
}

fn cockpit_buttons() -> Line {
    cockpit_buttons_line(&vehicles::status())
}

fn cockpit_buttons_line(summary: &VehiclesStatus) -> Line {
    if !(summary.active && summary.seat.is_some_and(|s| s.driver)) {
        return Line::waiting("Not driving");
    }
    let offers = |kind| summary.offered.contains(&kind);
    let mut here = Vec::new();
    if offers(ControlKind::Wheel) {
        here.push(SmolStr::new_static("horn"));
        here.push(SmolStr::new_static("nitrous button"));
    }
    if offers(ControlKind::Collective) {
        here.push(SmolStr::new_static("twist grip"));
    }
    if here.is_empty() {
        return Line::waiting("None in this seat");
    }
    let o = &summary.outputs;
    let working = format_smolstr!(
        "{}",
        match (o.horn, o.nitrous) {
            (true, true) => ", horn and nitrous on",
            (true, false) => ", horn on",
            (false, true) => ", nitrous on",
            (false, false) => "",
        }
    );
    Line::ok(format_smolstr!(
        "{}{}, {} presses",
        SmolStr::from(here.join(", ")),
        working,
        summary.button_presses
    ))
}

fn flight_controls_line(summary: &VehiclesStatus) -> Line {
    let flying = summary.active
        && summary
            .seat
            .is_some_and(|s| s.driver && s.class == VehicleClass::Plane);
    if !flying {
        return Line::waiting("Not flying a plane");
    }
    let held = |kind| summary.holds.contains(&Some(kind));
    let o = &summary.outputs;
    let stick_held = held(ControlKind::FlightStick) || held(ControlKind::Yoke);
    let stick = format_smolstr!(
        "{}",
        match (stick_held, o.flight_stick) {
            (true, _) => "Stick held",
            (false, Some(_)) => "Stick returning",
            (false, None) => "Stick free",
        }
    );
    let rudder = o
        .rudder
        .map_or_else(SmolStr::default, |r| format_smolstr!(", rudder {r:+.2}"));
    let throttle = o.throttle.map_or_else(
        || SmolStr::new_static("throttle by trigger"),
        |t| {
            format_smolstr!(
                "throttle {t:+.2}{}",
                if o.afterburner {
                    SmolStr::new_static(" afterburner")
                } else {
                    SmolStr::default()
                }
            )
        },
    );
    Line::ok(format_smolstr!(
        "{}{}, {}, {} cues, {} burns",
        stick,
        rudder,
        throttle,
        summary.cues,
        summary.afterburner_lights
    ))
}

#[cfg(test)]
mod tests {
    use jc3vr_vehicles::controllers::vehicles::physical::Outputs;

    use super::*;
    use crate::status::Health;

    fn flying() -> VehiclesStatus {
        VehiclesStatus {
            active: true,
            seat: Some(VehicleSeat {
                class: VehicleClass::Plane,
                driver: true,
            }),
            offered: &[ControlKind::FlightStick, ControlKind::Throttle],
            holds: [Some(ControlKind::Throttle), Some(ControlKind::FlightStick)],
            outputs: Outputs {
                flight_stick: Some(glam::Vec2::ZERO),
                rudder: Some(0.25),
                throttle: Some(1.0),
                afterburner: true,
                ..Outputs::NONE
            },
            cues: 3,
            afterburner_lights: 1,
            button_presses: 2,
            ..VehiclesStatus::default()
        }
    }

    #[test]
    fn the_mounted_gun_names_the_hands_on_it() {
        assert_eq!(
            mounted_gun_line(false, &flying(), 0).text(),
            "Waiting, Not on a mounted gun"
        );
        let manned = VehiclesStatus {
            mounted_holds: [true, false],
            mounted_grip_events: 3,
            ..flying()
        };
        assert_eq!(
            mounted_gun_line(true, &manned, 12).text(),
            "Ok, Left hand on, 3 grabs and releases, 12 kicks"
        );
    }

    #[test]
    fn the_passenger_aim_waits_for_a_seat_and_a_gun_hand() {
        let passenger = Some(VehicleSeat {
            class: VehicleClass::Car,
            driver: false,
        });
        let aiming = PassengerStatus {
            aiming: true,
            updates: 10,
            starts: 2,
        };
        assert_eq!(
            passenger_aim_line(passenger, aiming).text(),
            "Ok, Following the hand, 2 starts"
        );
        let idle = PassengerStatus {
            aiming: false,
            ..aiming
        };
        assert_eq!(passenger_aim_line(passenger, idle).health, Health::Waiting);
        assert_eq!(
            passenger_aim_line(None, PassengerStatus::default()).text(),
            "Waiting, Not a passenger"
        );
    }

    #[test]
    fn the_cockpit_buttons_name_what_the_seat_has() {
        assert_eq!(
            cockpit_buttons_line(&flying()).text(),
            "Waiting, None in this seat"
        );
        let car = VehiclesStatus {
            seat: Some(VehicleSeat {
                class: VehicleClass::Car,
                driver: true,
            }),
            offered: &[ControlKind::Wheel],
            outputs: Outputs {
                horn: true,
                ..Outputs::NONE
            },
            ..flying()
        };
        assert_eq!(
            cockpit_buttons_line(&car).text(),
            "Ok, horn, nitrous button, horn on, 2 presses"
        );
        let passenger = VehiclesStatus {
            seat: Some(VehicleSeat {
                class: VehicleClass::Car,
                driver: false,
            }),
            ..car
        };
        assert_eq!(cockpit_buttons_line(&passenger).health, Health::Waiting);
    }

    #[test]
    fn the_flight_controls_wait_for_a_plane_and_read_out_the_hands() {
        let car = VehiclesStatus {
            seat: Some(VehicleSeat {
                class: VehicleClass::Car,
                driver: true,
            }),
            ..flying()
        };
        assert_eq!(flight_controls_line(&car).health, Health::Waiting);
        assert_eq!(
            flight_controls_line(&flying()).text(),
            "Ok, Stick held, rudder +0.25, throttle +1.00 afterburner, 3 cues, 1 burns"
        );
        let parked = VehiclesStatus {
            holds: [None; 2],
            outputs: Outputs::NONE,
            ..flying()
        };
        assert_eq!(
            flight_controls_line(&parked).text(),
            "Ok, Stick free, throttle by trigger, 3 cues, 1 burns"
        );
    }
}
