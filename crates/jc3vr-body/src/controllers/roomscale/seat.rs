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

use std::sync::LazyLock;

use jc3vr_core::hash::StateTable;
use jc3vr_view::vr::RecenterKind;

use crate::controllers::roomscale::config::SeatTransition;

/// Under [`SeatTransition::Fade`], how long the view stays dark after the seat locks, so that the
/// re-based view settles before it clears, seconds.
pub const SEAT_FADE_HOLD_S: f32 = 0.3;

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

/// Classify a body rule state by its name hash. `None` (no readable rule state) and a state in no
/// table are out.
pub fn classify(state: Option<u32>) -> SeatPhase {
    static TABLE: LazyLock<StateTable<SeatPhase>> = LazyLock::new(|| {
        StateTable::new(
            PHASE_STATES
                .iter()
                .flat_map(|&(phase, names)| names.iter().map(move |&name| (name, phase))),
        )
    });
    state
        .and_then(|state| TABLE.get(state))
        .unwrap_or(SeatPhase::Out)
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
    /// Re-base the cockpit's yaw by the head's offset from the body when it stopped following
    /// ([`jc3vr_view::vr::request_seat_yaw_rebase`]), in place of a recenter.
    pub rebase_yaw: bool,
}

/// The action `mode` takes at `event`. Under [`SeatTransition::Fade`] the fade clears only after
/// `hold_s`, once the re-based view has settled.
pub fn action(mode: SeatTransition, event: SeatEvent, hold_s: f32) -> SeatAction {
    let hold_s = hold_s.max(0.0);
    match (mode, event) {
        (SeatTransition::Ride, SeatEvent::SeatLocked) => SeatAction {
            rebase_yaw: true,
            ..SeatAction::default()
        },
        (SeatTransition::Ride, _) => SeatAction::default(),
        (_, SeatEvent::EntryBegan) => SeatAction {
            fade: Some(1.0),
            ..SeatAction::default()
        },
        (SeatTransition::Instant, SeatEvent::SeatLocked) => SeatAction {
            fade: Some(0.0),
            recenter: Some(RecenterKind::Full),
            hold_s: 0.0,
            ..SeatAction::default()
        },
        (SeatTransition::Fade, SeatEvent::SeatLocked) => SeatAction {
            fade: Some(0.0),
            recenter: Some(RecenterKind::Full),
            hold_s,
            ..SeatAction::default()
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
            ..SeatAction::default()
        },
        (SeatTransition::Fade, SeatEvent::ExitDone) => SeatAction {
            fade: Some(0.0),
            recenter: Some(RecenterKind::YawAndHeight),
            hold_s,
            ..SeatAction::default()
        },
    }
}

/// The state names of each phase, from the state names of Rico's base animation graph (`rico_base`), the graph the body rule system runs. No name is in two phases.
pub const PHASE_STATES: [(SeatPhase, &[&str]); 4] = [
    (SeatPhase::Locked, LOCKED),
    (SeatPhase::Shuffle, SHUFFLE),
    (SeatPhase::Entering, ENTERING),
    (SeatPhase::Exiting, EXITING),
];

