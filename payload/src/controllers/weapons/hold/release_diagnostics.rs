//! Copied observations of the latest inventory-gun release attempt.
//!
//! Native identifiers are correlation values only. No reader dereferences them, and no record
//! retains a weapon reference or a native body. Cached gun motion has no known producer timestamp.

use std::time::Duration;

use glam::{Quat, Vec3};
use jc3gi::character::inventory::WeaponSlot;

use crate::controllers::{
    frame::{self, ControllerFrame, Hand},
    motion::{self, MotionSample, ReleaseEstimate},
    physical_hands::BodyError,
    weapons::hold::equip::DockRefusal,
};

/// The cockpit-to-world transform sampled on an input tick, without controller translation.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct ReleaseFrame {
    pub origin: Vec3,
    pub rotation: Quat,
    pub scale: f32,
    pub teleport_epoch: u64,
    pub recenters: u32,
}

/// A fresh rig transform and its world-space motion, excluding hand motion.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct RigMotion {
    pub frame: ReleaseFrame,
    pub motion: WorldMotion,
}

/// The bounded, per-input-tick producer of inherited movement for gun releases.
#[derive(Debug, Copy, Clone, Default)]
pub(crate) struct ReleaseTransport {
    previous: Option<ReleaseFrame>,
}

impl ReleaseTransport {
    pub(crate) const fn new() -> Self {
        Self { previous: None }
    }

    pub(crate) fn update(&mut self, frame: Option<ReleaseFrame>, dt: f32) -> Option<RigMotion> {
        let previous = self.previous;
        self.previous = frame.filter(|frame| frame.valid());
        let current = self.previous?;
        let previous = previous?;
        if !(0.001..=MAX_RELEASE_AGE_S).contains(&dt)
            || current.teleport_epoch != previous.teleport_epoch
            || current.recenters != previous.recenters
            || (current.scale - previous.scale).abs() > 1e-4
            || current.origin.distance(previous.origin) > 5.0
        {
            return None;
        }
        let turn = (current.rotation * previous.rotation.inverse()).normalize();
        let turn = if turn.w < 0.0 { -turn } else { turn };
        let angle = turn.to_scaled_axis();
        if !angle.is_finite() || angle.length() > std::f32::consts::FRAC_PI_4 {
            return None;
        }
        Some(RigMotion {
            frame: current,
            motion: WorldMotion {
                linear: (current.origin - previous.origin) / dt,
                angular: angle / dt,
            },
        })
    }
}

impl ReleaseFrame {
    fn valid(self) -> bool {
        self.origin.is_finite()
            && self.rotation.is_finite()
            && (self.rotation.length_squared() - 1.0).abs() < 1e-3
            && self.scale.is_finite()
            && self.scale > 0.0
    }
}

/// The copied release estimate used by the native drop adapter, captured before disarm.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct ReleaseMotion {
    pub world: WorldMotion,
    pub source: SampleSource,
    pub history_age_s: Option<f32>,
}

impl ReleaseMotion {
    pub(crate) const REST: Self = Self {
        world: WorldMotion {
            linear: Vec3::ZERO,
            angular: Vec3::ZERO,
        },
        source: SampleSource::UnavailableAtRelease,
        history_age_s: None,
    };
}

