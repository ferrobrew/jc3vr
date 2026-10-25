//! Bounded read-only raw tracking and native pre-IK evidence.
//!
//! The raw packet uses the head locate and controller publication receipt from one OpenXR frame.
//! The native capture has a separate host time and is never joined to it as a simulation pair.
//! Missing composition and source epochs prevent construction of the checked body observation.

use std::time::{Duration, Instant};

use glam::{Mat4, Quat, Vec3};
use openxr as xr;
use parking_lot::Mutex;

use crate::{
    controllers::{
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

/// Separate source availability and the missing checked-observation prerequisites.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct ObservationStatus {
    pub raw: RawObservation,
    pub native: NativeObservation,
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
    SLOT.lock().read(
        Instant::now(),
        DIAGNOSTIC_MAX_AGE,
        OriginCounters::current(),
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

    fn read(&self, now: Instant, max_age: Duration, origin: OriginCounters) -> ObservationStatus {
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
        .all(|root| {
            root.is_finite()
                && root.determinant().abs() > 1.0e-6
                && root.inverse().is_finite()
                && root.x_axis.w == 0.0
                && root.y_axis.w == 0.0
                && root.z_axis.w == 0.0
                && root.w_axis.w == 1.0
        })
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

static SLOT: Mutex<Publisher> = Mutex::new(Publisher::new());

#[cfg(test)]
#[path = "live_observation_tests.rs"]
mod tests;
