//! The ripcord: a handle on the player's own body, at the hip on the dominant side, behind the
//! holster, taken with the grip and pulled out to open the parachute from a fall or the wingsuit.
//! Pure.
//!
//! It is placed on the tracked body rather than on Rico's: in the wingsuit and a fall Rico flies
//! prone under the view, so his drawn hip is nowhere the player's hand can reach, while the
//! player's own hip is where a hand reaching for a ripcord goes. The hip is estimated from the body
//! dimensions below the shoulder joints along the torso's heading ([`centre`]), standing or seated,
//! since a seated player's hip sits as far below the shoulders as a standing one's.
//!
//! It works like a parachute handle ([`crate::controllers::vehicles::parachute`]): a grip press with
//! the hand inside its volume takes it, a press that misses by a little and that no other volume
//! claims takes it too ([`reach_candidate`]), and it is held only while the grip is. A hold that
//! carries the hand the pull distance from where it took the ripcord, in any direction, pulls it
//! ([`step`]): a ripcord comes out and forward, or out and up, as the arm swings, never along one
//! line. A hold pulls once; the hand lets go and takes it again to pull again.

use glam::{Quat, Vec3};

use crate::controllers::{
    body::BodyDimensions,
    frame::Hand,
    grab::{Candidate, GrabVolume},
    vehicles::physical::Release,
};

/// The ripcord's hold, while a hand has it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RipcordHold {
    pub hand: Hand,
    /// The hand's grip when it took the ripcord, in the cockpit frame.
    pub rest: Vec3,
    /// Whether this hold has pulled the ripcord.
    pub pulled: bool,
}

/// Who holds the ripcord.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct RipcordState {
    pub hold: Option<RipcordHold>,
}

impl RipcordState {
    pub const fn new() -> Self {
        Self { hold: None }
    }
}

/// A take, a pull, or a release, for the log.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RipcordEvent {
    Taken(Hand),
    Pulled(Hand),
    Released(Hand, Release),
}

/// One tick's inputs.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StepInputs {
    /// The hand on the ripcord's side.
    pub side: Hand,
    /// Each hand's grip in the cockpit frame, `None` while untracked.
    pub grips: [Option<Vec3>; 2],
    /// Whether each hand's grip is held this tick.
    pub held: [bool; 2],
    /// Whether each hand's grip press latched the ripcord's volume this tick.
    pub grabs: [bool; 2],
    /// How far the holding hand must come from where it took the ripcord to pull it, metres.
    pub pull_m: f32,
}

/// Where the ripcord's volume is centred, in the cockpit frame, for shoulder joints at
/// `shoulders` (by [`Hand::index`]) on a torso turned by `torso`, a body of `body`'s dimensions,
/// and the ripcord on `side`: [`FROM_HIP_M`] from the hip joint on that side, mirrored for the
/// left.
pub fn centre(shoulders: [Vec3; 2], torso: Quat, body: &BodyDimensions, side: Hand) -> Vec3 {
    let shoulder = shoulders[side.index()];
    let drop = (body.shoulder_height - body.height * HIP_HEIGHT_FRACTION).max(0.0);
    let hip = shoulder - Vec3::Y * drop;
    let offset = match side {
        Hand::Right => FROM_HIP_M,
        Hand::Left => Vec3::new(-FROM_HIP_M.x, FROM_HIP_M.y, FROM_HIP_M.z),
    };
    hip + torso * offset
}

/// The ripcord's grab volume, for its side's hand only: at `centre` while it is free, and on the
/// holding hand's grip while it is held.
pub fn candidate(
    state: &RipcordState,
    centre: Vec3,
    side: Hand,
    grips: [Option<Vec3>; 2],
) -> Candidate {
    let held = state
        .hold
        .and_then(|hold| grips[hold.hand.index()])
        .filter(|g| g.is_finite());
    Candidate {
        volume: GrabVolume::Ripcord,
        centre: held.unwrap_or(centre),
        radius: RADIUS_M,
        hand: Some(side),
    }
}

/// The ripcord's wider reach while it is free, which takes a press no other volume claims.
pub fn reach_candidate(state: &RipcordState, centre: Vec3, side: Hand) -> Option<Candidate> {
    state.hold.is_none().then_some(Candidate {
        volume: GrabVolume::Ripcord,
        centre,
        radius: REACH_M,
        hand: Some(side),
    })
}