/// The actual release selection, shared by both hands and the inventory and carried routes.
/// Hand history is cockpit-relative; rig motion contains world transport exactly once.
pub(crate) fn estimate_release(input: ReleaseInput<'_>) -> ReleaseMotion {
    let Some(rig) = input.rig.filter(|rig| {
        rig.frame.valid()
            && rig.motion.linear.is_finite()
            && rig.motion.angular.is_finite()
            && input.point_world.is_finite()
    }) else {
        return ReleaseMotion::REST;
    };
    let inherited = WorldMotion {
        linear: rig.motion.linear
            + rig
                .motion
                .angular
                .cross(input.point_world - rig.frame.origin),
        angular: rig.motion.angular,
    };
    let fallback = ReleaseMotion {
        world: inherited,
        source: SampleSource::RigTransportOnly,
        history_age_s: None,
    };
    let Some(controller) = input.controller else {
        return fallback;
    };
    if !(controller.focused
        && controller.active
        && controller.position_valid
        && controller.orientation_valid
        && controller.position_tracked
        && controller.orientation_tracked)
        || controller.publication_age.as_secs_f32() > MAX_RELEASE_AGE_S
    {
        return fallback;
    }
    let release_s = controller.display_time_ns as f64 * 1e-9;
    let Some(latest) = input
        .samples
        .iter()
        .rev()
        .find(|sample| sample.time_s <= release_s)
    else {
        return fallback;
    };
    let age_s = (release_s - latest.time_s) as f32;
    if !(0.0..=MAX_RELEASE_AGE_S).contains(&age_s) || !input.point_world.is_finite() {
        return fallback;
    }
    let mut samples = input
        .samples
        .iter()
        .copied()
        .filter(|sample| sample.time_s >= input.since_s && sample.time_s <= release_s)
        .collect::<Vec<_>>();
    for sample in &mut samples {
        sample.linear_velocity = sample
            .linear_velocity
            .filter(|velocity| velocity.is_finite());
        sample.angular_velocity = sample
            .angular_velocity
            .filter(|velocity| velocity.is_finite());
    }
    for index in 1..samples.len() {
        let previous = samples[index - 1];
        let sample = &mut samples[index];
        let dt = (sample.time_s - previous.time_s) as f32;
        if sample.angular_velocity.is_none()
            && dt > 0.0
            && sample.orientation.is_finite()
            && previous.orientation.is_finite()
        {
            let turn = (sample.orientation * previous.orientation.inverse()).normalize();
            let turn = if turn.w < 0.0 { -turn } else { turn };
            let angular = turn.to_scaled_axis() / dt;
            sample.angular_velocity = angular.is_finite().then_some(angular);
        }
    }
    let point_cockpit =
        rig.frame.rotation.inverse() * (input.point_world - rig.frame.origin) / rig.frame.scale;
    let offset = latest.orientation.inverse() * (point_cockpit - latest.position);
    let estimate = ReleaseEstimate {
        window_s: 0.25,
        peak_span_s: 0.015,
    };
    let Some(linear) = motion::release_velocity(&samples, release_s, offset, estimate) else {
        return fallback;
    };
    let angular =
        motion::release_angular_velocity(&samples, release_s, estimate).unwrap_or(Vec3::ZERO);
    let world = WorldMotion {
        linear: rig.frame.rotation * linear * rig.frame.scale + inherited.linear,
        angular: rig.frame.rotation * angular + inherited.angular,
    };
    if !(world.linear.is_finite() && world.angular.is_finite()) {
        return fallback;
    }
    ReleaseMotion {
        world,
        source: SampleSource::TimestampedHandHistory,
        history_age_s: Some(age_s),
    }
}

pub(crate) struct ReleaseInput<'a> {
    pub rig: Option<RigMotion>,
    pub controller: Option<&'a ControllerContext>,
    pub samples: &'a [MotionSample],
    pub since_s: f64,
    pub point_world: Vec3,
}

pub(crate) const MAX_RELEASE_AGE_S: f32 = 0.15;

/// A fixed-size readout containing only the latest release attempt.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ReleaseStatus {
    pub attempts: u64,
    pub latest: Option<ReleaseRecord>,
}

impl ReleaseStatus {
    pub const EMPTY: Self = Self {
        attempts: 0,
        latest: None,
    };

    pub(crate) fn record(&mut self, mut record: ReleaseRecord) {
        self.attempts = self.attempts.saturating_add(1);
        record.sequence = self.attempts;
        self.latest = Some(record);
    }
}

/// One input-tick attempt, copied before publication to the readout.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ReleaseRecord {
    pub sequence: u64,
    /// The physical controller hand, independent of the acting or dominant hand.
    pub hand: Hand,
    /// The loadout object's address, copied while it is held. This is not a live reference.
    pub weapon: Option<usize>,
    pub route: ReleaseRoute,
    pub max_speed_mps: f32,
    pub controller: Option<ControllerContext>,
    pub stages: ReleaseStages,
    pub drop: Option<DropObservation>,
    pub refusal: Option<DockRefusal>,
    /// No existing pre-disarm motion read is added for diagnostics.
    pub before_disarm_sample: Unavailable,
    pub native_readback: NativeReadback,
}

impl ReleaseRecord {
    pub(crate) fn new(hand: Hand, max_speed_mps: f32) -> Self {
        Self {
            sequence: 0,
            hand,
            weapon: None,
            route: ReleaseRoute::Unresolved,
            max_speed_mps,
            controller: frame::with_latest(|frame| ControllerContext::from_frame(frame, hand)),
            stages: ReleaseStages::default(),
            drop: None,
            refusal: None,
            before_disarm_sample: Unavailable::NoExistingObservation,
            native_readback: NativeReadback::UNAVAILABLE,
        }
    }
}

/// The route reached by the release executor.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ReleaseRoute {
    Unresolved,
    Wielded,
    ArmedInHand,
}

/// Returned calls, not observations of the weapon after those calls.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct ReleaseStages {
    pub before_disarm: bool,
    pub disarm_returned: bool,
    pub slot_clear_returned: bool,
    pub reconcile_returned: bool,
}

/// Latest published controller context, used to validate the timestamped hand release estimate.
/// The reported cockpit velocities remain a readout; the estimator reads the motion history.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ControllerContext {
    pub frame_sequence: u64,
    /// Runtime predicted display time; not a timestamp of the cached gun velocity or grip edge.
    pub display_time_ns: i64,
    /// Host publication age at the attempt, not the age of the cached gun motion.
    pub publication_age: Duration,
    pub focused: bool,
    pub active: bool,
    pub position_valid: bool,
    pub orientation_valid: bool,
    pub position_tracked: bool,
    pub orientation_tracked: bool,
    pub linear_cockpit_mps: Option<Vec3>,
    pub angular_cockpit_rad_s: Option<Vec3>,
}

