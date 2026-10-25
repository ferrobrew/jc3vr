//! Bounded read-only tracking, native pre-IK, and movement-completion evidence.
//!
//! The raw packet uses the head locate and controller publication receipt from one OpenXR frame.
//! Native and completion captures remain independent of it; host times never join simulation sides.
//! Missing composition and source epochs prevent construction of the checked body observation.

use std::time::{Duration, Instant};

use glam::{Mat4, Quat, Vec3};
use openxr as xr;
use parking_lot::Mutex;

use crate::{
    controllers::{
        body::rig::root::{
            self, CompletionSnapshot, CompletionSnapshotAvailability, CompletionUnavailable,
            MovementCompletion,
        },
        frame::{ControllerFrame, Pose, TrackedPose},
        roomscale::teleport,
    },
    vr,
};

/// The head located before replay replacement, with the exact transform used to rebase grips.
/// `cockpit_from_local` is a transform value, not a complete origin-generation identity.
pub(crate) struct LocatedTracking {
    pub head: xr::Posef,
    pub flags: xr::ViewStateFlags,
    pub display_time_ns: i64,
    pub cockpit_from_local: xr::Posef,
}

/// Raw cockpit-space poses with the controller publisher's actual sequence and host time.
/// Invalid components retain their runtime flags; inactive grips remain absent.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct RawPacket {
    pub head: TrackedPose,
    pub grips: [Option<TrackedPose>; 2],
    pub focused: bool,
    pub sequence: u64,
    pub display_time_ns: i64,
    pub published_at: Instant,
    pub cockpit_from_local: Pose,
}

/// Values read together before the local character's procedural IK writes.
/// The previous anchor uses the current animated head joint through the previous root.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct NativeCapture {
    pub previous_root: Mat4,
    pub current_root: Mat4,
    pub previous_anchor: Vec3,
    pub current_anchor: Vec3,
    pub current_head_model: Vec3,
    pub current_neck_model: Vec3,
    pub current_eye_mid_model: Vec3,
}

/// Evidence still required before publishing a checked body observation.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum MissingEvidence {
    CompletedLedgerAndShiftPair,
    CompositionBasisPair,
    /// Owners of both matched composition sides, beyond one consumed request's owner.
    OwnerAtCompletion,
    OriginEpoch,
    CalibrationAndScaleEpochs,
    AuthorityEpoch,
    PreviousNativePose,
    SupportIdentity,
}

/// Why one source is unavailable for current diagnostic inspection.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Unavailable {
    NotCaptured,
    NoLiveTracking,
    MixedDisplayTimes,
    InvalidCapture,
    FuturePublication,
    Stale { age: Duration },
    OriginCountersChanged,
}

/// Raw tracking availability, independent of native capture and body-work eligibility.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum RawAvailability {
    Unavailable(Unavailable),
    Observed { age: Duration },
}

/// A fixed-size owned snapshot with no allocation on publication or status reads.
/// Unavailable snapshots omit the packet rather than retaining previously observed tracking.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct RawObservation {
    pub availability: RawAvailability,
    pub packet: Option<RawPacket>,
}

impl RawObservation {
    fn unavailable(reason: Unavailable) -> Self {
        Self {
            availability: RawAvailability::Unavailable(reason),
            packet: None,
        }
    }

    fn observed(packet: RawPacket, age: Duration) -> Self {
        Self {
            availability: RawAvailability::Observed { age },
            packet: Some(packet),
        }
    }
}

/// The latest native capture, never a checked body observation.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) enum NativeObservation {
    Unavailable(Unavailable),
    Observed {
        capture: NativeCapture,
        age: Duration,
    },
}

/// Diagnostic rejection of a completion snapshot, separate from movement eligibility.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum CompletionRejection {
    Source(CompletionUnavailable),
    Diagnostic(Unavailable),
}

