//! The per-session input state: acquiring the actions at bring-up (created or re-wrapped), the
//! action spaces and hand trackers, and releasing them at teardown (stashed or destroyed).

use std::sync::atomic::Ordering;

use anyhow::Context as _;
use openxr as xr;
use openxr::{sys, sys::Handle as _};

use crate::{
    controllers::frame::{self, ButtonSet, Hand, InteractionProfile},
    vr::input::{
        ActionsOrigin, HandTrackingStatus, InputStatus, PROFILES_DIRTY, STATUS,
        actions::Actions,
        layout::{self, HandsAction, Profile},
        stash::{self, InputStash},
    },
};

/// The controller input of one session. Created at bring-up by [`InputState::acquire`] and ended
/// by exactly one of [`InputState::persist`] or [`InputState::destroy`] at teardown; dropping it
/// otherwise leaks the actions.
pub(crate) struct InputState {
    pub(super) actions: Actions,
    /// The grip spaces, indexed by [`Hand::index`].
    pub(super) grip: [xr::Space; 2],
    /// The aim spaces, indexed by [`Hand::index`].
    pub(super) aim: [xr::Space; 2],
    /// The hand trackers, created on demand while hand tracking is wanted and supported.
    pub(super) trackers: Option<[xr::HandTracker; 2]>,
    /// Whether tracker creation failed, so it is not retried every frame.
    pub(super) trackers_failed: bool,
    /// Whether the instance has `XR_EXT_hand_tracking` and the system supports it.
    pub(super) hand_tracking_supported: bool,
    /// The instance's joint locate call, present with `XR_EXT_hand_tracking`.
    pub(super) locate_hand_joints: Option<sys::pfn::LocateHandJointsEXT>,
    /// Whether the joints are located with the controller-conforming motion range: the instance
    /// has `XR_EXT_hand_joints_motion_range`.
    pub(super) conforming_motion_range: bool,
    /// The profile paths, for recognising the bound profile.
    pub(super) profile_paths: [(xr::Path, InteractionProfile); 2],
    /// The interaction profile of each hand.
    pub(super) profiles: [InteractionProfile; 2],
    /// The buttons of the previous frame, for the derived presses' hysteresis.
    pub(super) previous_buttons: [ButtonSet; 2],
    /// Whether the per-frame update is in a failure streak, and its length, for rate-limited
    /// logging.
    pub(super) failing: bool,
    pub(super) failure_count: u32,
}

impl InputState {
    /// Obtain the actions for `session`: re-wrap the stashed ones when they belong to `instance`
    /// and match this layout, attaching them if `session` is not the one they were attached to;
    /// otherwise create, suggest, and attach fresh ones. Then create the action spaces and, when
    /// wanted, the hand trackers. Records the outcome for [`crate::vr::input::status`].
    pub(crate) fn acquire(
        instance: &xr::Instance,
        system: xr::SystemId,
        session: &xr::Session<xr::D3D11>,
        want_hand_tracking: bool,
    ) -> anyhow::Result<InputState> {
        let result = Self::acquire_inner(instance, system, session, want_hand_tracking);
        let mut status = STATUS.lock();
        match &result {
            Ok((state, origin)) => {
                *status = InputStatus {
                    attached: true,
                    origin: Some(*origin),
                    error: None,
                    profiles: [InteractionProfile::None; 2],
                    hand_tracking: state.hand_tracking_status(instance),
                };
            }
            Err(e) => {
                *status = InputStatus {
                    error: Some(format!("{e:#}")),
                    ..InputStatus::default()
                };
            }
        }
        result.map(|(state, _)| state)
    }

    /// Release the input for a reinject: destroy the spaces and trackers, and stash the action
    /// handles without destroying them, so the reinjected payload re-wraps the attached sets.
    pub(crate) fn persist(self, instance: &xr::Instance, session: &xr::Session<xr::D3D11>) {
        let stash = InputStash {
            instance: instance.as_raw().into_raw(),
            session: session.as_raw().into_raw(),
            layout: layout::fingerprint(),
            handles: self.actions.raw_handles(),
        };
        let InputState {
            actions,
            grip,
            aim,
            trackers,
            ..
        } = self;
        drop(trackers);
        drop(grip);
        drop(aim);
        stash::store(&stash);
        // The handles now belong to the stash; `Actions` has no destructor, so dropping it
        // releases them without destroying anything.
        drop(actions);
        tracing::info!(target: "vr", "input: stashed the action handles for the next injection");
        Self::clear_published();
    }

    /// End the input for good: destroy the spaces, trackers, and action sets, and clear any stash.
    pub(crate) fn destroy(self, instance: &xr::Instance) {
        let InputState {
            actions,
            grip,
            aim,
            trackers,
            ..
        } = self;
        drop(trackers);
        drop(grip);
        drop(aim);
        actions.destroy(instance);
        stash::clear();
        Self::clear_published();
    }

    fn clear_published() {
        frame::clear();
        let _ = crate::vr::input::haptics::take();
        let mut status = STATUS.lock();
        status.attached = false;
        status.profiles = [InteractionProfile::None; 2];
        status.hand_tracking.trackers_created = false;
    }

    pub(super) fn hand_tracking_status(&self, instance: &xr::Instance) -> HandTrackingStatus {
        HandTrackingStatus {
            extension_enabled: instance.exts().ext_hand_tracking.is_some(),
            system_supported: self.hand_tracking_supported,
            trackers_created: self.trackers.is_some(),
            conforming_motion_range: instance.exts().ext_hand_joints_motion_range.is_some(),
        }
    }

