//! The parachute's handles, standing in for a canopy's toggle lines. Pure.
//!
//! One handle hangs a little above, ahead of, and outside each shoulder, where the game's
//! parachute rig places it ([`crate::controllers::vehicles::parachute_rig`]), and only that side's
//! hand takes it. The hands are not on the handles until the player takes them: like the other grab
//! volumes, a grip press with the hand inside a handle's volume takes it, and it is held only while
//! the grip is: only releasing the grip lets go ([`step`]), however far the hand goes from the
//! handle, so a pull that runs down and back toward the body keeps it. The volumes are generous,
//! shrunk only so that the two handles' volumes do not overlap ([`HandleRadii`]); where a handle's
//! volume overlaps a body volume, the nearer one takes the press, so a press right at a shoulder
//! socket still draws its weapon. Around each handle a wider reach ([`reach_candidates`]) takes the
//! handle for a press that no other volume claims, so that a hand that misses the handle by a
//! little takes it rather than firing the grapple.
//!
//! Each handle is marked while the handles act ([`marks`]): faintly out of reach, brighter in reach,
//! and lit while held, so the player can find them.
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
    frame::Hand,
    grab::{Candidate, GrabVolume},
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

/// How far from a handle's centre a hand's grip may be for a grip press to take it, metres, where the
/// handles are far enough apart ([`HandleRadii`]). Within it, the handle and any other target in
/// reach go to the nearer, so a press right at a shoulder socket takes the socket.
pub const RADIUS_M: f32 = 0.20;

/// How far from a handle's centre a grip press outside every other volume still takes it rather
/// than firing the grapple, metres, where the handles are far enough apart. At least [`RADIUS_M`].
pub const REACH_M: f32 = 0.43;

/// The smallest a handle's volume shrinks to for handles resting close together, metres.
pub const MIN_RADIUS_M: f32 = 0.10;

/// The free handles' radii: [`RADIUS_M`] and [`REACH_M`], each shrunk to half the handles' spacing
/// while both rest where they can be taken, so that neither handle's volume reaches over the
/// other's, but no smaller than [`MIN_RADIUS_M`].
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HandleRadii {
    pub radius: f32,
    pub reach: f32,
}

impl HandleRadii {
    /// The radii for free handles resting at `centres`, `None` for a side whose rest is not known.
    pub fn of(centres: [Option<Vec3>; 2]) -> Self {
        let half_spacing = match centres {
            [Some(left), Some(right)] => left.distance(right) * 0.5,
            _ => f32::INFINITY,
        };
        let cap = if half_spacing.is_nan() {
            f32::INFINITY
        } else {
            half_spacing.max(MIN_RADIUS_M)
        };
        Self {
            radius: RADIUS_M.min(cap),
            reach: REACH_M.min(cap),
        }
    }
}

/// Pulls shorter than this act on nothing, metres.
pub const DEADZONE_M: f32 = 0.03;

/// The handles' grab volumes: a free hand's handle at its place by the shoulder, a held one where
/// the hand holds it, each for its own side's hand only.
pub fn candidates(
    config: &ParachuteHandlesConfig,
    state: &HandleState,
    centres: [Vec3; 2],
    grips: [Option<Vec3>; 2],
    radii: HandleRadii,
) -> [Candidate; 2] {
    Hand::BOTH.map(|hand| {
        let i = hand.index();
        let held = state.holds[i].and_then(|hold| Some(anchor_point(config, hold, grips[i]?)));
        Candidate {
            volume: GrabVolume::ParachuteHandle(hand),
            centre: held.unwrap_or(centres[i]),
            radius: radii.radius,
            hand: Some(hand),
        }
    })
}

/// The handles' wider reach: the free handles' volumes grown to the reach, which take a press that
/// no other volume claims.
pub fn reach_candidates(
    state: &HandleState,
    centres: [Vec3; 2],
    radii: HandleRadii,
) -> impl Iterator<Item = Candidate> {
    Hand::BOTH
        .into_iter()
        .filter(move |hand| state.holds[hand.index()].is_none())
        .map(move |hand| Candidate {
            volume: GrabVolume::ParachuteHandle(hand),
            centre: centres[hand.index()],
            radius: radii.reach,
            hand: Some(hand),
        })
}