/// Advance the ripcord by one input tick: act on the grab, let go when the grip opens or the hand
/// is lost, and pull once the hand has come the pull distance from where it took the ripcord.
/// Returns whether the ripcord was pulled this tick.
pub fn step(state: &mut RipcordState, inputs: &StepInputs, events: &mut Vec<RipcordEvent>) -> bool {
    if let Some(hold) = state.hold
        && hold.hand != inputs.side
    {
        release(state, Release::Gone, events);
    }
    let hand = inputs.side;
    let i = hand.index();
    let Some(grip) = inputs.grips[i].filter(|g| g.is_finite()) else {
        release(state, Release::Gone, events);
        return false;
    };
    if inputs.grabs[i] {
        // A grab while holding is a release and a fresh press between two ticks: the hold stays
        // from where it was taken, and does not pull again.
        if state.hold.is_none() {
            state.hold = Some(RipcordHold {
                hand,
                rest: grip,
                pulled: false,
            });
            events.push(RipcordEvent::Taken(hand));
        }
    } else if !inputs.held[i] {
        release(state, Release::GripReleased, events);
        return false;
    }
    let Some(hold) = state.hold.as_mut() else {
        return false;
    };
    let pull = if inputs.pull_m.is_finite() {
        inputs.pull_m.max(MIN_PULL_M)
    } else {
        MIN_PULL_M
    };
    if hold.pulled || grip.distance(hold.rest) < pull {
        return false;
    }
    hold.pulled = true;
    events.push(RipcordEvent::Pulled(hand));
    true
}

/// Let go of the ripcord, as when the player leaves the fall or the wingsuit or the ripcord is
/// switched off.
pub fn release_all(state: &mut RipcordState, events: &mut Vec<RipcordEvent>) {
    release(state, Release::Gone, events);
}

/// The ripcord's centre from the hip joint along the torso's axes (+X out, +Y up, +Z back),
/// metres: a little outside, above, and behind the joint, at the back of the hip, behind the
/// holster.
pub const FROM_HIP_M: Vec3 = Vec3::new(0.06, 0.06, 0.12);

/// The standing height of the hip joints as a fraction of the standing height: a generic
/// anthropometric figure, which the body dimensions do not carry because it varies little between
/// adults.
pub const HIP_HEIGHT_FRACTION: f32 = 0.53;

/// How far from the ripcord's centre a hand's grip may be for a grip press to take it over every
/// other volume, metres.
pub const RADIUS_M: f32 = 0.12;

/// How far from the ripcord's centre a grip press outside every other volume still takes it,
/// metres. At least [`RADIUS_M`].
pub const REACH_M: f32 = 0.22;

/// The shortest pull distance a setting can ask for, metres: shorter, the hand's own tremor on
/// taking the ripcord would pull it.
pub const MIN_PULL_M: f32 = 0.05;

