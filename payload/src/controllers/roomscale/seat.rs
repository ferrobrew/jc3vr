//! Seat transitions: telling a vehicle entry, a locked seat, and an exit apart, and what each
//! transition mode does at their edges.
//!
//! The game's vehicle predicates do not separate these: the attach-state set includes the locked
//! mounted-gun seat and the motorcycle's reverse, and several entry and exit paths are in neither
//! set. The phase is therefore classified from the body rule system's current state by name. The
//! names are hashed at startup with the engine's state-name hash, so no hash value is embedded, and
//! a state in none of the tables counts as out of the vehicle, which is the safe default: no fade
//! and no re-base.
//!
//! The re-base at seat lock is what makes a seated view work under a head-anchored camera: the
//! seated head bone sits much lower and elsewhere than the standing one, so the cockpit offset the
//! player had standing is wrong once seated. A shuffle between seats inside the vehicle stays
//! locked and does not re-base.

use std::{collections::HashSet, sync::OnceLock};

use jc3gi::hash::hashlittle;

use crate::{controllers::config::SeatTransition, vr::RecenterKind};

/// Where the character is in a vehicle sequence.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum SeatPhase {
    /// Not in a vehicle sequence.
    #[default]
    Out,
    /// Walking to, opening, climbing into, or hijacking a seat.
    Entering,
    /// Seated, including a manned mounted gun and the motorcycle.
    Locked,
    /// Moving between seats inside the vehicle.
    Shuffle,
    /// Climbing out, stepping onto the roof, or ejecting.
    Exiting,
}

impl SeatPhase {
    pub fn label(self) -> &'static str {
        match self {
            SeatPhase::Out => "out",
            SeatPhase::Entering => "entering",
            SeatPhase::Locked => "locked",
            SeatPhase::Shuffle => "shuffling seats",
            SeatPhase::Exiting => "exiting",
        }
    }
}

/// The edges of a vehicle sequence.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SeatEvent {
    EntryBegan,
    SeatLocked,
    ExitBegan,
    ExitDone,
}

/// Classify a body rule state by its name hash, with the game's state-name hash. `None` (no
/// readable rule state) is out.
pub fn classify(state: Option<u32>) -> SeatPhase {
    static TABLES: OnceLock<SeatTables> = OnceLock::new();
    TABLES
        .get_or_init(|| SeatTables::new(|name| hashlittle(name.as_bytes()) as u32))
        .classify(state)
}

/// The seat phases' state-name hashes.
pub struct SeatTables {
    locked: HashSet<u32>,
    shuffle: HashSet<u32>,
    entering: HashSet<u32>,
    exiting: HashSet<u32>,
}

impl SeatTables {
    /// Hash the phases' state names with `hash`.
    pub fn new(hash: impl Fn(&str) -> u32) -> Self {
        let set = |names: &[&str]| names.iter().map(|name| hash(name)).collect();
        Self {
            locked: set(LOCKED),
            shuffle: set(SHUFFLE),
            entering: set(ENTERING),
            exiting: set(EXITING),
        }
    }

    /// Classify a state hash; `None` and a state in no table are out.
    pub fn classify(&self, state: Option<u32>) -> SeatPhase {
        let Some(state) = state else {
            return SeatPhase::Out;
        };
        self.phase_of(state)
    }

    fn phase_of(&self, state: u32) -> SeatPhase {
        if self.locked.contains(&state) {
            SeatPhase::Locked
        } else if self.shuffle.contains(&state) {
            SeatPhase::Shuffle
        } else if self.entering.contains(&state) {
            SeatPhase::Entering
        } else if self.exiting.contains(&state) {
            SeatPhase::Exiting
        } else {
            SeatPhase::Out
        }
    }
}

/// The phase across frames, reporting its edges.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct SeatTracker {
    phase: SeatPhase,
}

impl SeatTracker {
    pub const fn new() -> Self {
        Self {
            phase: SeatPhase::Out,
        }
    }

    pub fn phase(&self) -> SeatPhase {
        self.phase
    }

