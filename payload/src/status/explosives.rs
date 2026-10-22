//! The status rows for the explosives and magazines the hands handle as objects (the
//! grenade's pin, the live grenades within reach of a catch, the C4 pressed on, and the magazines
//! in the world), and for aiming a long gun down its sights.

use smol_str::format_smolstr;

use crate::{
    controllers::{
        menu::read_config,
        weapons::{ads, c4, collide, grenade, reload::loose},
    },
    status::Line,
};

pub(super) fn gun_against_walls() -> Line {
    if !read_config(|c| c.controllers.weapons.collide.enabled) {
        return Line::off("");
    }
    let s = collide::status();
    if !s.weapon {
        return Line::waiting("NO LONG GUN MEASURED");
    }
    Line::ok(format_smolstr!(
        "{}, {} CONTACTS{}",
        if s.touching {
            "AGAINST A WALL"
        } else {
            "CLEAR"
        },
        s.contacts,
        if s.touching {
            format!(
                ", TURNED {:.0} DEG, PULLED BACK {:.0} CM",
                s.turn_deg,
                s.retract_m * 100.0
            )
        } else {
            String::new()
        }
    ))
}

pub(super) fn down_the_sights() -> Line {
    if !read_config(|c| c.controllers.weapons.ads.enabled) {
        return Line::off("");
    }
    let s = ads::status();
    if !s.weapon {
        return Line::waiting("NO LONG GUN MEASURED");
    }
    Line::ok(format_smolstr!(
        "{}, SIGHTED {}, SHOULDERED {}, WELDED {}{}",
        if s.scoped { "SCOPED" } else { "IRON SIGHTS" },
        s.sighted,
        s.shouldered,
        s.welded,
        s.eye_offset_m
            .map_or(String::new(), |m| format!(", EYE {:.1} CM OFF", m * 100.0))
    ))
}

pub(super) fn magazines() -> Line {
    if !read_config(|c| {
        let reload = c.controllers.weapons.reload;
        reload.enabled && reload.loose.enabled
    }) {
        return Line::off("");
    }
    let c = loose::counts();
    Line::ok(format_smolstr!(
        "{} IN THE WORLD, {} PICKED UP, {} SEATED",
        c.in_world,
        c.picked_up,
        c.seated
    ))
}

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
