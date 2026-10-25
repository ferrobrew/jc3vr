//! The status rows for the vehicles: the plane's flight stick and throttle, the cockpit
//! buttons, the passenger's aim, and the mounted gun.

use crate::{
    controllers::{
        menu::read_config,
        mode::{VehicleClass, VehicleSeat},
        vehicles::{
            self, VehicleSummary,
            config::{CockpitButtonsConfig, MountedConfig},
            mounted_recoil,
            passenger::{self, PassengerStatus},
            physical::ControlKind,
        },
    },
    status::{Health, Line, Text, prose, text},
};

pub(super) fn flight_controls() -> Line {
    let on = read_config(|c| {
        let p = &c.controllers.vehicles.physical;
        p.flight_stick.enabled || p.throttle.enabled
    });
    flight_controls_line(on, &vehicles::summary())
}

pub(super) fn mounted_gun() -> Line {
    let config = read_config(|c| c.controllers.vehicles.mounted);
    mounted_gun_line(&config, &vehicles::summary(), mounted_recoil::shots())
}

fn mounted_gun_line(config: &MountedConfig, summary: &VehicleSummary, shots: u64) -> Line {
    if !(config.aim_by_hands || config.anchor_hands) {
        return Line::off("");
    }
    if !summary.mounted {
        return Line::new(Health::Waiting, prose("Not on a mounted gun"));
    }
    let hands = prose(match (config.grip_handles, summary.mounted_holds) {
        (false, _) => "Hands always on",
        (true, [true, true]) => "Both hands on",
        (true, [true, false]) => "Left hand on",
        (true, [false, true]) => "Right hand on",
        (true, [false, false]) => "Hands off, gun held",
    });
    let recoil = if config.recoil {
        text!(", ", shots.to_string(), " kicks")
    } else {
        Text::default()
    };
    Line::ok(text!(
        hands,
        ", ",
        summary.mounted_grip_events.to_string(),
        " grabs and releases",
        recoil
    ))
}

pub(super) fn passenger_aim() -> Line {
    let on = read_config(|c| c.controllers.vehicles.passenger_body_by_hand);
    let seat = vehicles::summary().seat;
    passenger_aim_line(on, seat, passenger::status())
}

fn passenger_aim_line(on: bool, seat: Option<VehicleSeat>, status: PassengerStatus) -> Line {
    if !on {
        return Line::off("");
    }
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

pub(super) fn cockpit_buttons() -> Line {
    let config = read_config(|c| c.controllers.vehicles.physical.buttons);
    cockpit_buttons_line(&config, &vehicles::summary())
}

fn cockpit_buttons_line(config: &CockpitButtonsConfig, summary: &VehicleSummary) -> Line {
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
    if config.twist_throttle && offers(ControlKind::Handlebars) {
        here.push(prose("throttle grip"));
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

fn flight_controls_line(on: bool, summary: &VehicleSummary) -> Line {
    if !on {
        return Line::off("");
    }
    let flying = summary.active
        && summary
            .seat
            .is_some_and(|s| s.driver && s.class == VehicleClass::Plane);
    if !flying {
        return Line::new(Health::Waiting, prose("Not flying a plane"));
    }
    let held = |kind| summary.holds.contains(&Some(kind));
    let o = &summary.outputs;
    let stick = prose(match (held(ControlKind::FlightStick), o.flight_stick) {
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
    use super::*;
    use crate::controllers::vehicles::physical::Outputs;

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
        let config = MountedConfig::new();
        assert_eq!(
            mounted_gun_line(&config, &flying(), 0).text(),
            "WAITING, NOT ON A MOUNTED GUN"
        );
        let manned = VehicleSummary {
            mounted: true,
            mounted_holds: [true, false],
            mounted_grip_events: 3,
            ..flying()
        };
        assert_eq!(
            mounted_gun_line(&config, &manned, 12).text(),
            "OK, LEFT HAND ON, 3 GRABS AND RELEASES, 12 KICKS"
        );
        let off = MountedConfig {
            grip_handles: false,
            recoil: false,
            ..config
        };
        assert_eq!(
            mounted_gun_line(&off, &manned, 12).text(),
            "OK, HANDS ALWAYS ON, 3 GRABS AND RELEASES"
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
            passenger_aim_line(false, passenger, aiming).health,
            Health::Off
        );
        assert_eq!(
            passenger_aim_line(true, passenger, aiming).text(),
            "OK, FOLLOWING THE HAND, 2 STARTS"
        );
        let idle = PassengerStatus {
            aiming: false,
            ..aiming
        };
        assert_eq!(
            passenger_aim_line(true, passenger, idle).health,
            Health::Waiting
        );
        assert_eq!(
            passenger_aim_line(true, None, PassengerStatus::default()).text(),
            "WAITING, NOT A PASSENGER"
        );
    }

    #[test]
    fn the_cockpit_buttons_name_what_the_seat_has() {
        let config = CockpitButtonsConfig::new();
        let off = CockpitButtonsConfig {
            horn: false,
            nitrous_button: false,
            twist_nitrous: false,
            twist_throttle: false,
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
        assert_eq!(flight_controls_line(false, &flying()).health, Health::Off);
        let car = VehicleSummary {
            seat: Some(VehicleSeat {
                class: VehicleClass::Car,
                driver: true,
            }),
            ..flying()
        };
        assert_eq!(flight_controls_line(true, &car).health, Health::Waiting);
        assert_eq!(
            flight_controls_line(true, &flying()).text(),
            "OK, STICK HELD, RUDDER +0.25, THROTTLE +1.00 AFTERBURNER, 3 CUES, 1 BURNS"
        );
        let parked = VehicleSummary {
            holds: [None; 2],
            outputs: Outputs::NONE,
            ..flying()
        };
        assert_eq!(
            flight_controls_line(true, &parked).text(),
            "OK, STICK FREE, THROTTLE BY TRIGGER, 3 CUES, 1 BURNS"
        );
    }
}
