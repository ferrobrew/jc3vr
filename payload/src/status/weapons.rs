//! The status rows for the weapons the hands hold: which weapon each hand has and how it
//! came there, and why the last thing the hands tried with a weapon was refused.

use jc3gi::character::inventory::WeaponSlot;
use smol_str::{SmolStr, format_smolstr};

use crate::{
    controllers::{
        frame::Hand,
        hands::grip,
        loadout::slot_label,
        menu::read_config,
        weapons::{
            self,
            hold::{self, Cause, armed},
            regions::{self, HandRole},
            reload::{
                self,
                chamber::{ActionKind, Chamber},
            },
        },
    },
    status::Line,
};

pub(super) fn left_hand() -> Line {
    hand(Hand::Left)
}

pub(super) fn right_hand() -> Line {
    hand(Hand::Right)
}

pub(super) fn left_action() -> Line {
    action(Hand::Left)
}

pub(super) fn right_action() -> Line {
    action(Hand::Right)
}

/// The action of `hand`'s gun on its travel: where its part is, whether a hand holds it, and
/// whether its slide is locked back, with the chamber it loaded.
fn action(hand: Hand) -> Line {
    if !read_config(|c| {
        let reload = c.controllers.weapons.reload;
        reload.enabled && reload.action.physical
    }) {
        return Line::off("");
    }
    let status = reload::status();
    if status.weapons.iter().all(Option::is_none) {
        return Line::waiting("NO WEAPON RELOADED BY HAND");
    }
    let Some(weapon) = status.weapons[hand.index()] else {
        return Line::ok("NO GUN");
    };
    let Some(travel) = weapon.action else {
        return Line::ok("NOTHING TO WORK");
    };
    let kind = weapon.plan.action;
    let chamber = match weapon.chamber {
        Chamber::Empty => "CHAMBER EMPTY",
        Chamber::Chambered => "CHAMBERED",
    };
    let place = if travel.locked_back {
        "LOCKED BACK".into()
    } else if kind == ActionKind::Bolt && travel.lift > 0.0 {
        format_smolstr!(
            "UP {:.0}%, BACK {:.0}%",
            travel.lift * 100.0,
            travel.position * 100.0
        )
    } else if travel.position > 0.0 {
        format_smolstr!("BACK {:.0}%", travel.position * 100.0)
    } else {
        SmolStr::new("HOME")
    };
    Line::ok(format_smolstr!(
        "{}: {place}{}, {chamber}, {} WORKED",
        kind.label().to_uppercase(),
        if travel.grip.is_some() { ", HELD" } else { "" },
        status.counts.racks
    ))
}

pub(super) fn left_hand_on_gun() -> Line {
    hand_on_gun(Hand::Left)
}

pub(super) fn right_hand_on_gun() -> Line {
    hand_on_gun(Hand::Right)
}

/// Where `hand` holds the wielded gun, and what it does there: fires it from the grip, braces it,
/// or carries it by another part, unable to fire.
fn hand_on_gun(hand: Hand) -> Line {
    if !read_config(|c| c.controllers.weapons.regions.enabled) {
        return Line::off("");
    }
    let status = regions::status();
    let shape = status.shape.map(|s| {
        format!(
            "the gun is {:.0} cm long{}{}{}",
            s.length_m * 100.0,
            if s.foregrip { ", with a foregrip" } else { "" },
            if s.magazine { ", a magazine well" } else { "" },
            if s.action { ", an action" } else { "" },
        )
    });
    let Some(on) = status.hands[hand.index()] else {
        if status.unread {
            return Line::failed("GUN'S SHAPE NOT READ");
        }
        return Line::waiting("NOT ON A GUN");
    };
    let role = match on.role {
        HandRole::Carrying => "CARRYING, CANNOT FIRE".to_owned(),
        role => role.label().to_uppercase(),
    };
    let line = Line::ok(format_smolstr!(
        "{}, {role}, {} CARRIES, {} BRACES, {} GRIPS TAKEN",
        on.region.label().to_uppercase(),
        status.carries,
        status.braces,
        status.grips_taken
    ));
    match shape {
        Some(shape) => line.with_note(shape),
        None => line,
    }
}

pub(super) fn armed() -> Line {
    if !read_config(|c| {
        let hold = c.controllers.weapons.hold;
        hold.either_hand && hold.second_weapon
    }) {
        return Line::off("");
    }
    let armed = armed::status();
    let held: Vec<String> = Hand::BOTH
        .into_iter()
        .filter_map(|hand| {
            let a = armed[hand.index()]?;
            Some(format!(
                "{} IN {}, {}/{}",
                slot_label(a.slot).to_uppercase(),
                hand.label().to_uppercase(),
                a.magazine.0,
                a.magazine.1
            ))
        })
        .collect();
    if held.is_empty() {
        Line::ok("NONE")
    } else {
        Line::ok(SmolStr::new(held.join("; ")))
    }
}

pub(super) fn docks() -> Line {
    if !read_config(|c| {
        let holsters = c.controllers.weapons.holsters;
        holsters.enabled && holsters.follow_docks
    }) {
        return Line::off("");
    }
    Line::ok(format_smolstr!(
        "{} ON THEIR WEAPONS",
        weapons::status().docks_followed
    ))
}

pub(super) fn right_grip() -> Line {
    match grip::right_grip_read() {
        None => Line::waiting("NO GUN IN THE RIGHT HAND YET"),
        Some(read) => Line::ok(format_smolstr!(
            "{}, OWN {:.0} DEG FROM MIRRORED",
            if read.mirrored { "MIRRORED" } else { "OWN" },
            read.angle_deg
        )),
    }
}

