//! Complete live head/controller observations for the wingsuit gestures.
//!
//! The frame loop supplies current rebased eye poses before replay replaces them. The sampler
//! completes the packet with the actual controller publication receipt. No global head is queried.

use std::{sync::LazyLock, time::Instant};

use glam::{Quat, Vec3};
use jc3gi::input::input_action_map::Action::{CANCEL, OPEN_WINGSUIT, WINGSUIT_AIRBRAKE};
use openxr as xr;
use parking_lot::Mutex;

use crate::controllers::{
    bindings::hold::{Desired, MapKind, Target, Want},
    body::BodyDimensions,
    config::GripOffset,
    frame::{ControllerFrame, Pose, TrackedPose},
    mode::{AirborneVariant, Mode},
    vehicles::wingsuit_gestures::{
        FlightPose, GestureCommands, GestureHead, GestureObservation, GestureSample, SuitTransition,
    },
};

/// Source-owned discontinuities. Ordinary head/controller movement does not advance the epoch.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum EpochChange {
    Recenter,
    FollowBank,
    RuntimeReset,
    ReplayStarted,
    ReplayEnded,
    SourceClockChanged,
}

/// The live producer deliberately does not pair replay controllers with a live located head.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum PoseSource {
    Live,
    Replay,
}

/// Head data from this frame's located and already-rebased eye views, before any replay replacement.
/// The same baseline and predicted display time must be passed to controller sampling.
pub(crate) struct LocatedHead {
    pub eyes: [xr::Posef; 2],
    pub flags: xr::ViewStateFlags,
    pub display_time_ns: i64,
}

/// Single-use completion token. Neither a partial head nor a receipt alone is readable.
#[derive(Debug)]
pub(crate) struct PendingHead {
    epoch: u64,
    ticket: u64,
    display_time_ns: i64,
    head: TrackedPose,
}

/// A complete publication. Its controller poses have the same cockpit baseline as its head.
#[derive(Debug, Clone)]
pub(crate) struct FlightPacket {
    head: GestureHead,
    controllers: ControllerFrame,
}

impl FlightPacket {
    pub(crate) fn sample(&self) -> GestureSample {
        self.head.sample
    }

    /// Derive palms and shoulder-relative distances from this packet, not a later snapshot.
    pub(crate) fn measure(
        &self,
        calibration: &FlightCalibration<'_>,
        now: Instant,
    ) -> Option<FlightPose> {
        FlightPose::from_tracking(&GestureObservation {
            sample: self.head.sample,
            head: self.head,
            controllers: &self.controllers,
            grip_offset: calibration.grip_offset,
            dimensions: calibration.dimensions,
            torso: None,
            now,
        })
    }
}

/// Physical player dimensions and the configured controller grip-to-wrist transform.
/// `None` torso in the observation uses the packet's own head yaw rather than a global estimate.
pub(crate) struct FlightCalibration<'a> {
    pub grip_offset: &'a GripOffset,
    pub dimensions: &'a BodyDimensions,
}

/// Start this live frame even when no tracked views are available, clearing the previous packet.
pub(crate) fn begin_located(head: Option<LocatedHead>) -> Option<PendingHead> {
    SLOT.lock().begin(head)
}

/// Complete only after `frame::publish` returns its real sequence and host publication time.
/// `controllers` is the sampled frame retained before that call, never read back from a global slot.
pub(crate) fn complete(
    pending: PendingHead,
    controllers: ControllerFrame,
    receipt: (u64, Instant),
) -> bool {
    SLOT.lock().complete(pending, controllers, receipt)
}

/// Match the input tick's exact controller publication before returning one complete packet.
pub(crate) fn for_frame(frame: &ControllerFrame, now: Instant) -> Option<FlightPacket> {
    SLOT.lock().for_frame(frame, now)
}

/// Drop the current/pending packet when no sample is published. This does not replace the clock.
pub(crate) fn unavailable() {
    SLOT.lock().cancel();
}

/// Called at the actual source mutation, not inferred from a sequence reset or pose-value change.
/// Overflow permanently disables publication instead of reusing an epoch identity.
pub(crate) fn source_changed(change: EpochChange) {
    SLOT.lock().source_changed(change);
}

