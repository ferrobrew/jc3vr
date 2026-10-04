//! The parachute's handles, standing in for a canopy's toggle lines. Pure.
//!
//! One handle hangs above and outside each shoulder ([`handle_centres`]), and only that side's hand
//! takes it. The hands are not on the handles until the player takes them: like the other grab
//! volumes, a grip press with the hand inside a handle's volume takes it, and it is held only while
//! the grip is: releasing the grip, or moving further than the release distance from it, lets go
//! ([`step`]), as a physical vehicle control does ([`crate::controllers::vehicles::physical`]).
//!
//! A held handle is pulled by how far its hand has come down from where it took the handle, over a
//! deadzone, reaching a full pull at the full-pull distance ([`pull`]). The two pulls steer
//! ([`steer`]) in the move stick's convention (+X right, +Y forward):
//!
//! - their difference banks toward the side pulled further, as one toggle pulled turns a canopy;
//! - the share both are pulled, past the brake threshold, holds the stick back, which the game's
//!   parachute reads as its slow-down (`docs/engine/gameplay/parachute-locomotion.md`, "The stick's
//!   vertical axis").
//!
//! A handle let go, or never taken, is not pulled, so releasing a handle leaves that side neutral.
//! The hands do not dive: pushing the stick forward still does, since the handles' vector is added
//! to the stick's ([`crate::controllers::vehicles::arm_steering::combine`]).
//!
//! Heights are measured in the cockpit frame, whose vertical is the player's own, and the place a
//! handle was taken is kept in that frame, so a head moving while the hands hold still steers
//! nothing. The holding hand is drawn on the handle ([`anchor_point`]): straight below where it was
//! taken, no further down than a full pull and no higher than where it was taken.

use glam::{Vec2, Vec3};

use crate::controllers::{
    bindings::grab::{Candidate, GrabVolume},
    frame::Hand,
    vehicles::{config::ParachuteHandlesConfig, physical::Release},
};

/// A held handle.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HandleHold {
    /// The hand's grip when it took the handle, in the cockpit frame: the handle's rest.
    pub rest: Vec3,
}

/// What each hand holds, indexed by [`Hand::index`].
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HandleState {
    pub holds: [Option<HandleHold>; 2],
}

impl HandleState {
    pub const fn new() -> Self {
        Self { holds: [None; 2] }
    }
}

/// A take or a release, for the log.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HandleEvent {
    Taken(Hand),
    Released(Hand, Release),
}

/// One tick's inputs.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StepInputs {
    /// Each hand's grip in the cockpit frame, `None` while untracked.
    pub grips: [Option<Vec3>; 2],
    /// Whether each hand's grip is held this tick.
    pub held: [bool; 2],
    /// Whether each hand's grip press latched its handle's volume this tick.
    pub grabs: [bool; 2],
}

/// Where the handles' volumes are centred for shoulder joints at `shoulders`, indexed by
/// [`Hand::index`]: each shoulder's offset, mirrored for the left.
pub fn handle_centres(config: &ParachuteHandlesConfig, shoulders: [Vec3; 2]) -> [Vec3; 2] {
    let right = config.offset_m;
    let left = Vec3::new(-right.x, right.y, right.z);
    [shoulders[0] + left, shoulders[1] + right]
}

/// The handles' grab volumes: a free hand's handle at its place by the shoulder, a held one where
/// the hand holds it, each for its own side's hand only.
pub fn candidates(
    config: &ParachuteHandlesConfig,
    state: &HandleState,
    centres: [Vec3; 2],
    grips: [Option<Vec3>; 2],
) -> [Candidate; 2] {
    Hand::BOTH.map(|hand| {
        let i = hand.index();
        let held = state.holds[i].and_then(|hold| Some(anchor_point(config, hold, grips[i]?)));
        Candidate {
            volume: GrabVolume::ParachuteHandle(hand),
            centre: held.unwrap_or(centres[i]),
            radius: config.radius_m,
            hand: Some(hand),
        }
    })
}

/// Advance the handles by one input tick: act on the grabs, and let go of the handles whose hands
/// were lost or moved away. Returns each hand's pull, `0..=1`.
pub fn step(
    state: &mut HandleState,
    config: &ParachuteHandlesConfig,
    inputs: &StepInputs,
    events: &mut Vec<HandleEvent>,
) -> [f32; 2] {
    for hand in Hand::BOTH {
        let i = hand.index();
        let Some(grip) = inputs.grips[i].filter(|g| g.is_finite()) else {
            release(state, hand, Release::Gone, events);
            continue;
        };
        if inputs.grabs[i] {
            // A grab while holding is a release and a fresh press between two ticks: the handle
            // stays taken from where it was.
            if state.holds[i].is_none() {
                state.holds[i] = Some(HandleHold { rest: grip });
                events.push(HandleEvent::Taken(hand));
            }
        } else if !inputs.held[i] {
            release(state, hand, Release::GripReleased, events);
        }
        if let Some(hold) = state.holds[i]
            && anchor_point(config, hold, grip).distance(grip) > config.release_distance_m
        {
            release(state, hand, Release::MovedAway, events);
        }
    }
    Hand::BOTH.map(|hand| {
        let i = hand.index();
        state.holds[i]
            .zip(inputs.grips[i])
            .map_or(0.0, |(hold, grip)| pull(config, hold, grip))
    })
}

