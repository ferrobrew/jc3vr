//! Why the magazine pouch is not a grab target on a tick: a note for the grip press's log, so a
//! grab at the belt that takes nothing says what the pouch was waiting on.

/// What the pouch's grab target waited on, in the order the input tick checks it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum PouchRefusal {
    /// The character's skeleton could not be read, so the pouch has no place.
    NoBody,
    /// No hand is free to take a round.
    NoFreeHand,
    /// No gun in the other hand is reloaded by hand.
    NoGun,
    /// The gun has no rounds left for it in the reserve, and they are not unlimited.
    NoRounds,
    /// The gun takes nothing from the pouch now: a magazine only while its magazine is out, a shell
    /// or a rocket only while it is short.
    GunTakesNothing,
    /// A round is already in hand.
    Holding,
}

impl PouchRefusal {
    pub(super) const fn label(self) -> &'static str {
        match self {
            PouchRefusal::NoBody => "magazine pouch: not placed, the skeleton was not read",
            PouchRefusal::NoFreeHand => "magazine pouch: not offered, no hand is free",
            PouchRefusal::NoGun => "magazine pouch: not offered, no gun reloaded by hand",
            PouchRefusal::NoRounds => "magazine pouch: not offered, no rounds left for the gun",
            PouchRefusal::GunTakesNothing => {
                "magazine pouch: not offered, the gun's magazine is in (eject it first)"
            }
            PouchRefusal::Holding => "magazine pouch: not offered, a round is already in hand",
        }
    }
}

/// The input tick's facts the pouch's grab target depends on.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct PouchFacts {
    pub placed: bool,
    pub free: bool,
    pub gun: bool,
    pub stocked: bool,
    pub takes: bool,
    pub holding: bool,
}

/// Why the pouch is not offered with `facts`, or `None` when it is.
pub(super) fn refusal(facts: PouchFacts) -> Option<PouchRefusal> {
    if !facts.placed {
        Some(PouchRefusal::NoBody)
    } else if !facts.free {
        Some(PouchRefusal::NoFreeHand)
    } else if !facts.gun {
        Some(PouchRefusal::NoGun)
    } else if !facts.stocked {
        Some(PouchRefusal::NoRounds)
    } else if !facts.takes {
        Some(PouchRefusal::GunTakesNothing)
    } else if facts.holding {
        Some(PouchRefusal::Holding)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFFERED: PouchFacts = PouchFacts {
        placed: true,
        free: true,
        gun: true,
        stocked: true,
        takes: true,
        holding: false,
    };

    #[test]
    fn an_offered_pouch_has_no_refusal() {
        assert_eq!(refusal(OFFERED), None);
    }

    #[test]
    fn each_missing_fact_names_its_refusal() {
        let cases = [
            (
                PouchFacts {
                    placed: false,
                    ..OFFERED
                },
                PouchRefusal::NoBody,
            ),
            (
                PouchFacts {
                    free: false,
                    ..OFFERED
                },
                PouchRefusal::NoFreeHand,
            ),
            (
                PouchFacts {
                    gun: false,
                    ..OFFERED
                },
                PouchRefusal::NoGun,
            ),
            (
                PouchFacts {
                    stocked: false,
                    ..OFFERED
                },
                PouchRefusal::NoRounds,
            ),
            (
                PouchFacts {
                    takes: false,
                    ..OFFERED
                },
                PouchRefusal::GunTakesNothing,
            ),
            (
                PouchFacts {
                    holding: true,
                    ..OFFERED
                },
                PouchRefusal::Holding,
            ),
        ];
        for (facts, expected) in cases {
            assert_eq!(refusal(facts), Some(expected), "{facts:?}");
        }
    }
}
