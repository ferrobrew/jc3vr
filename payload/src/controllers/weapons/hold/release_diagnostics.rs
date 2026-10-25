//! Copied observations of the latest inventory-gun release attempt.
//!
//! Native identifiers are correlation values only. No reader dereferences them, and no record
//! retains a weapon reference or a native body. Cached gun motion has no known producer timestamp.

use std::time::Duration;

use glam::Vec3;
use jc3gi::character::inventory::WeaponSlot;

use crate::controllers::{
    frame::{self, ControllerFrame, Hand},
    physical_hands::BodyError,
    weapons::hold::equip::DockRefusal,
};

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

/// Latest published controller context, separate from the cached gun sample used for the drop.
///
/// These cockpit-frame values are not a release estimate and are never supplied to the gun setter.
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
    pub write: NativeWrite,
    pub on_drop_returned: bool,
    pub slot_clear_returned: bool,
}

impl DropObservation {
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
            write: NativeWrite::NotAttempted,
            on_drop_returned: false,
            slot_clear_returned: false,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DropBranch {
    HeavySlotNativeDrop,
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
}

/// World-space engine velocities; no cockpit conversion or carried-velocity addition is applied.
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
    HeavySlotDelegated,
    MissingMotion,
    MissingBody,
    MissingMotionAndBody,
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
