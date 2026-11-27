//! The boost wingsuit's take-off from the ground along the head's heading.
//!
//! The take-off from the ground (`S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND_INTO`, then
//! `..._FROM_GROUND`) launches the suit along the character's facing. In VR the body faces where
//! the grounded rig puts it, under the view, not where the head looks, so the suit left off to one
//! side of the player's gaze. A take-off is a movement action, so it may turn the body: the head's
//! heading relative to the view stands in for the torso's as the body's follow target
//! ([`held_heading`], which the head pose reads on each input tick), and the game's own turn
//! brings the body around under the view, so the view does not move.
//!
//! The game turns the body only while its movement task runs, which it stops for the wind-up, so
//! the turn starts with the hold that leads to it: once the use button (`OPEN_WINGSUIT`, with the
//! other actions on the gamepad's button) has been held on the ground for the tuning's
//! `take_off.turn_after_s`, the follow target tracks the head's heading. On entering the take-off
//! the heading is held where it was until the take-off ends. Letting go of the button before the
//! wind-up lets the target go.

use std::{sync::LazyLock, time::Instant};

use body_rig::math::angle::heading;
use jc3gi::{
    character::character::Character,
    input::input_action_map::{Action, LocalPlayerActionMap},
};
use jc3vr_body::{
    controllers::{
        mode,
        mode::{Mode, OnFootVariant},
    },
    hooks::animation::active_state_type_id,
};
use jc3vr_core::hash::StateSet;
use jc3vr_view::headpose;
use parking_lot::Mutex;

use crate::controllers::vehicles::tuning::VEHICLES;

/// The head's heading relative to the view that the body follows in place of the torso's while
/// the take-off holds it, or `None`: the head pose's participant. [`tick`] is its one writer.
pub fn held_heading() -> Option<f32> {
    *HEADING.lock()
}

/// Track the take-off for this game tick.
///
/// # Safety
///
/// The game thread, outside the world update: reads the body rule system's current state and the
/// local player's action map.
pub(crate) unsafe fn tick() {
    let character = unsafe { Character::GetLocalPlayerCharacter().as_ref() };
    let state = character.and_then(|c| unsafe { active_state_type_id(c) });
    let taking_off = state.is_some_and(is_take_off_state);
    let holding =
        mode::current().mode == Mode::OnFoot(OnFootVariant::Ground) && unsafe { use_held() };
    let mut tracker = TRACKER.lock();
    let edge = tracker.step(holding, taking_off, Instant::now());
    let phase = tracker.phase;
    drop(tracker);
    let head = || headpose::xr::cockpit_pose().map(|pose| heading(pose.orientation));
    match (edge, phase) {
        (Some(Edge::Entered), _) => {
            let heading = head();
            set_held_heading(heading);
            tracing::info!(
                target: "controllers",
                heading_deg = heading.map(f32::to_degrees),
                following = headpose::xr::follow_status().0,
                "wingsuit: the take-off from the ground holds the body on the head's heading"
            );
        }
        (Some(Edge::Left | Edge::Released), _) => set_held_heading(None),
        (Some(Edge::Held), _) => {
            let heading = head();
            set_held_heading(heading);
            tracing::info!(
                target: "controllers",
                heading_deg = heading.map(f32::to_degrees),
                following = headpose::xr::follow_status().0,
                "wingsuit: the use button held on the ground turns the body to the head's heading"
            );
        }
        (None, Phase::Held) => set_held_heading(head()),
        (None, _) => {}
    }
}

/// Set or clear [`held_heading`]; a heading that is not finite clears it.
fn set_held_heading(heading: Option<f32>) {
    *HEADING.lock() = heading.filter(|h| h.is_finite());
}

/// The heading of [`held_heading`].
static HEADING: Mutex<Option<f32>> = Mutex::new(None);

/// A change in the take-off's hold or in whether the take-off runs.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Edge {
    /// The use button has been held on the ground for the tuning's `take_off.turn_after_s`.
    Held,
    /// The button was let go after [`Edge::Held`], without a take-off.
    Released,
    /// The take-off started.
    Entered,
    /// The take-off ended.
    Left,
}

/// Where the take-off stands.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(crate) enum Phase {
    #[default]
    Idle,
    /// The use button is down on the ground, since the instant held.
    Pressed(Instant),
    /// The use button has been down long enough to turn the body.
    Held,
    TakingOff,
}

/// The take-off's phase across ticks.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(crate) struct Tracker {
    phase: Phase,
}

