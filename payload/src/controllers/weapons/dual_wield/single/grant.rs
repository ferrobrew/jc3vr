//! When the dual-wield slot's weapon gets a second pistol of its own. Pure.
//!
//! A pistol pair the game hands over (a new game's loadout, a supply drop, a mission's give-back,
//! the respawn after a load) is two pistols, so it is split: the slot keeps it as one pistol and a
//! second pistol of the same kind, the twin, is spawned and carried on the other hip. A pistol the
//! player picked up is one pistol, and gets no twin; neither does a pistol the mod moved into the
//! slot itself, or one a respawn or a mission's give-back restores as one pistol. Those arrive
//! already single, or under a cover that says what they are ([`Grants::cover_rack_copy`],
//! [`Grants::cover_respawn`], and [`Grants::cover_mission`]).
//!
//! [`Grants::observe`] follows the slot's weapon one input tick at a time and says what to do with
//! a weapon that arrived: make it single, ask for its twin, or leave it the game's pair. A pair
//! keeps the game's behaviour until its twin comes ([`Grants::arrived`]), and keeps it for good
//! when the twin never comes, there is no hip to carry it on, or the carried weapons are stashed
//! for a mission.

/// How long a twin is waited for once asked, seconds: the respawn's own wait.
pub const TWIN_WAIT_S: f32 = 10.0;
/// How long a rack's copy is expected after the game's pickup of a rack with unlimited stock,
/// seconds.
pub const RACK_COPY_COVER_S: f32 = 5.0;
/// How long a respawn's cover waits for the slot's pistol to come back after a reset, seconds.
pub const RESPAWN_COVER_S: f32 = 20.0;

/// One input tick's view of the dual-wield slot.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GrantTick {
    /// Whether the pistols are made separate at all.
    pub enabled: bool,
    /// The slot's weapon.
    pub slot: Option<usize>,
    /// Whether the slot's weapon is a single pistol already.
    pub single: bool,
    /// Whether the slot's weapon has more than one gun; a weapon of one gun is one weapon
    /// already.
    pub guns: bool,
    /// Whether the carried weapons are stashed for a mission.
    pub stashed: bool,
    /// Whether a hip socket is free for a twin.
    pub socket_free: bool,
    /// The tick's length, seconds.
    pub dt: f32,
}

/// Why a weapon that arrived in the slot is one pistol.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum OneWhy {
    /// A rack with unlimited stock handed over a copy for one pistol.
    RackCopy,
    /// The respawn after a reset brought back a slot that held one pistol.
    Respawn,
    /// A mission gave back a slot that held one pistol.
    MissionReturn,
}

impl OneWhy {
    pub const fn label(self) -> &'static str {
        match self {
            OneWhy::RackCopy => "a rack's copy",
            OneWhy::Respawn => "the respawn of one pistol",
            OneWhy::MissionReturn => "a mission's give-back of one pistol",
        }
    }
}

/// Why a pair stays the game's pair.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PairWhy {
    /// The carried weapons are stashed for a mission.
    Stashed,
    /// No hip socket is free for the twin.
    NoSocket,
    /// The twin never came.
    TwinNeverCame,
}

impl PairWhy {
    pub const fn label(self) -> &'static str {
        match self {
            PairWhy::Stashed => "the carried weapons are stashed for a mission",
            PairWhy::NoSocket => "no hip socket is free for the second pistol",
            PairWhy::TwinNeverCame => "the second pistol never came",
        }
    }
}

/// What to do about the slot's weapon this tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Grant {
    Nothing,
    /// Make the slot's weapon a single pistol, with no twin.
    One {
        object: usize,
        why: OneWhy,
    },
    /// Ask for a twin for the slot's pair.
    Request {
        object: usize,
    },
    /// The pair stays the game's pair.
    Pair {
        object: usize,
        why: PairWhy,
    },
    /// The pair whose twin was asked for left the slot first.
    Cancelled {
        object: usize,
    },
}

/// Where the slot's weapon is in getting its twin.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum TwinState {
    /// No weapon, or one that needs nothing.
    Idle,
    /// The twin was asked for this long ago.
    Pending { object: usize, age_s: f32 },
    /// The weapon has its twin, or is one pistol.
    Done { object: usize },
    /// The weapon stays the game's pair.
    Pair { object: usize, why: PairWhy },
}

