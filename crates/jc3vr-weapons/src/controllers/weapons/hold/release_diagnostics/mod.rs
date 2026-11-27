//! Copied observations of the latest inventory-gun release attempt.
//!
//! Native identifiers are correlation values only. No reader dereferences them, and no record
//! retains a weapon reference or a native body. Cached gun motion has no known producer timestamp.
//!
//! Each attempt is also a tracking-recording event ([`ReleaseRecord::event`]) while a recording or
//! the lookback collects.

use glam::Vec3;
use jc3gi::character::inventory::WeaponSlot;
use jc3vr_body::controllers::{
    physics::{
        error::PhysicsError,
        release::{ControllerContext, ReleaseMotion, ReleaseSource},
    },
    telemetry::Thrown,
};
use jc3vr_input::frame::{self, Hand};
use jc3vr_view::recorder::tracking::format::{debug_label, hand_to_file};
use tracking_format::event::{GunRelease, ReleaseController, ReleaseDrop, WorldMotion};

use crate::controllers::weapons::hold::equip::DockRefusal;

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

impl ReleaseRecord {
    /// The gun's motion as a recorded throw, when the release requested one of the native drop.
    pub(crate) fn thrown(&self) -> Option<Thrown> {
        let drop = self.drop?;
        let requested = drop.requested_world_motion?;
        Some(Thrown {
            hand: self.hand,
            item: "gun",
            linear: requested.linear,
            angular: requested.angular,
            time_scale: drop.time_scale,
            source: Some(drop.release_source.label()),
            armed: None,
        })
    }

    /// The attempt as a tracking-recording event. The controller's poses and flags are left to the
    /// frame of the same display time; nested runtime values are recorded as their Debug text.
    pub(crate) fn event(&self) -> GunRelease {
        GunRelease {
            hand: hand_to_file(self.hand),
            attempt: self.sequence,
            weapon: self.weapon.map(|weapon| weapon as u64),
            route: debug_label(&self.route),
            max_speed_mps: self.max_speed_mps,
            refusal: self.refusal.map(|refusal| debug_label(&refusal)),
            before_disarm: self.stages.before_disarm,
            disarm_returned: self.stages.disarm_returned,
            slot_clear_returned: self.stages.slot_clear_returned,
            reconcile_returned: self.stages.reconcile_returned,
            controller: self.controller.map(|controller| ReleaseController {
                frame_sequence: controller.frame_sequence,
                display_time_ns: controller.display_time_ns,
                publication_age_ns: u64::try_from(controller.publication_age.as_nanos())
                    .unwrap_or(u64::MAX),
            }),
            drop: self.drop.map(|drop| ReleaseDrop {
                weapon: drop.weapon as u64,
                slot: drop.slot.map(|slot| debug_label(&slot)),
                sample_stage: debug_label(&drop.sample_stage),
                sample_source: debug_label(&drop.sample_source),
                cached_motion: drop.cached_world_motion,
                native_body: drop.native_body_id,
                requested_motion: drop.requested_world_motion,
                release_source: debug_label(&drop.release_source),
                history_age_s: drop.history_age_s,
                write: debug_label(&drop.write),
                on_drop_returned: drop.on_drop_returned,
                slot_clear_returned: drop.slot_clear_returned,
            }),
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
    /// The game's time scale the release was converted at.
    pub time_scale: f32,
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
        let requested = release.capped(max_speed_mps);
        self.release_source = release.source.into();
        self.history_age_s = release.history_age_s;
        self.time_scale = release.time_scale.get();
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
            time_scale: 1.0,
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

impl SampleSource {
    /// The source's name, as a recorded throw carries it.
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::CachedWeaponDataWorldMatrices => "cached weapon world matrices",
            Self::TimestampedHandHistory => ReleaseSource::TimestampedHandHistory.label(),
            Self::RigTransportOnly => ReleaseSource::RigTransportOnly.label(),
            Self::UnavailableAtRelease => ReleaseSource::UnavailableAtRelease.label(),
        }
    }
}

impl From<ReleaseSource> for SampleSource {
    fn from(source: ReleaseSource) -> Self {
        match source {
            ReleaseSource::TimestampedHandHistory => Self::TimestampedHandHistory,
            ReleaseSource::RigTransportOnly => Self::RigTransportOnly,
            ReleaseSource::UnavailableAtRelease => Self::UnavailableAtRelease,
        }
    }
}

/// Setter completion is distinct from applied velocity, which requires a native readback.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum NativeWrite {
    NotAttempted,
    MissingBody,
    MissingOwner,
    Rejected(PhysicsError),
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
mod tests;
