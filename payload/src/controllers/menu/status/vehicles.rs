//! The status page's rows for the vehicles: the plane's flight stick and throttle, and the cockpit
//! buttons.

use smol_str::format_smolstr;

use crate::controllers::{
    menu::{
        read_config,
        status::{Health, Line},
    },
    mode::VehicleClass,
    vehicles::{self, VehicleSummary, config::CockpitButtonsConfig, physical::ControlKind},
};

pub(super) fn flight_controls() -> Line {
    let on = read_config(|c| {
        let p = &c.controllers.vehicles.physical;
        p.flight_stick.enabled || p.throttle.enabled
    });
    flight_controls_line(on, &vehicles::summary())
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
        return Line::new(Health::Waiting, "NOT DRIVING");
    }
    let offers = |kind| summary.offered.contains(&kind);
    let mut here = Vec::new();
    if offers(ControlKind::Wheel) {
        if config.horn {
            here.push("HORN");
        }
        if config.nitrous_button {
            here.push("NITROUS BUTTON");
        }
    }
    if config.twist_nitrous && (offers(ControlKind::Handlebars) || offers(ControlKind::Collective))
    {
        here.push("TWIST GRIP");
    }
    if here.is_empty() {
        return Line::new(Health::Waiting, "NONE IN THIS SEAT");
    }
    let o = &summary.outputs;
    let working = match (o.horn, o.nitrous) {
        (true, true) => ", HORN AND NITROUS ON",
        (true, false) => ", HORN ON",
        (false, true) => ", NITROUS ON",
        (false, false) => "",
    };
    Line::ok(format_smolstr!(
        "{}{working}, {} PRESSES",
        here.join(", "),
        summary.button_presses
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
        return Line::new(Health::Waiting, "NOT FLYING A PLANE");
    }
    let held = |kind| summary.holds.contains(&Some(kind));
    let o = &summary.outputs;
    let stick = match (held(ControlKind::FlightStick), o.flight_stick) {
        (true, _) => "STICK HELD",
        (false, Some(_)) => "STICK RETURNING",
        (false, None) => "STICK FREE",
    };
    let rudder = o
        .rudder
        .map_or_else(Default::default, |r| format_smolstr!(", RUDDER {r:+.2}"));
    let throttle = o.throttle.map_or_else(
        || "THROTTLE BY TRIGGER".into(),
        |t| {
            format_smolstr!(
                "THROTTLE {t:+.2}{}",
                if o.afterburner { " AFTERBURNER" } else { "" }
            )
        },
    );
    Line::ok(format_smolstr!(
        "{stick}{rudder}, {throttle}, {} CUES, {} BURNS",
        summary.cues,
        summary.afterburner_lights
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::{mode::VehicleSeat, vehicles::physical::Outputs};

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
        }
    }

    #[test]
    fn the_cockpit_buttons_name_what_the_seat_has() {
        let config = CockpitButtonsConfig::new();
        let off = CockpitButtonsConfig {
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