/// Invalid command contexts emit no partial action list. The caller retains canopy precedence.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum CommandRejection {
    CanopyRequested,
    WrongMode,
    ConflictingCommands,
}

/// Translate typed transitions through the existing narrow player-map action, with mode guards.
/// Both suit transitions use `OPEN_WINGSUIT`; the broad use/vehicle-entry family is not emitted.
pub(crate) fn desired_for(
    commands: GestureCommands,
    mode: Mode,
    canopy_requested: bool,
) -> Result<Vec<Desired>, CommandRejection> {
    if canopy_requested {
        return Err(CommandRejection::CanopyRequested);
    }
    let suit = mode == Mode::Airborne(AirborneVariant::Wingsuit);
    if commands.transition.is_some() && (commands.brake || commands.reserve_pitch) {
        return Err(CommandRejection::ConflictingCommands);
    }
    let transition = match commands.transition {
        Some(SuitTransition::Deploy) if mode == Mode::Airborne(AirborneVariant::Falling) => true,
        Some(SuitTransition::Close) if suit => true,
        Some(SuitTransition::Deploy | SuitTransition::Close) => {
            return Err(CommandRejection::WrongMode);
        }
        None => false,
    };
    if (commands.brake || commands.reserve_pitch) && !suit {
        return Err(CommandRejection::WrongMode);
    }
    let mut desired = Vec::with_capacity(2);
    if transition {
        desired.push(Desired {
            target: Target {
                map: MapKind::Player,
                action: OPEN_WINGSUIT,
            },
            want: Want::Click,
        });
    }
    if commands.brake {
        desired.extend([CANCEL, WINGSUIT_AIRBRAKE].map(|action| Desired {
            target: Target {
                map: MapKind::Player,
                action,
            },
            want: Want::Hold(1.0),
        }));
    }
    Ok(desired)
}

struct Producer {
    epoch: Option<u64>,
    ticket: u64,
    pending: Option<u64>,
    source: PoseSource,
    packet: Option<FlightPacket>,
}

impl Producer {
    fn new() -> Self {
        Self {
            epoch: Some(1),
            ticket: 0,
            pending: None,
            source: PoseSource::Live,
            packet: None,
        }
    }

    fn cancel(&mut self) {
        self.pending = None;
        self.packet = None;
    }

    fn source_changed(&mut self, change: EpochChange) {
        self.cancel();
        self.epoch = self.epoch.and_then(|epoch| epoch.checked_add(1));
        self.source = match change {
            EpochChange::ReplayStarted => PoseSource::Replay,
            EpochChange::ReplayEnded => PoseSource::Live,
            _ => self.source,
        };
    }

    fn begin(&mut self, head: Option<LocatedHead>) -> Option<PendingHead> {
        self.cancel();
        if self.source != PoseSource::Live {
            return None;
        }
        let epoch = self.epoch?;
        let head = head?;
        let pose = located_center(&head)?;
        self.ticket = self.ticket.checked_add(1)?;
        self.pending = Some(self.ticket);
        Some(PendingHead {
            epoch,
            ticket: self.ticket,
            display_time_ns: head.display_time_ns,
            head: pose,
        })
    }

    fn complete(
        &mut self,
        pending: PendingHead,
        mut controllers: ControllerFrame,
        (sequence, published_at): (u64, Instant),
    ) -> bool {
        if self.epoch != Some(pending.epoch)
            || self.pending != Some(pending.ticket)
            || self.source != PoseSource::Live
        {
            return false;
        }
        self.pending = None;
        if controllers.display_time_ns != pending.display_time_ns
            || !controllers.focused
            || sequence == 0
            || controllers
                .hands
                .iter()
                .any(|hand| !hand.active || !hand.grip.is_tracked())
        {
            return false;
        }
        controllers.sequence = sequence;
        controllers.published_at = published_at;
        let sample = GestureSample {
            epoch: pending.epoch,
            sequence,
            time_ns: pending.display_time_ns,
        };
        self.packet = Some(FlightPacket {
            head: GestureHead {
                sample,
                pose: pending.head,
                published_at,
            },
            controllers,
        });
        true
    }

