//! The live tracking packet: the head locate and the controller publication receipt from one
//! OpenXR frame, published by the frame loop for the diagnostics that read live tracking.
//!
//! The packet is replaced on every frame and cleared before each acquisition, so a reader never
//! sees tracking from an earlier frame as current. Joining it with the native pose and the body's
//! movement completion is the rig's
//! ([`crate::controllers::body::rig::live_observation`]).

use std::time::{Duration, Instant};

use glam::Quat;
use jc3vr_input::frame::{ControllerFrame, Pose, TrackedPose};
use openxr as xr;
use parking_lot::Mutex;

use crate::{controllers::body::rig::origin_provenance::OriginEvidence, vr::convert};

/// The located head, with the exact transform used to rebase grips.
/// `cockpit_from_local` is a transform value, not a complete origin-generation identity.
#[derive(Clone, Copy)]
pub struct LocatedTracking {
    pub head: xr::Posef,
    pub flags: xr::ViewStateFlags,
    pub display_time_ns: i64,
    pub cockpit_from_local: xr::Posef,
    pub origin: OriginEvidence,
}

/// Raw cockpit-space poses with the controller publisher's actual sequence and host time.
/// Invalid components retain their runtime flags; inactive grips remain absent.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RawPacket {
    pub head: TrackedPose,
    pub grips: [Option<TrackedPose>; 2],
    pub focused: bool,
    pub sequence: u64,
    pub display_time_ns: i64,
    pub published_at: Instant,
    pub cockpit_from_local: Pose,
    pub origin: OriginEvidence,
}

/// Why one source is unavailable for current diagnostic inspection.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Unavailable {
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
pub enum RawAvailability {
    Unavailable(Unavailable),
    Observed { age: Duration },
}

/// A fixed-size owned snapshot with no allocation on publication or status reads.
/// Unavailable snapshots omit the packet rather than retaining previously observed tracking.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RawObservation {
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

/// How old a publication may be and still count for current diagnostic inspection.
pub const DIAGNOSTIC_MAX_AGE: Duration = Duration::from_millis(500);

/// Publish only the sampler-owned controller frame returned with its real publication receipt.
/// No latest-value global controller is paired with the located live head.
pub(crate) fn publish_tracking(
    located: Option<LocatedTracking>,
    controllers: Option<ControllerFrame>,
) {
    *SLOT.lock() = make_raw_packet(located, controllers);
}

/// Clear tracking before acquisition, including every early return and locate failure.
pub(crate) fn tracking_unavailable() {
    *SLOT.lock() = Err(Unavailable::NoLiveTracking);
}

/// The latest publication, or why there is none, without an age check.
pub fn publication() -> Result<RawPacket, Unavailable> {
    *SLOT.lock()
}

/// The latest publication's current availability, read without engine pointers.
pub(crate) fn status() -> RawObservation {
    observe(publication(), Instant::now(), DIAGNOSTIC_MAX_AGE)
}

/// The availability of `publication` at `now`: unavailable past `max_age` or before it was
/// published, so no stale packet is retained.
pub fn observe(
    publication: Result<RawPacket, Unavailable>,
    now: Instant,
    max_age: Duration,
) -> RawObservation {
    match publication {
        Err(reason) => RawObservation::unavailable(reason),
        Ok(packet) => match age_at(packet.published_at, now, max_age, true) {
            Ok(age) => RawObservation::observed(packet, age),
            Err(reason) => RawObservation::unavailable(reason),
        },
    }
}

pub fn make_raw_packet(
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
            pose: convert::pose(located.head),
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
        cockpit_from_local: convert::pose(located.cockpit_from_local),
        origin: located.origin,
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

pub fn age_at(
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

pub fn unit_rotation(rotation: Quat) -> bool {
    rotation.is_finite() && (rotation.length_squared() - 1.0).abs() <= 1.0e-4
}

static SLOT: Mutex<Result<RawPacket, Unavailable>> = Mutex::new(Err(Unavailable::NotCaptured));