    /// Create the hand trackers if they are wanted, supported, and not yet created, or drop them
    /// if they are not wanted.
    pub(super) fn sync_trackers(&mut self, session: &xr::Session<xr::D3D11>, want: bool) {
        if !want || !self.hand_tracking_supported {
            if self.trackers.take().is_some() {
                tracing::info!(target: "vr", "input: hand trackers released");
                STATUS.lock().hand_tracking.trackers_created = false;
            }
            self.trackers_failed = false;
            return;
        }
        if self.trackers.is_some() || self.trackers_failed {
            return;
        }
        let created = session
            .create_hand_tracker(xr::Hand::LEFT)
            .and_then(|left| Ok([left, session.create_hand_tracker(xr::Hand::RIGHT)?]));
        match created {
            Ok(trackers) => {
                tracing::info!(target: "vr", "input: hand trackers created");
                self.trackers = Some(trackers);
                STATUS.lock().hand_tracking.trackers_created = true;
            }
            Err(e) => {
                tracing::warn!(target: "vr", "input: creating the hand trackers failed: {e}");
                self.trackers_failed = true;
            }
        }
    }

    fn acquire_inner(
        instance: &xr::Instance,
        system: xr::SystemId,
        session: &xr::Session<xr::D3D11>,
        want_hand_tracking: bool,
    ) -> anyhow::Result<(InputState, ActionsOrigin)> {
        let instance_raw = instance.as_raw().into_raw();
        let session_raw = session.as_raw().into_raw();
        let (actions, origin) = match stash::take() {
            Some(s) if s.layout == layout::fingerprint() && s.instance == instance_raw => {
                // SAFETY: the stash names this instance and this layout, and was written by the
                // previous payload's `persist`, which destroyed nothing.
                let actions = unsafe { Actions::rewrap(instance, &s.handles) }
                    .context("input: re-wrapping the persisted actions")?;
                if s.session == session_raw {
                    // Reading a state validates the re-wrapped handles against the session.
                    actions
                        .pose_active(session, HandsAction::GripPose, Hand::Left)
                        .context("input: the persisted actions are unusable")?;
                    (actions, ActionsOrigin::Rewrapped)
                } else {
                    (actions, ActionsOrigin::RewrappedAndAttached)
                }
            }
            Some(s) if s.instance == instance_raw => {
                // The previous payload's action sets are alive on this instance under this
                // layout's names, and a set it attached to this session blocks attaching another.
                // Put the stash back so a payload with the matching layout can still use it.
                stash::store(&s);
                anyhow::bail!(
                    "input: controller input is unavailable: a payload with a different action \
                     layout created the action sets on the persisted OpenXR instance; restart the \
                     game"
                );
            }
            stale => {
                if let Some(s) = stale {
                    tracing::warn!(
                        target: "vr",
                        same_instance = s.instance == instance_raw,
                        same_layout = s.layout == layout::fingerprint(),
                        "input: discarding an action-handle stash that does not match; creating fresh actions",
                    );
                }
                let actions = Actions::create(instance).context("input: creating the actions")?;
                actions.suggest_bindings(instance);
                (actions, ActionsOrigin::Created)
            }
        };
        tracing::info!(target: "vr", ?origin, "input: actions acquired");

        if origin != ActionsOrigin::Rewrapped
            && let Err(e) = actions.attach(session)
        {
            actions.destroy(instance);
            return Err(e.context(
                "input: controller input is unavailable for this session (a session accepts \
                 action sets once; if a previous payload attached different ones, restart the \
                 game)",
            ));
        }

        let spaces = Hand::BOTH.map(|hand| {
            Ok::<_, anyhow::Error>((
                actions.create_space(session, HandsAction::GripPose, hand)?,
                actions.create_space(session, HandsAction::AimPose, hand)?,
            ))
        });
        let [left, right] = spaces;
        let (left, right) = match (left, right) {
            (Ok(l), Ok(r)) => (l, r),
            (Err(e), _) | (_, Err(e)) => {
                // The sets are attached to this session now, and a session accepts action sets
                // once, so destroying them would leave it without controller input for good.
                // Stash them instead, for a reinject to re-wrap and retry the spaces.
                stash::store(&InputStash {
                    instance: instance_raw,
                    session: session_raw,
                    layout: layout::fingerprint(),
                    handles: actions.raw_handles(),
                });
                drop(actions);
                return Err(e.context("input: creating the controller spaces"));
            }
        };

        let hand_tracking_supported = instance.exts().ext_hand_tracking.is_some()
            && instance.supports_hand_tracking(system).unwrap_or(false);
        let profile_paths = Profile::ALL.map(|p| {
            (
                instance.string_to_path(p.path()).unwrap_or(xr::Path::NULL),
                match p {
                    Profile::ValveIndex => InteractionProfile::ValveIndex,
                    Profile::OculusTouch => InteractionProfile::OculusTouch,
                },
            )
        });
        PROFILES_DIRTY.store(true, Ordering::Relaxed);

        let mut state = InputState {
            actions,
            grip: [left.0, right.0],
            aim: [left.1, right.1],
            trackers: None,
            trackers_failed: false,
            hand_tracking_supported,
            locate_hand_joints: instance
                .exts()
                .ext_hand_tracking
                .map(|ext| ext.locate_hand_joints),
            conforming_motion_range: instance.exts().ext_hand_joints_motion_range.is_some(),
            profile_paths,
            profiles: [InteractionProfile::None; 2],
            previous_buttons: [ButtonSet::EMPTY; 2],
            failing: false,
            failure_count: 0,
        };
        state.sync_trackers(session, want_hand_tracking);
        Ok((state, origin))
    }
}
