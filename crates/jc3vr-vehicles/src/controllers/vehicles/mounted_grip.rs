//! The hands' hold on a mounted gun's handles. Pure.
//!
//! Each hand takes its own handle like a physical control: a grip press with the hand's grip near
//! the handle takes it, and only releasing the grip lets go: a holding hand that drifts off its
//! handle keeps aiming from where it is, however far it goes. Only a holding hand is drawn on its handle and aims the gun, so one hand can let go
//! to throw a grenade or reach for something while the other keeps the gun, and with both hands off
//! the gun stays where they left it.

use glam::Vec3;
use jc3vr_body::controllers::{
    grab::{Candidate, GrabVolume},
    hands::tiers,
};
use jc3vr_input::frame::Hand;

use crate::controllers::vehicles::{physical::Release, tuning::VEHICLES};

/// The hands' hold on the handles.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(crate) struct MountedGrip {
    /// Whether each hand holds its handle, indexed by [`Hand::index`].
    pub holds: [bool; 2],
    /// Each hand's share of the pose on its handle, blending in on a grab and out on a release.
    pub weights: [f32; 2],
}

impl MountedGrip {
    pub const fn new() -> Self {
        Self {
            holds: [false; 2],
            weights: [0.0; 2],
        }
    }

    /// The hands that aim the gun: the holding hands that are tracked.
    pub fn aiming(&self, tracked: [bool; 2]) -> [bool; 2] {
        Hand::BOTH.map(|hand| self.holds[hand.index()] && tracked[hand.index()])
    }

    /// Each hand's share of the pose on its handle.
    pub fn shares(&self) -> [f32; 2] {
        self.weights
    }
}

/// A grab or a release of a handle, the release with why.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum MountedGripEvent {
    Grabbed(Hand),
    Released(Hand, Release),
}

/// One tick's inputs.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct GripStep {
    /// Each hand's handle in the cockpit frame, where the gun places one; `None` off the gun.
    pub handles: [Option<Vec3>; 2],
    /// Each hand's grip position in the cockpit frame, `None` while untracked.
    pub grips: [Option<Vec3>; 2],
    /// Whether each hand's grip is held this tick.
    pub held: [bool; 2],
    /// Whether each hand's grip press latched its handle as a grab this tick.
    pub grabs: [bool; 2],
    pub dt: f32,
    /// How long a hand takes to blend onto or off its handle, seconds.
    pub blend_s: f32,
}

/// The handles' grab volumes: one sphere per handle, for its own hand.
pub fn candidates(handles: [Option<Vec3>; 2]) -> Vec<Candidate> {
    Hand::BOTH
        .into_iter()
        .filter_map(|hand| {
            Some(Candidate {
                volume: GrabVolume::MountedHandle(hand),
                centre: handles[hand.index()]?,
                radius: VEHICLES.get().mounted.grab_radius_m,
                hand: Some(hand),
            })
        })
        .collect()
}

/// Advance the hold by one tick: take the grabbed handles, let go of those whose grip is released,
/// and blend the hands' shares.
pub fn step(state: &mut MountedGrip, inputs: &GripStep, events: &mut Vec<MountedGripEvent>) {
    for hand in Hand::BOTH {
        let i = hand.index();
        let handle = inputs.handles[i];
        let was = state.holds[i];
        let now = match (handle, inputs.grips[i]) {
            (Some(_), Some(_)) => (was || inputs.grabs[i]) && inputs.held[i],
            _ => false,
        };
        state.holds[i] = now;
        match (was, now) {
            (false, true) => events.push(MountedGripEvent::Grabbed(hand)),
            (true, false) => {
                let why = if handle.is_some() && inputs.grips[i].is_some() {
                    Release::GripReleased
                } else {
                    Release::Gone
                };
                events.push(MountedGripEvent::Released(hand, why));
            }
            _ => {}
        }
        let target = if now { 1.0 } else { 0.0 };
        state.weights[i] = tiers::step_weight(
            state.weights[i],
            target,
            inputs.dt,
            inputs.blend_s,
            inputs.blend_s,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(grips: [Option<Vec3>; 2]) -> GripStep {
        GripStep {
            handles: [
                Some(Vec3::new(-0.2, 0.0, -0.5)),
                Some(Vec3::new(0.2, 0.0, -0.5)),
            ],
            grips,
            held: [true; 2],
            grabs: [false; 2],
            dt: 0.1,
            blend_s: 0.1,
        }
    }

    #[test]
    fn a_hand_takes_its_handle_with_the_grip_and_lets_go_with_it() {
        let near = [
            Some(Vec3::new(-0.2, 0.02, -0.5)),
            Some(Vec3::new(0.2, 0.0, -0.48)),
        ];
        let mut state = MountedGrip::new();
        let mut events = Vec::new();
        // Resting there without a press takes nothing.
        step(&mut state, &inputs(near), &mut events);
        assert_eq!(state.holds, [false; 2]);
        let mut grab = inputs(near);
        grab.grabs = [false, true];
        step(&mut state, &grab, &mut events);
        assert_eq!(state.holds, [false, true]);
        assert_eq!(events, [MountedGripEvent::Grabbed(Hand::Right)]);
        assert_eq!(state.weights[1], 1.0);
        assert_eq!(state.aiming([true; 2]), [false, true]);

        let mut release = inputs(near);
        release.held = [true, false];
        step(&mut state, &release, &mut events);
        assert_eq!(state.holds, [false; 2]);
        assert_eq!(
            events.last(),
            Some(&MountedGripEvent::Released(
                Hand::Right,
                Release::GripReleased
            ))
        );
    }

    #[test]
    fn a_hand_that_drifts_off_its_handle_keeps_it_until_the_grip_opens() {
        let mut state = MountedGrip::new();
        let mut events = Vec::new();
        let near = [Some(Vec3::new(-0.2, 0.0, -0.5)), None];
        let mut grab = inputs(near);
        grab.grabs = [true, false];
        step(&mut state, &grab, &mut events);
        assert!(state.holds[0]);
        let far = [Some(Vec3::new(-0.2, 0.0, 0.7)), None];
        step(&mut state, &inputs(far), &mut events);
        assert!(state.holds[0]);
        assert_eq!(state.aiming([true, false]), [true, false]);
        let mut open = inputs(far);
        open.held = [false; 2];
        step(&mut state, &open, &mut events);
        assert!(!state.holds[0]);
        assert_eq!(
            events.last(),
            Some(&MountedGripEvent::Released(
                Hand::Left,
                Release::GripReleased
            ))
        );
    }
}