    /// Fold in this frame's phase and return the edge it makes, if any.
    pub fn observe(&mut self, next: SeatPhase) -> Option<SeatEvent> {
        use SeatPhase::*;
        let event = match (self.phase, next) {
            (a, b) if a == b => None,
            (Out | Exiting, Entering) => Some(SeatEvent::EntryBegan),
            // From out too: an entry path in none of the tables shows only its end.
            (Out | Entering | Exiting, Locked | Shuffle) => Some(SeatEvent::SeatLocked),
            (Locked | Shuffle, Locked | Shuffle) => None,
            (Locked | Shuffle, Exiting) => Some(SeatEvent::ExitBegan),
            (Locked | Shuffle | Exiting | Entering, Out) => Some(SeatEvent::ExitDone),
            (Locked | Shuffle, Entering) => None,
            (Entering, Exiting) => Some(SeatEvent::ExitBegan),
            (Out, Exiting) => Some(SeatEvent::ExitBegan),
            _ => None,
        };
        self.phase = next;
        event
    }
}

/// What a transition mode does at an edge.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct SeatAction {
    /// The seat fade's new target, if it changes.
    pub fade: Option<f32>,
    /// Hold the fade at its current target this long before applying [`fade`](Self::fade), seconds.
    pub hold_s: f32,
    /// The recenter to request.
    pub recenter: Option<RecenterKind>,
}

/// The action `mode` takes at `event`. Under [`SeatTransition::Fade`] the fade clears only after
/// `hold_s`, once the re-based view has settled.
pub fn action(mode: SeatTransition, event: SeatEvent, hold_s: f32) -> SeatAction {
    let hold_s = hold_s.max(0.0);
    match (mode, event) {
        (SeatTransition::Ride, _) => SeatAction::default(),
        (_, SeatEvent::EntryBegan) => SeatAction {
            fade: Some(1.0),
            ..SeatAction::default()
        },
        (SeatTransition::Instant, SeatEvent::SeatLocked) => SeatAction {
            fade: Some(0.0),
            recenter: Some(RecenterKind::Full),
            hold_s: 0.0,
        },
        (SeatTransition::Fade, SeatEvent::SeatLocked) => SeatAction {
            fade: Some(0.0),
            recenter: Some(RecenterKind::Full),
            hold_s,
        },
        (SeatTransition::Instant, SeatEvent::ExitBegan) => SeatAction {
            fade: Some(0.0),
            ..SeatAction::default()
        },
        (SeatTransition::Fade, SeatEvent::ExitBegan) => SeatAction {
            fade: Some(1.0),
            ..SeatAction::default()
        },
        (SeatTransition::Instant, SeatEvent::ExitDone) => SeatAction {
            fade: Some(0.0),
            recenter: Some(RecenterKind::YawAndHeight),
            hold_s: 0.0,
        },
        (SeatTransition::Fade, SeatEvent::ExitDone) => SeatAction {
            fade: Some(0.0),
            recenter: Some(RecenterKind::YawAndHeight),
            hold_s,
        },
    }
}

/// The state names of each phase, from the game's animation state names.
const LOCKED: &[&str] = &[
    "S_IDLE_VEHICLE",
    "S_IDLE_DRIVER_VEHICLE",
    "S_IDLE_PASSENGER_VEHICLE",
    "S_REVERSE_DRIVER_VEHICLE",
    "S_FORWARD_TO_REVERSE_DRIVER_VEHICLE",
    "S_REVERSE_TO_FORWARD_DRIVER_VEHICLE",
    "S_DRIVER_HIDE_IN_VEHICLE",
    "S_PASS_HIDE_IN_VEHICLE",
    "S_IDLE_VEHICLE_MG",
    "S_RIDING_MC",
    "S_RIDING_MC_AIM_0",
    "S_RIDING_MC_AIM_135L",
    "S_RIDING_MC_AIM_135R",
    "S_REVERSE_MC",
];

const SHUFFLE: &[&str] = &[
    "S_DRIVER_TO_PASS1_VEHICLE",
    "S_PASS1_TO_DRIVER_VEHICLE",
    "S_DRIVER_TO_PASSENGER",
    "S_PASSENGER_TO_DRIVER",
    "S_SWITCHING_SEAT_TO_DRIVER",
    "S_SWITCH_DRIVING_DIRECTION",
];

