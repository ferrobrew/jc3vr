//! Complete live head/controller observations for the wingsuit gestures.
//!
//! The frame loop supplies current rebased eye poses. The sampler completes the packet with the actual controller publication receipt. No global head is queried.

use std::{sync::LazyLock, time::Instant};

use openxr as xr;
use parking_lot::Mutex;

use crate::{
    controllers::frame::{ControllerFrame, Pose, TrackedPose},
    vr::convert,
};

/// Source-owned discontinuities. Ordinary head/controller movement does not advance the epoch.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum EpochChange {
    Recenter,
    FollowBank,
    RuntimeReset,
    SourceClockChanged,
}

/// Identity and pose time of an input observation. A newer epoch permits clock/sequence reset.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct GestureSample {
    pub epoch: u64,
    pub sequence: u64,
    pub time_ns: i64,
}

/// A head pose published from the same locate and cockpit baseline as the controller observation.
///
/// The producer must preserve the original sample identity and tracking flags. Copying the current
/// controller stamp onto a separately queried global head pose does not satisfy this contract.
#[derive(Debug, Copy, Clone)]
pub(crate) struct GestureHead {
    pub sample: GestureSample,
    pub pose: TrackedPose,
    pub published_at: Instant,
}

/// Head data from this frame's located and already-rebased eye views.
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

    /// The head from this tracked publication, before body or vehicle composition.
    pub(crate) fn head_pose(&self) -> Pose {
        self.head.pose.pose
    }

    /// The head from the same locate and cockpit baseline as the controllers.
    pub(crate) fn head(&self) -> GestureHead {
        self.head
    }

    /// The controller frame published with the head.
    pub(crate) fn controllers(&self) -> &ControllerFrame {
        &self.controllers
    }
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

/// The current source epoch: it advances at every change of the cockpit frame's source (a
/// recenter, a follow bank, each share of a seat-lock yaw re-base, a runtime reset).
/// `None` once it has overflowed.
pub(crate) fn epoch() -> Option<u64> {
    SLOT.lock().epoch
}

struct Producer {
    epoch: Option<u64>,
    ticket: u64,
    pending: Option<u64>,
    packet: Option<FlightPacket>,
}

impl Producer {
    fn new() -> Self {
        Self {
            epoch: Some(1),
            ticket: 0,
            pending: None,
            packet: None,
        }
    }

    fn cancel(&mut self) {
        self.pending = None;
        self.packet = None;
    }

    fn source_changed(&mut self, _change: EpochChange) {
        self.cancel();
        self.epoch = self.epoch.and_then(|epoch| epoch.checked_add(1));
    }

    fn begin(&mut self, head: Option<LocatedHead>) -> Option<PendingHead> {
        self.cancel();
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
        if self.epoch != Some(pending.epoch) || self.pending != Some(pending.ticket) {
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
    let eyes = head.eyes.map(convert::pose);
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

/// Serialize tests that share the live packet with history source changes, and leave it empty.
#[cfg(test)]
pub(crate) fn test_guard() -> impl Drop {
    struct Guard {
        _serial: parking_lot::ReentrantMutexGuard<'static, ()>,
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            SLOT.lock().cancel();
        }
    }

    let guard = Guard {
        _serial: crate::vr::input::semantic::history::TEST_LOCK.lock(),
    };
    SLOT.lock().cancel();
    guard
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use glam::Vec3;

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
    fn global_test_guard_restores_an_empty_packet_on_setup_and_unwind() {
        let _serial = crate::vr::input::semantic::history::TEST_LOCK.lock();
        assert!(begin_located(Some(head(100))).is_some());
        {
            let _guard = test_guard();
            let slot = SLOT.lock();
            assert!(slot.packet.is_none() && slot.pending.is_none());
        }
        let panicked = std::panic::catch_unwind(|| {
            let _guard = test_guard();
            assert!(begin_located(Some(head(100))).is_some());
            panic!("exercise flight packet test cleanup");
        });
        assert!(panicked.is_err());
        let slot = SLOT.lock();
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
    fn superseded_pending_tokens_cannot_overwrite_the_newer_complete_packet() {
        let mut producer = Producer::new();
        let now = Instant::now();
        let old = producer.begin(Some(head(100))).unwrap();
        let current = publish(&mut producer, 200, 2, now);
        assert!(!producer.complete(old, frame(100, now), (1, now)));
        assert!(producer.for_frame(&current, now).is_some());
    }
}
