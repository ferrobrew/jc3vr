//! The status rows for the weapons the hands hold: which weapon each hand has and how it
//! came there, and why the last thing the hands tried with a weapon was refused.

use jc3gi::character::inventory::WeaponSlot;
use jc3vr_body::controllers::hands::grip;
use jc3vr_input::frame::Hand;
use jc3vr_weapons::controllers::{
    loadout::{Delivery, TakeSource, live::Expect, slot_label},
    weapons,
    weapons::{hold, hold::armed, regions},
};
use smol_str::{SmolStr, format_smolstr};

use crate::status::{Line, Row};

pub(crate) const LEFT_HAND: Row = Row::new("Left hand", left_hand);
pub(crate) const RIGHT_HAND: Row = Row::new("Right hand", right_hand);
pub(crate) const LEFT_HAND_ON_GUN: Row = Row::new("Left hand on gun", left_hand_on_gun);
pub(crate) const RIGHT_HAND_ON_GUN: Row = Row::new("Right hand on gun", right_hand_on_gun);
pub(crate) const TAKES: Row = Row::new("Take from hands", takes);
pub(crate) const RACKS: Row = Row::new("Take from racks", racks);
pub(crate) const ARMED: Row = Row::new("Armed beside", armed);
pub(crate) const RIGHT_GRIP: Row = Row::new("Right hand grip", right_grip);

fn left_hand() -> Line {
    hand(Hand::Left)
}

fn right_hand() -> Line {
    hand(Hand::Right)
}

fn left_hand_on_gun() -> Line {
    hand_on_gun(Hand::Left)
}

fn right_hand_on_gun() -> Line {
    hand_on_gun(Hand::Right)
}

/// Where `hand` holds the wielded gun, and what it does there: fires it from the grip, braces it,
/// or carries it by another part, unable to fire.
fn hand_on_gun(hand: Hand) -> Line {
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
            return Line::failed("Gun's shape not read");
        }
        return Line::waiting("Not on a gun");
    };
    let line = Line::ok(format_smolstr!(
        "{}, {}, {} braces, {} passed to the brace",
        on.region.label(),
        on.role.label(),
        status.braces,
        status.let_go
    ));
    match shape {
        Some(shape) => line.with_note(shape),
        None => line,
    }
}

fn armed() -> Line {
    let armed = armed::status();
    let held: Vec<SmolStr> = Hand::BOTH
        .into_iter()
        .filter_map(|hand| {
            let a = armed[hand.index()]?;
            Some(format_smolstr!(
                "{} in {}, {}/{}",
                slot_label(a.slot),
                hand.label(),
                a.magazine.0,
                a.magazine.1
            ))
        })
        .collect();
    if held.is_empty() {
        Line::ok("None")
    } else {
        Line::ok(SmolStr::from(held.join("; ")))
    }
}

fn right_grip() -> Line {
    match grip::right_grip_read() {
        None => Line::waiting("No gun in the right hand yet"),
        Some(read) => Line::ok(format_smolstr!(
            "{}, own {:.0} deg from mirrored",
            if read.mirrored {
                SmolStr::new_static("Mirrored")
            } else {
                SmolStr::new_static("Own")
            },
            read.angle_deg
        )),
    }
}

fn takes() -> Line {
    let take = hold::status().take;
    Line::ok(format_smolstr!(
        "{} in hands near, {} holstered near, {} taken",
        take.held_targets,
        take.holstered_targets,
        take.counts.steals
    ))
}

fn racks() -> Line {
    let take = hold::status().take;
    Line::ok(format_smolstr!(
        "{} near, {} taken, {} copies",
        take.rack_targets,
        take.counts.rack_takes,
        take.counts.rack_copies
    ))
}

fn hand(hand: Hand) -> Line {
    let wielded = weapons::status()
        .wielded
        .map(|w| (w.weapon.wielded.slot, w.weapon.wielded.hands));
    let origin = hold::status().origin;
    let beside = armed::status()[hand.index()].map(|a| (hand, a.slot));
    match in_hand(hand, wielded, origin, beside) {
        Some((slot, how)) => Line::ok(format_smolstr!("{}, {}", slot_label(slot), how.label())),
        None => Line::ok("Empty"),
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
    origin: Option<Expect>,
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
            Some(Expect::Draw) => HowHeld::Docked,
            Some(Expect::Take(_, Delivery::Remote)) => HowHeld::Snatched,
            Some(Expect::Take(TakeSource::World, Delivery::Local)) => HowHeld::PickedUp,
            Some(Expect::Take(TakeSource::Character, Delivery::Local)) => HowHeld::Stolen,
            Some(Expect::Take(
                TakeSource::Rack | TakeSource::RackCopy | TakeSource::Locker,
                Delivery::Local,
            )) => HowHeld::Rack,
            Some(Expect::Take(
                TakeSource::Give | TakeSource::Pouch | TakeSource::Catch,
                Delivery::Local,
            ))
            | None => HowHeld::Game,
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
            in_hand(
                Hand::Left,
                left_rifle,
                Some(Expect::Take(TakeSource::Character, Delivery::Local)),
                None
            ),
            Some((RIFLE, HowHeld::Stolen))
        );
        assert_eq!(
            in_hand(
                Hand::Right,
                left_rifle,
                Some(Expect::Take(TakeSource::Character, Delivery::Local)),
                None
            ),
            None
        );
        assert_eq!(
            in_hand(Hand::Left, left_rifle, None, None),
            Some((RIFLE, HowHeld::Game))
        );
        assert_eq!(
            in_hand(Hand::Left, left_rifle, Some(Expect::Draw), None),
            Some((RIFLE, HowHeld::Docked))
        );
    }

    #[test]
    fn a_snatched_weapon_reads_snatched_whatever_its_source() {
        let left_rifle = Some((RIFLE, [true, false]));
        for source in [TakeSource::World, TakeSource::Character, TakeSource::Rack] {
            assert_eq!(
                in_hand(
                    Hand::Left,
                    left_rifle,
                    Some(Expect::Take(source, Delivery::Remote)),
                    None
                ),
                Some((RIFLE, HowHeld::Snatched))
            );
        }
    }

    #[test]
    fn a_weapon_beside_the_wielded_one_names_its_own_hand() {
        let left_rifle = Some((RIFLE, [true, false]));
        let beside = Some((Hand::Right, SPECIAL));
        assert_eq!(
            in_hand(
                Hand::Right,
                left_rifle,
                Some(Expect::Take(TakeSource::World, Delivery::Local)),
                beside
            ),
            Some((SPECIAL, HowHeld::Beside))
        );
        assert_eq!(
            in_hand(
                Hand::Left,
                left_rifle,
                Some(Expect::Take(TakeSource::World, Delivery::Local)),
                beside
            ),
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