/// Advance the handles by one input tick: act on the grabs, and let go of the handles whose grips
/// were released or whose hands were lost. Returns each hand's pull, `0..=1`.
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
    if !drop.is_finite() || drop <= DEADZONE_M {
        return 0.0;
    }
    let span = (config.full_pull_m - DEADZONE_M).max(1e-3);
    ((drop - DEADZONE_M) / span).min(1.0)
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
    use crate::controllers::{
        grab::grab_volume, loadout::Socket, vehicles::arm_steering::combine,
        weapons::volumes::SOCKETS,
    };

    const CONFIG: ParachuteHandlesConfig = ParachuteHandlesConfig::new();

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
        let pulls = step(state, &CONFIG, &inputs, &mut events);
        (pulls, events)
    }

    fn both_taken() -> (HandleState, [Vec3; 2]) {
        let centres = centres();
        let mut state = HandleState::new();
        tick(&mut state, centres.map(Some), [true; 2]);
        (state, centres)
    }

    fn radii() -> HandleRadii {
        HandleRadii::of(centres().map(Some))
    }

    #[test]
    fn each_handle_is_for_its_own_hand() {
        let centres = centres();
        let c = candidates(&CONFIG, &HandleState::new(), centres, [None; 2], radii());
        assert_eq!(
            grab_volume(centres[1], Hand::Right, &c),
            Some(GrabVolume::ParachuteHandle(Hand::Right))
        );
        assert_eq!(grab_volume(centres[1], Hand::Left, &c), None);
        // A hand reaching across the body to the other side's handle takes nothing.
        let reach: Vec<_> = reach_candidates(&HandleState::new(), centres, radii()).collect();
        assert_eq!(grab_volume(centres[0], Hand::Right, &reach), None);
        assert_eq!(grab_volume(centres[1], Hand::Left, &reach), None);
    }

    #[test]
    fn the_handles_are_generous_without_overlapping_each_other() {
        // Far apart, the volumes are their full size, well past a hand's width.
        let wide = HandleRadii::of([Some(Vec3::X * -1.0), Some(Vec3::X)]);
        assert_eq!(
            wide,
            HandleRadii {
                radius: RADIUS_M,
                reach: REACH_M
            }
        );
        // At the usual spacing, the reach shrinks to meet the other handle's, and no further.
        let spacing = centres()[0].distance(centres()[1]);
        let usual = radii();
        assert_eq!(usual.radius, RADIUS_M);
        assert!(usual.reach * 2.0 <= spacing + 1e-6, "{usual:?}");
        assert!((usual.reach * 2.0 - spacing).abs() < 1e-5, "{usual:?}");
        // Handles resting nearly together keep a usable volume.
        let close = HandleRadii::of([Some(Vec3::ZERO), Some(Vec3::X * 0.02)]);
        assert_eq!(close.radius, MIN_RADIUS_M);
        assert_eq!(close.reach, MIN_RADIUS_M);
        // With one side's rest unknown, nothing shrinks the other.
        let one = HandleRadii::of([None, Some(Vec3::X)]);
        assert_eq!(
            one,
            HandleRadii {
                radius: RADIUS_M,
                reach: REACH_M
            }
        );
    }

    #[test]
    fn a_press_at_a_shoulder_socket_takes_the_socket_over_the_handle() {
        let centres = centres();
        let socket = Candidate {
            volume: GrabVolume::Socket(Socket::Shoulder(Hand::Right)),
            centre: centres[1] + Vec3::new(-0.10, -0.10, 0.04),
            radius: SOCKETS.shoulder_radius_m,
            hand: Some(Hand::Right),
        };
        assert!(socket.centre.distance(centres[1]) < RADIUS_M);
        let mut c = candidates(&CONFIG, &HandleState::new(), centres, [None; 2], radii()).to_vec();
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
        let radii = radii();
        let near = centres[1] + Vec3::X * (radii.radius + radii.reach) * 0.5;
        let placed = candidates(&CONFIG, &state, centres, [None; 2], radii);
        assert_eq!(grab_volume(near, Hand::Right, &placed), None);
        let reach: Vec<_> = reach_candidates(&state, centres, radii).collect();
        assert_eq!(
            grab_volume(near, Hand::Right, &reach),
            Some(GrabVolume::ParachuteHandle(Hand::Right))
        );
        assert_eq!(grab_volume(near, Hand::Left, &reach), None);
        let far = centres[1] + Vec3::X * radii.reach * 1.1;
        assert_eq!(grab_volume(far, Hand::Right, &reach), None);
        // A held handle has no reach of its own: its hand is in its volume already.
        let (state, _) = both_taken();
        assert_eq!(reach_candidates(&state, centres, radii).count(), 0);
    }

    #[test]
    fn a_handle_is_held_only_once_taken_and_only_while_the_grip_is() {
        let centres = centres();
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
    fn a_natural_pull_down_and_back_keeps_the_handle() {
        let (mut state, centres) = both_taken();
        // Down a full pull and back toward the body by a third of a metre, and out a little.
        let pulled = centres[1] + Vec3::new(0.08, -CONFIG.full_pull_m, 0.3);
        let (pulls, events) = tick(&mut state, [Some(centres[0]), Some(pulled)], [false; 2]);
        assert!(events.is_empty(), "{events:?}");
        assert!((pulls[1] - 1.0).abs() < 1e-5, "{pulls:?}");
        // Further down than a full pull and further back still.
        let deep = centres[1] + Vec3::new(0.0, -CONFIG.full_pull_m - 0.2, 0.4);
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
        assert_eq!(pull(&CONFIG, hold, Vec3::Y * 0.2), 0.0, "raising is slack");
        assert_eq!(pull(&CONFIG, hold, -Vec3::Y * DEADZONE_M), 0.0);
        let half = DEADZONE_M + (CONFIG.full_pull_m - DEADZONE_M) * 0.5;
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