const LOCKED: &[&str] = &[
    "S_AIM_IDLE_RIDING_MOTORBIKE",
    "S_AIM_IDLE_TO_AIM_RIDING_MOTORBIKE",
    "S_AIM_IDLE_TO_IDLE_LEFT_RIDING_MOTORBIKE",
    "S_AIM_IDLE_TO_SLOW_RIDING_MOTORBIKE",
    "S_AIM_RIDING_MOTORBIKE",
    "S_AIM_TO_AIM_IDLE_RIDING_MOTORBIKE",
    "S_FAST_RIDING_MOTORBIKE",
    "S_FAST_TO_SLOW_RIDING_MOTORBIKE",
    "S_FAST_TO_STAND_RIDING_MOTORBIKE",
    "S_FORWARD_TO_REVERSE_DRIVER_VEHICLE",
    "S_IDLE_DRIVER_VEHICLE",
    "S_IDLE_LEFT_RIDING_MOTORBIKE",
    "S_IDLE_LEFT_TO_AIM_IDLE_RIDING_MOTORBIKE",
    "S_IDLE_LEFT_TO_SLOW_RIDING_MOTORBIKE",
    "S_IDLE_PASSENGER_VEHICLE",
    "S_IDLE_RIGHT_RIDING_MOTORBIKE",
    "S_IDLE_RIGHT_TO_AIM_IDLE_RIDING_MOTORBIKE",
    "S_IDLE_RIGHT_TO_SLOW_RIDING_MOTORBIKE",
    "S_IDLE_VEHICLE_SPINNING_MOUNTED_WEAPON",
    "S_MOUNTED_WEAPON",
    "S_REVERSE_DRIVER_VEHICLE",
    "S_REVERSE_LEFT_RIDING_MOTORBIKE",
    "S_REVERSE_RIGHT_RIDING_MOTORBIKE",
    "S_REVERSE_TO_FORWARD_DRIVER_VEHICLE",
    "S_SLOW_RIDING_MOTORBIKE",
    "S_SLOW_TO_FAST_RIDING_MOTORBIKE",
    "S_SLOW_TO_IDLE_LEFT_RIDING_MOTORBIKE",
    "S_SLOW_TO_STAND_RIDING_MOTORBIKE",
    "S_STAND_RIDING_MOTORBIKE",
    "S_STAND_TO_FAST_RIDING_MOTORBIKE",
    "S_STAND_TO_SLOW_RIDING_MOTORBIKE",
    "S_VEHICLE_FIXED_MOUNTED_WEAPON",
];

const SHUFFLE: &[&str] = &[
    "S_DRIVER_TO_SPINNING_MOUNTED_WEAPON_VEHICLE",
    "S_PASS1_TO_DRIVER_VEHICLE",
    "S_PASS1_TO_SPINNING_MOUNTED_WEAPON_VEHICLE",
    "S_SPINNING_MOUNTED_WEAPON_TO_PASS1_VEHICLE",
];

