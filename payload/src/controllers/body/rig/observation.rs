//! Immutable raw tracking and matching simulation-pair provenance for the body rig.
//!
//! Raw poses remain in player-scale cockpit metres before posture mapping, arm remapping, and
//! completed-chase compensation. Native rig landmarks remain separate model-space observations.
//! This contract neither estimates a body nor requests, credits, or applies root movement.

use std::{
    error::Error,
    fmt,
    time::{Duration, Instant},
};

use glam::{Mat4, Quat, Vec3};

use crate::controllers::{
    body::rig::{
        frames::{Frames, SkeletonLandmarks},
        root::{owner::RootOwner, shift::CockpitBasis},
    },
    frame::{Hand, Pose, TrackedPose},
};

/// Coordinate and ownership versions supplied by the publishing adapter.
///
/// A recenter or teleport changes `origin`; calibration, scale, and authority changes advance their
/// respective versions. These are equality tokens, not clocks or confidence measures.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct Epochs {
    pub origin: u64,
    pub calibration: u64,
    pub world_scale: u64,
    pub authority: u64,
}

/// One locate operation shared by the head and both grips.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct SampleStamp {
    /// The tracking publication sequence, not a simulation-step counter.
    pub sequence: u64,
    /// The OpenXR predicted display time in its nanosecond clock, including for replay.
    pub display_time_ns: i64,
    /// The host publication time. It is not comparable to `display_time_ns`.
    pub published_at: Instant,
    pub epochs: Epochs,
}

/// A raw head or grip locate result. Unavailable components retain their validity flags.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct RawPose {
    pub stamp: SampleStamp,
    /// Cockpit metres, unit orientation, metres per second, and radians per second.
    pub tracked: TrackedPose,
}

/// Inputs from one tracking publication. Hands use `Hand::index`, left then right.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct RawTracking {
    pub head: RawPose,
    /// `None` denotes an inactive hand; a located but invalid hand remains `Some`.
    pub grips: [Option<RawPose>; 2],
    pub focused: bool,
}

/// One side of the engine's completed simulation pair.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum PairSide {
    Previous,
    Current,
}

/// A completed simulation side and its coordinate/authority versions.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct StepStamp {
    pub step: u64,
    pub epochs: Epochs,
}

/// Corresponding root, anchor, composition basis, and compensation for one simulation side.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct CompositionSide {
    pub stamp: StepStamp,
    /// The observed model-to-world transform, with translation in world metres.
    pub root: Mat4,
    /// The camera anchor in world metres, already carried by the root/support owner.
    pub anchor_world: Vec3,
    pub basis: CockpitBasis,
    /// Completed compensation in cockpit metres, never a pending root request.
    pub shift_cockpit: Vec3,
    pub owner: RootOwner,
    /// Optional native landmarks captured before procedural writes, in model metres.
    /// Their step, epochs, and root transform are this side's, rather than a later solved pose's.
    pub native_rig: Option<SkeletonLandmarks>,
}

/// A checked simulation pair. A discontinuity must reseed both sides in the new epoch.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct CompositionPair {
    previous: CompositionSide,
    current: CompositionSide,
}

/// One immutable raw tracking packet related to one checked simulation pair.
///
/// Private fields prevent replacing a grip, pair side, or epoch after construction. Reusing this
/// value does not make a new observation: consumers retain `SampleStamp::sequence` and the step.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Observation {
    raw: RawTracking,
    pair: CompositionPair,
}

/// A rejected observation or an observation that is no longer eligible for a consumer.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum ObservationError {
    MixedSamples,
    EpochMismatch,
    ReversedSteps,
    ScaleMismatch,
    OwnerChangedWithoutEpoch,
    UncollapsedStep,
    InvalidTracking,
    InvalidComposition,
    InvalidNativeRig,
    InvalidHead,
    Unfocused,
    FuturePublication,
    StaleSample,
}

