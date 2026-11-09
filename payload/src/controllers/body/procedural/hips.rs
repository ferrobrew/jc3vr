//! The clip's hip heading is followed for visual stance correction. It does not steer the
//! character root, so animated stop turns cannot feed their yaw back into body following.
//!
//! The gate measures the upper-leg line before IK while on foot. Brief hand-over cases retain
//! the follower; losing grounded authority resets it.

use body_rig::angle::{line_yaw, wrap};
use glam::Quat;

use crate::controllers::body::procedural::{authority::Case, rig::RigBone, solver::pose::RigPose};

/// What the hips' follower does in a hand-over case.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum Track {
    /// Measure the clip's hips and follow them: on foot.
    Observe,
    /// Keep the followed heading without measuring during a brief hand-over on the ground.
    Hold,
    /// Stop following: switched off, or off the ground.
    Reset,
}

impl Track {
    pub(super) const fn of(case: Case) -> Self {
        match case {
            Case::OnFoot => Self::Observe,
            Case::HitReaction | Case::Jump | Case::AuthoredMotion | Case::NoMovementTask => {
                Self::Hold
            }
            Case::NotInHeadset
            | Case::SwitchedOff
            | Case::NotInGameplay
            | Case::Sequence
            | Case::Dead
            | Case::Ragdoll
            | Case::GettingUp
            | Case::Seated
            | Case::StuntStanding
            | Case::StuntHanging
            | Case::Vehicle
            | Case::GrappleAttach
            | Case::GrappleLock
            | Case::Traversal
            | Case::LedgeClimb
            | Case::Swimming
            | Case::Wingsuit
            | Case::Parachute
            | Case::Flight
            | Case::HandClimb => Self::Reset,
        }
    }
}

/// The followed heading of the clip's hips.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(super) struct HipFollower {
    yaw: Option<f32>,
}

impl HipFollower {
    pub(super) const fn new() -> Self {
        Self { yaw: None }
    }

    /// The followed heading, while it follows.
    pub(super) fn yaw(&self) -> Option<f32> {
        self.yaw
    }

    /// Measure, hold, or stop following, as the hand-over case `case` calls for ([`Track`]).
    pub(super) fn track(&mut self, case: Case, animated: &RigPose, dt: f32) {
        match Track::of(case) {
            Track::Observe => self.observe(animated, dt),
            Track::Hold => {}
            Track::Reset => self.reset(),
        }
    }

    /// Fold the animated hip heading into the visual stance follower.
    fn observe(&mut self, animated: &RigPose, dt: f32) {
        let measured = line_yaw(
            animated.position(RigBone::LeftUpLeg),
            animated.position(RigBone::RightUpLeg),
        );
        self.step(measured, dt);
    }

    /// Stop following, off the ground or with the follow switched off.
    fn reset(&mut self) {
        self.yaw = None;
    }

    /// Follow `measured` over `dt` seconds: the first measurement is taken whole, and later ones
    /// are followed the short way round with a time constant of [`SMOOTHING_S`].
    fn step(&mut self, measured: Option<f32>, dt: f32) -> Option<f32> {
        let measured = measured.filter(|yaw| yaw.is_finite())?;
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let yaw = match self.yaw {
            Some(yaw) => wrap(yaw + wrap(measured - yaw) * (dt / (SMOOTHING_S + dt))),
            None => wrap(measured),
        };
        self.yaw = Some(yaw);
        self.yaw
    }
}

/// The hips' follower's time constant, seconds: long against a stride, so the walk's swing of the
/// hips averages out, and short against standing still.
const SMOOTHING_S: f32 = 0.8;