/// Diagnostic completion availability, independent of the owned completion record.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum CompletionAvailability {
    Unavailable(CompletionRejection),
    Observed { age: Duration },
}

/// A fixed-size latest completion, never matched to tracking or a rendered composition.
/// Unavailable observations omit the record rather than retaining a previous completion.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct CompletionObservation {
    availability: CompletionAvailability,
    completion: Option<MovementCompletion>,
}

impl CompletionObservation {
    fn unavailable(reason: CompletionRejection) -> Self {
        Self {
            availability: CompletionAvailability::Unavailable(reason),
            completion: None,
        }
    }

    fn observed(completion: MovementCompletion, age: Duration) -> Self {
        Self {
            availability: CompletionAvailability::Observed { age },
            completion: Some(completion),
        }
    }

    pub(crate) fn availability(&self) -> CompletionAvailability {
        self.availability
    }

    pub(crate) fn completion(&self) -> Option<MovementCompletion> {
        self.completion
    }
}

/// Separate source availability and the missing checked-observation prerequisites.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct ObservationStatus {
    pub raw: RawObservation,
    pub native: NativeObservation,
    pub completion: CompletionObservation,
    pub missing: [MissingEvidence; 8],
}

/// Publish only the sampler-owned controller frame returned with its real publication receipt.
/// No latest-value global or replay controller is paired with the located live head.
pub(crate) fn publish_tracking(
    located: Option<LocatedTracking>,
    controllers: Option<ControllerFrame>,
) {
    SLOT.lock()
        .publish_tracking(located, controllers, OriginCounters::current());
}

/// Clear tracking before acquisition, including every early return and locate failure.
pub(crate) fn tracking_unavailable() {
    SLOT.lock().raw = Err(Unavailable::NoLiveTracking);
}

/// Replace the single owned capture. Host time is not a simulation-step identity.
pub(crate) fn publish_native(capture: NativeCapture) {
    SLOT.lock()
        .publish_native(capture, Instant::now(), OriginCounters::current());
}

/// Clear a previous capture when the local hook cannot read its animation controller.
pub(crate) fn native_unavailable() {
    SLOT.lock().native = None;
}

/// Read current diagnostic availability without reading engine pointers or composing a pose.
pub(crate) fn status() -> ObservationStatus {
    let completion = root::completion_snapshot();
    let now = Instant::now();
    SLOT.lock().read_completion_status(
        now,
        DIAGNOSTIC_MAX_AGE,
        OriginCounters::current(),
        completion,
    )
}

const DIAGNOSTIC_MAX_AGE: Duration = Duration::from_millis(500);
const MISSING: [MissingEvidence; 8] = [
    MissingEvidence::CompletedLedgerAndShiftPair,
    MissingEvidence::CompositionBasisPair,
    MissingEvidence::OwnerAtCompletion,
    MissingEvidence::OriginEpoch,
    MissingEvidence::CalibrationAndScaleEpochs,
    MissingEvidence::AuthorityEpoch,
    MissingEvidence::PreviousNativePose,
    MissingEvidence::SupportIdentity,
];

/// These existing counters do not cover runtime resets, follow banking, or calibration changes.
/// Equality is only a diagnostic invalidation check, never a complete origin epoch.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct OriginCounters {
    teleports: u64,
    recenters: u32,
}

impl OriginCounters {
    fn current() -> Self {
        Self {
            teleports: teleport::epoch(),
            recenters: vr::recenter_count(),
        }
    }
}

#[derive(Debug, Copy, Clone)]
struct RawPublication {
    packet: RawPacket,
    origin: OriginCounters,
}

#[derive(Debug, Copy, Clone)]
struct NativePublication {
    capture: NativeCapture,
    observed_at: Instant,
    origin: OriginCounters,
}

struct Publisher {
    raw: Result<RawPublication, Unavailable>,
    native: Option<NativePublication>,
}

