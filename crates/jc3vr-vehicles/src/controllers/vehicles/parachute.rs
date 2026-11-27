//! The parachute's handles, standing in for a canopy's toggle lines. Pure.
//!
//! One handle hangs a little above, ahead of, and outside each shoulder, where the game's
//! parachute rig places it ([`crate::controllers::vehicles::parachute_rig`]), and only that side's
//! hand takes it. The hands are not on the handles until the player takes them: like the other grab
//! volumes, a grip press with the hand inside a handle's volume takes it, and it is held only while
//! the grip is: only releasing the grip lets go ([`step`]), however far the hand goes from the
//! handle, so a pull that runs down and back toward the body keeps it. The volumes are generous,
//! and at the usual spacing the two handles' volumes overlap. That is harmless: each handle is a
//! target for its own side's hand alone, so no hand can take the wrong one. Where a handle's volume
//! overlaps a body volume, the nearer one takes the press, so a press right at a shoulder socket
//! still draws its weapon. Around each handle a wider reach ([`reach_candidates`]) takes the
//! handle for a press that no other volume claims, so that a hand that misses the handle by a
//! little takes it rather than firing the grapple.
//!
//! Each handle is marked while the handles act ([`marks`]): faintly out of reach, brighter in reach,
//! and lit while held, so the player can find them.
//!
//! A held handle is pulled by how far its hand has come down from where it took the handle, over a
//! deadzone, reaching a full pull at the full-pull distance, and pushed by how far it has gone up
//! or forward the same way ([`pull`], signed: a pull positive, a push negative). The two steer
//! ([`steer`]) in the move stick's convention (+X right, +Y forward), as a skydiver's toggles and
//! front risers do:
//!
//! - the difference of the pulls banks toward the side pulled further, as one toggle pulled turns
//!   a canopy;
//! - the share both are pulled, past the brake threshold, holds the stick back, which the game's
//!   parachute reads as its slow-down (`docs/engine/gameplay/parachute-locomotion.md`, "The stick's
//!   vertical axis"), as both toggles pulled brake a canopy;
//! - the share both are pushed up or forward pushes the stick forward, which the game's parachute
//!   reads as its dive, as both front risers pulled down steepen a canopy's glide.
//!
//! A handle let go, or never taken, is neither pulled nor pushed, so releasing a handle leaves that
//! side neutral. The handles' vector is added to the stick's
//! ([`crate::controllers::vehicles::arm_steering::combine`]).
//!
//! Heights are measured in the cockpit frame, whose vertical is the player's own, and the place a
//! handle was taken is kept in that frame, so a head moving while the hands hold still steers
//! nothing. The holding hand is drawn on the handle ([`anchor_point`]): where it was taken, moved
//! down, up, or forward with the hand by no more than a full pull.

use body_rig::math::axis::past_deadzone;
use glam::{Vec2, Vec3};
use jc3vr_body::controllers::grab::{Candidate, GrabVolume};
use jc3vr_input::frame::Hand;