impl CompositionPair {
    /// Check both complete sides together. Equal steps support a collapsed startup/reseed pair.
    pub(crate) fn new(
        previous: CompositionSide,
        current: CompositionSide,
    ) -> Result<Self, ObservationError> {
        if previous.stamp.epochs != current.stamp.epochs {
            return Err(ObservationError::EpochMismatch);
        }
        if previous.stamp.step > current.stamp.step {
            return Err(ObservationError::ReversedSteps);
        }
        if previous.basis.world_scale != current.basis.world_scale {
            return Err(ObservationError::ScaleMismatch);
        }
        if previous.owner != current.owner {
            return Err(ObservationError::OwnerChangedWithoutEpoch);
        }
        validate_side(previous)?;
        validate_side(current)?;
        if previous.stamp.step == current.stamp.step && previous != current {
            return Err(ObservationError::UncollapsedStep);
        }
        Ok(Self { previous, current })
    }

    pub(crate) fn side(&self, side: PairSide) -> &CompositionSide {
        match side {
            PairSide::Previous => &self.previous,
            PairSide::Current => &self.current,
        }
    }
}

impl Observation {
    /// Validate declared coherence without changing tracking flags or applying a body fit.
    ///
    /// The adapter must obtain each source at its actual boundary. Matching stamps cannot prove
    /// that a caller read an engine transform or a tracker under the declared stamp.
    pub(crate) fn new(raw: RawTracking, pair: CompositionPair) -> Result<Self, ObservationError> {
        if raw.head.stamp.epochs != pair.current.stamp.epochs {
            return Err(ObservationError::EpochMismatch);
        }
        validate_tracking(raw.head.tracked)?;
        for grip in raw.grips.iter().flatten() {
            if grip.stamp != raw.head.stamp {
                return Err(ObservationError::MixedSamples);
            }
            validate_tracking(grip.tracked)?;
        }
        Ok(Self { raw, pair })
    }

    pub(crate) fn raw(&self) -> &RawTracking {
        &self.raw
    }

    pub(crate) fn pair(&self) -> &CompositionPair {
        &self.pair
    }

    /// Check consumer freshness on the host clock without imposing a new production age limit.
    /// An invalid head prevents new body work; invalid hands remain independently unavailable.
    pub(crate) fn check_at(
        &self,
        now: Instant,
        max_age: Duration,
        epochs: Epochs,
    ) -> Result<(), ObservationError> {
        if self.raw.head.stamp.epochs != epochs {
            return Err(ObservationError::EpochMismatch);
        }
        let published_at = self.raw.head.stamp.published_at;
        // Windows can round a small negative checked duration to zero; ordering remains strict.
        if now < published_at {
            return Err(ObservationError::FuturePublication);
        }
        let age = now
            .checked_duration_since(published_at)
            .ok_or(ObservationError::FuturePublication)?;
        if age > max_age {
            return Err(ObservationError::StaleSample);
        }
        if !self.raw.focused {
            return Err(ObservationError::Unfocused);
        }
        if !self.raw.head.tracked.is_valid() {
            return Err(ObservationError::InvalidHead);
        }
        Ok(())
    }

    /// Reuse the rig's point mapping with the selected side's anchor, basis, shift, and root.
    /// Root/support transport is already in the anchor, so no root delta is added here.
    /// This raw composition omits accessibility/posture remapping and is not a camera replacement.
    /// Live consumers must first call `check_at`; these mappings also support historical inspection.
    pub(crate) fn frames(&self, side: PairSide) -> Result<Frames, ObservationError> {
        if !self.raw.head.tracked.is_valid() {
            return Err(ObservationError::InvalidHead);
        }
        let composition = self.pair.side(side);
        let head_cockpit = self.raw.head.tracked.pose.position;
        // Accepted rounding error must not turn the derived rigid frame into a scale transform.
        let view = composition.basis.body.normalize();
        let frames = Frames {
            head_world: composition.anchor_world
                + view
                    * ((head_cockpit - composition.shift_cockpit) * composition.basis.world_scale),
            head_cockpit,
            view,
            world_scale: composition.basis.world_scale,
            character: composition.root,
        };
        frames
            .is_usable()
            .then_some(frames)
            .ok_or(ObservationError::InvalidComposition)
    }

