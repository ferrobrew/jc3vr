//! The wrist detonator's press detector, and where the button and the fingertip are. Pure.
//!
//! The button is a disc on the inside of the off-hand's wrist, fixed to the off-hand controller's
//! grip pose, with a face normal pointing out of the wrist. The dominant hand's index fingertip
//! presses it. Both are in the cockpit frame, where the controller frame reports both hands, so
//! their relative placement does not depend on the hand remap or the world scale.
//!
//! A press must be a deliberate push into the face, so that brushing the wrist does not detonate:
//!
//! - the fingertip must reach the face from in front of it, within the face's radius. A fingertip
//!   that arrives beside the button already behind the face's plane (a slide along the arm) is
//!   blocked until it comes back out in front;
//! - it must then push `press_depth_m` past the face, and either have been moving into the face at
//!   least `min_approach_speed_mps` as it reached it, or stay pushed in for `dwell_s`;
//! - a fingertip further than `max_depth_m` past the face is a hand passing through the wrist, and
//!   is blocked;
//! - once pressed, the button presses again only after the fingertip has come back out to
//!   `release_depth_m`, which is less than `press_depth_m`, so a fingertip resting at the press
//!   depth does not chatter.
//!
//! A hand that loses tracking resets the detector without a press.

use glam::Vec3;

use crate::controllers::{
    frame::{HAND_JOINT_COUNT, Hand, HandJoint, Pose},
    weapons::config::WristDetonatorConfig,
};

/// The button's face in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ButtonFace {
    pub centre: Vec3,
    /// Unit length, out of the wrist.
    pub normal: Vec3,
}

/// The button's face on the `off` hand whose controller grip is at `grip`: the configured left-hand
/// placement, mirrored for the right hand. `None` for a degenerate normal.
pub fn button_face(off: Hand, grip: Pose, tuning: &WristDetonatorConfig) -> Option<ButtonFace> {
    let local_normal = mirror_from_left(off, tuning.normal).try_normalize()?;
    let local_centre = mirror_from_left(off, tuning.offset_m);
    let orientation = grip.orientation.normalize();
    let face = ButtonFace {
        centre: grip.position + orientation * local_centre,
        normal: (orientation * local_normal).normalize(),
    };
    (face.centre.is_finite() && face.normal.is_finite()).then_some(face)
}

/// What the fingertip is read from on one hand.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FingertipSource<'a> {
    /// The tracked hand skeleton, when the runtime reports one.
    pub joints: Option<&'a [HandJoint; HAND_JOINT_COUNT]>,
    /// The controller's aim pose.
    pub aim: Pose,
    /// Whether the index finger touches the trigger.
    pub trigger_touch: bool,
}

/// The dominant hand's index fingertip in the cockpit frame: the tracked skeleton's index tip when
/// it is valid, and otherwise an estimate in the controller's aim frame, ahead of the controller
/// while the finger points and at the trigger while it rests there.
pub fn fingertip(hand: Hand, source: FingertipSource<'_>, tuning: &WristDetonatorConfig) -> Vec3 {
    if let Some(tip) = source
        .joints
        .map(|joints| joints[INDEX_TIP_JOINT])
        .filter(|joint| joint.valid && joint.pose.position.is_finite())
    {
        return tip.pose.position;
    }
    let offset = if source.trigger_touch {
        tuning.fingertip_resting_m
    } else {
        tuning.fingertip_pointing_m
    };
    let offset = match hand {
        Hand::Right => offset,
        Hand::Left => Vec3::new(-offset.x, offset.y, offset.z),
    };
    source.aim.position + source.aim.orientation.normalize() * offset
}

/// Where the detector is.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub enum PressPhase {
    /// No fingertip or no button: a hand is untracked or the detonator is off.
    #[default]
    Lost,
    /// The fingertip is in front of the face, or beside it short of its plane: free to press.
    Armed {
        /// The fastest the fingertip has moved into the face while near it, metres per second.
        approach_mps: f32,
    },
    /// The fingertip came through the face from in front and is pushing into it.
    Pushing {
        /// Whether it was moving fast enough as it reached the face to press at once.
        fast: bool,
        /// How long it has stayed at the press depth, seconds.
        dwell_s: f32,
    },
    /// The button pressed; it waits for the fingertip to come back out.
    Pressed,
    /// The fingertip reached the plane from the side, or went too deep; it waits for the fingertip
    /// to come back out in front.
    Blocked,
}