/// The slot's weapon across input ticks.
#[derive(Debug, Clone, PartialEq)]
pub struct Grants {
    seen: Option<usize>,
    state: TwinState,
    rack_copy: Option<f32>,
    respawn: Option<(bool, f32)>,
    mission: Option<bool>,
}

impl Default for Grants {
    fn default() -> Self {
        Self::new()
    }
}

impl Grants {
    pub const fn new() -> Self {
        Self {
            seen: None,
            state: TwinState::Idle,
            rack_copy: None,
            respawn: None,
            mission: None,
        }
    }

    pub fn state(&self) -> TwinState {
        self.state
    }

    /// The game's pickup of a rack with unlimited stock asked for a copy.
    pub fn cover_rack_copy(&mut self) {
        self.rack_copy = Some(RACK_COPY_COVER_S);
    }

    /// A reset is under way, and the slot's pistol comes back as one pistol (`Some(true)`), a pair
    /// (`Some(false)`), or with nothing known of it (`None`, which a pair the game hands over
    /// follows).
    pub fn cover_respawn(&mut self, single: Option<bool>) {
        self.respawn = single.map(|single| (single, RESPAWN_COVER_S));
    }

    /// A mission took the slots' weapons, the pistol slot holding one pistol or not.
    pub fn cover_mission(&mut self, single: bool) {
        self.mission = Some(single);
    }

    /// Forget the slot's weapon and the covers: the pistols are no longer made separate, or the
    /// world was reset.
    pub fn clear(&mut self) {
        *self = Self::new();
    }

    /// Follow the slot for one tick.
    pub fn observe(&mut self, tick: GrantTick) -> Grant {
        if !tick.enabled {
            self.clear();
            return Grant::Nothing;
        }
        let dt = if tick.dt.is_finite() {
            tick.dt.max(0.0)
        } else {
            0.0
        };
        self.age_covers(dt);
        if tick.slot != self.seen {
            let left = self.seen;
            self.seen = tick.slot;
            let cancelled = match self.state {
                TwinState::Pending { object, .. } if Some(object) == left => Some(object),
                _ => None,
            };
            self.state = TwinState::Idle;
            let arrived = tick.slot.map(|object| self.arrive(object, tick));
            return match (arrived, cancelled) {
                (Some(grant), _) if grant != Grant::Nothing => grant,
                (_, Some(object)) => Grant::Cancelled { object },
                (Some(grant), None) => grant,
                (None, None) => Grant::Nothing,
            };
        }
        match &mut self.state {
            TwinState::Pending { object, age_s } => {
                let object = *object;
                if tick.single {
                    self.state = TwinState::Done { object };
                    return Grant::Nothing;
                }
                *age_s += dt;
                if *age_s > TWIN_WAIT_S {
                    let why = PairWhy::TwinNeverCame;
                    self.state = TwinState::Pair { object, why };
                    return Grant::Pair { object, why };
                }
                Grant::Nothing
            }
            _ => Grant::Nothing,
        }
    }

    /// The twin asked for has come, with the slot as `slot` and `single` whether its weapon is a
    /// single pistol already. Returns the pair to give it to, which is then done; `None` when the
    /// twin is not wanted any more.
    pub fn arrived(&mut self, slot: Option<usize>, single: bool) -> Option<usize> {
        match self.state {
            TwinState::Pending { object, .. } if slot == Some(object) && !single => {
                self.state = TwinState::Done { object };
                Some(object)
            }
            _ => None,
        }
    }

    /// The twin asked for `object` could not be carried: the pair stays the game's.
    pub fn refused(&mut self, object: usize) {
        if matches!(self.state, TwinState::Pending { object: o, .. } | TwinState::Done { object: o } if o == object)
        {
            self.state = TwinState::Pair {
                object,
                why: PairWhy::NoSocket,
            };
        }
    }