    /// The composed head target in world metres, without a body estimate or a write.
    pub(crate) fn head_world(&self, side: PairSide) -> Result<Pose, ObservationError> {
        let frames = self.frames(side)?;
        Ok(Pose {
            position: frames.head_world,
            orientation: (frames.view * self.raw.head.tracked.pose.orientation).normalize(),
        })
    }

    /// The raw grip composed once into world metres. This is not a remapped wrist or a solved hand.
    /// Runtime-inferred but valid components remain usable; tracking flags stay in `raw()`.
    pub(crate) fn grip_world(
        &self,
        hand: Hand,
        side: PairSide,
    ) -> Result<Option<Pose>, ObservationError> {
        let Some(grip) = self.raw.grips[hand.index()].filter(|g| g.tracked.is_valid()) else {
            return Ok(None);
        };
        let frames = self.frames(side)?;
        let position = frames.cockpit_to_world(grip.tracked.pose.position);
        if !position.is_finite() {
            return Err(ObservationError::InvalidComposition);
        }
        Ok(Some(Pose {
            position,
            orientation: (frames.view * grip.tracked.pose.orientation).normalize(),
        }))
    }
}

impl fmt::Display for ObservationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cause = match self {
            Self::MixedSamples => "the head and grips have different sample stamps",
            Self::EpochMismatch => "the tracking and composition epochs do not match",
            Self::ReversedSteps => "the simulation pair runs backward",
            Self::ScaleMismatch => "the simulation pair has different world scales",
            Self::OwnerChangedWithoutEpoch => "the root owner changes within one authority epoch",
            Self::UncollapsedStep => "equal step stamps contain different composition sides",
            Self::InvalidTracking => "a valid tracking component is non-finite or not normalized",
            Self::InvalidComposition => "a composition transform is unusable",
            Self::InvalidNativeRig => "a native rig landmark is non-finite",
            Self::InvalidHead => "the head pose is unavailable",
            Self::Unfocused => "the tracking sample has no input focus",
            Self::FuturePublication => "the publication is later than the consumer's host time",
            Self::StaleSample => "the tracking sample exceeds the consumer's age limit",
        };
        write!(f, "body observation: {cause}")
    }
}

impl Error for ObservationError {}

fn validate_side(side: CompositionSide) -> Result<(), ObservationError> {
    if !side.anchor_world.is_finite()
        || !side.shift_cockpit.is_finite()
        || !unit_rotation(side.basis.body)
        || !side.basis.world_scale.is_finite()
        || side.basis.world_scale <= 0.0
        || !side.root.is_finite()
        || side.root.determinant().abs() <= 1.0e-6
        || !side.root.inverse().is_finite()
        || side.root.x_axis.w != 0.0
        || side.root.y_axis.w != 0.0
        || side.root.z_axis.w != 0.0
        || side.root.w_axis.w != 1.0
    {
        return Err(ObservationError::InvalidComposition);
    }
    if side.native_rig.is_some_and(|rig| !rig.is_finite()) {
        return Err(ObservationError::InvalidNativeRig);
    }
    Ok(())
}

fn validate_tracking(tracked: TrackedPose) -> Result<(), ObservationError> {
    if (tracked.position_valid && !tracked.pose.position.is_finite())
        || (tracked.orientation_valid && !unit_rotation(tracked.pose.orientation))
        || tracked.linear_velocity.is_some_and(|v| !v.is_finite())
        || tracked.angular_velocity.is_some_and(|v| !v.is_finite())
    {
        return Err(ObservationError::InvalidTracking);
    }
    Ok(())
}

fn unit_rotation(rotation: Quat) -> bool {
    rotation.is_finite() && (rotation.length_squared() - 1.0).abs() <= 1.0e-4
}

#[cfg(test)]
mod tests;
