//! The status rows for the vehicles: the plane's flight stick and throttle, the cockpit
//! buttons, the passenger's aim, and the mounted gun.

use jc3vr_body::controllers::{
    grab::cockpit::ControlKind,
    mode::{VehicleClass, VehicleSeat},
};
use jc3vr_vehicles::controllers::{
    vehicles,
    vehicles::{
        VehicleSummary, mounted_recoil, passenger,
        passenger::PassengerStatus,
        tuning::{CockpitButtonsTuning, VEHICLES},
    },
};

use crate::status::{Health, Line, Row, Text, prose, text};

pub(crate) const FLIGHT_CONTROLS: Row =
    Row::new("FLIGHT CONTROLS", "Flight controls", flight_controls);
pub(crate) const COCKPIT_BUTTONS: Row =
    Row::new("COCKPIT BUTTONS", "Cockpit buttons", cockpit_buttons);
pub(crate) const PASSENGER_AIM: Row = Row::new("PASSENGER AIM", "Passenger aim", passenger_aim);
pub(crate) const MOUNTED_GUN: Row = Row::new("MOUNTED GUN", "Mounted gun", mounted_gun);

fn flight_controls() -> Line {
    flight_controls_line(&vehicles::summary())
}

fn mounted_gun() -> Line {
    mounted_gun_line(&vehicles::summary(), mounted_recoil::shots())
}

fn mounted_gun_line(summary: &VehicleSummary, shots: u64) -> Line {
    if !summary.mounted {
        return Line::new(Health::Waiting, prose("Not on a mounted gun"));
    }
    let hands = prose(match summary.mounted_holds {
        [true, true] => "Both hands on",
        [true, false] => "Left hand on",
        [false, true] => "Right hand on",
        [false, false] => "Hands off, gun held",
    });
    Line::ok(text!(
        hands,
        ", ",
        summary.mounted_grip_events.to_string(),
        " grabs and releases, ",
        shots.to_string(),
        " kicks"
    ))
}

fn passenger_aim() -> Line {
    let seat = vehicles::summary().seat;
    passenger_aim_line(seat, passenger::status())
}

fn passenger_aim_line(seat: Option<VehicleSeat>, status: PassengerStatus) -> Line {
    match (seat.is_some_and(|s| !s.driver), status.aiming) {
        (true, true) => Line::ok(text!(
            "Following the hand, ",
            status.starts.to_string(),
            " starts"
        )),
        (true, false) => Line::new(Health::Waiting, prose("No gun hand aiming")),
        (false, _) if status.starts > 0 => Line::ok(text!(
            "Not a passenger, ",
            status.starts.to_string(),
            " starts"
        )),
        (false, _) => Line::new(Health::Waiting, prose("Not a passenger")),
    }
}

fn cockpit_buttons() -> Line {
    cockpit_buttons_line(&VEHICLES.get().physical.buttons, &vehicles::summary())
}

fn cockpit_buttons_line(config: &CockpitButtonsTuning, summary: &VehicleSummary) -> Line {
    if !config.any() {
        return Line::off("");
    }
    if !(summary.active && summary.seat.is_some_and(|s| s.driver)) {
        return Line::new(Health::Waiting, prose("Not driving"));
    }
    let offers = |kind| summary.offered.contains(&kind);
    let mut here = Vec::new();
    if offers(ControlKind::Wheel) {
        if config.horn {
            here.push(prose("horn"));
        }
        if config.nitrous_button {
            here.push(prose("nitrous button"));
        }
    }
    if config.twist_nitrous && offers(ControlKind::Collective) {
        here.push(prose("twist grip"));
    }
    if here.is_empty() {
        return Line::new(Health::Waiting, prose("None in this seat"));
    }
    let o = &summary.outputs;
    let working = prose(match (o.horn, o.nitrous) {
        (true, true) => ", horn and nitrous on",
        (true, false) => ", horn on",
        (false, true) => ", nitrous on",
        (false, false) => "",
    });
    Line::ok(text!(
        Text::join(here, ", "),
        working,
        ", ",
        summary.button_presses.to_string(),
        " presses"
    ))
}

