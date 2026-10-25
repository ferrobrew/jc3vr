//! The status rows for the explosives and magazines the hands handle as objects (the
//! grenade's pin, the live grenades within reach of a catch, the C4 pressed on, and the magazines
//! in the world), and for aiming a long gun down its sights.

use crate::{
    controllers::{
        menu::read_config,
        weapons::{ads, c4, collide, grenade, reload::loose},
    },
    status::{Line, Text, prose, text},
};

pub(super) fn gun_against_walls() -> Line {
    if !read_config(|c| c.controllers.weapons.collide.enabled) {
        return Line::off("");
    }
    let s = collide::status();
    if !s.weapon() {
        return Line::waiting(prose("No long gun measured"));
    }
    let touching = s.hands.iter().find(|h| h.touching);
    let turned = touching.map_or_else(Text::default, |h| {
        text!(
            ", turned ",
            format!("{:.0}", h.turn_deg),
            " deg, pulled back ",
            format!("{:.0}", h.retract_m * 100.0),
            " cm"
        )
    });
    Line::ok(text!(
        if touching.is_some() {
            prose("Against a wall")
        } else {
            prose("Clear")
        },
        ", ",
        s.contacts.to_string(),
        " contacts",
        turned
    ))
}

pub(super) fn down_the_sights() -> Line {
    if !read_config(|c| c.controllers.weapons.ads.enabled) {
        return Line::off("");
    }
    let s = ads::status();
    if !s.weapon {
        return Line::waiting(prose("No long gun measured"));
    }
    let raised = s.raised.map_or_else(Text::default, |h| {
        text!(" in the ", prose(h.label()), " hand")
    });
    let eye = s.eye_offset_m.map_or_else(Text::default, |m| {
        text!(", eye ", format!("{:.1}", m * 100.0), " cm off")
    });
    Line::ok(text!(
        if s.scoped {
            prose("Scoped")
        } else {
            prose("Iron sights")
        },
        raised,
        ", sighted ",
        s.sighted.to_string(),
        ", shouldered ",
        s.shouldered.to_string(),
        ", welded ",
        s.welded.to_string(),
        eye
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
    Line::ok(text!(
        c.in_world.to_string(),
        " in the world, ",
        c.picked_up.to_string(),
        " picked up, ",
        c.seated.to_string(),
        " seated"
    ))
}

pub(super) fn c4_by_hand() -> Line {
    if !read_config(|c| c.controllers.weapons.c4_throw) {
        return Line::off("");
    }
    let status = c4::status();
    Line::ok(text!(
        (status.thrown - status.stuck.min(status.thrown)).to_string(),
        " thrown, ",
        status.stuck.to_string(),
        " pressed on, ",
        status.stuck_on_objects.to_string(),
        " on objects",
        if read_config(|c| c.controllers.weapons.c4.stick.enabled) {
            Text::default()
        } else {
            prose(", pressing off")
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
    let pin = status
        .pin
        .map_or_else(|| prose("No grenade in hand"), |p| prose(p.label()));
    let last = status
        .last_puller
        .map_or_else(Text::default, |p| text!(", last by ", prose(p.label())));
    Line::ok(text!(
        pin,
        ", ",
        status.pins_pulled.to_string(),
        " pulled",
        last
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
    let caught = status.caught_hand.map_or_else(Text::default, |h| {
        text!(", one in the ", prose(h.label()), " hand")
    });
    Line::ok(text!(
        c.tracked.to_string(),
        " live, ",
        c.in_reach.to_string(),
        " in reach, ",
        c.caught.to_string(),
        " caught, ",
        c.thrown.to_string(),
        " thrown on",
        caught
    ))
}