/// Let go of both handles, as when the parachute closes or the handles are switched off.
pub fn release_all(state: &mut HandleState, events: &mut Vec<HandleEvent>) {
    for hand in Hand::BOTH {
        release(state, hand, Release::Gone, events);
    }
}

/// How far a handle held by `hold` is pulled with its hand's grip at `grip`, `0..=1`: the drop
/// below where it was taken, over the deadzone, as a share of the rest of a full pull.
pub fn pull(config: &ParachuteHandlesConfig, hold: HandleHold, grip: Vec3) -> f32 {
    let drop = hold.rest.y - grip.y;
    if !drop.is_finite() || drop <= config.deadzone_m {
        return 0.0;
    }
    let span = (config.full_pull_m - config.deadzone_m).max(1e-3);
    ((drop - config.deadzone_m) / span).min(1.0)
}

/// The stick vector the pulls steer with (+X right, +Y forward): the right pull less the left one
/// banks, and the share both are pulled past the brake threshold holds the stick back.
pub fn steer(pulls: [f32; 2], config: &ParachuteHandlesConfig) -> Vec2 {
    let [left, right] = pulls.map(|p| {
        if p.is_finite() {
            p.clamp(0.0, 1.0)
        } else {
            0.0
        }
    });
    let both = left.min(right);
    let threshold = config.brake_threshold.clamp(0.0, 0.95);
    let brake = if both <= threshold {
        0.0
    } else {
        ((both - threshold) / (1.0 - threshold)).min(1.0)
    };
    Vec2::new(right - left, -brake)
}

/// Where the hand holding `hold` sits on its handle for its grip at `grip`: straight below where it
/// took the handle, between there and a full pull further down.
pub fn anchor_point(config: &ParachuteHandlesConfig, hold: HandleHold, grip: Vec3) -> Vec3 {
    let drop = (hold.rest.y - grip.y).clamp(0.0, config.full_pull_m.max(0.0));
    let drop = if drop.is_finite() { drop } else { 0.0 };
    hold.rest - Vec3::Y * drop
}