fn release(state: &mut RipcordState, why: Release, events: &mut Vec<RipcordEvent>) {
    if let Some(hold) = state.hold.take() {
        events.push(RipcordEvent::Released(hold.hand, why));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::grab::grab_volume;

    const PULL_M: f32 = 0.15;

    const BODY: BodyDimensions = BodyDimensions {
        height: 1.80,
        eye_height: 1.68,
        shoulder_height: 1.47,
        shoulder_width: 0.40,
        arm_span: 1.80,
        upper_arm: 0.34,
        forearm: 0.26,
        hand: 0.19,
        seated_eye_height: None,
    };

    const SHOULDERS: [Vec3; 2] = [Vec3::new(-0.2, -0.2, 0.1), Vec3::new(0.2, -0.2, 0.1)];

    fn tick(
        state: &mut RipcordState,
        grip: Option<Vec3>,
        grab: bool,
        held: bool,
    ) -> (bool, Vec<RipcordEvent>) {
        let mut events = Vec::new();
        let inputs = StepInputs {
            side: Hand::Right,
            grips: [None, grip],
            held: [false, held],
            grabs: [false, grab],
            pull_m: PULL_M,
        };
        let pulled = step(state, &inputs, &mut events);
        (pulled, events)
    }

    #[test]
    fn the_ripcord_sits_behind_the_hip_on_its_side_below_the_shoulders() {
        let right = centre(SHOULDERS, Quat::IDENTITY, &BODY, Hand::Right);
        let left = centre(SHOULDERS, Quat::IDENTITY, &BODY, Hand::Left);
        assert!(right.x > SHOULDERS[1].x && left.x < SHOULDERS[0].x);
        assert!((right.x + left.x).abs() < 1e-5, "mirrored: {right} {left}");
        // The hip is about half a metre below the shoulders, the ripcord a little above it.
        let below = SHOULDERS[1].y - right.y;
        assert!((0.40..0.55).contains(&below), "{below}");
        assert!(
            right.z > SHOULDERS[1].z,
            "behind the shoulders' line: {right}"
        );
    }

    #[test]
    fn the_ripcord_turns_with_the_torso() {
        let torso = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let turned = centre([Vec3::ZERO; 2], torso, &BODY, Hand::Right);
        // Facing -X: out is -Z and back is +X.
        assert!(turned.z < 0.0 && turned.x > 0.0, "{turned}");
    }

    #[test]
    fn only_its_side_s_hand_takes_it_and_a_near_miss_reaches_it() {
        let at = centre(SHOULDERS, Quat::IDENTITY, &BODY, Hand::Right);
        let state = RipcordState::new();
        let placed = [candidate(&state, at, Hand::Right, [None; 2])];
        assert_eq!(
            grab_volume(at, Hand::Right, &placed),
            Some(GrabVolume::Ripcord)
        );
        assert_eq!(grab_volume(at, Hand::Left, &placed), None);
        let near = at + Vec3::X * (RADIUS_M + REACH_M) * 0.5;
        assert_eq!(grab_volume(near, Hand::Right, &placed), None);
        let reach: Vec<_> = reach_candidate(&state, at, Hand::Right)
            .into_iter()
            .collect();
        assert_eq!(
            grab_volume(near, Hand::Right, &reach),
            Some(GrabVolume::Ripcord)
        );
    }

    #[test]
    fn a_pull_out_from_where_it_was_taken_opens_once_per_hold() {
        let at = Vec3::new(0.3, -0.7, 0.2);
        let mut state = RipcordState::new();
        let (pulled, events) = tick(&mut state, Some(at), true, true);
        assert!(!pulled);
        assert_eq!(events, vec![RipcordEvent::Taken(Hand::Right)]);
        // Out and forward, short of the pull distance: nothing yet.
        let short = at + Vec3::new(0.06, 0.0, -0.06);
        assert!(!tick(&mut state, Some(short), false, true).0);
        // Out and forward past it: one pull.
        let out = at + Vec3::new(0.12, 0.02, -0.12);
        let (pulled, events) = tick(&mut state, Some(out), false, true);
        assert!(pulled);
        assert_eq!(events, vec![RipcordEvent::Pulled(Hand::Right)]);
        // Carrying on pulls nothing more.
        assert!(!tick(&mut state, Some(out + Vec3::X * 0.2), false, true).0);
        // A release and a fresh hold pull again.
        let (_, events) = tick(&mut state, Some(out), false, false);
        assert_eq!(
            events,
            vec![RipcordEvent::Released(Hand::Right, Release::GripReleased)]
        );
        tick(&mut state, Some(at), true, true);
        assert!(tick(&mut state, Some(at + Vec3::Y * 0.2), false, true).0);
    }

    #[test]
    fn a_regrab_between_ticks_keeps_the_hold_and_does_not_pull_again() {
        let at = Vec3::ZERO;
        let mut state = RipcordState::new();
        tick(&mut state, Some(at), true, true);
        assert!(tick(&mut state, Some(Vec3::X * 0.2), false, true).0);
        let (pulled, events) = tick(&mut state, Some(Vec3::X * 0.3), true, true);
        assert!(!pulled);
        assert!(events.is_empty(), "{events:?}");
        assert_eq!(state.hold.map(|h| h.rest), Some(at));
    }

    #[test]
    fn a_lost_hand_or_a_released_grip_lets_go_without_pulling() {
        let mut state = RipcordState::new();
        tick(&mut state, Some(Vec3::ZERO), true, true);
        let (pulled, events) = tick(&mut state, None, false, false);
        assert!(!pulled);
        assert_eq!(
            events,
            vec![RipcordEvent::Released(Hand::Right, Release::Gone)]
        );
        // A grip that is not held never takes it, and a non-finite grip is lost.
        let (_, events) = tick(&mut state, Some(Vec3::ZERO), false, false);
        assert!(events.is_empty());
        let (_, events) = tick(&mut state, Some(Vec3::NAN), true, true);
        assert!(events.is_empty());
        assert_eq!(state, RipcordState::new());
    }

    #[test]
    fn a_tiny_or_invalid_pull_setting_still_needs_a_real_pull() {
        let mut state = RipcordState::new();
        let mut events = Vec::new();
        let mut inputs = StepInputs {
            side: Hand::Right,
            grips: [None, Some(Vec3::ZERO)],
            held: [false, true],
            grabs: [false, true],
            pull_m: 0.0,
        };
        assert!(!step(&mut state, &inputs, &mut events));
        inputs.grabs = [false; 2];
        inputs.grips[1] = Some(Vec3::X * (MIN_PULL_M * 0.5));
        assert!(!step(&mut state, &inputs, &mut events));
        inputs.pull_m = f32::NAN;
        assert!(!step(&mut state, &inputs, &mut events));
        inputs.grips[1] = Some(Vec3::X * MIN_PULL_M * 1.5);
        assert!(step(&mut state, &inputs, &mut events));
    }

    #[test]
    fn a_hold_by_the_other_side_is_dropped_when_the_side_changes() {
        let mut state = RipcordState {
            hold: Some(RipcordHold {
                hand: Hand::Left,
                rest: Vec3::ZERO,
                pulled: false,
            }),
        };
        let (_, events) = tick(&mut state, Some(Vec3::ONE), false, true);
        assert_eq!(
            events,
            vec![RipcordEvent::Released(Hand::Left, Release::Gone)]
        );
        assert_eq!(state.hold, None);
    }
}
