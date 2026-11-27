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
    },
};

/// What each hand holds now, by [`Hand::index`], with `wielded` the wielded weapon as the caller
/// read it.
pub(crate) fn sample(wielded: Option<&HeldWeapon>) -> [HandContents; 2] {
    let guns = wielded.map_or([false; 2], |w| w.wielded.gun_hands());
    let detonator = c4::detonator_hand();
    let round = reload::holding_hand();
    let brace = foregrip::status().hold.filter(|hold| hold.engaged);
    let fire = tick::fire();
    Hand::BOTH.map(|hand| HandContents {
        item: live::held_kind(hand),
        wielded: guns[hand.index()],
        armed: armed::in_hand(hand),
        grenade: grenade::in_hand(hand),
        charge: c4::charge_of(hand),
        detonator: detonator == Some(hand),
        round: round == Some(hand),
        braces: brace
            .filter(|hold| hold.holder.other() == hand)
            .map(|hold| hold.weapon),
        fired: fire[hand.index()],
    })
}