    fn for_frame(&self, frame: &ControllerFrame, now: Instant) -> Option<FlightPacket> {
        let packet = self.packet.as_ref()?;
        let sample = packet.head.sample;
        (self.epoch == Some(sample.epoch)
            && self.source == PoseSource::Live
            && frame.sequence == sample.sequence
            && frame.display_time_ns == sample.time_ns
            && frame.published_at == packet.head.published_at
            && frame.focused
            && now >= frame.published_at
            && frame.is_fresh_at(now))
        .then(|| packet.clone())
    }
}

fn located_center(head: &LocatedHead) -> Option<TrackedPose> {
    let flags = head.flags;
    if !flags.contains(
        xr::ViewStateFlags::POSITION_VALID
            | xr::ViewStateFlags::ORIENTATION_VALID
            | xr::ViewStateFlags::POSITION_TRACKED
            | xr::ViewStateFlags::ORIENTATION_TRACKED,
    ) {
        return None;
    }
    let eyes = head.eyes.map(|pose| Pose {
        position: Vec3::new(pose.position.x, pose.position.y, pose.position.z),
        orientation: Quat::from_xyzw(
            pose.orientation.x,
            pose.orientation.y,
            pose.orientation.z,
            pose.orientation.w,
        ),
    });
    if eyes.iter().any(|pose| {
        !pose.position.is_finite()
            || !pose.orientation.is_finite()
            || (pose.orientation.length_squared() - 1.0).abs() > 0.02
    }) {
        return None;
    }
    Some(TrackedPose {
        pose: Pose {
            position: (eyes[0].position + eyes[1].position) * 0.5,
            orientation: eyes[0]
                .orientation
                .slerp(eyes[1].orientation, 0.5)
                .normalize(),
        },
        position_valid: true,
        orientation_valid: true,
        position_tracked: true,
        orientation_tracked: true,
        linear_velocity: None,
        angular_velocity: None,
    })
}

static SLOT: LazyLock<Mutex<Producer>> = LazyLock::new(|| Mutex::new(Producer::new()));