const ENTERING: &[&str] = &[
    "S_BAY_DRIVER_DOOR_TO_DRIVER_VEHICLE",
    "S_DRIVER_SIDE_ENTER_VEHICLE",
    "S_ENTERED_DRIVER_NO_DOOR_VEHICLE",
    "S_ENTERED_DRIVER_VEHICLE",
    "S_ENTER_BAY_TO_BAY_DRIVER_DOOR_VEHICLE",
    "S_ENTER_BAY_TO_BAY_STUNT_DOOR_VEHICLE",
    "S_ENTER_DRIVER_LEFT_MOTORBIKE",
    "S_ENTER_DRIVER_LEFT_MOTORBIKE_WATER",
    "S_ENTER_DRIVER_NO_DOOR_VEHICLE",
    "S_ENTER_DRIVER_RIGHT_MOTORBIKE",
    "S_ENTER_DRIVER_RIGHT_MOTORBIKE_WATER",
    "S_ENTER_DRIVER_SIDE_VEHICLE",
    "S_ENTER_DRIVER_UPRIGHT_SADDLE_LEFT_MOTORBIKE",
    "S_ENTER_DRIVER_UPRIGHT_SADDLE_RIGHT_MOTORBIKE",
    "S_ENTER_DRIVER_UPRIGHT_WHEELS_LEFT_MOTORBIKE",
    "S_ENTER_DRIVER_UPRIGHT_WHEELS_RIGHT_MOTORBIKE",
    "S_ENTER_DRIVER_VEHICLE",
    "S_ENTER_FROM_WATER_LINE_DRIVER_SIDE_VEHICLE_P1",
    "S_ENTER_FROM_WATER_LINE_DRIVER_SIDE_VEHICLE_P2",
    "S_ENTER_FROM_WATER_LINE_ON_GROUND_VEHICLE_P1",
    "S_ENTER_FROM_WATER_LINE_ON_GROUND_VEHICLE_P2",
    "S_ENTER_FROM_WATER_LINE_PASSENGER_SIDE_VEHICLE_P1",
    "S_ENTER_FROM_WATER_LINE_PASSENGER_SIDE_VEHICLE_P2",
    "S_ENTER_KICK_OFF_LEFT_MOTORBIKE",
    "S_ENTER_KICK_OFF_RIGHT_MOTORBIKE",
    "S_ENTER_LINE_DRIVER_SIDE_VEHICLE_P1",
    "S_ENTER_LINE_DRIVER_SIDE_VEHICLE_P2",
    "S_ENTER_LINE_ON_GROUND_VEHICLE_P1",
    "S_ENTER_LINE_ON_GROUND_VEHICLE_P2",
    "S_ENTER_LINE_TO_SPINNING_MOUNTED_WEAPON_VEHICLE_P1",
    "S_ENTER_LINE_TO_SPINNING_MOUNTED_WEAPON_VEHICLE_P2",
    "S_ENTER_MOUNTED_WEAPON_FROM_BACK",
    "S_ENTER_MOUNTED_WEAPON_FROM_LEFT",
    "S_ENTER_MOUNTED_WEAPON_FROM_LEFT_PULL_OUT",
    "S_ENTER_MOUNTED_WEAPON_FROM_RIGHT",
    "S_ENTER_MOUNTED_WEAPON_FROM_RIGHT_PULL_OUT",
    "S_ENTER_ON_GROUND_THROW_OUT_NO_DOOR_VEHICLE",
    "S_ENTER_ON_GROUND_THROW_OUT_VEHICLE",
    "S_ENTER_ON_GROUND_UNLOCK_VEHICLE",
    "S_ENTER_PASSENGER_NO_DOOR_VEHICLE",
    "S_ENTER_PASSENGER_ON_GROUND_THROW_OUT_NO_DOOR_VEHICLE",
    "S_ENTER_PASSENGER_ON_GROUND_THROW_OUT_VEHICLE",
    "S_ENTER_PASSENGER_ON_GROUND_UNLOCK_VEHICLE",
    "S_ENTER_PASSENGER_SIDE_VEHICLE",
    "S_ENTER_PASSENGER_VEHICLE",
    "S_ENTER_STUNT_TO_BAY_STUNT_SIDE_CLOSE_VEHICLE",
    "S_ENTER_STUNT_TO_BAY_STUNT_SIDE_FAR_VEHICLE",
    "S_ENTER_STUNT_TO_DRIVER_ON_GROUND_VEHICLE",
    "S_ENTER_STUNT_TO_DRIVER_SIDE_CLOSE_VEHICLE",
    "S_ENTER_STUNT_TO_DRIVER_SIDE_FAR_VEHICLE",
    "S_ENTER_STUNT_TO_DRIVER_SIDE_VEHICLE",
    "S_ENTER_STUNT_TO_DRIVER_VEHICLE",
    "S_ENTER_STUNT_TO_SPINNING_MOUNTED_WEAPON_VEHICLE",
    "S_ENTER_STUNT_TO_STUNT_SIDE_CLOSE_VEHICLE",
    "S_ENTER_STUNT_TO_STUNT_SIDE_FAR_VEHICLE",
    "S_ENTER_STUNT_TO_STUNT_SIDE_VEHICLE",
    "S_ENTER_STUNT_TO_TRANS_ENTER_VEHICLE_P1",
    "S_ENTER_VEHICLE_FIXED_MOUNTED_WEAPON",
    "S_GRAPPLE_REEL_TO_DRIVER_MOTORBIKE_OCCUPIED",
    "S_GRAPPLE_REEL_TO_DRIVER_MOTORBIKE_UNOCCUPIED_0",
    "S_GRAPPLE_REEL_TO_DRIVER_MOTORBIKE_UNOCCUPIED_120L",
    "S_GRAPPLE_REEL_TO_DRIVER_MOTORBIKE_UNOCCUPIED_120R",
    "S_GRAPPLE_REEL_TO_DRIVER_OCCUPIED",
    "S_GRAPPLE_REEL_TO_DRIVER_SIDE",
    "S_GRAPPLE_REEL_TO_DRIVER_UNOCCUPIED_0",
    "S_GRAPPLE_REEL_TO_DRIVER_UNOCCUPIED_120L",
    "S_GRAPPLE_REEL_TO_DRIVER_UNOCCUPIED_120R",
    "S_HANG_TO_DRIVERSIDE_VEHICLE",
    "S_HANG_TO_DRIVERSIDE_VEHICLE2",
    "S_ON_GROUND_ENTER_VEHICLE",
    "S_ON_GROUND_TO_ENTER_DRIVER_SIDE_VEHICLE",
    "S_PASSENGER_ON_GROUND_ENTER_VEHICLE",
    "S_PASSENGER_ON_GROUND_TO_ENTER_SIDE_VEHICLE",
    "S_PASSENGER_SIDE_ENTER_VEHICLE",
    "S_STUNT_SIDE_TO_DRIVER_ON_GROUND_VEHICLE",
    "S_STUNT_SIDE_TO_DRIVER_SIDE_VEHICLE",
    "S_STUNT_SIDE_TO_DRIVER_VEHICLE",
    "S_STUNT_TO_DRIVER_ON_GROUND_VEHICLE",
    "S_STUNT_TO_DRIVER_SIDE_CLOSE_VEHICLE",
    "S_STUNT_TO_DRIVER_SIDE_FAR_VEHICLE",
    "S_STUNT_TO_DRIVER_SIDE_VEHICLE",
    "S_STUNT_TO_DRIVER_VEHICLE",
    "S_STUNT_TO_SPINNING_MOUNTED_WEAPON_VEHICLE",
];

