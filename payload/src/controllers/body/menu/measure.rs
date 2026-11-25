//! The live measurements the calibration steps read: the head's height above the floor, and the
//! distance between the controllers' grips.

use glam::Vec3;

use crate::{
    controllers::{
        body::calibration::Unavailable,
        frame::{self, Hand, TrackedPose},
    },
    vr::{self, stage::StageInfo},
};

/// The head's height above the floor now.
pub(super) fn live_eye_height() -> Result<f32, Unavailable> {
    eye_height_from(vr::render_params(0).is_some(), vr::stage::stage())
}

/// The distance between the controllers' grips now.
pub(super) fn live_grip_span() -> Result<f32, Unavailable> {
    let grips = frame::with_latest(|f| Hand::BOTH.map(|hand| f.hand(hand).grip));
    grip_span_from(vr::render_params(0).is_some(), grips)
}

pub(super) fn eye_height_from(rendering: bool, stage: StageInfo) -> Result<f32, Unavailable> {
    if !rendering {
        return Err(Unavailable::NoHeadset);
    }
    if !stage.available {
        return Err(Unavailable::NoFloor);
    }
    stage.head_height.ok_or(Unavailable::HeadNotTracked)
}

/// The grip span from both hands' grips, indexed by [`Hand::index`]. A grip counts while its
/// position is valid, whether measured or inferred: arms spread in a T-pose are outside the view
/// of a headset's own cameras, where inside-out tracking reports the controllers' positions as
/// inferred.
pub(super) fn grip_span_from(
    rendering: bool,
    grips: Option<[TrackedPose; 2]>,
) -> Result<f32, Unavailable> {
    if !rendering {
        return Err(Unavailable::NoHeadset);
    }
    let grips = grips.ok_or(Unavailable::NoControllers)?;
    let position = |hand: Hand| -> Result<Vec3, Unavailable> {
        let grip = grips[hand.index()];
        if grip.position_valid {
            Ok(grip.pose.position)
        } else {
            Err(Unavailable::ControllerLost(hand))
        }
    };
    Ok(position(Hand::Left)?.distance(position(Hand::Right)?))
}
