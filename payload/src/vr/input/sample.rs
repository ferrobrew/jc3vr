//! The per-frame input update: sync the actions, read every input, locate the controller spaces
//! and hand joints at the predicted display time, re-base them into the cockpit frame, publish the
//! [`ControllerFrame`], and flush the haptics queue.

use std::{ptr, sync::atomic::Ordering, time::Instant};

use glam::{Quat, Vec3};
use openxr as xr;
use openxr::sys;

use crate::{
    controllers::{
        config::ControllersConfig,
        frame::{
            self, ActionSetKind, ButtonSet, ControllerFrame, HAND_JOINT_COUNT, Hand, HandFrame,
            HandInputs, HandJoint, InteractionProfile, Pose, TrackedPose, derive_buttons,
        },
        scheme,
    },
    vr::{
        input::{
            PROFILES_DIRTY, STATUS, active_action_set,
            haptics::{self, HapticPulse},
            layout::{BoolInput, FloatInput, HandsAction, Vec2Input},
            state::InputState,
        },
        recenter::Baseline,
    },
};

impl InputState {
    /// Run the frame's input update. Called by the VR frame loop under the runtime lock, once per
    /// frame, after the eye views are located. With the controllers disabled, publishes nothing
    /// and drops the queued haptics.
    pub(crate) fn update(
        &mut self,
        session: &xr::Session<xr::D3D11>,
        local: &xr::Space,
        time: xr::Time,
        baseline: Option<Baseline>,
        cfg: &ControllersConfig,
    ) {
        if !cfg.enabled {
            frame::clear();
            let _ = haptics::take();
            self.previous_buttons = [ButtonSet::EMPTY; 2];
            return;
        }
        self.sync_trackers(session, cfg.input.hand_tracking);

        let mode = active_action_set();
        let focused = match self.actions.sync(session, mode) {
            Ok(focused) => focused,
            Err(e) => {
                self.note_failure(&e);
                frame::clear();
                return;
            }
        };
        self.refresh_profiles(session);

        let sampling = Sampling {
            session,
            local,
            time,
            baseline,
            mode,
            cfg,
        };
        let mut hands: [HandFrame; 2] = Default::default();
        let mut first_error = None;
        for hand in Hand::BOTH {
            match self.sample_hand(&sampling, hand) {
                Ok(frame) => hands[hand.index()] = frame,
                Err(e) => {
                    first_error.get_or_insert(e);
                }
            }
        }
        self.previous_buttons = [hands[0].buttons, hands[1].buttons];

        let frame = ControllerFrame {
            sequence: 0,
            display_time_ns: time.as_nanos(),
            published_at: Instant::now(),
            focused,
            action_set: mode,
            hands,
        };
        scheme::tick(&frame, cfg);
        frame::publish(frame);

        if let Err(e) = self.flush_haptics(session, cfg) {
            first_error.get_or_insert(e);
        }
        match first_error {
            Some(e) => self.note_failure(&e),
            None => self.note_success(),
        }
    }

    /// Re-query each hand's interaction profile after the runtime reported a change.
    fn refresh_profiles(&mut self, session: &xr::Session<xr::D3D11>) {
        if !PROFILES_DIRTY.swap(false, Ordering::Relaxed) {
            return;
        }
        for hand in Hand::BOTH {
            let path = session.current_interaction_profile(self.actions.hand_path(hand));
            self.profiles[hand.index()] = match path {
                Ok(p) if p == xr::Path::NULL => InteractionProfile::None,
                Ok(p) => self
                    .profile_paths
                    .iter()
                    .find(|(known, _)| *known == p)
                    .map_or(InteractionProfile::Other, |&(_, profile)| profile),
                Err(e) => {
                    tracing::warn!(target: "vr", "input: querying the {} profile failed: {e}", hand.label());
                    InteractionProfile::None
                }
            };
        }
        tracing::info!(target: "vr", profiles = ?self.profiles, "input: interaction profiles");
        STATUS.lock().profiles = self.profiles;
    }