use crate::controllers::vehicles::{
    physical::Release,
    tuning::{ParachuteHandlesTuning, VEHICLES},
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

/// The handles' grab volumes: a free hand's handle at its place by the shoulder, a held one where
/// the hand holds it, each for its own side's hand only.
pub fn candidates(
    config: &ParachuteHandlesTuning,
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

/// The handles' wider reach: the free handles' volumes grown to the reach, which take a press that
/// no other volume claims.
pub(crate) fn reach_candidates(
    state: &HandleState,
    centres: [Vec3; 2],
) -> impl Iterator<Item = Candidate> {
    let radius = VEHICLES.get().parachute_handles.reach_m;
    Hand::BOTH
        .into_iter()
        .filter(move |hand| state.holds[hand.index()].is_none())
        .map(move |hand| Candidate {
            volume: GrabVolume::ParachuteHandle(hand),
            centre: centres[hand.index()],
            radius,
            hand: Some(hand),
        })
}

/// Advance the handles by one input tick: act on the grabs, and let go of the handles whose grips
/// were released or whose hands were lost. Returns each hand's signed pull, `-1..=1` ([`pull`]).
pub fn step(
    state: &mut HandleState,
    config: &ParachuteHandlesTuning,
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

/// How far a handle held by `hold` is pulled with its hand's grip at `grip`, `-1..=1`: the drop
/// below where it was taken, over the deadzone, as a share of the rest of a full pull, positive;
/// or, with no drop, the rise above it or the travel forward of it (the cockpit's -Z), whichever is
/// further, the same way, negative.
pub fn pull(config: &ParachuteHandlesTuning, hold: HandleHold, grip: Vec3) -> f32 {
    if !grip.is_finite() {
        return 0.0;
    }
    let share = |travel: f32| past_deadzone(travel.max(0.0), config.deadzone_m, config.full_pull_m);
    let drop = share(hold.rest.y - grip.y);
    if drop > 0.0 {
        return drop;
    }
    -share((grip.y - hold.rest.y).max(hold.rest.z - grip.z))
}

/// The stick vector the pulls steer with (+X right, +Y forward): the right pull less the left one
/// banks, the share both are pulled past the brake threshold holds the stick back, and the share
/// both are pushed pushes it forward.
pub fn steer(pulls: [f32; 2], config: &ParachuteHandlesTuning) -> Vec2 {
    let [left, right] = pulls.map(|p| {
        if p.is_finite() {
            p.clamp(-1.0, 1.0)
        } else {
            0.0
        }
    });
    let both = left.max(0.0).min(right.max(0.0));
    let threshold = config.brake_threshold.clamp(0.0, 0.95);
    let brake = if both <= threshold {
        0.0
    } else {
        ((both - threshold) / (1.0 - threshold)).min(1.0)
    };
    let dive = (-left).max(0.0).min((-right).max(0.0));
    Vec2::new(right.max(0.0) - left.max(0.0), dive - brake)
}

/// Where the hand holding `hold` sits on its handle for its grip at `grip`: where it took the
/// handle, moved down, up, or forward with the hand by no more than a full pull.
pub(crate) fn anchor_point(config: &ParachuteHandlesTuning, hold: HandleHold, grip: Vec3) -> Vec3 {
    let full = config.full_pull_m.max(0.0);
    let offset = grip - hold.rest;
    if !offset.is_finite() {
        return hold.rest;
    }
    hold.rest + Vec3::new(0.0, offset.y.clamp(-full, full), offset.z.clamp(-full, 0.0))
}

fn release(state: &mut HandleState, hand: Hand, why: Release, events: &mut Vec<HandleEvent>) {
    if state.holds[hand.index()].take().is_some() {
        events.push(HandleEvent::Released(hand, why));
    }
}

#[cfg(test)]
mod tests {
    use jc3vr_body::controllers::grab::grab_volume;
    use jc3vr_weapons::controllers::{loadout::Socket, weapons::hold::tuning::HOLD};

    use super::*;
    use crate::controllers::vehicles::arm_steering::combine;

    fn config() -> &'static ParachuteHandlesTuning {
        &VEHICLES.get().parachute_handles
    }

    /// The handles' centres: above, ahead of, and outside the shoulders.
    fn centres() -> [Vec3; 2] {
        [
            Vec3::new(-0.35, -0.08, -0.02),
            Vec3::new(0.35, -0.08, -0.02),
        ]
    }

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
        let pulls = step(state, config(), &inputs, &mut events);
        (pulls, events)
    }

    fn both_taken() -> (HandleState, [Vec3; 2]) {
        let centres = centres();
        let mut state = HandleState::new();
        tick(&mut state, centres.map(Some), [true; 2]);
        (state, centres)
    }

    #[test]
    fn each_handle_is_for_its_own_hand() {
        let centres = centres();
        let c = candidates(config(), &HandleState::new(), centres, [None; 2]);
        assert_eq!(
            grab_volume(centres[1], Hand::Right, &c),
            Some(GrabVolume::ParachuteHandle(Hand::Right))
        );
        assert_eq!(grab_volume(centres[1], Hand::Left, &c), None);
        // A hand reaching across the body to the other side's handle takes nothing.
        let reach: Vec<_> = reach_candidates(&HandleState::new(), centres).collect();
        assert_eq!(grab_volume(centres[0], Hand::Right, &reach), None);
        assert_eq!(grab_volume(centres[1], Hand::Left, &reach), None);
    }

    /// The handles keep their full size however close together they rest, and their volumes
    /// overlap at the usual spacing: each is its own hand's alone, so a hand anywhere in the
    /// overlap takes only its own side's handle.
    #[test]
    fn the_handles_overlap_harmlessly_at_full_size() {
        let centres = centres();
        let c = candidates(config(), &HandleState::new(), centres, [None; 2]);
        let reach: Vec<_> = reach_candidates(&HandleState::new(), centres).collect();
        assert!(c.iter().all(|c| c.radius == config().radius_m));
        assert!(reach.iter().all(|c| c.radius == config().reach_m));
        assert!(config().reach_m * 2.0 > centres[0].distance(centres[1]));
        let middle = (centres[0] + centres[1]) * 0.5;
        assert_eq!(
            grab_volume(middle, Hand::Left, &reach),
            Some(GrabVolume::ParachuteHandle(Hand::Left))
        );
        assert_eq!(
            grab_volume(middle, Hand::Right, &reach),
            Some(GrabVolume::ParachuteHandle(Hand::Right))
        );
    }

    #[test]
    fn a_press_at_a_shoulder_socket_takes_the_socket_over_the_handle() {
        let centres = centres();
        let socket = Candidate {
            volume: GrabVolume::Socket(Socket::Shoulder(Hand::Right)),
            centre: centres[1] + Vec3::new(-0.10, -0.10, 0.04),
            radius: HOLD.get().volumes.sockets.shoulder_radius_m,
            hand: Some(Hand::Right),
        };
        assert!(socket.centre.distance(centres[1]) < config().radius_m);
        let mut c = candidates(config(), &HandleState::new(), centres, [None; 2]).to_vec();
        c.push(socket);
        assert_eq!(
            grab_volume(socket.centre, Hand::Right, &c),
            Some(socket.volume)
        );
        assert_eq!(
            grab_volume(centres[1], Hand::Right, &c),
            Some(GrabVolume::ParachuteHandle(Hand::Right))
        );
    }

    #[test]
    fn a_press_near_a_free_handle_reaches_it_but_not_over_the_body() {
        let centres = centres();
        let state = HandleState::new();
        let near = centres[1] + Vec3::X * (config().radius_m + config().reach_m) * 0.5;
        let placed = candidates(config(), &state, centres, [None; 2]);
        assert_eq!(grab_volume(near, Hand::Right, &placed), None);
        let reach: Vec<_> = reach_candidates(&state, centres).collect();
        assert_eq!(
            grab_volume(near, Hand::Right, &reach),
            Some(GrabVolume::ParachuteHandle(Hand::Right))
        );
        assert_eq!(grab_volume(near, Hand::Left, &reach), None);
        let far = centres[1] + Vec3::X * config().reach_m * 1.1;
        assert_eq!(grab_volume(far, Hand::Right, &reach), None);
        // A held handle has no reach of its own: its hand is in its volume already.
        let (state, _) = both_taken();
        assert_eq!(reach_candidates(&state, centres).count(), 0);
    }

    #[test]
    fn a_handle_is_held_only_once_taken_and_only_while_the_grip_is() {
        let centres = centres();
        let mut state = HandleState::new();
        let down = centres[1] - Vec3::Y * config().full_pull_m;
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
    fn a_natural_pull_down_and_back_keeps_the_handle() {
        let (mut state, centres) = both_taken();
        // Down a full pull and back toward the body by a third of a metre, and out a little.
        let pulled = centres[1] + Vec3::new(0.08, -config().full_pull_m, 0.3);
        let (pulls, events) = tick(&mut state, [Some(centres[0]), Some(pulled)], [false; 2]);
        assert!(events.is_empty(), "{events:?}");
        assert!((pulls[1] - 1.0).abs() < 1e-5, "{pulls:?}");
        // Further down than a full pull and further back still.
        let deep = centres[1] + Vec3::new(0.0, -config().full_pull_m - 0.2, 0.4);
        let (_, events) = tick(&mut state, [Some(centres[0]), Some(deep)], [false; 2]);
        assert!(events.is_empty(), "{events:?}");
    }

    #[test]
    fn a_hand_far_from_its_handle_keeps_it_and_one_that_loses_tracking_lets_go() {
        let (mut state, centres) = both_taken();
        let away = centres[0] + Vec3::new(0.0, -0.1, 1.0);
        let (pulls, events) = tick(&mut state, [Some(away), None], [false; 2]);
        assert_eq!(
            events,
            vec![HandleEvent::Released(Hand::Right, Release::Gone)]
        );
        assert!(state.holds[0].is_some());
        assert!(pulls[0] > 0.0, "{pulls:?}");
        let (_, events) = tick_held(&mut state, [Some(away), None], [false; 2], [false; 2]);
        assert_eq!(
            events,
            vec![HandleEvent::Released(Hand::Left, Release::GripReleased)]
        );
        assert_eq!(state, HandleState::new());
    }

    #[test]
    fn the_pull_is_measured_from_where_the_handle_was_taken() {
        let hold = HandleHold { rest: Vec3::ZERO };
        assert_eq!(
            pull(config(), hold, Vec3::Y * config().deadzone_m),
            0.0,
            "a small raise is slack"
        );
        assert!(pull(config(), hold, Vec3::Y * 0.2) < 0.0, "raising pushes");
        assert!(
            pull(config(), hold, Vec3::NEG_Z * 0.2) < 0.0,
            "reaching forward pushes"
        );
        assert_eq!(pull(config(), hold, Vec3::Y), -1.0);
        assert_eq!(pull(config(), hold, -Vec3::Y * config().deadzone_m), 0.0);
        let half = config().deadzone_m + (config().full_pull_m - config().deadzone_m) * 0.5;
        assert!((pull(config(), hold, -Vec3::Y * half) - 0.5).abs() < 1e-5);
        assert_eq!(pull(config(), hold, -Vec3::Y), 1.0);
        assert_eq!(pull(config(), hold, Vec3::NAN), 0.0);
    }

    #[test]
    fn one_handle_banks_to_its_side_and_both_brake() {
        assert_eq!(steer([0.0, 1.0], config()), Vec2::new(1.0, 0.0));
        assert_eq!(steer([0.6, 0.0], config()), Vec2::new(-0.6, 0.0));
        assert_eq!(steer([1.0, 1.0], config()), Vec2::new(0.0, -1.0));
        let light = steer([config().brake_threshold; 2], config());
        assert_eq!(light, Vec2::ZERO, "a light pull on both does not brake");
        let turning_brake = steer([0.6, 1.0], config());
        assert!(
            turning_brake.x > 0.0 && turning_brake.y < 0.0,
            "{turning_brake}"
        );
        assert_eq!(steer([f32::NAN, 0.0], config()), Vec2::ZERO);
    }

    #[test]
    fn both_handles_pushed_dive_and_one_alone_does_nothing() {
        assert_eq!(steer([-1.0, -1.0], config()), Vec2::new(0.0, 1.0));
        assert_eq!(steer([-0.5, -0.8], config()), Vec2::new(0.0, 0.5));
        assert_eq!(steer([-1.0, 0.0], config()), Vec2::ZERO);
        let turning_dive = steer([-0.6, 0.0], config());
        assert_eq!(
            turning_dive,
            Vec2::ZERO,
            "a push on one side neither banks nor dives"
        );
    }

    #[test]
    fn the_stick_adds_to_the_handles_and_still_dives() {
        let brake = steer([1.0, 1.0], config());
        assert_eq!(combine(Vec2::new(0.0, 1.0), Some(brake)), Vec2::ZERO);
        assert_eq!(
            combine(Vec2::new(0.5, 1.0), Some(Vec2::ZERO)),
            Vec2::new(0.5, 1.0)
        );
        assert_eq!(
            combine(Vec2::new(0.8, 0.0), Some(steer([0.0, 1.0], config()))),
            Vec2::new(1.0, 0.0)
        );
    }

    #[test]
    fn the_held_hand_is_drawn_where_it_took_the_handle_moved_within_a_full_pull() {
        let hold = HandleHold { rest: Vec3::ZERO };
        let a = anchor_point(config(), hold, Vec3::new(0.05, -0.1, 0.02));
        assert_eq!(a, Vec3::new(0.0, -0.1, 0.0));
        let high = anchor_point(config(), hold, Vec3::Y);
        assert!((high.y - config().full_pull_m).abs() < 1e-6);
        let ahead = anchor_point(config(), hold, Vec3::NEG_Z * 0.1);
        assert_eq!(ahead, Vec3::NEG_Z * 0.1);
        let low = anchor_point(config(), hold, -Vec3::Y);
        assert!((low.y + config().full_pull_m).abs() < 1e-6);
    }
}