impl Publisher {
    const fn new() -> Self {
        Self {
            raw: Err(Unavailable::NotCaptured),
            native: None,
        }
    }

    fn publish_tracking(
        &mut self,
        located: Option<LocatedTracking>,
        controllers: Option<ControllerFrame>,
        origin: OriginCounters,
    ) {
        self.raw =
            make_raw_packet(located, controllers).map(|packet| RawPublication { packet, origin });
    }

    fn publish_native(
        &mut self,
        capture: NativeCapture,
        observed_at: Instant,
        origin: OriginCounters,
    ) {
        self.native = Some(NativePublication {
            capture,
            observed_at,
            origin,
        });
    }

    #[cfg(test)]
    fn read(&self, now: Instant, max_age: Duration, origin: OriginCounters) -> ObservationStatus {
        self.read_completion_status(
            now,
            max_age,
            origin,
            CompletionSnapshot::unavailable(CompletionUnavailable::NotCompleted),
        )
    }

    fn read_completion_status(
        &self,
        now: Instant,
        max_age: Duration,
        origin: OriginCounters,
        completion: CompletionSnapshot,
    ) -> ObservationStatus {
        ObservationStatus {
            raw: match self.raw {
                Err(reason) => RawObservation::unavailable(reason),
                Ok(publication) => match age_at(
                    publication.packet.published_at,
                    now,
                    max_age,
                    publication.origin == origin,
                ) {
                    Ok(age) => RawObservation::observed(publication.packet, age),
                    Err(reason) => RawObservation::unavailable(reason),
                },
            },
            native: self.read_native(now, max_age, origin),
            completion: read_completion(completion, now, max_age),
            missing: MISSING,
        }
    }

    fn read_native(
        &self,
        now: Instant,
        max_age: Duration,
        origin: OriginCounters,
    ) -> NativeObservation {
        let unavailable = NativeObservation::Unavailable;
        let Some(publication) = self.native else {
            return unavailable(Unavailable::NotCaptured);
        };
        let age = match age_at(
            publication.observed_at,
            now,
            max_age,
            publication.origin == origin,
        ) {
            Ok(age) => age,
            Err(reason) => return unavailable(reason),
        };
        if !valid_capture(publication.capture) {
            return unavailable(Unavailable::InvalidCapture);
        }
        NativeObservation::Observed {
            capture: publication.capture,
            age,
        }
    }
}

fn read_completion(
    snapshot: CompletionSnapshot,
    now: Instant,
    max_age: Duration,
) -> CompletionObservation {
    if let CompletionSnapshotAvailability::Unavailable(reason) = snapshot.availability() {
        return CompletionObservation::unavailable(CompletionRejection::Source(reason));
    }
    let Some(completion) = snapshot.completion() else {
        return CompletionObservation::unavailable(CompletionRejection::Diagnostic(
            Unavailable::InvalidCapture,
        ));
    };
    let age = match age_at(completion.completed_at, now, max_age, true) {
        Ok(age) => age,
        Err(reason) => {
            return CompletionObservation::unavailable(CompletionRejection::Diagnostic(reason));
        }
    };
    let request_valid = completion.ledger.request.is_none_or(|request| {
        request.movement.task.is_finite()
            && request.movement.chase.is_finite()
            && request.credited.is_finite()
    });
    if completion.sequence == 0
        || !valid_root(completion.previous_root)
        || !valid_root(completion.current_root)
        || !completion.ledger.movement.previous.is_finite()
        || !completion.ledger.movement.current.is_finite()
        || !request_valid
    {
        return CompletionObservation::unavailable(CompletionRejection::Diagnostic(
            Unavailable::InvalidCapture,
        ));
    }
    CompletionObservation::observed(completion, age)
}

