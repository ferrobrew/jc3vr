//! Which weapon the true reload acts on while the hands hold two: the wielded weapon or the second
//! weapon ([`crate::controllers::weapons::hold::second`]). Pure.
//!
//! The reload follows one weapon at a time: its guns, its well, its chamber, the stop on the game's
//! own reload, and the shots it learns from. With a second weapon in the other hand, the reload
//! turns to whichever of the two is lower: a weapon whose magazine is out or empty while the other
//! still has rounds, or one whose magazine is a good share lower than the other's. It does not turn
//! while a reload step is under way (a round in a hand, or a well pushed into the belt pouch), so a
//! reload is never cut in half. Each weapon's chamber and magazine are remembered across a turn.

/// The weapon the reload acts on.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum ReloadTarget {
    /// The wielded weapon.
    #[default]
    Primary,
    /// The second weapon.
    Second,
}

impl ReloadTarget {
    pub const fn label(self) -> &'static str {
        match self {
            ReloadTarget::Primary => "the wielded weapon",
            ReloadTarget::Second => "the second weapon",
        }
    }
}

/// A weapon's magazine, as the focus weighs it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Fill {
    pub magazine: i32,
    pub capacity: i32,
    /// Whether the magazine is out of the gun.
    pub out: bool,
}

impl Fill {
    /// How full the magazine is, `0..=1`; an out magazine is empty.
    pub fn fraction(self) -> f32 {
        if self.out || self.capacity <= 0 {
            return if self.out { 0.0 } else { 1.0 };
        }
        (self.magazine.max(0) as f32 / self.capacity as f32).clamp(0.0, 1.0)
    }

    pub fn empty(self) -> bool {
        self.out || self.magazine <= 0
    }
}

/// The weapon the reload acts on next, from the `current` one, the wielded weapon's magazine
/// (`primary`), the second weapon's (`second`, `None` without one), and whether a reload step is
/// under way (`busy`).
pub fn choose(
    current: ReloadTarget,
    primary: Option<Fill>,
    second: Option<Fill>,
    busy: bool,
) -> ReloadTarget {
    let Some(second) = second else {
        return ReloadTarget::Primary;
    };
    let Some(primary) = primary else {
        return ReloadTarget::Second;
    };
    if busy {
        return current;
    }
    let (here, there, other) = match current {
        ReloadTarget::Primary => (primary, second, ReloadTarget::Second),
        ReloadTarget::Second => (second, primary, ReloadTarget::Primary),
    };
    let emptier = there.empty() && !here.empty();
    let lower = there.fraction() + SWITCH_MARGIN < here.fraction();
    if emptier || lower { other } else { current }
}

/// How much lower the other weapon's magazine must be, as a share of its capacity, before the
/// reload turns to it.
pub const SWITCH_MARGIN: f32 = 0.25;

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(magazine: i32, capacity: i32) -> Option<Fill> {
        Some(Fill {
            magazine,
            capacity,
            out: false,
        })
    }

    #[test]
    fn without_a_second_weapon_the_reload_stays_on_the_wielded_one() {
        for current in [ReloadTarget::Primary, ReloadTarget::Second] {
            assert_eq!(
                choose(current, fill(0, 30), None, false),
                ReloadTarget::Primary
            );
        }
    }

    #[test]
    fn the_reload_turns_to_the_emptier_weapon() {
        assert_eq!(
            choose(ReloadTarget::Primary, fill(20, 30), fill(0, 8), false),
            ReloadTarget::Second
        );
        assert_eq!(
            choose(ReloadTarget::Second, fill(0, 30), fill(5, 8), false),
            ReloadTarget::Primary
        );
        // A magazine out of the gun counts as empty.
        let out = Some(Fill {
            magazine: 8,
            capacity: 8,
            out: true,
        });
        assert_eq!(
            choose(ReloadTarget::Primary, fill(30, 30), out, false),
            ReloadTarget::Second
        );
    }

    #[test]
    fn a_small_difference_does_not_turn_the_reload() {
        assert_eq!(
            choose(ReloadTarget::Primary, fill(20, 30), fill(5, 8), false),
            ReloadTarget::Primary
        );
        assert_eq!(
            choose(ReloadTarget::Primary, fill(28, 30), fill(3, 8), false),
            ReloadTarget::Second,
            "far lower turns it"
        );
        // Both empty: the reload stays where it is.
        assert_eq!(
            choose(ReloadTarget::Second, fill(0, 30), fill(0, 8), false),
            ReloadTarget::Second
        );
    }

    #[test]
    fn a_reload_under_way_keeps_its_weapon() {
        assert_eq!(
            choose(ReloadTarget::Primary, fill(20, 30), fill(0, 8), true),
            ReloadTarget::Primary
        );
        // Unless the weapon it was on is gone.
        assert_eq!(
            choose(ReloadTarget::Second, fill(20, 30), None, true),
            ReloadTarget::Primary
        );
    }

    #[test]
    fn a_weapon_without_a_magazine_size_reads_as_full() {
        let unknown = Fill {
            magazine: 0,
            capacity: 0,
            out: false,
        };
        assert_eq!(unknown.fraction(), 1.0);
        assert!(unknown.empty());
    }
}