pub(super) fn square_pose(animated: &RigPose, target: f32, weight: f32) -> RigPose {
    let mut pose = *animated;
    if !target.is_finite() || !weight.is_finite() || weight <= 0.0 {
        return pose;
    }
    let Some(heading) = line_yaw(
        animated.position(RigBone::LeftUpLeg),
        animated.position(RigBone::RightUpLeg),
    ) else {
        return pose;
    };
    pose.rotate_about(
        RigBone::Pelvis,
        Quat::from_rotation_y(wrap(target - heading) * weight.clamp(0.0, 1.0)),
        animated.position(RigBone::Pelvis),
    );
    pose
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    #[test]
    fn visual_squaring_keeps_the_root_and_torso_and_preserves_leg_lengths() {
        let rig = crate::controllers::body::procedural::rig::Rig::from_skeleton(
            &crate::controllers::body::procedural::rig::tests::skeleton(),
        )
        .unwrap();
        let mut animated = RigPose {
            bones: rig.reference,
        };
        let pivot = animated.position(RigBone::Pelvis);
        animated.rotate_about(RigBone::Pelvis, Quat::from_rotation_y(-0.6), pivot);
        let target = 0.35;
        let squared = square_pose(&animated, target, 1.0);
        let yaw = line_yaw(
            squared.position(RigBone::LeftUpLeg),
            squared.position(RigBone::RightUpLeg),
        )
        .unwrap();
        assert!(wrap(yaw - target).abs() < 1e-5);
        for bone in RigBone::ALL {
            if !bone.descends_from(RigBone::Pelvis) {
                assert_eq!(squared.get(bone), animated.get(bone));
            } else if let Some(parent) = bone.parent() {
                let length = animated.position(bone).distance(animated.position(parent));
                assert!(
                    (squared.position(bone).distance(squared.position(parent)) - length).abs()
                        < 1e-5
                );
            }
        }
        assert_eq!(square_pose(&animated, target, 0.0), animated);
    }

    #[test]
    fn the_first_measurement_is_taken_whole() {
        let mut follower = HipFollower::new();
        assert!((follower.step(Some(-0.4), DT).unwrap() + 0.4).abs() < 1e-6);
        assert_eq!(follower.step(None, DT), None);
        assert!((follower.yaw.unwrap() + 0.4).abs() < 1e-6);
    }

    #[test]
    fn a_stride_swing_averages_out_and_a_new_stance_is_reached() {
        let mut follower = HipFollower::new();
        follower.step(Some(0.0), DT);
        // A walk swinging the hips ten degrees either way, about once a second.
        let swing = 10.0f32.to_radians();
        let mut furthest = 0.0f32;
        for i in 0..600 {
            let t = i as f32 * DT;
            let yaw = follower
                .step(Some(swing * (t * std::f32::consts::TAU).sin()), DT)
                .unwrap();
            if t > 2.0 {
                furthest = furthest.max(yaw.abs());
            }
        }
        assert!(furthest < swing * 0.25, "{}", furthest.to_degrees());
        // Standing in the bladed idle, the follower reaches it within a few seconds.
        let idle = -24.0f32.to_radians();
        let mut yaw = 0.0;
        for _ in 0..(4.0 / DT) as usize {
            yaw = follower.step(Some(idle), DT).unwrap();
        }
        assert!((yaw - idle).abs() < 1.0f32.to_radians());
    }

    #[test]
    fn a_brief_case_on_the_ground_holds_the_heading_and_leaving_it_resets() {
        assert_eq!(Track::of(Case::OnFoot), Track::Observe);
        for case in [
            Case::HitReaction,
            Case::Jump,
            Case::AuthoredMotion,
            Case::NoMovementTask,
        ] {
            assert_eq!(Track::of(case), Track::Hold, "{case:?}");
        }
        for case in [
            Case::SwitchedOff,
            Case::Flight,
            Case::Swimming,
            Case::Vehicle,
            Case::Ragdoll,
        ] {
            assert_eq!(Track::of(case), Track::Reset, "{case:?}");
        }
        // A held heading carries on smoothly when the follower measures again.
        let mut follower = HipFollower::new();
        follower.step(Some(-0.4), DT);
        let yaw = follower.step(Some(0.0), DT).unwrap();
        assert!((yaw + 0.4).abs() < 0.01, "{yaw}");
    }

    #[test]
    fn the_follower_goes_the_short_way_round() {
        let mut follower = HipFollower::new();
        follower.step(Some(3.1), DT);
        let yaw = follower.step(Some(-3.1), 1.0).unwrap();
        assert!(yaw.abs() > 3.0, "{yaw}");
    }
}