const EXITING: &[&str] = &[
    "S_DRIVER_TO_PASSENGER_EXIT_VEHICLE",
    "S_DRIVER_TO_STUNT_SIDE_P2_VEHICLE",
    "S_DRIVER_TO_STUNT_SIDE_VEHICLE",
    "S_DRIVER_UP_TO_STUNT_AREA_VEHICLE",
    "S_DRIVER_UP_TO_STUNT_AREA_VEHICLE_P2",
    "S_EXIT_DRIVER_VEHICLE_IN_WATER",
    "S_EXIT_DRIVER_VEHICLE_IN_WATER_DETATCHED",
    "S_EXIT_DRIVER_VEHICLE_UPSIDE_DOWN",
    "S_EXIT_DRIVER_VEHICLE_UPSIDE_DOWN_DETATCHED",
    "S_EXIT_IN_WATER_MOTORBIKE",
    "S_EXIT_IN_WATER_MOTORBIKE_DETATCHED",
    "S_EXIT_LEFT_MOTORBIKE",
    "S_EXIT_LEFT_MOTORBIKE_DETATCHED",
    "S_EXIT_MOUNTED_WEAPON",
    "S_EXIT_MOUNTED_WEAPON_TO_LEFT",
    "S_EXIT_MOUNTED_WEAPON_TO_RIGHT",
    "S_EXIT_PASSENGER_VEHICLE_UPSIDE_DOWN",
    "S_EXIT_RIGHT_MOTORBIKE",
    "S_EXIT_RIGHT_MOTORBIKE_DETATCHED",
    "S_EXIT_SPINNING_MOUNTED_TO_FALL",
    "S_EXIT_SPINNING_MOUNTED_TO_FALL_DETATCHED",
    "S_EXIT_SPINNING_MOUNTED_VEHICLE_IN_WATER",
    "S_EXIT_SPINNING_MOUNTED_VEHICLE_IN_WATER_DETATCHED",
    "S_EXIT_TO_STUNT_P2_VEHICLE",
    "S_EXIT_TO_STUNT_VEHICLE",
    "S_EXIT_VEHICLE",
    "S_EXIT_VEHICLE_DETATCHED",
    "S_EXIT_VEHICLE_PARA",
    "S_EXIT_VEHICLE_PARA_DETACHED",
    "S_EXIT_VEHICLE_TO_FALL",
    "S_EXIT_VEHICLE_TO_FALL_DETATCHED",
    "S_ROLL_OUT_EXIT_LEFT_MOTORBIKE",
    "S_ROLL_OUT_EXIT_LEFT_MOTORBIKE_DETATCHED",
    "S_ROLL_OUT_EXIT_RIGHT_MOTORBIKE",
    "S_ROLL_OUT_EXIT_RIGHT_MOTORBIKE_DETATCHED",
    "S_ROLL_OUT_EXIT_VEHICLE",
    "S_ROLL_OUT_EXIT_VEHICLE_DETATCHED",
    "S_SPINNING_MOUNTED_WEAPON_TO_STUNT_SIDE_VEHICLE",
    "S_SPINNING_MOUNTED_WEAPON_TO_STUNT_VEHICLE",
    "S_SPINNING_MOUNTED_WEAPON_TO_STUNT_VEHICLE_P2",
    "S_STUNT_BACKFLIP_JUMP_EXIT_VEHICLE",
    "S_STUNT_BACKFLIP_JUMP_EXIT_VEHICLE_DETACHED",
    "S_STUNT_EXIT_VEHICLE",
    "S_STUNT_EXIT_VEHICLE_P2",
    "S_STUNT_EXIT_VEHICLE_TOO_STEEP",
    "S_STUNT_EXIT_VEHICLE_TOO_STEEP_DETACHED",
    "S_STUNT_TO_FREEFALL_EXIT_VEHICLE_ATTACHED",
    "S_STUNT_TO_FREEFALL_EXIT_VEHICLE_DETACHED",
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use jc3vr_core::hash::hashlittle;

    use super::*;

    fn state(name: &str) -> Option<u32> {
        Some(hashlittle(name.as_bytes()))
    }

    #[test]
    fn states_classify_by_name() {
        assert_eq!(
            classify(state("S_ENTER_DRIVER_RIGHT_MOTORBIKE")),
            SeatPhase::Entering
        );
        assert_eq!(classify(state("S_IDLE_DRIVER_VEHICLE")), SeatPhase::Locked);
        assert_eq!(
            classify(state("S_IDLE_VEHICLE_SPINNING_MOUNTED_WEAPON")),
            SeatPhase::Locked
        );
        assert_eq!(
            classify(state("S_PASS1_TO_DRIVER_VEHICLE")),
            SeatPhase::Shuffle
        );
        assert_eq!(
            classify(state("S_ROLL_OUT_EXIT_LEFT_MOTORBIKE")),
            SeatPhase::Exiting
        );
        assert_eq!(classify(state("S_IDLE")), SeatPhase::Out);
        assert_eq!(classify(None), SeatPhase::Out);
    }

    #[test]
    fn no_state_is_in_two_phases() {
        let names: Vec<&str> = PHASE_STATES
            .iter()
            .flat_map(|(_, names)| names.iter().copied())
            .collect();
        let hashes: HashSet<u32> = names
            .iter()
            .map(|name| hashlittle(name.as_bytes()))
            .collect();
        assert_eq!(hashes.len(), names.len());
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
    fn ride_re_bases_only_the_yaw_and_the_others_re_base_at_the_lock() {
        for event in [
            SeatEvent::EntryBegan,
            SeatEvent::ExitBegan,
            SeatEvent::ExitDone,
        ] {
            assert_eq!(
                action(SeatTransition::Ride, event, 0.3),
                SeatAction::default()
            );
        }
        assert_eq!(
            action(SeatTransition::Ride, SeatEvent::SeatLocked, 0.3),
            SeatAction {
                rebase_yaw: true,
                ..SeatAction::default()
            }
        );
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