    fn sample_hand(&self, sampling: &Sampling, hand: Hand) -> anyhow::Result<HandFrame> {
        let Sampling {
            session,
            local,
            time,
            baseline,
            mode,
            cfg,
        } = *sampling;
        let actions = &self.actions;
        let mut active = false;
        let mut read_bool = |input| -> anyhow::Result<bool> {
            let state = actions.bool_state(session, mode, input, hand)?;
            active |= state.active;
            Ok(state.value)
        };
        let trigger_click = read_bool(BoolInput::TriggerClick)?;
        let trigger_touch = read_bool(BoolInput::TriggerTouch)?;
        let thumbstick_click = read_bool(BoolInput::ThumbstickClick)?;
        let thumbstick_touch = read_bool(BoolInput::ThumbstickTouch)?;
        let trackpad_touch = read_bool(BoolInput::TrackpadTouch)?;
        let a_click = read_bool(BoolInput::AClick)?;
        let a_touch = read_bool(BoolInput::ATouch)?;
        let b_click = read_bool(BoolInput::BClick)?;
        let b_touch = read_bool(BoolInput::BTouch)?;
        let system_click = read_bool(BoolInput::SystemClick)?;

        let mut read_float = |input| -> anyhow::Result<f32> {
            let state = actions.float_state(session, mode, input, hand)?;
            active |= state.active;
            Ok(state.value)
        };
        let trigger = read_float(FloatInput::Trigger)?;
        let squeeze = read_float(FloatInput::Squeeze)?;
        let squeeze_force = read_float(FloatInput::SqueezeForce)?;
        let trackpad_force = read_float(FloatInput::TrackpadForce)?;

        let mut read_vec2 = |input| -> anyhow::Result<glam::Vec2> {
            let state = actions.vec2_state(session, mode, input, hand)?;
            active |= state.active;
            Ok(state.value)
        };
        let thumbstick = read_vec2(Vec2Input::Thumbstick)?;
        let trackpad = read_vec2(Vec2Input::Trackpad)?;

        active |= actions.pose_active(session, HandsAction::GripPose, hand)?;

        let inputs = HandInputs {
            trigger,
            trigger_click,
            trigger_touch,
            squeeze,
            squeeze_force,
            thumbstick,
            thumbstick_click,
            thumbstick_touch,
            trackpad,
            trackpad_force,
            trackpad_touch,
            a_click,
            a_touch,
            b_click,
            b_touch,
            system_click,
        };
        let buttons = derive_buttons(
            &inputs,
            self.previous_buttons[hand.index()],
            cfg.input.trigger_press,
            cfg.input.squeeze_press,
        );

        let locate = |space: &xr::Space| -> anyhow::Result<TrackedPose> {
            let (location, velocity) = space.relate(local, time)?;
            Ok(tracked_pose(&location, Some(&velocity), baseline))
        };
        let grip = locate(&self.grip[hand.index()])?;
        let aim = locate(&self.aim[hand.index()])?;

        let joints = match &self.trackers {
            Some(trackers) => local
                .locate_hand_joints(&trackers[hand.index()], time)?
                .map(|locations| joints_to_cockpit(&locations, baseline)),
            None => None,
        };

        Ok(HandFrame {
            active,
            profile: self.profiles[hand.index()],
            grip,
            aim,
            inputs,
            buttons,
            joints,
        })
    }

    /// Play the queued pulses, scaled by the haptics intensity, or drop them with haptics off.
    fn flush_haptics(
        &self,
        session: &xr::Session<xr::D3D11>,
        cfg: &ControllersConfig,
    ) -> anyhow::Result<()> {
        let pending = haptics::take();
        let haptics_cfg = cfg.weapons.haptics;
        if !haptics_cfg.enabled {
            return Ok(());
        }
        for hand in Hand::BOTH {
            if let Some(pulse) = pending[hand.index()] {
                let vibration = vibration(pulse, haptics_cfg.intensity);
                self.actions.apply_haptic(session, hand, &vibration)?;
            }
        }
        Ok(())
    }

    fn note_failure(&mut self, error: &anyhow::Error) {
        self.failure_count += 1;
        if !self.failing {
            self.failing = true;
            tracing::warn!(target: "vr", "input: the frame update failed: {error:#}");
        } else if self.failure_count.is_multiple_of(FAILURE_LOG_INTERVAL) {
            tracing::warn!(target: "vr", count = self.failure_count, "input: the frame update is still failing: {error:#}");
        }
    }

    fn note_success(&mut self) {
        if self.failing {
            tracing::info!(target: "vr", frames = self.failure_count, "input: the frame update recovered");
            self.failing = false;
            self.failure_count = 0;
        }
    }
}

/// What one frame's sample reads against.
struct Sampling<'a> {
    session: &'a xr::Session<xr::D3D11>,
    /// The LOCAL reference space, the base every location is taken in.
    local: &'a xr::Space,
    /// The frame's predicted display time.
    time: xr::Time,
    /// The recenter baseline, re-basing LOCAL into the cockpit frame.
    baseline: Option<Baseline>,
    /// The active mode action set.
    mode: ActionSetKind,
    cfg: &'a ControllersConfig,
}

/// How many consecutive failed frames pass between repeated failure logs (a few seconds at a
/// headset's frame rate).
const FAILURE_LOG_INTERVAL: u32 = 300;

/// `XR_FREQUENCY_UNSPECIFIED`: the runtime's default vibration frequency.
const FREQUENCY_UNSPECIFIED: f32 = 0.0;

fn vibration(pulse: HapticPulse, intensity: f32) -> sys::HapticVibration {
    sys::HapticVibration {
        ty: sys::HapticVibration::TYPE,
        next: ptr::null(),
        duration: xr::Duration::from_nanos(
            i64::try_from(pulse.duration.as_nanos()).unwrap_or(i64::MAX),
        ),
        frequency: pulse.frequency_hz.unwrap_or(FREQUENCY_UNSPECIFIED),
        amplitude: (pulse.amplitude * intensity).clamp(0.0, 1.0),
    }
}

