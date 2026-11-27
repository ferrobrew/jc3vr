//! What each hand holds: one [`HandContents`] per hand, the answer to "what is in this hand" that
//! the table's items, the guns the game wields or the hold arms, the explosives, the reload's
//! round, the brace, and the fire input each hold a part of.
//!
//! The weapon handling samples the record from its sources whenever it decides on the hands
//! mid-tick, and publishes the tick's last sample once, at the end of its input tick
//! ([`publish`]), for the readers outside the tick ([`hands`]). A stand-down publishes empty hands.

use jc3vr_core::published::{Published, Stage};

use crate::controllers::loadout::item::ItemKind;

/// What one hand holds, from every source that holds a part of it.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct HandContents {
    /// The kind of the table's item in the hand.
    pub item: Option<ItemKind>,
    /// Whether the hand holds a gun of the wielded weapon. The C4 the game's planting mode wields
    /// is not a gun.
    pub wielded: bool,
    /// The weapon armed in the hand, by its address.
    pub armed: Option<usize>,
    /// The grenade in the hand.
    pub grenade: Option<HeldGrenade>,
    /// The C4 throw that belongs to the hand.
    pub charge: Option<HeldCharge>,
    /// Whether the hand holds the C4 detonator.
    pub detonator: bool,
    /// Whether the hand holds a round for the reload.
    pub round: bool,
    /// The weapon whose foregrip the hand braces, by its address, held by the other hand.
    pub braces: Option<usize>,
    /// Whether the hand's fire input is on.
    pub fired: bool,
}

/// A grenade in a hand.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HeldGrenade {
    /// A grenade of the table's, with no native actor of its own until it is thrown.
    Safe,
    /// A beacon the hand is about to throw.
    Beacon,
    /// A live grenade the hand caught.
    Caught,
}

/// The C4 throw that belongs to a hand, from the grab at the pouch until the charge leaves it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HeldCharge {
    /// No primed charge is in the hand: the weapon has not primed it yet.
    Asked,
    /// The primed charge at this address is in the hand.
    Primed(usize),
}

/// What a hand that opens its grip on a held thing passes to the other hand's grab.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Passed {
    /// A safe grenade, which moves between the hands as a table item.
    Grenade,
    /// The primed charge.
    Charge,
    /// The gun, when the hand holds one.
    Gun,
}

impl HandContents {
    /// Whether the hand holds a gun: one of the wielded weapon's, or one armed in it.
    pub fn gun(&self) -> bool {
        self.wielded || self.armed.is_some()
    }

    /// Whether the hand holds nothing: no gun, nothing of the table's, no grenade, not the
    /// detonator, and no round.
    pub fn free(&self) -> bool {
        !self.gun()
            && self.item.is_none()
            && self.grenade.is_none()
            && !self.detonator
            && !self.round
    }

    /// Whether the hand holds an explosive that its fire input would otherwise draw a gun over: a
    /// grenade, the C4 throw's charge, or the detonator.
    pub fn explosive(&self) -> bool {
        self.grenade.is_some() || self.charge.is_some() || self.detonator
    }

    /// What the other hand's grab of this hand's held thing takes from it: a safe grenade first,
    /// then a primed charge, and otherwise the gun.
    pub fn passed(&self) -> Passed {
        match (self.grenade, self.charge) {
            (Some(HeldGrenade::Safe), _) => Passed::Grenade,
            (_, Some(HeldCharge::Primed(_))) => Passed::Charge,
            _ => Passed::Gun,
        }
    }
}

/// The hands as the weapon handling's last input tick left them, by [`Hand::index`](jc3vr_input::frame::Hand::index): empty before
/// the first tick and after a stand-down.
pub fn hands() -> [HandContents; 2] {
    HANDS
        .latest()
        .map_or([HandContents::default(); 2], |hands| hands.value)
}

/// Publish the tick's hands, by [`Hand::index`](jc3vr_input::frame::Hand::index). Called once, at the end of the input tick.
pub fn publish(hands: [HandContents; 2]) {
    HANDS.publish(hands);
}

/// Publish empty hands: the weapon handling stood down.
pub fn clear() {
    HANDS.clear();
}

static HANDS: Published<[HandContents; 2]> = Published::new(Stage::InputGame);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hand_with_anything_of_any_source_is_not_free() {
        assert!(HandContents::default().free());
        let holding = [
            HandContents {
                wielded: true,
                ..Default::default()
            },
            HandContents {
                armed: Some(1),
                ..Default::default()
            },
            HandContents {
                item: Some(ItemKind::Prop),
                ..Default::default()
            },
            HandContents {
                grenade: Some(HeldGrenade::Caught),
                ..Default::default()
            },
            HandContents {
                detonator: true,
                ..Default::default()
            },
            HandContents {
                round: true,
                ..Default::default()
            },
        ];
        for contents in holding {
            assert!(!contents.free(), "{contents:?}");
        }
        // The brace and the fire input are not things held.
        assert!(
            HandContents {
                braces: Some(1),
                fired: true,
                ..Default::default()
            }
            .free()
        );
    }

    #[test]
    fn a_safe_grenade_passes_before_a_charge_and_a_charge_before_the_gun() {
        let primed = Some(HeldCharge::Primed(7));
        let contents = |grenade, charge| HandContents {
            wielded: true,
            grenade,
            charge,
            ..Default::default()
        };
        assert_eq!(
            contents(Some(HeldGrenade::Safe), primed).passed(),
            Passed::Grenade
        );
        assert_eq!(
            contents(Some(HeldGrenade::Caught), primed).passed(),
            Passed::Charge
        );
        assert_eq!(
            contents(None, Some(HeldCharge::Asked)).passed(),
            Passed::Gun
        );
        assert_eq!(
            contents(Some(HeldGrenade::Beacon), None).passed(),
            Passed::Gun
        );
    }
}
