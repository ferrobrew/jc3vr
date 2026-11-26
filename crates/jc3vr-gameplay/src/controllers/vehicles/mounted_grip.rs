//! The hands' hold on a mounted gun's handles, behind
//! [`MountedConfig::grip_handles`](crate::controllers::vehicles::config::MountedConfig::grip_handles).
//! Pure.
//!
//! Each hand takes its own handle like a physical control: a grip press with the hand's grip near
//! the handle takes it, and only releasing the grip lets go: a holding hand that drifts off its
//! handle keeps aiming from where it is, however far it goes. Only a holding hand is drawn on its handle and aims the gun, so one hand can let go
//! to throw a grenade or reach for something while the other keeps the gun, and with both hands off
//! the gun stays where they left it. Off, both hands sit on the handles and aim the gun whenever
//! the character mans it.

use glam::Vec3;

use crate::controllers::{
    frame::Hand,
    grab::{Candidate, GrabVolume},
    hands::tiers,
    vehicles::{config::MountedConfig, physical::Release},
};

/// The hands' hold on the handles.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct MountedGrip {
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

    /// The hands that aim the gun: the holding hands, or with the grip off, every tracked hand.
    pub fn aiming(&self, config: &MountedConfig, tracked: [bool; 2]) -> [bool; 2] {
        if config.grip_handles {
            Hand::BOTH.map(|hand| self.holds[hand.index()] && tracked[hand.index()])
        } else {
            tracked
        }
    }

    /// Each hand's share of the pose on its handle: the blend, or with the grip off, all of it.
    pub fn shares(&self, config: &MountedConfig) -> [f32; 2] {
        if config.grip_handles {
            self.weights
        } else {
            [1.0; 2]
        }
    }
}

/// A grab or a release of a handle, the release with why.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MountedGripEvent {
    Grabbed(Hand),
    Released(Hand, Release),
}

/// One tick's inputs.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GripStep<'a> {
    pub config: &'a MountedConfig,
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
pub fn candidates(config: &MountedConfig, handles: [Option<Vec3>; 2]) -> Vec<Candidate> {
    if !config.grip_handles {
        return Vec::new();
    }
    Hand::BOTH
        .into_iter()
        .filter_map(|hand| {
            Some(Candidate {
                volume: GrabVolume::MountedHandle(hand),
                centre: handles[hand.index()]?,
                radius: GRAB_RADIUS_M,
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
            _ if !inputs.config.grip_handles => false,
            (Some(_), Some(_)) => (was || inputs.grabs[i]) && inputs.held[i],
            _ => false,
        };
        state.holds[i] = now;
        match (was, now) {
            (false, true) => events.push(MountedGripEvent::Grabbed(hand)),
            (true, false) => {
                let why = if inputs.config.grip_handles
                    && handle.is_some()
                    && inputs.grips[i].is_some()
                {
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

/// How far from a handle a hand's grip may be for a grip press to take it, metres.
pub(super) const GRAB_RADIUS_M: f32 = 0.12;

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs<'a>(config: &'a MountedConfig, grips: [Option<Vec3>; 2]) -> GripStep<'a> {
        GripStep {
            config,
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
        let config = MountedConfig::new();
        let near = [
            Some(Vec3::new(-0.2, 0.02, -0.5)),
            Some(Vec3::new(0.2, 0.0, -0.48)),
        ];
        let mut state = MountedGrip::new();
        let mut events = Vec::new();
        // Resting there without a press takes nothing.
        step(&mut state, &inputs(&config, near), &mut events);
        assert_eq!(state.holds, [false; 2]);
        let mut grab = inputs(&config, near);
        grab.grabs = [false, true];
        step(&mut state, &grab, &mut events);
        assert_eq!(state.holds, [false, true]);
        assert_eq!(events, [MountedGripEvent::Grabbed(Hand::Right)]);
        assert_eq!(state.weights[1], 1.0);
        assert_eq!(state.aiming(&config, [true; 2]), [false, true]);

        let mut release = inputs(&config, near);
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
        let config = MountedConfig::new();
        let mut state = MountedGrip::new();
        let mut events = Vec::new();
        let near = [Some(Vec3::new(-0.2, 0.0, -0.5)), None];
        let mut grab = inputs(&config, near);
        grab.grabs = [true, false];
        step(&mut state, &grab, &mut events);
        assert!(state.holds[0]);
        let far = [Some(Vec3::new(-0.2, 0.0, 0.7)), None];
        step(&mut state, &inputs(&config, far), &mut events);
        assert!(state.holds[0]);
        assert_eq!(state.aiming(&config, [true, false]), [true, false]);
        let mut open = inputs(&config, far);
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

    #[test]
    fn with_the_grip_off_every_tracked_hand_holds_and_aims() {
        let config = MountedConfig {
            grip_handles: false,
            ..MountedConfig::new()
        };
        let state = MountedGrip::new();
        assert_eq!(state.aiming(&config, [true, false]), [true, false]);
        assert_eq!(state.shares(&config), [1.0; 2]);
        assert!(candidates(&config, [Some(Vec3::ZERO); 2]).is_empty());
    }
}