    /// What a weapon that just arrived in the slot is.
    fn arrive(&mut self, object: usize, tick: GrantTick) -> Grant {
        // Every cover is for the next weapon to arrive, whatever it turns out to be.
        let rack_copy = self.rack_copy.take().is_some();
        let respawn = self.respawn.take().map(|(single, _)| single);
        let mission = self.mission.take();
        if tick.single || !tick.guns {
            self.state = TwinState::Done { object };
            return Grant::Nothing;
        }
        let one = if rack_copy {
            Some(OneWhy::RackCopy)
        } else if let Some(single) = respawn {
            single.then_some(OneWhy::Respawn)
        } else if let Some(single) = mission {
            single.then_some(OneWhy::MissionReturn)
        } else {
            None
        };
        if let Some(why) = one {
            self.state = TwinState::Done { object };
            return Grant::One { object, why };
        }
        let why = if tick.stashed {
            Some(PairWhy::Stashed)
        } else if !tick.socket_free {
            Some(PairWhy::NoSocket)
        } else {
            None
        };
        match why {
            Some(why) => {
                self.state = TwinState::Pair { object, why };
                Grant::Pair { object, why }
            }
            None => {
                self.state = TwinState::Pending { object, age_s: 0.0 };
                Grant::Request { object }
            }
        }
    }

