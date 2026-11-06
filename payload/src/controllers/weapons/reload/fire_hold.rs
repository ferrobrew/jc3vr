//! Why the reload keeps a hand's gun from firing: the one decision behind the reload's fire hold,
//! which the weapon handling's trigger refusal names when a press fires nothing.

/// Why the reload keeps a gun from firing.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FireHold {
    /// The gun is in the belt pouch, from the belt reload until its mouth leaves the pouch.
    BeltPouch,
    /// The gun's magazine is being ejected.
    Ejecting,
    /// The other hand is pulling the gun's magazine out.
    MagazinePulled,
    /// A round is going into the gun.
    RoundGoingIn,
    /// The gun's action is out of battery.
    ActionOpen,
    /// The gun's magazine is out, with no round in the chamber to fire.
    MagazineOut,
    /// The gun's chamber and its magazine are empty.
    Empty,
    /// The gun's chamber is empty, with rounds in its magazine for the action to chamber.
    ChamberEmpty,
}

impl FireHold {
    pub const fn label(self) -> &'static str {
        match self {
            FireHold::BeltPouch => "the gun is in the belt pouch",
            FireHold::Ejecting => "the magazine is being ejected",
            FireHold::MagazinePulled => "the other hand is pulling the magazine out",
            FireHold::RoundGoingIn => "a round is going into the gun",
            FireHold::ActionOpen => "the action is open",
            FireHold::MagazineOut => "the magazine is out",
            FireHold::Empty => "the chamber and the magazine are empty",
            FireHold::ChamberEmpty => "the chamber is empty, with rounds in the magazine",
        }
    }
}

/// What the reload knows of a hand's gun that decides whether it may fire.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(super) struct FireFacts {
    /// The belt reload holds the gun.
    pub belt: bool,
    /// An eject of the gun's magazine is under way.
    pub ejecting: bool,
    /// The other hand is pulling the gun's magazine out.
    pub extracting: bool,
    /// A round is caught in the gun's well with a hand still on it.
    pub inserting: bool,
    /// The gun's plan chambers a round, so its chamber decides whether it fires.
    pub chambering: bool,
    pub chamber_empty: bool,
    /// The physical action is on and the gun's action is out of battery.
    pub out_of_battery: bool,
    /// The game's magazine-out flag.
    pub magazine_out: bool,
    /// The game's magazine holds no rounds.
    pub magazine_empty: bool,
}

/// Why the reload keeps the gun `facts` describes from firing, or `None` when it may fire. A
/// handling under way (the belt, an eject, a pull, an insertion) is named before the gun's state,
/// and the gun's state from the most to the least specific: an open action, a missing magazine,
/// and then the chamber.
pub(super) fn fire_hold(facts: &FireFacts) -> Option<FireHold> {
    let chamber_empty = facts.chambering && facts.chamber_empty;
    if facts.belt {
        Some(FireHold::BeltPouch)
    } else if facts.ejecting {
        Some(FireHold::Ejecting)
    } else if facts.extracting {
        Some(FireHold::MagazinePulled)
    } else if facts.inserting {
        Some(FireHold::RoundGoingIn)
    } else if facts.chambering && facts.out_of_battery {
        Some(FireHold::ActionOpen)
    } else if facts.magazine_out && (!facts.chambering || facts.chamber_empty) {
        Some(FireHold::MagazineOut)
    } else if chamber_empty && facts.magazine_empty {
        Some(FireHold::Empty)
    } else if chamber_empty {
        Some(FireHold::ChamberEmpty)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use test_case::test_case;

    use super::*;

    const CHAMBERING: FireFacts = FireFacts {
        belt: false,
        ejecting: false,
        extracting: false,
        inserting: false,
        chambering: true,
        chamber_empty: false,
        out_of_battery: false,
        magazine_out: false,
        magazine_empty: false,
    };

    #[test]
    fn a_loaded_gun_fires() {
        assert_eq!(fire_hold(&CHAMBERING), None);
        assert_eq!(
            fire_hold(&FireFacts {
                magazine_empty: true,
                ..CHAMBERING
            }),
            None,
            "the round in the chamber fires from an empty magazine"
        );
        assert_eq!(
            fire_hold(&FireFacts {
                magazine_out: true,
                ..CHAMBERING
            }),
            None,
            "the round in the chamber fires with the magazine out"
        );
    }

    #[test_case(FireFacts { chamber_empty: true, magazine_empty: true, ..CHAMBERING }, FireHold::Empty; "dry fire on an empty magazine")]
    #[test_case(FireFacts { chamber_empty: true, ..CHAMBERING }, FireHold::ChamberEmpty; "rounds left but none chambered")]
    #[test_case(FireFacts { chamber_empty: true, magazine_empty: true, out_of_battery: true, ..CHAMBERING }, FireHold::ActionOpen; "the slide locked back on the last shot")]
    #[test_case(FireFacts { chamber_empty: true, magazine_out: true, magazine_empty: true, ..CHAMBERING }, FireHold::MagazineOut; "the magazine dropped from an empty chamber")]
    #[test_case(FireFacts { chambering: false, magazine_out: true, ..CHAMBERING }, FireHold::MagazineOut; "a gun without a chamber and its magazine out")]
    #[test_case(FireFacts { belt: true, chamber_empty: true, ..CHAMBERING }, FireHold::BeltPouch; "the belt reload before the gun's state")]
    #[test_case(FireFacts { ejecting: true, magazine_out: true, ..CHAMBERING }, FireHold::Ejecting; "an eject under way")]
    #[test_case(FireFacts { extracting: true, ..CHAMBERING }, FireHold::MagazinePulled; "the other hand pulling the magazine")]
    #[test_case(FireFacts { inserting: true, chamber_empty: true, ..CHAMBERING }, FireHold::RoundGoingIn; "a round going in")]
    fn a_held_gun_names_what_holds_it(facts: FireFacts, expected: FireHold) {
        assert_eq!(fire_hold(&facts), Some(expected));
    }

    #[test]
    fn a_gun_without_a_chamber_ignores_the_chamber_and_the_action() {
        let facts = FireFacts {
            chambering: false,
            chamber_empty: true,
            out_of_battery: true,
            magazine_empty: true,
            ..CHAMBERING
        };
        assert_eq!(fire_hold(&facts), None);
    }
}
