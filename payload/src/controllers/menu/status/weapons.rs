//! The status page's rows for the weapons the hands hold: which weapon each hand has and how it
//! came there, and why the last thing the hands tried with a weapon was refused.

use jc3gi::character::inventory::WeaponSlot;
use smol_str::{SmolStr, format_smolstr};

use crate::controllers::{
    frame::Hand,
    hands::grip,
    menu::{read_config, status::Line},
    weapons::{
        self,
        hold::{self, holder::Cause, second},
        holster::slot_label,
        reload::{self, focus::ReloadTarget},
    },
};

pub(super) fn left_hand() -> Line {
    hand(Hand::Left)
}

pub(super) fn right_hand() -> Line {
    hand(Hand::Right)
}

pub(super) fn second_weapon() -> Line {
    if !read_config(|c| {
        let hold = c.controllers.weapons.hold;
        hold.either_hand && hold.second_weapon
    }) {
        return Line::off("");
    }
    let second = second::status();
    let focus = reload::status().focus;
    match second.held {
        None => Line::ok("NONE"),
        Some((hand, slot)) => Line::ok(format_smolstr!(
            "{} IN {}, {}, RELOAD ON {}",
            slot_label(slot).to_uppercase(),
            hand.label().to_uppercase(),
            second.magazine.map_or(
                "NO MAGAZINE".into(),
                |(magazine, capacity)| format_smolstr!("{magazine}/{capacity}")
            ),
            match focus {
                ReloadTarget::Primary => "THE WIELDED",
                ReloadTarget::Second => "IT",
            }
        )),
    }
}

pub(super) fn left_grip() -> Line {
    if !read_config(|c| c.controllers.weapons.hold.either_hand) {
        return Line::off("");
    }
    match grip::left_grip_read() {
        None => Line::waiting("NO GUN IN THE LEFT HAND YET"),
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
    let second = second::status().held;
    match in_hand(hand, wielded, origin, second) {
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
    /// Drawn by the game on its own (the weapon wheel, a switch, an automatic draw).
    Game,
    /// Held as the second weapon, taken from its place on the body.
    Second,
}

impl HowHeld {
    const fn label(self) -> &'static str {
        match self {
            HowHeld::Docked => "DOCKED",
            HowHeld::PickedUp => "PICKED UP",
            HowHeld::Stolen => "STOLEN",
            HowHeld::Rack => "RACK",
            HowHeld::Game => "GAME'S DRAW",
            HowHeld::Second => "SECOND, DOCKED",
        }
    }
}

/// The weapon `hand` holds and how it came there, from the wielded weapon's slot and the hands its
/// guns ride (`wielded`), what brought it into the hands (`origin`), and the second weapon's hand
/// and slot.
fn in_hand(
    hand: Hand,
    wielded: Option<(WeaponSlot, [bool; 2])>,
    origin: Option<Cause>,
    second: Option<(Hand, WeaponSlot)>,
) -> Option<(WeaponSlot, HowHeld)> {
    if let Some((second_hand, slot)) = second
        && second_hand == hand
    {
        return Some((slot, HowHeld::Second));
    }
    let (slot, hands) = wielded?;
    hands[hand.index()].then_some((
        slot,
        match origin {
            Some(Cause::Draw) => HowHeld::Docked,
            Some(Cause::Pickup) => HowHeld::PickedUp,
            Some(Cause::Steal) => HowHeld::Stolen,
            Some(Cause::Rack) => HowHeld::Rack,
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
    fn the_second_weapon_names_its_own_hand() {
        let left_rifle = Some((RIFLE, [true, false]));
        let second = Some((Hand::Right, SPECIAL));
        assert_eq!(
            in_hand(Hand::Right, left_rifle, Some(Cause::Pickup), second),
            Some((SPECIAL, HowHeld::Second))
        );
        assert_eq!(
            in_hand(Hand::Left, left_rifle, Some(Cause::Pickup), second),
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