/// Convert a located space into a [`TrackedPose`] in the cockpit frame.
fn tracked_pose(
    location: &xr::SpaceLocation,
    velocity: Option<&xr::SpaceVelocity>,
    baseline: Option<Baseline>,
) -> TrackedPose {
    let flags = location.location_flags;
    let velocity_flags = velocity.map(|v| v.velocity_flags);
    let has = |f: xr::SpaceVelocityFlags| velocity_flags.is_some_and(|flags| flags.contains(f));
    TrackedPose {
        pose: to_cockpit(location.pose, baseline),
        position_valid: flags.contains(xr::SpaceLocationFlags::POSITION_VALID),
        orientation_valid: flags.contains(xr::SpaceLocationFlags::ORIENTATION_VALID),
        position_tracked: flags.contains(xr::SpaceLocationFlags::POSITION_TRACKED),
        orientation_tracked: flags.contains(xr::SpaceLocationFlags::ORIENTATION_TRACKED),
        linear_velocity: velocity
            .filter(|_| has(xr::SpaceVelocityFlags::LINEAR_VALID))
            .map(|v| rotate_to_cockpit(v.linear_velocity, baseline)),
        angular_velocity: velocity
            .filter(|_| has(xr::SpaceVelocityFlags::ANGULAR_VALID))
            .map(|v| rotate_to_cockpit(v.angular_velocity, baseline)),
    }
}

fn joints_to_cockpit(
    locations: &xr::HandJointLocations,
    baseline: Option<Baseline>,
) -> [HandJoint; HAND_JOINT_COUNT] {
    std::array::from_fn(|i| {
        let joint = &locations[i];
        let flags = joint.location_flags;
        HandJoint {
            pose: to_cockpit(joint.pose, baseline),
            radius: joint.radius,
            valid: flags.contains(xr::SpaceLocationFlags::POSITION_VALID)
                && flags.contains(xr::SpaceLocationFlags::ORIENTATION_VALID),
            tracked: flags.contains(xr::SpaceLocationFlags::POSITION_TRACKED)
                && flags.contains(xr::SpaceLocationFlags::ORIENTATION_TRACKED),
        }
    })
}

/// Re-base a LOCAL-space pose into the cockpit frame, as the eye views are.
fn to_cockpit(pose: xr::Posef, baseline: Option<Baseline>) -> Pose {
    let pose = baseline.map_or(pose, |b| b.rebase(pose));
    let p = pose.position;
    let q = pose.orientation;
    Pose {
        position: Vec3::new(p.x, p.y, p.z),
        orientation: Quat::from_xyzw(q.x, q.y, q.z, q.w),
    }
}

/// Rotate a LOCAL-space vector (a velocity) into the cockpit frame.
fn rotate_to_cockpit(v: xr::Vector3f, baseline: Option<Baseline>) -> Vec3 {
    let v = Vec3::new(v.x, v.y, v.z);
    baseline.map_or(v, |b| b.rotate(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tracked_location_converts_with_its_flags() {
        let location = xr::SpaceLocation {
            location_flags: xr::SpaceLocationFlags::POSITION_VALID
                | xr::SpaceLocationFlags::ORIENTATION_VALID
                | xr::SpaceLocationFlags::ORIENTATION_TRACKED,
            pose: xr::Posef {
                orientation: xr::Quaternionf::IDENTITY,
                position: xr::Vector3f {
                    x: 0.1,
                    y: 1.2,
                    z: -0.3,
                },
            },
        };
        let velocity = xr::SpaceVelocity {
            velocity_flags: xr::SpaceVelocityFlags::LINEAR_VALID,
            linear_velocity: xr::Vector3f {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
            angular_velocity: xr::Vector3f::default(),
        };
        let tracked = tracked_pose(&location, Some(&velocity), None);
        assert!(tracked.is_valid());
        assert!(!tracked.is_tracked(), "the position is inferred");
        assert_eq!(tracked.pose.position, Vec3::new(0.1, 1.2, -0.3));
        assert_eq!(tracked.linear_velocity, Some(Vec3::X));
        assert_eq!(tracked.angular_velocity, None);
    }

    #[test]
    fn haptic_amplitude_is_scaled_and_clamped() {
        let pulse = HapticPulse {
            duration: std::time::Duration::from_millis(30),
            frequency_hz: None,
            amplitude: 0.8,
        };
        let v = vibration(pulse, 0.5);
        assert!((v.amplitude - 0.4).abs() < 1e-6);
        assert_eq!(v.frequency, FREQUENCY_UNSPECIFIED);
        assert_eq!(v.duration.as_nanos(), 30_000_000);
        assert_eq!(vibration(pulse, 2.0).amplitude, 1.0);
    }
}
