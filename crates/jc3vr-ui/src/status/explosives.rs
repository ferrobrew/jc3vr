//! The status rows for the explosives and magazines the hands handle as objects (the
//! grenade's pin, the live grenades within reach of a catch, the C4 pressed on, and the magazines
//! in the world), and for aiming a long gun down its sights and holding it against walls.

use jc3vr_gameplay::controllers::config::CONTROLLERS_CONFIG;
use jc3vr_weapons::controllers::weapons::{ads, c4, collide, grenade, reload::loose};
use smol_str::{SmolStr, format_smolstr};

use crate::status::{Line, Row};

pub(crate) const LIVE_GRENADES: Row = Row::new("Live grenades", live_grenades);
pub(crate) const C4_BY_HAND: Row = Row::new("C4 by hand", c4_by_hand);
pub(crate) const MAGAZINES: Row = Row::new("Magazines", magazines);
pub(crate) const DOWN_THE_SIGHTS: Row = Row::new("Down the sights", down_the_sights);
pub(crate) const GUN_AGAINST_WALLS: Row = Row::new("Gun against walls", gun_against_walls);

fn gun_against_walls() -> Line {
    let s = collide::status();
    if !s.weapon() {
        return Line::waiting("No long gun measured");
    }
    let touching = s.hands.iter().find(|h| h.touching);
    let turned = touching.map_or_else(SmolStr::default, |h| {
        format_smolstr!(
            ", turned {:.0} deg, pulled back {:.0} cm",
            h.turn_deg,
            h.retract_m * 100.0
        )
    });
    Line::ok(format_smolstr!(
        "{}, {} contacts{}",
        if touching.is_some() {
            SmolStr::new_static("Against a wall")
        } else {
            SmolStr::new_static("Clear")
        },
        s.contacts,
        turned
    ))
}

fn down_the_sights() -> Line {
    let s = ads::status();
    if !s.weapon {
        return Line::waiting("No long gun measured");
    }
    let raised = s.raised.map_or_else(SmolStr::default, |h| {
        format_smolstr!(" in the {} hand", h.label())
    });
    let eye = s.eye_offset_m.map_or_else(SmolStr::default, |m| {
        format_smolstr!(", eye {:.1} cm off", m * 100.0)
    });
    Line::ok(format_smolstr!(
        "{}{}, sighted {}, shouldered {}, welded {}{}",
        if s.scoped {
            SmolStr::new_static("Scoped")
        } else {
            SmolStr::new_static("Iron sights")
        },
        raised,
        s.sighted,
        s.shouldered,
        s.welded,
        eye
    ))
}

fn magazines() -> Line {
    if !CONTROLLERS_CONFIG.query(|c| c.weapons.reload.enabled) {
        return Line::off("");
    }
    let c = loose::counts();
    Line::ok(format_smolstr!(
        "{} in the world, {} picked up, {} seated",
        c.in_world,
        c.picked_up,
        c.seated
    ))
}

fn c4_by_hand() -> Line {
    let status = c4::status();
    Line::ok(format_smolstr!(
        "{} thrown, {} pressed on, {} on objects",
        (status.thrown - status.stuck.min(status.thrown)),
        status.stuck,
        status.stuck_on_objects
    ))
}

fn live_grenades() -> Line {
    let status = grenade::status();
    let c = status.catch;
    let caught = status.caught_hand.map_or_else(SmolStr::default, |h| {
        format_smolstr!(", one in the {} hand", h.label())
    });
    Line::ok(format_smolstr!(
        "{} live, {} in reach, {} caught, {} thrown on{}",
        c.tracked,
        c.in_reach,
        c.caught,
        c.thrown,
        caught
    ))
}