impl Tracker {
    /// Advance one tick, with `holding` whether the use button is down on the ground and
    /// `taking_off` whether the take-off runs, and return the change.
    pub(crate) fn step(&mut self, holding: bool, taking_off: bool, now: Instant) -> Option<Edge> {
        let was = self.phase;
        self.phase = match (taking_off, holding, was) {
            (true, _, _) => Phase::TakingOff,
            (false, true, Phase::Pressed(since))
                if now.saturating_duration_since(since) >= VEHICLES.get().take_off.turn_after() =>
            {
                Phase::Held
            }
            (false, true, Phase::Pressed(since)) => Phase::Pressed(since),
            (false, true, Phase::Held) => Phase::Held,
            (false, true, Phase::Idle | Phase::TakingOff) => Phase::Pressed(now),
            (false, false, _) => Phase::Idle,
        };
        match (was, self.phase) {
            (Phase::TakingOff, Phase::TakingOff) => None,
            (_, Phase::TakingOff) => Some(Edge::Entered),
            (Phase::TakingOff, _) => Some(Edge::Left),
            (Phase::Held, Phase::Held) => None,
            (_, Phase::Held) => Some(Edge::Held),
            (Phase::Held, _) => Some(Edge::Released),
            _ => None,
        }
    }
}

static TRACKER: Mutex<Tracker> = Mutex::new(Tracker { phase: Phase::Idle });

/// Whether the local player's `OPEN_WINGSUIT` action is down, from whichever device drives it.
///
/// # Safety
///
/// The game thread, outside the world update.
unsafe fn use_held() -> bool {
    let Some(map) = (unsafe { LocalPlayerActionMap::get() }) else {
        return false;
    };
    unsafe { map.GetActionEffector(Action::OPEN_WINGSUIT).as_ref() }
        .is_some_and(|effector| effector.m_Value >= HELD_VALUE)
}

/// The effector value at which the action counts as down: half of a full press.
const HELD_VALUE: f32 = 0.5;

/// The rule states of the take-off from the ground.
pub const TAKE_OFF_STATES: [&str; 2] = [
    "S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND_INTO",
    "S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND",
];

/// Whether the rule state `state` is the take-off from the ground.
fn is_take_off_state(state: u32) -> bool {
    static STATES: LazyLock<StateSet> = LazyLock::new(|| StateSet::new(TAKE_OFF_STATES));
    STATES.contains(state)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use jc3vr_core::hash::hashlittle;

    use super::*;

    fn after(start: Instant, s: f32) -> Instant {
        start + Duration::from_secs_f32(s)
    }

    #[test]
    fn a_hold_turns_the_body_and_the_take_off_keeps_the_heading_until_it_ends() {
        let start = Instant::now();
        let mut tracker = Tracker::default();
        assert_eq!(tracker.step(false, false, start), None);
        assert_eq!(tracker.step(true, false, after(start, 0.1)), None);
        assert_eq!(tracker.step(true, false, after(start, 0.2)), None);
        assert_eq!(
            tracker.step(
                true,
                false,
                after(start, 0.11 + VEHICLES.get().take_off.turn_after_s)
            ),
            Some(Edge::Held)
        );
        assert_eq!(tracker.step(true, false, after(start, 0.8)), None);
        assert_eq!(tracker.phase, Phase::Held);
        assert_eq!(
            tracker.step(true, true, after(start, 1.1)),
            Some(Edge::Entered)
        );
        // The wind-up and the launch are one take-off, the button held through it or not.
        assert_eq!(tracker.step(true, true, after(start, 1.5)), None);
        assert_eq!(tracker.step(false, true, after(start, 2.0)), None);
        assert_eq!(
            tracker.step(false, false, after(start, 2.6)),
            Some(Edge::Left)
        );
    }

    #[test]
    fn a_tap_does_not_turn_and_letting_go_before_the_wind_up_lets_the_heading_go() {
        let start = Instant::now();
        let mut tracker = Tracker::default();
        assert_eq!(tracker.step(true, false, start), None);
        assert_eq!(tracker.step(true, false, after(start, 0.1)), None);
        assert_eq!(tracker.step(false, false, after(start, 0.15)), None);
        assert_eq!(tracker.phase, Phase::Idle);
        assert_eq!(tracker.step(true, false, after(start, 1.0)), None);
        assert_eq!(
            tracker.step(
                true,
                false,
                after(start, 1.01 + VEHICLES.get().take_off.turn_after_s)
            ),
            Some(Edge::Held)
        );
        assert_eq!(
            tracker.step(false, false, after(start, 1.5)),
            Some(Edge::Released)
        );
    }

    #[test]
    fn a_take_off_without_the_hold_seen_still_turns_on_entry() {
        let start = Instant::now();
        let mut tracker = Tracker::default();
        assert_eq!(tracker.step(false, true, start), Some(Edge::Entered));
        assert_eq!(
            tracker.step(false, false, after(start, 1.0)),
            Some(Edge::Left)
        );
    }

    #[test]
    fn both_take_off_states_count_and_others_do_not() {
        assert!(is_take_off_state(hashlittle(
            b"S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND_INTO"
        )));
        assert!(is_take_off_state(hashlittle(
            b"S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND"
        )));
        assert!(!is_take_off_state(hashlittle(b"S_IDLE")));
    }
}