const ENTERING: &[&str] = &[
    "S_ENTER_VEHICLE_FROM_GROUND",
    "S_ENTERING_VEHICLE_RIGHT",
    "S_INITIAL_ENTER_LEFT",
    "S_FINISHED_ENTER_LEFT",
    "S_ENTER_MG",
    "S_ENTER_MG_WITH_MG",
    "S_ENTER_MC",
    "S_STUNT_TO_DRIVER_VEHICLE",
    "S_GROUND_TO_NEUTRAL",
    "S_NEUTRAL_TO_DRIVER",
    "S_NEUTRAL_TO_STRUGGLE",
    "S_HIJACK_STRUGGLE_FAIL",
];

const EXITING: &[&str] = &[
    "S_EXITING_VEHICLE",
    "S_EXIT_VEHICLE_TO_GROUND",
    "S_EJECT_FROM_VEHICLE_ALIVE",
    "S_EJECT_FROM_VEHICLE_DEAD",
    "S_INITIAL_EXIT_RIGHT",
    "S_FINISHED_EXIT_LEFT",
    "S_FINISHED_EXIT_RIGHT",
    "S_DRIVER_TO_STUNT_VEHICLE",
    "S_STUNT_TO_JUMP_EXIT_VEHICLE",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for the game's hash, which lives in the game binary: FNV-1a.
    fn hash(name: &str) -> u32 {
        name.bytes().fold(0x811C_9DC5, |h, b| {
            (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
        })
    }

    fn state(name: &str) -> Option<u32> {
        Some(hash(name))
    }

    #[test]
    fn states_classify_by_name() {
        let tables = SeatTables::new(hash);
        let classify = |state| tables.classify(state);
        assert_eq!(
            classify(state("S_ENTER_VEHICLE_FROM_GROUND")),
            SeatPhase::Entering
        );
        assert_eq!(classify(state("S_IDLE_DRIVER_VEHICLE")), SeatPhase::Locked);
        assert_eq!(classify(state("S_IDLE_VEHICLE_MG")), SeatPhase::Locked);
        assert_eq!(
            classify(state("S_DRIVER_TO_PASS1_VEHICLE")),
            SeatPhase::Shuffle
        );
        assert_eq!(classify(state("S_EXITING_VEHICLE")), SeatPhase::Exiting);
        assert_eq!(classify(state("S_IDLE")), SeatPhase::Out);
        assert_eq!(classify(None), SeatPhase::Out);
    }

    #[test]
    fn an_entry_locks_and_an_exit_leaves() {
        let mut t = SeatTracker::default();
        assert_eq!(t.observe(SeatPhase::Entering), Some(SeatEvent::EntryBegan));
        assert_eq!(t.observe(SeatPhase::Entering), None);
        assert_eq!(t.observe(SeatPhase::Locked), Some(SeatEvent::SeatLocked));
        assert_eq!(t.observe(SeatPhase::Exiting), Some(SeatEvent::ExitBegan));
        assert_eq!(t.observe(SeatPhase::Out), Some(SeatEvent::ExitDone));
    }

    #[test]
    fn a_seat_shuffle_does_not_re_base() {
        let mut t = SeatTracker::default();
        t.observe(SeatPhase::Locked);
        assert_eq!(t.observe(SeatPhase::Shuffle), None);
        assert_eq!(t.observe(SeatPhase::Locked), None);
    }

    #[test]
    fn an_untabled_entry_still_locks() {
        let mut t = SeatTracker::default();
        assert_eq!(t.observe(SeatPhase::Locked), Some(SeatEvent::SeatLocked));
    }

    #[test]
    fn ride_does_nothing_and_the_others_re_base_at_the_lock() {
        for event in [
            SeatEvent::EntryBegan,
            SeatEvent::SeatLocked,
            SeatEvent::ExitBegan,
            SeatEvent::ExitDone,
        ] {
            assert_eq!(
                action(SeatTransition::Ride, event, 0.3),
                SeatAction::default()
            );
        }
        let instant = action(SeatTransition::Instant, SeatEvent::SeatLocked, 0.3);
        assert_eq!(instant.recenter, Some(RecenterKind::Full));
        assert_eq!(instant.hold_s, 0.0);
        let fade = action(SeatTransition::Fade, SeatEvent::SeatLocked, 0.3);
        assert_eq!(fade.recenter, Some(RecenterKind::Full));
        assert_eq!(fade.hold_s, 0.3);
        let exit = action(SeatTransition::Fade, SeatEvent::ExitDone, 0.3);
        assert_eq!(exit.recenter, Some(RecenterKind::YawAndHeight));
    }
}