pub(super) fn last_refusal() -> Line {
    match hold::last_refusal() {
        Some(refusal) => Line::ok(SmolStr::new(refusal.to_uppercase())),
        None => Line::ok("NONE"),
    }
}

pub(super) fn takes() -> Line {
    if !read_config(|c| c.controllers.weapons.hold.steal) {
        return Line::off("");
    }
    let take = hold::status().take;
    Line::ok(format_smolstr!(
        "{} IN HANDS NEAR, {} TAKEN",
        take.held_targets,
        take.steals
    ))
}

pub(super) fn racks() -> Line {
    if !read_config(|c| c.controllers.weapons.hold.racks) {
        return Line::off("");
    }
    let take = hold::status().take;
    Line::ok(format_smolstr!(
        "{} NEAR, {} TAKEN, {} COPIES",
        take.rack_targets,
        take.rack_takes,
        take.rack_copies
    ))
}

fn hand(hand: Hand) -> Line {
    if !read_config(|c| c.controllers.weapons.hold.either_hand) {
        return Line::off("");
    }
    let wielded = weapons::status()
        .wielded
        .map(|w| (w.weapon.wielded.slot, w.weapon.wielded.hands));
    let origin = hold::status().origin;
    let beside = armed::status()[hand.index()].map(|a| (hand, a.slot));
    match in_hand(hand, wielded, origin, beside) {
        Some((slot, how)) => Line::ok(format_smolstr!(
            "{}, {}",
            slot_label(slot).to_uppercase(),
            how.label()
        )),
        None => Line::ok("EMPTY"),
    }
}

/// How the weapon in a hand came there.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum HowHeld {
    /// Drawn from its place on the body by a grab.
    Docked,
    PickedUp,
    Stolen,
    /// Taken off a rack, or a rack's copy.
    Rack,
    /// Pulled into the hand by the grapple's snatch.
    Snatched,
    /// Drawn by the game on its own (the weapon wheel, a switch, an automatic draw).
    Game,
    /// Armed in the hand beside the wielded weapon.
    Beside,
}

impl HowHeld {
    const fn label(self) -> &'static str {
        match self {
            HowHeld::Docked => "DOCKED",
            HowHeld::PickedUp => "PICKED UP",
            HowHeld::Stolen => "STOLEN",
            HowHeld::Rack => "RACK",
            HowHeld::Snatched => "SNATCHED",
            HowHeld::Game => "GAME'S DRAW",
            HowHeld::Beside => "BESIDE THE WIELDED",
        }
    }
}

/// The weapon `hand` holds and how it came there, from the wielded weapon's slot and the hands its
/// guns ride (`wielded`), what brought it into the hands (`origin`), and the hand and slot of a
/// weapon armed beside it (`beside`).
fn in_hand(
    hand: Hand,
    wielded: Option<(WeaponSlot, [bool; 2])>,
    origin: Option<Cause>,
    beside: Option<(Hand, WeaponSlot)>,
) -> Option<(WeaponSlot, HowHeld)> {
    if let Some((beside_hand, slot)) = beside
        && beside_hand == hand
    {
        return Some((slot, HowHeld::Beside));
    }
    let (slot, hands) = wielded?;
    hands[hand.index()].then_some((
        slot,
        match origin {
            Some(Cause::Draw) => HowHeld::Docked,
            Some(Cause::Pickup) => HowHeld::PickedUp,
            Some(Cause::Steal) => HowHeld::Stolen,
            Some(Cause::Rack) => HowHeld::Rack,
            Some(Cause::Snatch) => HowHeld::Snatched,
            None => HowHeld::Game,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RIFLE: WeaponSlot = WeaponSlot::E_SLOT_TWO_HANDED;
    const SPECIAL: WeaponSlot = WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL;

    #[test]
    fn each_hand_names_its_weapon_and_how_it_came() {
        let left_rifle = Some((RIFLE, [true, false]));
        assert_eq!(
            in_hand(Hand::Left, left_rifle, Some(Cause::Steal), None),
            Some((RIFLE, HowHeld::Stolen))
        );
        assert_eq!(
            in_hand(Hand::Right, left_rifle, Some(Cause::Steal), None),
            None
        );
        assert_eq!(
            in_hand(Hand::Left, left_rifle, None, None),
            Some((RIFLE, HowHeld::Game))
        );
        assert_eq!(
            in_hand(Hand::Left, left_rifle, Some(Cause::Draw), None),
            Some((RIFLE, HowHeld::Docked))
        );
    }

    #[test]
    fn a_weapon_beside_the_wielded_one_names_its_own_hand() {
        let left_rifle = Some((RIFLE, [true, false]));
        let beside = Some((Hand::Right, SPECIAL));
        assert_eq!(
            in_hand(Hand::Right, left_rifle, Some(Cause::Pickup), beside),
            Some((SPECIAL, HowHeld::Beside))
        );
        assert_eq!(
            in_hand(Hand::Left, left_rifle, Some(Cause::Pickup), beside),
            Some((RIFLE, HowHeld::PickedUp))
        );
    }

    #[test]
    fn pistols_in_both_hands_fill_both() {
        let pair = Some((WeaponSlot::E_SLOT_DUAL_WIELD, [true; 2]));
        for hand in Hand::BOTH {
            assert!(in_hand(hand, pair, None, None).is_some());
        }
    }
}