    fn age_covers(&mut self, dt: f32) {
        if let Some(left) = &mut self.rack_copy {
            *left -= dt;
            if *left <= 0.0 {
                self.rack_copy = None;
            }
        }
        if let Some((_, left)) = &mut self.respawn {
            *left -= dt;
            if *left <= 0.0 {
                self.respawn = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAIR: usize = 0x1000;
    const OTHER: usize = 0x2000;
    const TICK: f32 = 1.0 / 60.0;

    fn tick(slot: Option<usize>) -> GrantTick {
        GrantTick {
            enabled: true,
            slot,
            single: false,
            guns: true,
            stashed: false,
            socket_free: true,
            dt: TICK,
        }
    }

    #[test]
    fn a_pair_the_game_hands_over_asks_for_its_twin_once() {
        let mut g = Grants::new();
        assert_eq!(g.observe(tick(None)), Grant::Nothing);
        assert_eq!(g.observe(tick(Some(PAIR))), Grant::Request { object: PAIR });
        assert_eq!(g.observe(tick(Some(PAIR))), Grant::Nothing);
        assert_eq!(g.arrived(Some(PAIR), false), Some(PAIR));
        assert_eq!(g.state(), TwinState::Done { object: PAIR });
        assert_eq!(g.observe(tick(Some(PAIR))), Grant::Nothing);
        assert_eq!(
            g.arrived(Some(PAIR), false),
            None,
            "a second twin is never given"
        );
    }

    #[test]
    fn a_single_pistol_never_gets_a_twin() {
        let mut g = Grants::new();
        let single = GrantTick {
            single: true,
            ..tick(Some(PAIR))
        };
        assert_eq!(g.observe(single), Grant::Nothing);
        assert_eq!(g.state(), TwinState::Done { object: PAIR });
        assert_eq!(g.arrived(Some(PAIR), true), None);
    }

    #[test]
    fn a_weapon_of_one_gun_is_never_split() {
        let mut g = Grants::new();
        g.cover_mission(false);
        let one_gun = GrantTick {
            guns: false,
            ..tick(Some(PAIR))
        };
        assert_eq!(g.observe(one_gun), Grant::Nothing);
        assert_eq!(g.state(), TwinState::Done { object: PAIR });
    }

    #[test]
    fn a_twin_that_never_comes_leaves_the_pair_as_it_is() {
        let mut g = Grants::new();
        g.observe(tick(Some(PAIR)));
        let mut last = Grant::Nothing;
        for _ in 0..((TWIN_WAIT_S / TICK) as usize + 2) {
            last = g.observe(tick(Some(PAIR)));
            if last != Grant::Nothing {
                break;
            }
        }
        assert_eq!(
            last,
            Grant::Pair {
                object: PAIR,
                why: PairWhy::TwinNeverCame
            }
        );
        assert_eq!(
            g.arrived(Some(PAIR), false),
            None,
            "a late twin is not wanted"
        );
        assert_eq!(
            g.observe(tick(Some(PAIR))),
            Grant::Nothing,
            "and not asked again"
        );
    }

    #[test]
    fn a_pair_that_leaves_the_slot_cancels_its_twin() {
        let mut g = Grants::new();
        g.observe(tick(Some(PAIR)));
        assert_eq!(g.observe(tick(None)), Grant::Cancelled { object: PAIR });
        assert_eq!(g.arrived(None, false), None);
        // A new pair in its place asks for its own.
        g.observe(tick(Some(PAIR)));
        assert_eq!(
            g.observe(tick(Some(OTHER))),
            Grant::Request { object: OTHER }
        );
        assert_eq!(g.arrived(Some(PAIR), false), None);
        assert_eq!(g.arrived(Some(OTHER), false), Some(OTHER));
    }

    #[test]
    fn a_rack_copy_is_one_pistol_while_its_cover_lasts() {
        let mut g = Grants::new();
        g.cover_rack_copy();
        assert_eq!(
            g.observe(tick(Some(PAIR))),
            Grant::One {
                object: PAIR,
                why: OneWhy::RackCopy
            }
        );
        // The cover is used up: the next pair is a pair.
        assert_eq!(
            g.observe(tick(Some(OTHER))),
            Grant::Request { object: OTHER }
        );
        let mut g = Grants::new();
        g.cover_rack_copy();
        for _ in 0..((RACK_COPY_COVER_S / TICK) as usize + 2) {
            g.observe(tick(None));
        }
        assert_eq!(
            g.observe(tick(Some(PAIR))),
            Grant::Request { object: PAIR },
            "the copy never came, so this is a pair"
        );
    }

    #[test]
    fn a_respawn_restores_what_the_slot_held() {
        let mut g = Grants::new();
        g.cover_respawn(Some(true));
        assert_eq!(
            g.observe(tick(Some(PAIR))),
            Grant::One {
                object: PAIR,
                why: OneWhy::Respawn
            }
        );
        let mut g = Grants::new();
        g.cover_respawn(Some(false));
        assert_eq!(g.observe(tick(Some(PAIR))), Grant::Request { object: PAIR });
        let mut g = Grants::new();
        g.cover_respawn(Some(true));
        g.cover_respawn(None);
        assert_eq!(
            g.observe(tick(Some(PAIR))),
            Grant::Request { object: PAIR },
            "a load that knows nothing of the pistols follows the game's pair"
        );
    }

    #[test]
    fn a_mission_gives_back_what_it_took() {
        let mut g = Grants::new();
        g.cover_mission(true);
        assert_eq!(
            g.observe(tick(Some(PAIR))),
            Grant::One {
                object: PAIR,
                why: OneWhy::MissionReturn
            }
        );
        let mut g = Grants::new();
        g.cover_mission(false);
        assert_eq!(g.observe(tick(Some(PAIR))), Grant::Request { object: PAIR });
    }

    #[test]
    fn a_cover_is_used_up_by_a_pistol_that_arrives_single() {
        let mut g = Grants::new();
        g.cover_mission(true);
        let single = GrantTick {
            single: true,
            ..tick(Some(PAIR))
        };
        assert_eq!(g.observe(single), Grant::Nothing);
        assert_eq!(
            g.observe(tick(Some(OTHER))),
            Grant::Request { object: OTHER },
            "a later pair the game hands over is split, not taken for the give-back"
        );
    }

    #[test]
    fn a_pair_without_a_hip_or_while_stashed_stays_a_pair() {
        let mut g = Grants::new();
        let stashed = GrantTick {
            stashed: true,
            ..tick(Some(PAIR))
        };
        assert_eq!(
            g.observe(stashed),
            Grant::Pair {
                object: PAIR,
                why: PairWhy::Stashed
            }
        );
        assert_eq!(g.observe(tick(Some(PAIR))), Grant::Nothing);
        let mut g = Grants::new();
        let full = GrantTick {
            socket_free: false,
            ..tick(Some(PAIR))
        };
        assert_eq!(
            g.observe(full),
            Grant::Pair {
                object: PAIR,
                why: PairWhy::NoSocket
            }
        );
    }

    #[test]
    fn switching_off_forgets_everything() {
        let mut g = Grants::new();
        g.cover_mission(true);
        g.observe(tick(Some(PAIR)));
        let off = GrantTick {
            enabled: false,
            ..tick(Some(PAIR))
        };
        assert_eq!(g.observe(off), Grant::Nothing);
        assert_eq!(g, Grants::new());
    }

    #[test]
    fn a_twin_that_cannot_be_carried_leaves_the_pair() {
        let mut g = Grants::new();
        g.observe(tick(Some(PAIR)));
        assert_eq!(g.arrived(Some(PAIR), false), Some(PAIR));
        g.refused(PAIR);
        assert_eq!(
            g.state(),
            TwinState::Pair {
                object: PAIR,
                why: PairWhy::NoSocket
            }
        );
    }
}