impl ControllerContext {
    fn from_frame(frame: &ControllerFrame, hand: Hand) -> Self {
        let controller = frame.hand(hand);
        let grip = controller.grip;
        Self {
            frame_sequence: frame.sequence,
            display_time_ns: frame.display_time_ns,
            publication_age: frame.age(),
            focused: frame.focused,
            active: controller.active,
            position_valid: grip.position_valid,
            orientation_valid: grip.orientation_valid,
            position_tracked: grip.position_tracked,
            orientation_tracked: grip.orientation_tracked,
            linear_cockpit_mps: grip.linear_velocity,
            angular_cockpit_rad_s: grip.angular_velocity,
        }
    }
}

/// Existing native motion and body reads, copied before `OnDrop` or the heavy-slot drop.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct DropObservation {
    /// The address read at the native drop site, for comparison with the loadout identity.
    pub weapon: usize,
    pub slot: Option<WeaponSlot>,
    pub branch: DropBranch,
    pub sample_stage: SampleStage,
    pub sample_source: SampleSource,
    pub sample_age: Unavailable,
    pub cached_world_motion: Option<WorldMotion>,
    /// From `WeaponData::m_PfxInstance`, distinct from the mod-owned held-gun body.
    /// This identifier is never used for a later native read.
    pub native_body_id: Option<u32>,
    /// Sanitized, capped world-space values passed to the existing native setter, if called.
    pub requested_world_motion: Option<WorldMotion>,
    pub release_source: SampleSource,
    pub history_age_s: Option<f32>,
    pub write: NativeWrite,
    pub on_drop_returned: bool,
    pub slot_clear_returned: bool,
}

impl DropObservation {
    /// Prepare the exact world-space request consumed by the native velocity setter.
    pub(crate) fn prepare_release(
        &mut self,
        release: ReleaseMotion,
        max_speed_mps: f32,
    ) -> WorldMotion {
        let cap = if max_speed_mps.is_finite() {
            max_speed_mps.max(0.0)
        } else {
            0.0
        };
        let finite = |velocity: Vec3| {
            if velocity.is_finite() {
                velocity
            } else {
                Vec3::ZERO
            }
        };
        let requested = WorldMotion {
            linear: finite(release.world.linear).clamp_length_max(cap),
            angular: finite(release.world.angular),
        };
        self.release_source = release.source;
        self.history_age_s = release.history_age_s;
        self.requested_world_motion = Some(requested);
        requested
    }

    pub(crate) fn sampled(
        weapon: usize,
        slot: Option<WeaponSlot>,
        sample_stage: SampleStage,
        motion: Option<(Vec3, Vec3)>,
        body: Option<u32>,
    ) -> Self {
        Self {
            weapon,
            slot,
            branch: DropBranch::OnDropThenVelocityWrite,
            sample_stage,
            sample_source: SampleSource::CachedWeaponDataWorldMatrices,
            sample_age: Unavailable::ProducerTimeUnknown,
            cached_world_motion: motion.map(|(linear, angular)| WorldMotion { linear, angular }),
            native_body_id: body,
            requested_world_motion: None,
            release_source: SampleSource::UnavailableAtRelease,
            history_age_s: None,
            write: NativeWrite::NotAttempted,
            on_drop_returned: false,
            slot_clear_returned: false,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DropBranch {
    OnDropThenVelocityWrite,
}

/// All samples are taken before the drop.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SampleStage {
    Wielded,
    AfterDisarm,
    OutsideInventory,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SampleSource {
    CachedWeaponDataWorldMatrices,
    TimestampedHandHistory,
    RigTransportOnly,
    UnavailableAtRelease,
}

/// World-space linear and angular velocities, after any source-specific conversion.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct WorldMotion {
    pub linear: Vec3,
    pub angular: Vec3,
}

impl WorldMotion {
    pub fn linear_is_finite(self) -> bool {
        self.linear.is_finite()
    }

    pub fn angular_is_finite(self) -> bool {
        self.angular.is_finite()
    }
}

/// Setter completion is distinct from applied velocity, which requires a native readback.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum NativeWrite {
    NotAttempted,
    MissingBody,
    MissingOwner,
    Rejected(BodyError),
    SetterReturned,
}

/// No native motion reader is called at any of these stages.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct NativeReadback {
    pub after_setter: Unavailable,
    pub after_slot_clear: Unavailable,
    pub next_step: Unavailable,
}

impl NativeReadback {
    const UNAVAILABLE: Self = Self {
        after_setter: Unavailable::ReadbackContractUnproven,
        after_slot_clear: Unavailable::LifetimeAndPhaseUnproven,
        next_step: Unavailable::LifetimeAndPhaseUnproven,
    };
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Unavailable {
    NoExistingObservation,
    ProducerTimeUnknown,
    ReadbackContractUnproven,
    LifetimeAndPhaseUnproven,
}

#[cfg(test)]
#[path = "release_diagnostics_tests.rs"]
mod tests;
