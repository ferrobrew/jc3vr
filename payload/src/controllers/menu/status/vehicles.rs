//! The status page's rows for the vehicles: the plane's flight stick and throttle.

use smol_str::format_smolstr;

use crate::controllers::{
    menu::{
        read_config,
        status::{Health, Line},
    },
    mode::VehicleClass,
    vehicles::{self, VehicleSummary, physical::ControlKind},
};

pub(super) fn flight_controls() -> Line {
    let on = read_config(|c| {
        let p = &c.controllers.vehicles.physical;
        p.flight_stick.enabled || p.throttle.enabled
    });
    flight_controls_line(on, &vehicles::summary())
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
        }
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
