//! The status rows for the weapons the hands hold: which weapon each hand has and how it
//! came there, and why the last thing the hands tried with a weapon was refused.

use jc3gi::character::inventory::WeaponSlot;

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
    status::{Line, Text, prose, text},
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
        return Line::waiting(prose("No weapon reloaded by hand"));
    }
    let Some(weapon) = status.weapons[hand.index()] else {
        return Line::ok(prose("No gun"));
    };
    let Some(travel) = weapon.action else {
        return Line::ok(prose("Nothing to work"));
    };
    let kind = weapon.plan.action;
    let chamber = prose(match weapon.chamber {
        Chamber::Empty => "chamber empty",
        Chamber::Chambered => "chambered",
    });
    let place = if travel.locked_back {
        prose("locked back")
    } else if kind == ActionKind::Bolt && travel.lift > 0.0 {
        text!(
            "up ",
            format!("{:.0}%", travel.lift * 100.0),
            ", back ",
            format!("{:.0}%", travel.position * 100.0)
        )
    } else if travel.position > 0.0 {
        text!("back ", format!("{:.0}%", travel.position * 100.0))
    } else {
        prose("home")
    };
    Line::ok(text!(
        prose(kind.label()),
        ": ",
        place,
        if travel.grip.is_some() {
            prose(", held")
        } else {
            Text::default()
        },
        ", ",
        chamber,
        ", ",
        status.counts.racks.to_string(),
        " worked"
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
            return Line::failed(prose("Gun's shape not read"));
        }
        return Line::waiting(prose("Not on a gun"));
    };
    let role = match on.role {
        HandRole::Carrying => prose("carrying, cannot fire"),
        role => prose(role.label()),
    };
    let line = Line::ok(text!(
        prose(on.region.label()),
        ", ",
        role,
        ", ",
        status.carries.to_string(),
        " carries, ",
        status.braces.to_string(),
        " braces, ",
        status.grips_taken.to_string(),
        " grips taken"
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
    let held: Vec<Text> = Hand::BOTH
        .into_iter()
        .filter_map(|hand| {
            let a = armed[hand.index()]?;
            Some(text!(
                prose(slot_label(a.slot)),
                " in ",
                prose(hand.label()),
                ", ",
                format!("{}/{}", a.magazine.0, a.magazine.1)
            ))
        })
        .collect();
    if held.is_empty() {
        Line::ok(prose("None"))
    } else {
        Line::ok(Text::join(held, "; "))
    }
}

pub(super) fn docks() -> Line {
    if !read_config(|c| {
        let holsters = c.controllers.weapons.holsters;
        holsters.enabled && holsters.follow_docks
    }) {
        return Line::off("");
    }
    Line::ok(text!(
        weapons::status().docks_followed.to_string(),
        " on their weapons"
    ))
}

pub(super) fn right_grip() -> Line {
    match grip::right_grip_read() {
        None => Line::waiting(prose("No gun in the right hand yet")),
        Some(read) => Line::ok(text!(
            if read.mirrored {
                prose("Mirrored")
            } else {
                prose("Own")
            },
            ", own ",
            format!("{:.0}", read.angle_deg),
            " deg from mirrored"
        )),
    }
}

pub(super) fn last_refusal() -> Line {
    match hold::last_refusal() {
        Some(refusal) => Line::ok(Text::pair(refusal.to_uppercase(), refusal)),
        None => Line::ok(prose("None")),
    }
}

pub(super) fn takes() -> Line {
    if !read_config(|c| c.controllers.weapons.hold.steal) {
        return Line::off("");
    }
    let take = hold::status().take;
    Line::ok(text!(
        take.held_targets.to_string(),
        " in hands near, ",
        take.steals.to_string(),
        " taken"
    ))
}

pub(super) fn racks() -> Line {
    if !read_config(|c| c.controllers.weapons.hold.racks) {
        return Line::off("");
    }
    let take = hold::status().take;
    Line::ok(text!(
        take.rack_targets.to_string(),
        " near, ",
        take.rack_takes.to_string(),
        " taken, ",
        take.rack_copies.to_string(),
        " copies"
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
        Some((slot, how)) => Line::ok(text!(prose(slot_label(slot)), ", ", prose(how.label()))),
        None => Line::ok(prose("Empty")),
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
            HowHeld::Docked => "docked",
            HowHeld::PickedUp => "picked up",
            HowHeld::Stolen => "stolen",
            HowHeld::Rack => "rack",
            HowHeld::Snatched => "snatched",
            HowHeld::Game => "game's draw",
            HowHeld::Beside => "beside the wielded",
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
