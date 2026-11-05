//! The boost wingsuit's take-off from the ground along the head's heading.
//!
//! The take-off from the ground (`S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND_INTO`, then
//! `..._FROM_GROUND`) launches the suit along the character's facing. In VR the body faces where the
//! grounded rig puts it, under the view, not where the head looks, so the suit left off to one side
//! of the player's gaze. A take-off is a movement action, so it may turn the body: on entering the
//! take-off, the head's heading relative to the view is captured and stands in for the torso's as
//! the body's follow target ([`crate::headpose::xr::set_take_off_heading`]) until the take-off ends.
//! The game's own turn then brings the body around during the wind-up, under the view, so the view
//! does not move.

use std::sync::OnceLock;

use jc3gi::{character::character::Character, hash::hashlittle};
use parking_lot::Mutex;

use crate::{
    headpose::{self, sim::body_yaw_of},
    hooks::animation::active_state_type_id,
};

/// Track the take-off for this game tick, with `enabled` from the debug UI.
///
/// # Safety
///
/// The game thread, outside the world update: reads the body rule system's current state.
pub(crate) unsafe fn tick(enabled: bool) {
    let character = unsafe { Character::GetLocalPlayerCharacter().as_ref() };
    let state = character.and_then(|c| unsafe { active_state_type_id(c) });
    let taking_off = enabled && state.is_some_and(is_take_off_state);
    let edge = TRACKER.lock().step(taking_off);
    match edge {
        Some(Edge::Entered) => {
            let heading = headpose::xr::cockpit_pose().map(|pose| body_yaw_of(pose.orientation));
            headpose::xr::set_take_off_heading(heading);
            tracing::info!(
                target: "controllers",
                heading_deg = heading.map(f32::to_degrees),
                following = headpose::xr::follow_status().0,
                "wingsuit: the take-off from the ground turns the body to the head's heading"
            );
        }
        Some(Edge::Left) => headpose::xr::set_take_off_heading(None),
        None => {}
    }
}

/// A change in whether the take-off runs.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Edge {
    Entered,
    Left,
}

/// Whether the take-off ran on the last tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(crate) struct Tracker {
    taking_off: bool,
}

impl Tracker {
    /// Advance one tick on which the take-off runs or not, and return the change.
    pub(crate) fn step(&mut self, taking_off: bool) -> Option<Edge> {
        let was = std::mem::replace(&mut self.taking_off, taking_off);
        match (was, taking_off) {
            (false, true) => Some(Edge::Entered),
            (true, false) => Some(Edge::Left),
            _ => None,
        }
    }
}

static TRACKER: Mutex<Tracker> = Mutex::new(Tracker { taking_off: false });

/// The rule states of the take-off from the ground.
const TAKE_OFF_STATES: [&str; 2] = [
    "S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND_INTO",
    "S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND",
];

/// Whether the rule state `state` is the take-off from the ground.
fn is_take_off_state(state: u32) -> bool {
    static HASHES: OnceLock<[u32; 2]> = OnceLock::new();
    HASHES
        .get_or_init(|| take_off_hashes(|name| hashlittle(name.as_bytes()) as u32))
        .contains(&state)
}

/// The take-off's state ids, by `hash` of their names (the game's hash in play).
fn take_off_hashes(hash: impl Fn(&str) -> u32) -> [u32; 2] {
    TAKE_OFF_STATES.map(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_take_off_turns_once_on_entry_and_lets_go_on_exit() {
        let mut tracker = Tracker::default();
        assert_eq!(tracker.step(false), None);
        assert_eq!(tracker.step(true), Some(Edge::Entered));
        // The wind-up and the launch are one take-off.
        assert_eq!(tracker.step(true), None);
        assert_eq!(tracker.step(false), Some(Edge::Left));
    }

    /// A stand-in for the game's name hash, which lives in the game's code.
    fn hash(name: &str) -> u32 {
        name.bytes().fold(0x811C_9DC5, |h, b| {
            (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
        })
    }

    #[test]
    fn both_take_off_states_count_and_others_do_not() {
        let hashes = take_off_hashes(hash);
        assert!(hashes.contains(&hash("S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND_INTO")));
        assert!(hashes.contains(&hash("S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND")));
        assert!(!hashes.contains(&hash("S_IDLE")));
    }
}
