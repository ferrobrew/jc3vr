//! Pure full-transform composition with explicit baseline and root semantics.

use glam::{Mat4, Quat, Vec3};

/// Identity of one finalized animation sample, independent of the render endpoints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Stamp {
    pub character: u64,
    pub continuity: u64,
    pub finalized: u64,
}

/// Position, orientation, and scale are retained together until matrix composition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Trs {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Trs {
    pub const IDENTITY: Self = Self {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    pub fn matrix(self) -> Result<Mat4, InvalidPair> {
        if !self.translation.is_finite()
            || !self.rotation.is_finite()
            || !self.scale.is_finite()
            || self.scale.abs().min_element() <= f32::EPSILON
            || (self.rotation.length_squared() - 1.0).abs() > 0.001
        {
            return Err(InvalidPair::InvalidTransform);
        }
        let matrix =
            Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation);
        if !matrix.is_finite() {
            return Err(InvalidPair::InvalidTransform);
        }
        Ok(matrix)
    }
}

/// The authored channel includes its complete local hierarchy, not one unparented camera bone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LocalSample {
    pub stamp: Stamp,
    pub transform: Trs,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RootSample {
    pub stamp: Stamp,
    pub transform: Trs,
}

/// Root and local data must have independent, equal capture stamps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Sample {
    pub root: RootSample,
    pub local: LocalSample,
}

impl Sample {
    pub fn stamp(self) -> Result<Stamp, InvalidPair> {
        if self.root.stamp != self.local.stamp {
            return Err(InvalidPair::MixedRootLocalTime);
        }
        self.root.transform.matrix()?;
        self.local.transform.matrix()?;
        Ok(self.root.stamp)
    }
}

/// The baseline belongs to the accepted run and is not an assumed eye origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Baseline {
    pub run: u64,
    pub sample: Sample,
}

/// Only established root semantics can select a composition operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RootTreatment {
    Unknown,
    SeparateFromAuthoredLocal,
    ComposeWithAuthoredLocal,
}

/// These operations are distinct; neither treats the raw local translation as an eye offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeltaRequest {
    AuthoredLocalRelativeToBaseline,
    RootCompensatedRelativeToBaseline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InvalidPair {
    MissingSample,
    MixedRootLocalTime,
    CharacterChanged,
    ContinuityChanged,
    RepeatedOrReversedSample,
    BaselineChanged,
    InvalidTransform,
    RootTreatmentUnknown,
    RootTreatmentMismatch,
    Teleport,
    Interrupted,
    SourceChanged,
    AdmissionMismatch,
    BlendEpochMismatch,
    InvalidBlendWeight,
    UnsupportedState,
    InvalidTransition,
    ClipTimeReversed,
}

/// A pair is constructed only from two distinct finalized local/root samples.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Pair {
    previous: Sample,
    current: Sample,
    baseline: Baseline,
}

impl Pair {
    pub fn new(previous: Sample, current: Sample, baseline: Baseline) -> Result<Self, InvalidPair> {
        let before = previous.stamp()?;
        let after = current.stamp()?;
        let origin = baseline.sample.stamp()?;
        same_continuity(before, after)?;
        same_continuity(origin, before)?;
        if before.finalized >= after.finalized || origin.finalized > before.finalized {
            return Err(InvalidPair::RepeatedOrReversedSample);
        }
        Ok(Self {
            previous,
            current,
            baseline,
        })
    }

    pub fn deltas(
        self,
        request: DeltaRequest,
        root_treatment: RootTreatment,
    ) -> Result<[Mat4; 2], InvalidPair> {
        match (request, root_treatment) {
            (_, RootTreatment::Unknown) => return Err(InvalidPair::RootTreatmentUnknown),
            (
                DeltaRequest::AuthoredLocalRelativeToBaseline,
                RootTreatment::SeparateFromAuthoredLocal,
            )
            | (
                DeltaRequest::RootCompensatedRelativeToBaseline,
                RootTreatment::ComposeWithAuthoredLocal,
            ) => {}
            _ => return Err(InvalidPair::RootTreatmentMismatch),
        }
        let baseline = channel(self.baseline.sample, request)?;
        let inverse = baseline.inverse();
        let deltas = [
            inverse * channel(self.previous, request)?,
            inverse * channel(self.current, request)?,
        ];
        if deltas.iter().any(|matrix| !matrix.is_finite()) {
            return Err(InvalidPair::InvalidTransform);
        }
        Ok(deltas)
    }
}

/// The view base excludes the tracked head; the head transform is applied exactly once here.
pub(crate) fn compose_view(
    view_base: Trs,
    authored_delta: Mat4,
    tracked_head: Trs,
) -> Result<Mat4, InvalidPair> {
    let result = view_base.matrix()? * authored_delta * tracked_head.matrix()?;
    if !result.is_finite() {
        return Err(InvalidPair::InvalidTransform);
    }
    Ok(result)
}

pub(crate) fn same_continuity(left: Stamp, right: Stamp) -> Result<(), InvalidPair> {
    if left.character != right.character {
        return Err(InvalidPair::CharacterChanged);
    }
    if left.continuity != right.continuity {
        return Err(InvalidPair::ContinuityChanged);
    }
    Ok(())
}

fn channel(sample: Sample, request: DeltaRequest) -> Result<Mat4, InvalidPair> {
    sample.stamp()?;
    let local = sample.local.transform.matrix()?;
    match request {
        DeltaRequest::AuthoredLocalRelativeToBaseline => Ok(local),
        DeltaRequest::RootCompensatedRelativeToBaseline => {
            Ok(sample.root.transform.matrix()? * local)
        }
    }
}