/// Serialize tests that share the live packet with replay and history source changes.
#[cfg(test)]
pub(crate) fn test_guard() -> impl Drop {
    struct Guard {
        _serial: parking_lot::ReentrantMutexGuard<'static, ()>,
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            source_changed(EpochChange::ReplayEnded);
        }
    }

    let guard = Guard {
        _serial: crate::vr::input::semantic::history::TEST_LOCK.lock(),
    };
    source_changed(EpochChange::ReplayEnded);
    guard
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::controllers::frame::{ActionSetKind, HandFrame};

    fn head(time_ns: i64) -> LocatedHead {
        LocatedHead {
            eyes: [
                xr::Posef {
                    position: xr::Vector3f {
                        x: -0.03,
                        y: 1.6,
                        z: 0.0,
                    },
                    ..xr::Posef::IDENTITY
                },
                xr::Posef {
                    position: xr::Vector3f {
                        x: 0.03,
                        y: 1.6,
                        z: 0.0,
                    },
                    ..xr::Posef::IDENTITY
                },
            ],
            flags: xr::ViewStateFlags::POSITION_VALID
                | xr::ViewStateFlags::ORIENTATION_VALID
                | xr::ViewStateFlags::POSITION_TRACKED
                | xr::ViewStateFlags::ORIENTATION_TRACKED,
            display_time_ns: time_ns,
        }
    }

    fn frame(time_ns: i64, now: Instant) -> ControllerFrame {
        ControllerFrame {
            sequence: 0,
            display_time_ns: time_ns,
            published_at: now,
            focused: true,
            action_set: ActionSetKind::Wingsuit,
            hands: [0, 1].map(|_| HandFrame {
                active: true,
                grip: TrackedPose {
                    pose: Pose::default(),
                    position_valid: true,
                    orientation_valid: true,
                    position_tracked: true,
                    orientation_tracked: true,
                    ..Default::default()
                },
                ..Default::default()
            }),
        }
    }

    fn publish(
        producer: &mut Producer,
        time_ns: i64,
        sequence: u64,
        now: Instant,
    ) -> ControllerFrame {
        let pending = producer.begin(Some(head(time_ns))).unwrap();
        let mut frame = frame(time_ns, now);
        assert!(producer.complete(pending, frame.clone(), (sequence, now)));
        frame.sequence = sequence;
        frame
    }

    #[test]
    fn global_test_guard_restores_an_empty_live_source_on_setup_and_unwind() {
        let _serial = crate::vr::input::semantic::history::TEST_LOCK.lock();
        source_changed(EpochChange::ReplayStarted);
        {
            let _guard = test_guard();
            let slot = SLOT.lock();
            assert_eq!(slot.source, PoseSource::Live);
            assert!(slot.packet.is_none() && slot.pending.is_none());
        }
        let panicked = std::panic::catch_unwind(|| {
            let _guard = test_guard();
            assert!(begin_located(Some(head(100))).is_some());
            source_changed(EpochChange::ReplayStarted);
            panic!("exercise flight packet test cleanup");
        });
        assert!(panicked.is_err());
        let slot = SLOT.lock();
        assert_eq!(slot.source, PoseSource::Live);
        assert!(slot.packet.is_none() && slot.pending.is_none());
    }

    #[test]
    fn one_publication_assigns_both_head_and_controller_identity_from_the_receipt() {
        let mut producer = Producer::new();
        let now = Instant::now();
        let frame = publish(&mut producer, 100, 41, now);
        let packet = producer.for_frame(&frame, now).unwrap();
        assert_eq!(
            packet.sample(),
            GestureSample {
                epoch: 1,
                sequence: 41,
                time_ns: 100
            }
        );
        assert_eq!(packet.head.published_at, packet.controllers.published_at);
        assert_eq!(packet.head.pose.pose.position, Vec3::new(0.0, 1.6, 0.0));
        assert!(packet.head.pose.is_tracked());
    }

    #[test]
    fn beginning_a_half_packet_immediately_removes_the_previous_complete_packet() {
        let mut producer = Producer::new();
        let now = Instant::now();
        let frame = publish(&mut producer, 100, 1, now);
        let pending = producer.begin(Some(head(200))).unwrap();
        assert!(producer.for_frame(&frame, now).is_none());
        producer.cancel();
        assert!(!producer.complete(pending, frame, (2, now)));
        assert!(producer.packet.is_none());
    }

    #[test]
    fn missing_or_untracked_head_cannot_retain_or_complete_an_old_packet() {
        let mut producer = Producer::new();
        let now = Instant::now();
        let frame = publish(&mut producer, 100, 1, now);
        assert!(producer.begin(None).is_none());
        assert!(producer.for_frame(&frame, now).is_none());
        let mut lost = head(200);
        lost.flags = xr::ViewStateFlags::POSITION_VALID | xr::ViewStateFlags::ORIENTATION_VALID;
        assert!(producer.begin(Some(lost)).is_none());
        assert!(producer.packet.is_none());
    }

    #[test]
    fn recenter_or_clock_change_between_halves_rejects_the_pending_head() {
        for change in [
            EpochChange::Recenter,
            EpochChange::FollowBank,
            EpochChange::RuntimeReset,
            EpochChange::SourceClockChanged,
        ] {
            let mut producer = Producer::new();
            let now = Instant::now();
            let pending = producer.begin(Some(head(100))).unwrap();
            producer.source_changed(change);
            assert!(!producer.complete(pending, frame(100, now), (1, now)));
            assert!(producer.packet.is_none());
            let next = publish(&mut producer, 10, 1, now);
            assert_eq!(producer.for_frame(&next, now).unwrap().sample().epoch, 2);
        }
    }

    #[test]
    fn controller_sequence_time_and_publication_mismatches_are_unavailable() {
        let mut producer = Producer::new();
        let now = Instant::now();
        let original = publish(&mut producer, 100, 1, now);
        for change in 0..4 {
            let mut different = original.clone();
            match change {
                0 => different.sequence += 1,
                1 => different.display_time_ns += 1,
                2 => different.published_at += Duration::from_nanos(1),
                _ => different.focused = false,
            }
            assert!(
                producer
                    .for_frame(&different, different.published_at)
                    .is_none()
            );
        }
        assert!(
            producer
                .for_frame(&original, now + Duration::from_secs(10))
                .is_none()
        );
    }

    #[test]
    fn failed_time_or_hand_validation_drops_the_candidate_without_exposing_half_data() {
        for change in 0..3 {
            let mut producer = Producer::new();
            let now = Instant::now();
            let pending = producer.begin(Some(head(100))).unwrap();
            let mut controllers = frame(100, now);
            match change {
                0 => controllers.display_time_ns += 1,
                1 => controllers.hands[0].grip.position_tracked = false,
                _ => controllers.focused = false,
            }
            assert!(!producer.complete(pending, controllers, (1, now)));
            assert!(producer.packet.is_none() && producer.pending.is_none());
        }
    }

    #[test]
    fn replay_never_pairs_live_head_with_recorded_controllers_and_handback_gets_a_new_epoch() {
        let mut producer = Producer::new();
        let now = Instant::now();
        let old = publish(&mut producer, 100, 1, now);
        producer.source_changed(EpochChange::ReplayStarted);
        assert!(producer.for_frame(&old, now).is_none());
        assert!(producer.begin(Some(head(200))).is_none());
        producer.source_changed(EpochChange::ReplayEnded);
        let new = publish(&mut producer, 50, 2, now);
        assert_eq!(producer.for_frame(&new, now).unwrap().sample().epoch, 3);
    }

    #[test]
    fn superseded_pending_tokens_cannot_overwrite_the_newer_complete_packet() {
        let mut producer = Producer::new();
        let now = Instant::now();
        let old = producer.begin(Some(head(100))).unwrap();
        let current = publish(&mut producer, 200, 2, now);
        assert!(!producer.complete(old, frame(100, now), (1, now)));
        assert!(producer.for_frame(&current, now).is_some());
    }

    #[test]
    fn typed_transitions_emit_only_the_narrow_player_action_in_the_correct_mode() {
        for (transition, mode) in [
            (
                SuitTransition::Deploy,
                Mode::Airborne(AirborneVariant::Falling),
            ),
            (
                SuitTransition::Close,
                Mode::Airborne(AirborneVariant::Wingsuit),
            ),
        ] {
            let commands = GestureCommands {
                transition: Some(transition),
                ..Default::default()
            };
            assert_eq!(
                desired_for(commands, mode, false).unwrap(),
                vec![Desired {
                    target: Target {
                        map: MapKind::Player,
                        action: OPEN_WINGSUIT
                    },
                    want: Want::Click,
                }]
            );
            assert_eq!(
                desired_for(commands, Mode::Ui, false),
                Err(CommandRejection::WrongMode)
            );
            assert_eq!(
                desired_for(commands, mode, true),
                Err(CommandRejection::CanopyRequested)
            );
        }
    }

    #[test]
    fn brake_uses_existing_held_pair_and_rejects_conflicts_or_nonflight_contexts() {
        let mode = Mode::Airborne(AirborneVariant::Wingsuit);
        let commands = GestureCommands {
            brake: true,
            reserve_pitch: true,
            ..Default::default()
        };
        let desired = desired_for(commands, mode, false).unwrap();
        assert_eq!(desired.len(), 2);
        assert_eq!(desired[0].target.action, CANCEL);
        assert_eq!(desired[1].target.action, WINGSUIT_AIRBRAKE);
        assert!(
            desired
                .iter()
                .all(|input| input.target.map == MapKind::Player && input.want == Want::Hold(1.0))
        );
        assert!(
            desired_for(GestureCommands::default(), mode, false)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            desired_for(commands, Mode::Airborne(AirborneVariant::Parachute), false),
            Err(CommandRejection::WrongMode)
        );
        assert_eq!(
            desired_for(
                GestureCommands {
                    transition: Some(SuitTransition::Close),
                    ..commands
                },
                mode,
                false
            ),
            Err(CommandRejection::ConflictingCommands)
        );
        assert_eq!(
            desired_for(
                GestureCommands {
                    transition: Some(SuitTransition::Deploy),
                    ..Default::default()
                },
                mode,
                false
            ),
            Err(CommandRejection::WrongMode)
        );
    }
}