fn release(state: &mut HandleState, hand: Hand, why: Release, events: &mut Vec<HandleEvent>) {
    if state.holds[hand.index()].take().is_some() {
        events.push(HandleEvent::Released(hand, why));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::{bindings::grab::grab_volume, vehicles::arm_steering::combine};

    const CONFIG: ParachuteHandlesConfig = ParachuteHandlesConfig::new();
    const SHOULDERS: [Vec3; 2] = [Vec3::new(-0.2, -0.2, 0.1), Vec3::new(0.2, -0.2, 0.1)];

    /// A tick with every tracked hand's grip held.
    fn tick(
        state: &mut HandleState,
        grips: [Option<Vec3>; 2],
        grabs: [bool; 2],
    ) -> ([f32; 2], Vec<HandleEvent>) {
        tick_held(state, grips, grabs, grips.map(|g| g.is_some()))
    }

    fn tick_held(
        state: &mut HandleState,
        grips: [Option<Vec3>; 2],
        grabs: [bool; 2],
        held: [bool; 2],
    ) -> ([f32; 2], Vec<HandleEvent>) {
        let mut events = Vec::new();
        let inputs = StepInputs { grips, held, grabs };
        let pulls = step(state, &CONFIG, &inputs, &mut events);
        (pulls, events)
    }

    fn both_taken() -> (HandleState, [Vec3; 2]) {
        let centres = handle_centres(&CONFIG, SHOULDERS);
        let mut state = HandleState::new();
        tick(&mut state, centres.map(Some), [true; 2]);
        (state, centres)
    }

    #[test]
    fn the_handles_sit_above_and_outside_their_shoulders_for_their_own_hand() {
        let centres = handle_centres(&CONFIG, SHOULDERS);
        assert!(centres[0].x < SHOULDERS[0].x && centres[1].x > SHOULDERS[1].x);
        assert!(centres.iter().all(|c| c.y > SHOULDERS[0].y));
        let c = candidates(&CONFIG, &HandleState::new(), centres, [None; 2]);
        assert_eq!(
            grab_volume(centres[1], Hand::Right, &c),
            Some(GrabVolume::ParachuteHandle(Hand::Right))
        );
        assert_eq!(grab_volume(centres[1], Hand::Left, &c), None);
    }

    #[test]
    fn a_handle_is_held_only_once_taken_and_only_while_the_grip_is() {
        let centres = handle_centres(&CONFIG, SHOULDERS);
        let mut state = HandleState::new();
        let down = centres[1] - Vec3::Y * CONFIG.full_pull_m;
        let (pulls, _) = tick(&mut state, [None, Some(down)], [false; 2]);
        assert_eq!(pulls, [0.0; 2], "an untaken handle is not pulled");

        let (_, events) = tick(&mut state, [None, Some(centres[1])], [false, true]);
        assert_eq!(events, vec![HandleEvent::Taken(Hand::Right)]);
        let (pulls, _) = tick(&mut state, [None, Some(down)], [false; 2]);
        assert!((pulls[1] - 1.0).abs() < 1e-5, "{pulls:?}");

        // A release and a press between two ticks keeps the handle where it was taken.
        let (_, events) = tick(&mut state, [None, Some(down)], [false, true]);
        assert!(events.is_empty());
        let (pulls, events) = tick_held(&mut state, [None, Some(down)], [false; 2], [false; 2]);
        assert_eq!(
            events,
            vec![HandleEvent::Released(Hand::Right, Release::GripReleased)]
        );
        assert_eq!(pulls, [0.0; 2], "a released handle is neutral");
    }

    #[test]
    fn a_hand_that_leaves_its_handle_or_tracking_lets_go() {
        let (mut state, centres) = both_taken();
        let away = centres[0] + Vec3::new(0.0, 0.0, 0.5);
        let (_, events) = tick(&mut state, [Some(away), None], [false; 2]);
        assert_eq!(
            events,
            vec![
                HandleEvent::Released(Hand::Left, Release::MovedAway),
                HandleEvent::Released(Hand::Right, Release::Gone),
            ]
        );
        assert_eq!(state, HandleState::new());
    }

    #[test]
    fn the_pull_is_measured_from_where_the_handle_was_taken() {
        let hold = HandleHold { rest: Vec3::ZERO };
        assert_eq!(pull(&CONFIG, hold, Vec3::Y * 0.2), 0.0, "raising is slack");
        assert_eq!(pull(&CONFIG, hold, -Vec3::Y * CONFIG.deadzone_m), 0.0);
        let half = CONFIG.deadzone_m + (CONFIG.full_pull_m - CONFIG.deadzone_m) * 0.5;
        assert!((pull(&CONFIG, hold, -Vec3::Y * half) - 0.5).abs() < 1e-5);
        assert_eq!(pull(&CONFIG, hold, -Vec3::Y), 1.0);
        assert_eq!(pull(&CONFIG, hold, Vec3::NAN), 0.0);
    }

    #[test]
    fn one_handle_banks_to_its_side_and_both_brake() {
        assert_eq!(steer([0.0, 1.0], &CONFIG), Vec2::new(1.0, 0.0));
        assert_eq!(steer([0.6, 0.0], &CONFIG), Vec2::new(-0.6, 0.0));
        assert_eq!(steer([1.0, 1.0], &CONFIG), Vec2::new(0.0, -1.0));
        let light = steer([CONFIG.brake_threshold; 2], &CONFIG);
        assert_eq!(light, Vec2::ZERO, "a light pull on both does not brake");
        let turning_brake = steer([0.6, 1.0], &CONFIG);
        assert!(
            turning_brake.x > 0.0 && turning_brake.y < 0.0,
            "{turning_brake}"
        );
        assert_eq!(steer([f32::NAN, 0.0], &CONFIG), Vec2::ZERO);
    }

    #[test]
    fn the_stick_adds_to_the_handles_and_still_dives() {
        let brake = steer([1.0, 1.0], &CONFIG);
        assert_eq!(combine(Vec2::new(0.0, 1.0), Some(brake)), Vec2::ZERO);
        assert_eq!(
            combine(Vec2::new(0.5, 1.0), Some(Vec2::ZERO)),
            Vec2::new(0.5, 1.0)
        );
        assert_eq!(
            combine(Vec2::new(0.8, 0.0), Some(steer([0.0, 1.0], &CONFIG))),
            Vec2::new(1.0, 0.0)
        );
    }

    #[test]
    fn the_held_hand_is_drawn_below_where_it_took_the_handle_within_a_full_pull() {
        let hold = HandleHold { rest: Vec3::ZERO };
        let a = anchor_point(&CONFIG, hold, Vec3::new(0.05, -0.1, 0.02));
        assert_eq!(a, Vec3::new(0.0, -0.1, 0.0));
        assert_eq!(anchor_point(&CONFIG, hold, Vec3::Y), Vec3::ZERO);
        let low = anchor_point(&CONFIG, hold, -Vec3::Y);
        assert!((low.y + CONFIG.full_pull_m).abs() < 1e-6);
    }
}