fn make_raw_packet(
    located: Option<LocatedTracking>,
    controllers: Option<ControllerFrame>,
) -> Result<RawPacket, Unavailable> {
    let (Some(located), Some(controllers)) = (located, controllers) else {
        return Err(Unavailable::NoLiveTracking);
    };
    if located.display_time_ns != controllers.display_time_ns {
        return Err(Unavailable::MixedDisplayTimes);
    }
    let packet = RawPacket {
        head: TrackedPose {
            pose: pose_from_xr(located.head),
            position_valid: located.flags.contains(xr::ViewStateFlags::POSITION_VALID),
            orientation_valid: located
                .flags
                .contains(xr::ViewStateFlags::ORIENTATION_VALID),
            position_tracked: located.flags.contains(xr::ViewStateFlags::POSITION_TRACKED),
            orientation_tracked: located
                .flags
                .contains(xr::ViewStateFlags::ORIENTATION_TRACKED),
            linear_velocity: None,
            angular_velocity: None,
        },
        grips: controllers
            .hands
            .map(|hand| hand.active.then_some(hand.grip)),
        focused: controllers.focused,
        sequence: controllers.sequence,
        display_time_ns: controllers.display_time_ns,
        published_at: controllers.published_at,
        cockpit_from_local: pose_from_xr(located.cockpit_from_local),
    };
    if packet.sequence == 0
        || !valid_pose(packet.cockpit_from_local)
        || !valid_tracking(packet.head)
        || packet
            .grips
            .iter()
            .flatten()
            .any(|grip| !valid_tracking(*grip))
    {
        return Err(Unavailable::InvalidCapture);
    }
    Ok(packet)
}

fn age_at(
    published_at: Instant,
    now: Instant,
    max_age: Duration,
    same_origin_counters: bool,
) -> Result<Duration, Unavailable> {
    if !same_origin_counters {
        return Err(Unavailable::OriginCountersChanged);
    }
    if now < published_at {
        return Err(Unavailable::FuturePublication);
    }
    let age = now.saturating_duration_since(published_at);
    if age > max_age {
        return Err(Unavailable::Stale { age });
    }
    Ok(age)
}

fn pose_from_xr(pose: xr::Posef) -> Pose {
    Pose {
        position: Vec3::new(pose.position.x, pose.position.y, pose.position.z),
        orientation: Quat::from_xyzw(
            pose.orientation.x,
            pose.orientation.y,
            pose.orientation.z,
            pose.orientation.w,
        ),
    }
}

fn valid_tracking(tracked: TrackedPose) -> bool {
    (!tracked.position_valid || tracked.pose.position.is_finite())
        && (!tracked.orientation_valid || unit_rotation(tracked.pose.orientation))
        && tracked
            .linear_velocity
            .is_none_or(|velocity| velocity.is_finite())
        && tracked
            .angular_velocity
            .is_none_or(|velocity| velocity.is_finite())
}

fn valid_pose(pose: Pose) -> bool {
    pose.position.is_finite() && unit_rotation(pose.orientation)
}

fn unit_rotation(rotation: Quat) -> bool {
    rotation.is_finite() && (rotation.length_squared() - 1.0).abs() <= 1.0e-4
}

fn valid_capture(capture: NativeCapture) -> bool {
    [capture.previous_root, capture.current_root]
        .into_iter()
        .all(valid_root)
        && [
            capture.previous_anchor,
            capture.current_anchor,
            capture.current_head_model,
            capture.current_neck_model,
            capture.current_eye_mid_model,
        ]
        .into_iter()
        .all(|point| point.is_finite())
}

fn valid_root(root: Mat4) -> bool {
    root.is_finite()
        && root.determinant().abs() > 1.0e-6
        && root.inverse().is_finite()
        && root.x_axis.w == 0.0
        && root.y_axis.w == 0.0
        && root.z_axis.w == 0.0
        && root.w_axis.w == 1.0
}

static SLOT: Mutex<Publisher> = Mutex::new(Publisher::new());

#[cfg(test)]
#[path = "live_observation_tests.rs"]
mod tests;