impl PressPhase {
    pub const fn label(self) -> &'static str {
        match self {
            PressPhase::Lost => "no fingertip or button",
            PressPhase::Armed { .. } => "armed",
            PressPhase::Pushing { fast: true, .. } => "pushing",
            PressPhase::Pushing { fast: false, .. } => "pushing slowly (dwell)",
            PressPhase::Pressed => "pressed, waiting for release",
            PressPhase::Blocked => "blocked, waiting for the finger to come back out",
        }
    }
}

/// One step's reading.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct PressReading {
    /// The button pressed on this step.
    pub pressed: bool,
    /// How far past the face the fingertip is, metres (negative in front of it).
    pub depth_m: Option<f32>,
    /// How far the fingertip is from the button's axis, metres.
    pub lateral_m: Option<f32>,
    pub phase: PressPhase,
}

/// The press detector's state across ticks.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct PressDetector {
    phase: PressPhase,
    /// The fingertip's depth on the previous step.
    last_depth_m: Option<f32>,
}

impl PressDetector {
    pub const fn new() -> Self {
        Self {
            phase: PressPhase::Lost,
            last_depth_m: None,
        }
    }

    /// Advance by `dt` seconds with the fingertip at `tip` and the button's `face`, either `None`
    /// while untracked.
    pub fn step(
        &mut self,
        tip: Option<Vec3>,
        face: Option<ButtonFace>,
        dt: f32,
        tuning: &WristDetonatorConfig,
    ) -> PressReading {
        let (Some(tip), Some(face)) = (tip.filter(|t| t.is_finite()), face) else {
            *self = Self::new();
            return PressReading::default();
        };
        let relative = tip - face.centre;
        let height = relative.dot(face.normal);
        let depth = -height;
        let lateral = (relative - face.normal * height).length();
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        // Positive while moving into the face.
        let approach = match self.last_depth_m {
            Some(last) if dt > 0.0 => (depth - last) / dt,
            _ => 0.0,
        };
        self.last_depth_m = Some(depth);

        let radius = tuning.radius_m.max(0.0);
        let on_face = lateral <= radius;
        let out_front = depth < tuning.release_depth_m;
        let too_deep = depth > tuning.max_depth_m;
        let near = on_face && depth > -APPROACH_ZONE_M;
        let mut pressed = false;
        self.phase = match self.phase {
            // A fingertip first seen behind the plane has not come from in front.
            PressPhase::Lost if out_front => PressPhase::Armed { approach_mps: 0.0 },
            PressPhase::Lost => PressPhase::Blocked,
            PressPhase::Armed { approach_mps } => {
                let approach_mps = if near {
                    approach_mps.max(approach)
                } else {
                    0.0
                };
                if too_deep {
                    PressPhase::Blocked
                } else if depth >= 0.0 && !on_face {
                    // Through the plane beside the button: a slide along the arm.
                    if out_front {
                        PressPhase::Armed { approach_mps: 0.0 }
                    } else {
                        PressPhase::Blocked
                    }
                } else if depth >= 0.0 {
                    let pushing = PressPhase::Pushing {
                        fast: approach_mps >= tuning.min_approach_speed_mps,
                        dwell_s: 0.0,
                    };
                    self.push(pushing, depth, dt, tuning, &mut pressed)
                } else {
                    PressPhase::Armed { approach_mps }
                }
            }
            pushing @ PressPhase::Pushing { .. } => {
                if too_deep {
                    PressPhase::Blocked
                } else if depth < 0.0 {
                    PressPhase::Armed { approach_mps: 0.0 }
                } else if !on_face {
                    // Slid off the face while pushed in.
                    if out_front {
                        PressPhase::Armed { approach_mps: 0.0 }
                    } else {
                        PressPhase::Blocked
                    }
                } else {
                    self.push(pushing, depth, dt, tuning, &mut pressed)
                }
            }
            PressPhase::Pressed | PressPhase::Blocked if out_front => {
                PressPhase::Armed { approach_mps: 0.0 }
            }
            phase @ (PressPhase::Pressed | PressPhase::Blocked) => phase,
        };
        PressReading {
            pressed,
            depth_m: Some(depth),
            lateral_m: Some(lateral),
            phase: self.phase,
        }
    }

