//! A single pistol's magazine. Pure.
//!
//! The game tunes the dual-wield weapon's magazine for both pistols together. Each single pistol
//! holds half of it, rounded up, so that two pistols made from one pair hold at least what the
//! pair held. The reserve stays the character's, shared by both. A pair split in two deals its
//! magazine to the slot's pistol first ([`split`]), and a pistol holding more than its half (one
//! the game's own reload filled to the tuned size, or one taken from the world full) is brought
//! down to its half with the rounds over it returned to the reserve ([`clamp`]).

/// A single pistol's magazine size for a dual-wield weapon tuned for `tuned` rounds: half,
/// rounded up. A negative size means no limit, and stays so.
pub const fn capacity(tuned: i32) -> i32 {
    if tuned < 0 { tuned } else { tuned - tuned / 2 }
}

/// How a pair's magazine is dealt between its two pistols.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Dealt {
    /// The rounds the slot's pistol keeps.
    pub slot: i32,
    /// The rounds the second pistol gets.
    pub twin: i32,
    /// Rounds neither holds, for the reserve: only when the pair held more than both halves.
    pub over: i32,
}

/// Deal the pair's magazine of `total` rounds, tuned for `tuned`, between its two pistols: the
/// slot's pistol filled first, the second taking the rest up to its half.
pub fn split(total: i32, tuned: i32) -> Dealt {
    let total = total.max(0);
    let half = capacity(tuned);
    if half < 0 {
        return Dealt {
            slot: total,
            twin: 0,
            over: 0,
        };
    }
    let slot = total.min(half);
    let twin = (total - slot).min(half);
    Dealt {
        slot,
        twin,
        over: total - slot - twin,
    }
}

/// What bringing a pistol down to its half does.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Clamped {
    /// The magazine afterwards.
    pub magazine: i32,
    /// The rounds to add to the reserve: zero with unlimited ammunition, whose rounds are not
    /// drawn from the reserve.
    pub returned: i32,
}

/// Bring a single pistol's magazine of `magazine` rounds, of a weapon tuned for `tuned`, down to
/// its half. `None` when it is within it.
pub fn clamp(magazine: i32, tuned: i32, unlimited: bool) -> Option<Clamped> {
    let half = capacity(tuned);
    if half < 0 || magazine <= half {
        return None;
    }
    Some(Clamped {
        magazine: half,
        returned: if unlimited { 0 } else { magazine - half },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_pistol_holds_half_rounded_up() {
        assert_eq!(capacity(24), 12);
        assert_eq!(capacity(15), 8);
        assert_eq!(capacity(1), 1);
        assert_eq!(capacity(0), 0);
        assert_eq!(capacity(-1), -1, "no limit stays no limit");
    }

    #[test]
    fn a_pair_is_dealt_slot_first_and_nothing_is_lost() {
        assert_eq!(
            split(24, 24),
            Dealt {
                slot: 12,
                twin: 12,
                over: 0
            }
        );
        assert_eq!(
            split(13, 24),
            Dealt {
                slot: 12,
                twin: 1,
                over: 0
            }
        );
        assert_eq!(
            split(5, 24),
            Dealt {
                slot: 5,
                twin: 0,
                over: 0
            }
        );
        assert_eq!(
            split(15, 15),
            Dealt {
                slot: 8,
                twin: 7,
                over: 0
            }
        );
        assert_eq!(split(-3, 24).slot, 0);
    }

    #[test]
    fn a_pair_overfilled_returns_what_neither_pistol_holds() {
        let dealt = split(30, 24);
        assert_eq!((dealt.slot, dealt.twin, dealt.over), (12, 12, 6));
        assert_eq!(dealt.slot + dealt.twin + dealt.over, 30);
    }

    #[test]
    fn an_unlimited_magazine_is_dealt_whole_to_the_slot() {
        assert_eq!(
            split(40, -1),
            Dealt {
                slot: 40,
                twin: 0,
                over: 0
            }
        );
    }

    #[test]
    fn a_full_reload_is_brought_down_to_the_half() {
        assert_eq!(
            clamp(24, 24, false),
            Some(Clamped {
                magazine: 12,
                returned: 12
            })
        );
        assert_eq!(
            clamp(24, 24, true),
            Some(Clamped {
                magazine: 12,
                returned: 0
            })
        );
        assert_eq!(clamp(12, 24, false), None);
        assert_eq!(clamp(3, 24, false), None);
        assert_eq!(clamp(99, -1, false), None);
    }
}