fn flight_controls_line(summary: &VehicleSummary) -> Line {
    let flying = summary.active
        && summary
            .seat
            .is_some_and(|s| s.driver && s.class == VehicleClass::Plane);
    if !flying {
        return Line::new(Health::Waiting, prose("Not flying a plane"));
    }
    let held = |kind| summary.holds.contains(&Some(kind));
    let o = &summary.outputs;
    let stick_held = held(ControlKind::FlightStick) || held(ControlKind::Yoke);
    let stick = prose(match (stick_held, o.flight_stick) {
        (true, _) => "Stick held",
        (false, Some(_)) => "Stick returning",
        (false, None) => "Stick free",
    });
    let rudder = o
        .rudder
        .map_or_else(Text::default, |r| text!(", rudder ", format!("{r:+.2}")));
    let throttle = o.throttle.map_or_else(
        || prose("throttle by trigger"),
        |t| {
            text!(
                "throttle ",
                format!("{t:+.2}"),
                if o.afterburner {
                    prose(" afterburner")
                } else {
                    Text::default()
                }
            )
        },
    );
    Line::ok(text!(
        stick,
        rudder,
        ", ",
        throttle,
        ", ",
        summary.cues.to_string(),
        " cues, ",
        summary.afterburner_lights.to_string(),
        " burns"
    ))
}

#[cfg(test)]
mod tests {
    use jc3vr_vehicles::controllers::vehicles::physical::Outputs;

    use super::*;

    fn flying() -> VehicleSummary {
        VehicleSummary {
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
            mounted: false,
            mounted_holds: [false; 2],
            mounted_grip_events: 0,
        }
    }

    #[test]
    fn the_mounted_gun_names_the_hands_on_it() {
        assert_eq!(
            mounted_gun_line(&flying(), 0).text(),
            "WAITING, NOT ON A MOUNTED GUN"
        );
        let manned = VehicleSummary {
            mounted: true,
            mounted_holds: [true, false],
            mounted_grip_events: 3,
            ..flying()
        };
        assert_eq!(
            mounted_gun_line(&manned, 12).text(),
            "OK, LEFT HAND ON, 3 GRABS AND RELEASES, 12 KICKS"
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
            "OK, FOLLOWING THE HAND, 2 STARTS"
        );
        let idle = PassengerStatus {
            aiming: false,
            ..aiming
        };
        assert_eq!(passenger_aim_line(passenger, idle).health, Health::Waiting);
        assert_eq!(
            passenger_aim_line(None, PassengerStatus::default()).text(),
            "WAITING, NOT A PASSENGER"
        );
    }

    #[test]
    fn the_cockpit_buttons_name_what_the_seat_has() {
        let config = VEHICLES.get().physical.buttons;
        let off = CockpitButtonsTuning {
            horn: false,
            nitrous_button: false,
            twist_nitrous: false,
            ..config
        };
        assert_eq!(cockpit_buttons_line(&off, &flying()).health, Health::Off);
        assert_eq!(
            cockpit_buttons_line(&config, &flying()).text(),
            "WAITING, NONE IN THIS SEAT"
        );
        let car = VehicleSummary {
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
            cockpit_buttons_line(&config, &car).text(),
            "OK, HORN, NITROUS BUTTON, HORN ON, 2 PRESSES"
        );
        let passenger = VehicleSummary {
            seat: Some(VehicleSeat {
                class: VehicleClass::Car,
                driver: false,
            }),
            ..car
        };
        assert_eq!(
            cockpit_buttons_line(&config, &passenger).health,
            Health::Waiting
        );
    }

    #[test]
    fn the_flight_controls_wait_for_a_plane_and_read_out_the_hands() {
        let car = VehicleSummary {
            seat: Some(VehicleSeat {
                class: VehicleClass::Car,
                driver: true,
            }),
            ..flying()
        };
        assert_eq!(flight_controls_line(&car).health, Health::Waiting);
        assert_eq!(
            flight_controls_line(&flying()).text(),
            "OK, STICK HELD, RUDDER +0.25, THROTTLE +1.00 AFTERBURNER, 3 CUES, 1 BURNS"
        );
        let parked = VehicleSummary {
            holds: [None; 2],
            outputs: Outputs::NONE,
            ..flying()
        };
        assert_eq!(
            flight_controls_line(&parked).text(),
            "OK, STICK FREE, THROTTLE BY TRIGGER, 3 CUES, 1 BURNS"
        );
    }
}
