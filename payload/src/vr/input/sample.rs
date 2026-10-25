//! The per-frame input update: sync the actions, read every raw input, locate the controller
//! spaces and hand joints at the predicted display time, re-base them into the cockpit frame,
//! publish the [`ControllerFrame`], read and publish the active semantic actions
//! ([`SemanticFrame`]), run the self-check and the bound-source refresh, and play what the
//! haptics mixer gives each hand.

use std::{ptr, sync::atomic::Ordering, time::Instant};

use glam::{Quat, Vec3};
use openxr as xr;
use openxr::sys;
use tracking_format::input::{DiscontinuityReason, InputRoute, RouteSample, ValueBits};

use crate::{
    controllers::{
        config::ControllersConfig,
        frame::{
            self, ButtonSet, ControllerFrame, HAND_JOINT_COUNT, Hand, HandFrame, HandInputs,
            HandJoint, InteractionProfile, Pose, TrackedPose, derive_buttons,
        },
        haptics::{self, mixer::HapticPulse},
        scheme,
    },
    vr::{
        input::{
            PROFILES_DIRTY, STATUS,
            actions::{SetId, State},
            active_action_set,
            layout::{BoolInput, FloatInput, HandsAction, Vec2Input},
            replay,
            self_check::CheckFrame,
            semantic::{
                SemanticSet,
                frame::{
                    self as semantic_frame, ActionState, ActionValue, SemanticFrame, SetState,
                },
                history,
            },
            sources,
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
            history::interrupt(DiscontinuityReason::Unavailable);
            frame::clear();
            semantic_frame::clear();
            haptics::clear();
            self.previous_buttons = [ButtonSet::EMPTY; 2];
            return;
        }
        self.sync_trackers(session, cfg.input.hand_tracking);

        let mode = active_action_set();
        let focused = match self.actions.sync(session, mode) {
            Ok(focused) => focused,
            Err(e) => {
                self.note_failure(&e);
                history::interrupt(DiscontinuityReason::Unavailable);
                frame::clear();
                semantic_frame::clear();
                return;
            }
        };
        let observed_at = history::observation_time();
        if self.refresh_profiles(session) {
            self.sources.mark_stale();
        }
        self.sources.update(
            session,
            &self.actions,
            &[
                SetId::Hands,
                SetId::Semantic(SemanticSet::Global),
                SetId::Semantic(SemanticSet::of(mode)),
            ],
        );

        // A replay's recorded frames stand in for the sampled ones. The actions are still synced,
        // which keeps the session's input healthy, and the haptics still reach the controllers.
        if replay::publish_replayed(cfg) {
            match self.flush_haptics(session, cfg) {
                Ok(()) => self.note_success(),
                Err(e) => self.note_failure(&e),
            }
            return;
        }

        replay::reset_on_handback(&mut self.previous_buttons);
        let sampling = Sampling {
            session,
            local,
            time,
            baseline,
            cfg,
        };
        let mut hands: [HandFrame; 2] = Default::default();
        let mut first_error = None;
        let generation = sources::observation_generation(self.profiles, self.dominant);
        let mut observed = Vec::new();
        let mut raw = [State::<bool>::default(); 3];
        for (index, (hand, input)) in [
            (Hand::Left, BoolInput::ThumbstickClick),
            (Hand::Right, BoolInput::ThumbstickClick),
            (Hand::Right, BoolInput::BClick),
        ]
        .into_iter()
        .enumerate()
        {
            match self.actions.bool_state(session, input, hand) {
                Ok(state) => {
                    raw[index] = state;
                    observed.push(RouteSample {
                        route: InputRoute::Raw(index as u8),
                        value: ValueBits::Bool(state.value),
                        active: state.active,
                        down: state.value,
                        change: Some(state.change),
                        sources: sources::observation_sources(SetId::Hands, input.name()),
                    });
                }
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        for hand in Hand::BOTH {
            match self.sample_hand(&sampling, hand, &raw) {
                Ok((frame, joints_error)) => {
                    hands[hand.index()] = frame;
                    if let Some(e) = joints_error {
                        first_error.get_or_insert(e);
                    }
                }
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

        let mut semantic = SemanticFrame {
            published_at: Instant::now(),
            action_set: mode,
            global: self.read_set(
                session,
                SemanticSet::Global,
                cfg,
                &mut first_error,
                &mut observed,
            ),
            mode: self.read_set(
                session,
                SemanticSet::of(mode),
                cfg,
                &mut first_error,
                &mut observed,
            ),
        };
        if cfg.input.self_check {
            self.self_check.update(&CheckFrame {
                session,
                actions: &self.actions,
                profiles: self.profiles,
                mode,
                focused,
                dominant: self.dominant,
                hands: &frame.hands,
                semantic: &semantic,
                config: &cfg.input,
            });
        }
        let (frame_sequence, published_at) = frame::publish(frame);
        semantic.published_at = published_at;
        semantic_frame::publish(semantic);
        history::publish_observation(
            history::LiveSample {
                samples: observed,
                context: mode.index() as u64,
                focused,
                generation,
                observed_at,
                frame_sequence,
            },
            first_error.is_none(),
        );

        if let Err(e) = self.flush_haptics(session, cfg) {
            first_error.get_or_insert(e);
        }
        match first_error {
            Some(e) => self.note_failure(&e),
            None => self.note_success(),
        }
    }

    /// Read every action of the semantic `set`, both hands, and derive each one's digital state
    /// against the previous frame's ([`semantic_frame::digital`]). An action that cannot be read
    /// reads at rest and inactive, and the first error rides alongside.
    fn read_set(
        &self,
        session: &xr::Session<xr::D3D11>,
        set: SemanticSet,
        cfg: &ControllersConfig,
        first_error: &mut Option<anyhow::Error>,
        observed: &mut Vec<RouteSample>,
    ) -> SetState {
        let previous = semantic_frame::previous_down(set);
        let rules = Hand::BOTH.map(|hand| cfg.input.press_rules(self.profiles[hand.index()], hand));
        let actions = set
            .actions()
            .iter()
            .enumerate()
            .map(|(index, def)| {
                Hand::BOTH.map(|hand| {
                    let (mut state, change) = self
                        .actions
                        .semantic_observation(session, set, index, hand)
                        .map(|(state, change)| (state, Some(change)))
                        .unwrap_or_else(|e| {
                            first_error.get_or_insert(e);
                            (ActionState::rest(def.kind), None)
                        });
                    let was_down = previous
                        .as_ref()
                        .and_then(|p| p.get(index))
                        .is_some_and(|p| p[hand.index()]);
                    state.down = semantic_frame::digital(
                        set,
                        index,
                        state.value,
                        &rules[hand.index()],
                        was_down,
                    );
                    observed.push(RouteSample {
                        route: InputRoute::Semantic {
                            set: set.index() as u8,
                            action: index as u16,
                            hand: hand.index() as u8,
                        },
                        value: match state.value {
                            ActionValue::Bool(value) => ValueBits::Bool(value),
                            ActionValue::Float(value) => ValueBits::Float(value.to_bits()),
                            ActionValue::Vec2(value) => {
                                ValueBits::Vec2([value.x.to_bits(), value.y.to_bits()])
                            }
                        },
                        active: state.active,
                        down: state.down,
                        change,
                        sources: sources::observation_sources(SetId::Semantic(set), def.name),
                    });
                    state
                })
            })
            .collect();
        SetState { set, actions }
    }

    /// Re-query each hand's interaction profile after the runtime reported a change. Returns
    /// whether it re-queried.
    fn refresh_profiles(&mut self, session: &xr::Session<xr::D3D11>) -> bool {
        if !PROFILES_DIRTY.swap(false, Ordering::Relaxed) {
            return false;
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
        true
    }

    /// Sample `hand`. A failure to locate the hand joints does not fail the hand, since the joints
    /// only feed the finger curls: the frame goes without them, and the error rides alongside.
    fn sample_hand(
        &self,
        sampling: &Sampling,
        hand: Hand,
        raw: &[State<bool>; 3],
    ) -> anyhow::Result<(HandFrame, Option<anyhow::Error>)> {
        let Sampling {
            session,
            local,
            time,
            baseline,
            cfg,
        } = *sampling;
        let actions = &self.actions;
        let mut active = false;
        let mut read_bool = |input| -> anyhow::Result<bool> {
            let state = match input {
                BoolInput::ThumbstickClick => raw[hand.index()],
                BoolInput::BClick if hand == Hand::Right => raw[2],
                _ => actions.bool_state(session, input, hand)?,
            };
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
            let state = actions.float_state(session, input, hand)?;
            active |= state.active;
            Ok(state.value)
        };
        let trigger = read_float(FloatInput::Trigger)?;
        let squeeze = read_float(FloatInput::Squeeze)?;
        let squeeze_force = read_float(FloatInput::SqueezeForce)?;
        let trackpad_force = read_float(FloatInput::TrackpadForce)?;

        let mut read_vec2 = |input| -> anyhow::Result<glam::Vec2> {
            let state = actions.vec2_state(session, input, hand)?;
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
        let profile = self.profiles[hand.index()];
        let buttons = derive_buttons(
            &inputs,
            self.previous_buttons[hand.index()],
            &cfg.input.press_rules(profile, hand),
        );

        let locate = |space: &xr::Space| -> anyhow::Result<TrackedPose> {
            let (location, velocity) = space.relate(local, time)?;
            Ok(tracked_pose(&location, Some(&velocity), baseline))
        };
        let grip = locate(&self.grip[hand.index()])?;
        let aim = locate(&self.aim[hand.index()])?;

        let mut joints_error = None;
        let joints = match (&self.trackers, self.locate_hand_joints) {
            (Some(trackers), Some(locate)) => {
                let motion_range = self
                    .conforming_motion_range
                    .then_some(sys::HandJointsMotionRangeEXT::CONFORMING_TO_CONTROLLER);
                // SAFETY: `locate` is the instance's own entry point, and the tracker and `local`
                // belong to the session being sampled.
                let located = unsafe {
                    locate_joints(locate, &trackers[hand.index()], local, time, motion_range)
                };
                match located {
                    Ok(locations) => {
                        locations.map(|locations| joints_to_cockpit(&locations, baseline))
                    }
                    Err(e) => {
                        joints_error = Some(anyhow::anyhow!(
                            "locating the {} hand joints: {e}",
                            hand.label()
                        ));
                        None
                    }
                }
            }
            _ => None,
        };

        let frame = HandFrame {
            active,
            profile,
            grip,
            aim,
            inputs,
            buttons,
            joints,
        };
        Ok((frame, joints_error))
    }

    /// Play what the haptics mixer gives each hand, scaled by the haptics intensity, or forget
    /// everything it holds with the haptics off.
    fn flush_haptics(
        &self,
        session: &xr::Session<xr::D3D11>,
        cfg: &ControllersConfig,
    ) -> anyhow::Result<()> {
        let haptics_cfg = cfg.haptics;
        if !haptics_cfg.enabled {
            haptics::clear();
            return Ok(());
        }
        let pending = haptics::mix(Instant::now(), &haptics_cfg.mix);
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

/// Locate a hand's joints in `base` at `time`, with `motion_range` chained onto the locate info when
/// given (`XR_EXT_hand_joints_motion_range`; the crate's own locate chains nothing). `None` while
/// the runtime reports the hand inactive.
///
/// # Safety
///
/// `locate` must be the `xrLocateHandJointsEXT` of the instance that owns `tracker` and `base`, and
/// a motion range may be given only when that instance has `XR_EXT_hand_joints_motion_range`.
unsafe fn locate_joints(
    locate: sys::pfn::LocateHandJointsEXT,
    tracker: &xr::HandTracker,
    base: &xr::Space,
    time: xr::Time,
    motion_range: Option<sys::HandJointsMotionRangeEXT>,
) -> xr::Result<Option<xr::HandJointLocations>> {
    let range_info = motion_range.map(|range| sys::HandJointsMotionRangeInfoEXT {
        ty: sys::HandJointsMotionRangeInfoEXT::TYPE,
        next: ptr::null(),
        hand_joints_motion_range: range,
    });
    let locate_info = sys::HandJointsLocateInfoEXT {
        ty: sys::HandJointsLocateInfoEXT::TYPE,
        next: range_info
            .as_ref()
            .map_or(ptr::null(), |info| ptr::from_ref(info).cast()),
        base_space: base.as_raw(),
        time,
    };
    let mut locations = [xr::HandJointLocation {
        location_flags: xr::SpaceLocationFlags::EMPTY,
        pose: xr::Posef::IDENTITY,
        radius: 0.0,
    }; HAND_JOINT_COUNT];
    let mut located = sys::HandJointLocationsEXT {
        ty: sys::HandJointLocationsEXT::TYPE,
        next: ptr::null_mut(),
        is_active: false.into(),
        joint_count: HAND_JOINT_COUNT as u32,
        joint_locations: locations.as_mut_ptr(),
    };
    let result = unsafe { locate(tracker.as_raw(), &locate_info, &mut located) };
    if result.into_raw() < 0 {
        return Err(result);
    }
    Ok(bool::from(located.is_active).then_some(locations))
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