    /// Advance a push at `depth`: press once the press depth is reached by a fast fingertip, or
    /// held there by a slow one for the dwell.
    fn push(
        &self,
        phase: PressPhase,
        depth: f32,
        dt: f32,
        tuning: &WristDetonatorConfig,
        pressed: &mut bool,
    ) -> PressPhase {
        let PressPhase::Pushing { fast, dwell_s } = phase else {
            return phase;
        };
        if depth < tuning.press_depth_m {
            return PressPhase::Pushing { fast, dwell_s };
        }
        let dwell_s = dwell_s + dt;
        if fast || dwell_s >= tuning.dwell_s.max(0.0) {
            *pressed = true;
            PressPhase::Pressed
        } else {
            PressPhase::Pushing { fast, dwell_s }
        }
    }
}

/// The `XR_EXT_hand_tracking` index of the index finger's tip (`XR_HAND_JOINT_INDEX_TIP_EXT`).
pub(super) const INDEX_TIP_JOINT: usize = 10;

/// How far in front of the face, metres, the fingertip's approach speed is watched: its speed into
/// the face while it is this close decides whether it reached the face fast.
const APPROACH_ZONE_M: f32 = 0.04;

/// A vector given for the left hand, mirrored across the controller's X for `hand`.
fn mirror_from_left(hand: Hand, v: Vec3) -> Vec3 {
    match hand {
        Hand::Left => v,
        Hand::Right => Vec3::new(-v.x, v.y, v.z),
    }
}

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    const DT: f32 = 1.0 / 90.0;

    fn tuning() -> WristDetonatorConfig {
        WristDetonatorConfig::new()
    }

    /// A button at the origin facing +X.
    const FACE: ButtonFace = ButtonFace {
        centre: Vec3::ZERO,
        normal: Vec3::X,
    };

    /// A fingertip on the button's axis at `depth` past the face.
    fn on_axis(depth: f32) -> Vec3 {
        Vec3::new(-depth, 0.0, 0.0)
    }

    /// Step the detector through `path`, returning how many presses happened.
    fn run(d: &mut PressDetector, path: impl IntoIterator<Item = Vec3>) -> usize {
        let t = tuning();
        path.into_iter()
            .filter(|tip| d.step(Some(*tip), Some(FACE), DT, &t).pressed)
            .count()
    }

    /// A straight move from depth `from` to `to` at `speed` metres per second.
    fn push(from: f32, to: f32, speed: f32) -> Vec<Vec3> {
        let step = speed * DT * (to - from).signum();
        let count = ((to - from).abs() / (speed * DT)).ceil() as usize;
        (0..=count)
            .map(|i| {
                on_axis(if i == count {
                    to
                } else {
                    from + step * i as f32
                })
            })
            .collect()
    }

    #[test]
    fn a_quick_push_from_in_front_presses_once() {
        let mut d = PressDetector::new();
        assert_eq!(run(&mut d, push(-0.05, 0.02, 0.4)), 1);
        // Holding it in does not press again.
        assert_eq!(run(&mut d, std::iter::repeat_n(on_axis(0.02), 60)), 0);
        assert_eq!(d.phase, PressPhase::Pressed);
    }

    #[test]
    fn the_press_needs_the_finger_back_out_past_the_release_depth() {
        let t = tuning();
        let mut d = PressDetector::new();
        assert_eq!(run(&mut d, push(-0.05, 0.02, 0.4)), 1);
        // Jitter around the press depth, never coming back out past the release depth.
        let jitter = (0..40).map(|i| {
            on_axis(if i % 2 == 0 {
                t.press_depth_m + 0.002
            } else {
                t.release_depth_m + 0.001
            })
        });
        assert_eq!(run(&mut d, jitter), 0);
        // Out, and in again: a second press.
        assert_eq!(run(&mut d, push(0.02, -0.03, 0.4)), 0);
        assert_eq!(run(&mut d, push(-0.03, 0.02, 0.4)), 1);
    }

    #[test]
    fn a_slow_push_presses_only_after_the_dwell() {
        let t = tuning();
        let mut d = PressDetector::new();
        assert_eq!(run(&mut d, push(-0.05, 0.02, 0.05)), 0);
        let dwell_ticks = (t.dwell_s / DT).ceil() as usize + 1;
        assert_eq!(
            run(&mut d, std::iter::repeat_n(on_axis(0.02), dwell_ticks)),
            1
        );
        // A slow touch that backs out before the dwell does nothing.
        let mut d = PressDetector::new();
        assert_eq!(run(&mut d, push(-0.05, 0.02, 0.05)), 0);
        assert_eq!(run(&mut d, std::iter::repeat_n(on_axis(0.02), 5)), 0);
        assert_eq!(run(&mut d, push(0.02, -0.05, 0.5)), 0);
    }

    #[test]
    fn a_brush_along_the_arm_into_the_button_is_blocked() {
        let mut d = PressDetector::new();
        // Behind the face's plane beside the button, then sliding sideways onto it, fast.
        let depth = 0.02;
        let slide = (0..30).map(|i| Vec3::new(-depth, 0.0, 0.10 - 0.005 * i as f32));
        assert_eq!(run(&mut d, slide), 0);
        assert_eq!(d.phase, PressPhase::Blocked);
        // Coming back out in front re-arms it.
        assert_eq!(run(&mut d, push(depth, -0.03, 0.4)), 0);
        assert_eq!(run(&mut d, push(-0.03, 0.02, 0.4)), 1);
    }

    #[test]
    fn a_hand_passing_through_the_wrist_does_not_press() {
        let t = tuning();
        let mut d = PressDetector::new();
        // So fast that one step goes from in front to beyond the deepest press.
        let through = [on_axis(-0.03), on_axis(t.max_depth_m + 0.02)];
        assert_eq!(run(&mut d, through), 0);
        assert_eq!(d.phase, PressPhase::Blocked);
    }

    #[test]
    fn a_tangential_swipe_across_the_face_does_not_press() {
        let mut d = PressDetector::new();
        // Skimming the face from in front, barely past it, moving sideways.
        let swipe =
            (0..30).map(|i| Vec3::new(-0.005 + 0.0007 * i as f32, 0.0, 0.06 - 0.004 * i as f32));
        assert_eq!(run(&mut d, swipe), 0);
    }

    #[test]
    fn losing_tracking_resets_without_a_press() {
        let t = tuning();
        let mut d = PressDetector::new();
        assert_eq!(run(&mut d, push(-0.05, 0.005, 0.4)), 0);
        let lost = d.step(None, Some(FACE), DT, &t);
        assert!(!lost.pressed);
        assert_eq!(lost.phase, PressPhase::Lost);
        // Reappearing already pushed in is not a press from in front.
        assert!(!d.step(Some(on_axis(0.02)), Some(FACE), DT, &t).pressed);
        assert_eq!(d.phase, PressPhase::Blocked);
    }

    #[test]
    fn the_button_mirrors_onto_the_right_wrist() {
        let t = tuning();
        let left = button_face(Hand::Left, Pose::IDENTITY, &t).unwrap();
        let right = button_face(Hand::Right, Pose::IDENTITY, &t).unwrap();
        assert_eq!(left.centre, t.offset_m);
        assert_eq!(
            right.centre,
            Vec3::new(-t.offset_m.x, t.offset_m.y, t.offset_m.z)
        );
        assert_eq!(left.normal, Vec3::X);
        assert_eq!(right.normal, Vec3::NEG_X);
        // The button turns with the grip.
        let turned = Pose {
            position: Vec3::new(1.0, 0.0, 0.0),
            orientation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        };
        let face = button_face(Hand::Left, turned, &t).unwrap();
        assert!(face.normal.distance(Vec3::NEG_Z) < 1e-5, "{face:?}");
        let degenerate = WristDetonatorConfig {
            normal: Vec3::ZERO,
            ..t
        };
        assert_eq!(button_face(Hand::Left, Pose::IDENTITY, &degenerate), None);
    }

    #[test]
    fn the_fingertip_prefers_the_tracked_skeleton() {
        let t = tuning();
        let aim = Pose::IDENTITY;
        let mut joints = [HandJoint::default(); HAND_JOINT_COUNT];
        let estimated = fingertip(
            Hand::Right,
            FingertipSource {
                joints: Some(&joints),
                aim,
                trigger_touch: false,
            },
            &t,
        );
        assert_eq!(estimated, t.fingertip_pointing_m);
        joints[INDEX_TIP_JOINT] = HandJoint {
            pose: Pose {
                position: Vec3::new(0.1, 0.2, 0.3),
                orientation: Quat::IDENTITY,
            },
            radius: 0.01,
            valid: true,
            tracked: true,
        };
        let tracked = fingertip(
            Hand::Right,
            FingertipSource {
                joints: Some(&joints),
                aim,
                trigger_touch: false,
            },
            &t,
        );
        assert_eq!(tracked, Vec3::new(0.1, 0.2, 0.3));
        // On the trigger, and mirrored for the left hand.
        let resting = fingertip(
            Hand::Left,
            FingertipSource {
                joints: None,
                aim,
                trigger_touch: true,
            },
            &t,
        );
        let r = t.fingertip_resting_m;
        assert_eq!(resting, Vec3::new(-r.x, r.y, r.z));
    }
}
