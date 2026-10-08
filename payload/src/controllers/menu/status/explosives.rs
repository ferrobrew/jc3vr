//! The status page's rows for the explosives and magazines the hands handle as objects: the
//! grenade's pin and the live grenades within reach of a catch.

use smol_str::format_smolstr;

use crate::controllers::{
    menu::{read_config, status::Line},
    weapons::{c4, grenade},
};

pub(super) fn c4_by_hand() -> Line {
    if !read_config(|c| c.controllers.weapons.c4_throw) {
        return Line::off("");
    }
    let status = c4::status();
    Line::ok(format_smolstr!(
        "{} THROWN, {} PRESSED ON, {} ON OBJECTS{}",
        status.thrown - status.stuck.min(status.thrown),
        status.stuck,
        status.stuck_on_objects,
        if read_config(|c| c.controllers.weapons.c4.stick.enabled) {
            ""
        } else {
            ", PRESSING OFF"
        }
    ))
}

pub(super) fn grenade_pin() -> Line {
    if !read_config(|c| {
        c.controllers.weapons.grenade_throw && c.controllers.weapons.grenade.pin.enabled
    }) {
        return Line::off("");
    }
    let status = grenade::status();
    Line::ok(format_smolstr!(
        "{}, {} PULLED{}",
        status
            .pin
            .map_or("NO GRENADE IN HAND", |p| p.label())
            .to_uppercase(),
        status.pins_pulled,
        status.last_puller.map_or(String::new(), |p| format!(
            ", LAST BY {}",
            p.label().to_uppercase()
        ))
    ))
}

pub(super) fn live_grenades() -> Line {
    if !read_config(|c| {
        c.controllers.weapons.grenade_throw && c.controllers.weapons.grenade.catch.enabled
    }) {
        return Line::off("");
    }
    let status = grenade::status();
    let c = status.catch;
    Line::ok(format_smolstr!(
        "{} LIVE, {} IN REACH, {} CAUGHT, {} THROWN ON{}",
        c.tracked,
        c.in_reach,
        c.caught,
        c.thrown,
        status.caught_hand.map_or(String::new(), |h| format!(
            ", ONE IN THE {} HAND",
            h.label().to_uppercase()
        ))
    ))
}
