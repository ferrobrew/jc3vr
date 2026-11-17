use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::{
    testing::visual::RequestId,
    vr::pose_control::{self, PoseValues},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PoseError {
    InvalidCaseId(&'static str),
    NonFiniteAngle(&'static str),
    AngleOutOfRange(&'static str),
}

impl std::fmt::Display for PoseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCaseId(reason) => write!(f, "visual pose: invalid case id: {reason}"),
            Self::NonFiniteAngle(angle) => {
                write!(f, "visual pose: the {angle} angle is not finite")
            }
            Self::AngleOutOfRange(angle) => {
                write!(f, "visual pose: the {angle} angle is outside [-180, 180]")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub(crate) struct OrientationOffset {
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub roll_deg: f32,
}

impl OrientationOffset {
    pub(crate) fn quaternion(self) -> Quat {
        Quat::from_euler(
            pose_control::EULER,
            self.yaw_deg.to_radians(),
            self.pitch_deg.to_radians(),
            self.roll_deg.to_radians(),
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PoseTable {
    case_id: String,
    yaw_deg: f32,
    pitch_deg: f32,
    roll_deg: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PoseTable")]
pub(crate) struct PoseRequest {
    case_id: RequestId,
    yaw_deg: f32,
    pitch_deg: f32,
    roll_deg: f32,
}

impl PoseRequest {
    pub(crate) fn new(
        case_id: RequestId,
        yaw_deg: f32,
        pitch_deg: f32,
        roll_deg: f32,
    ) -> Result<Self, PoseError> {
        for (angle, value) in [("yaw", yaw_deg), ("pitch", pitch_deg), ("roll", roll_deg)] {
            if !value.is_finite() {
                return Err(PoseError::NonFiniteAngle(angle));
            }
            if !(-180.0..=180.0).contains(&value) {
                return Err(PoseError::AngleOutOfRange(angle));
            }
        }
        Ok(Self {
            case_id,
            yaw_deg,
            pitch_deg,
            roll_deg,
        })
    }

    fn offset(&self) -> OrientationOffset {
        OrientationOffset {
            yaw_deg: self.yaw_deg,
            pitch_deg: self.pitch_deg,
            roll_deg: self.roll_deg,
        }
    }

    pub(crate) fn target(&self, base: PoseValues) -> PoseValues {
        PoseValues::from_pose(
            base.position,
            base.orientation() * self.offset().quaternion(),
        )
    }

    pub(crate) fn evidence(&self, base: PoseValues, applied: PoseValues) -> PoseEvidence {
        PoseEvidence {
            case_id: self.case_id.clone(),
            offset: self.offset(),
            anchor: PoseTransform::from(base),
            applied: PoseTransform::from(applied),
        }
    }

    pub(crate) fn matches_evidence(&self, evidence: &PoseEvidence) -> bool {
        if evidence.case_id != self.case_id || evidence.offset != self.offset() {
            return false;
        }
        let anchor_position = Vec3::from_array(evidence.anchor.position);
        let anchor_orientation = Quat::from_array(evidence.anchor.orientation);
        let applied_position = Vec3::from_array(evidence.applied.position);
        let applied_orientation = Quat::from_array(evidence.applied.orientation);
        anchor_position.is_finite()
            && anchor_orientation.is_finite()
            && applied_position.is_finite()
            && applied_orientation.is_finite()
            && applied_position.distance(anchor_position) <= POSE_POSITION_TOLERANCE
            && orientations_match(
                applied_orientation,
                anchor_orientation * self.offset().quaternion(),
            )
    }
}

impl TryFrom<PoseTable> for PoseRequest {
    type Error = PoseError;

    fn try_from(table: PoseTable) -> Result<Self, Self::Error> {
        let case_id = RequestId::parse(&table.case_id).map_err(PoseError::InvalidCaseId)?;
        Self::new(case_id, table.yaw_deg, table.pitch_deg, table.roll_deg)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub(crate) struct PoseTransform {
    pub position: [f32; 3],
    pub orientation: [f32; 4],
}

impl From<PoseValues> for PoseTransform {
    fn from(values: PoseValues) -> Self {
        Self {
            position: values.position.to_array(),
            orientation: values.orientation().to_array(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct PoseEvidence {
    pub case_id: RequestId,
    pub offset: OrientationOffset,
    pub anchor: PoseTransform,
    pub applied: PoseTransform,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PoseFrame {
    NotRequested,
    Defer,
    Apply(PoseValues),
    Observed(PoseEvidence),
}

const POSE_POSITION_TOLERANCE: f32 = 1e-5;
const POSE_ORIENTATION_TOLERANCE: f32 = 1e-5;

pub(crate) fn pose_frame(
    request: Option<&PoseRequest>,
    frame_identity: Option<i64>,
    rendered: Option<(PoseValues, PoseValues)>,
    live: Option<PoseValues>,
) -> PoseFrame {
    let Some(request) = request else {
        return PoseFrame::NotRequested;
    };
    if frame_identity.is_none() {
        return PoseFrame::Defer;
    }
    let Some((base, applied)) = rendered else {
        return PoseFrame::Defer;
    };
    let target = request.target(base);
    if pose_matches(applied, target) {
        return PoseFrame::Observed(request.evidence(base, applied));
    }
    if live.is_some_and(|current| pose_matches(current, target)) {
        PoseFrame::Defer
    } else {
        PoseFrame::Apply(target)
    }
}

pub(crate) fn pose_matches(actual: PoseValues, target: PoseValues) -> bool {
    actual.position.is_finite()
        && target.position.is_finite()
        && actual.position.distance(target.position) <= POSE_POSITION_TOLERANCE
        && orientations_match(actual.orientation(), target.orientation())
}

pub(crate) fn orientations_match(actual: Quat, target: Quat) -> bool {
    if !actual.is_finite() || !target.is_finite() {
        return false;
    }
    let actual = actual.normalize();
    let target = target.normalize();
    if !actual.is_finite() || !target.is_finite() {
        return false;
    }
    let actual = if actual.dot(target) < 0.0 {
        -actual
    } else {
        actual
    };
    (actual.x - target.x).abs() <= POSE_ORIENTATION_TOLERANCE
        && (actual.y - target.y).abs() <= POSE_ORIENTATION_TOLERANCE
        && (actual.z - target.z).abs() <= POSE_ORIENTATION_TOLERANCE
        && (actual.w - target.w).abs() <= POSE_ORIENTATION_TOLERANCE
}
