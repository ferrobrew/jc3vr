//! Each hand's [`HandContents`], sampled from the weapon handling's sources: the loadout table's
//! item, the wielded weapon's guns and the armed ones, the grenades, the C4 throw and the
//! detonator, the reload's round, the brace, and the fire input.

use jc3vr_input::frame::Hand;

use crate::controllers::{
    loadout::{contents::HandContents, live},
    weapons::{
        c4,
        engine::HeldWeapon,
        foregrip, grenade,
        hold::{armed, tick},
        reload,
        state::last_wielded,
    },
};

/// What each hand holds at the call, by [`Hand::index`], with the wielded weapon as the last input
/// tick began. Unlike the record the input tick publishes at its end
/// ([`hands`](crate::controllers::loadout::contents::hands)), it follows the beacon and the C4 throw
/// as their weapons' updates move them between ticks, and keeps the gun hands through a
/// stand-down. The brace is the engaged foregrip hold only, not one fading out after a release.
///
/// The sample takes the weapon handling's locks one after another (the reload's, the C4's, and the
/// foregrip's among them), so no caller may hold one of them.
pub fn now() -> [HandContents; 2] {
    sample(last_wielded().0.as_ref())
}

/// What each hand holds now, by [`Hand::index`], with `wielded` the wielded weapon as the caller
/// read it.
pub(crate) fn sample(wielded: Option<&HeldWeapon>) -> [HandContents; 2] {
    let detonator = c4::detonator_hand();
    let round = reload::holding_hand();
    let brace = foregrip::status().hold.filter(|hold| hold.engaged);
    let fire = tick::fire();
    let parts = Hand::BOTH.map(|hand| HandContents {
        item: live::held_kind(hand),
        wielded: false,
        armed: armed::in_hand(hand),
        grenade: grenade::in_hand(hand),
        charge: c4::charge_of(hand),
        detonator: detonator == Some(hand),
        round: round == Some(hand),
        braces: brace
            .filter(|hold| hold.holder.other() == hand)
            .map(|hold| hold.weapon),
        fired: fire[hand.index()],
    });
    with_wielded(parts, wielded)
}

/// `parts` with the hands holding a gun of `wielded` marked. The C4 the game's planting mode
/// wields marks no hand.
fn with_wielded(parts: [HandContents; 2], wielded: Option<&HeldWeapon>) -> [HandContents; 2] {
    let guns = wielded.map_or([false; 2], |w| w.wielded.gun_hands());
    Hand::BOTH.map(|hand| HandContents {
        wielded: guns[hand.index()],
        ..parts[hand.index()]
    })
}

#[cfg(test)]
mod tests {
    use jc3gi::character::inventory::WeaponSlot;
    use jc3vr_body::controllers::held::guns::HeftClass;

    use super::*;
    use crate::controllers::{loadout::contents::HeldCharge, weapons::engine::Wielded};

    fn wielding(slot: WeaponSlot) -> HeldWeapon {
        HeldWeapon {
            address: 1,
            wielded: Wielded {
                slot,
                hands: [true, false],
            },
            holder: Some(Hand::Left),
            two_handed: false,
            class: HeftClass::of(0, false),
        }
    }

    #[test]
    fn a_planted_charge_frees_its_hand_and_only_the_throw_or_the_detonator_fills_it() {
        let planting = wielding(WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE);
        let empty = [HandContents::default(); 2];
        assert_eq!(
            with_wielded(empty, Some(&planting)).map(|hand| hand.free()),
            [true; 2]
        );
        let detonator = [
            HandContents {
                detonator: true,
                ..Default::default()
            },
            HandContents::default(),
        ];
        assert_eq!(
            with_wielded(detonator, Some(&planting)).map(|hand| hand.free()),
            [false, true]
        );
        let throw = [
            HandContents {
                charge: Some(HeldCharge::Asked),
                ..Default::default()
            },
            HandContents::default(),
        ];
        assert_eq!(
            with_wielded(throw, Some(&planting)).map(|hand| hand.free()),
            [false, true]
        );
        let pistol = wielding(WeaponSlot::E_SLOT_DUAL_WIELD);
        assert_eq!(
            with_wielded(empty, Some(&pistol)).map(|hand| hand.free()),
            [false, true]
        );
    }
}
