//! Body calibration: the dimensions the body file holds, derived from the two things the headset can
//! measure, and the guided flow that measures them.
//!
//! The headset measures the standing eye height (the head above the STAGE floor, see
//! [`crate::vr::stage`]) and, in a T-pose, the distance between the two controllers' grip poses.
//! Every other dimension is derived from those with generic adult proportions ([`derive`]): the
//! segment ratios of Drillis and Contini's anthropometric tables as tabulated in Winter's
//! *Biomechanics and Motor Control of Human Movement*. The height, shoulder, and hand ratios are
//! the ones [`BodyDimensions::GENERIC_ADULT`] follows. The arm segments are not: the tables'
//! per-height upper arm and forearm lengths, with the shoulders and hands, add up to more than an
//! adult's arm span, so the derivation fits the two segments to the measured span instead and
//! takes only their proportion to each other from the tables. The generic adult keeps the tables'
//! longer segments.
//!
//! [`Calibration`] is the flow's state: which step it is on, what it has measured, and the capture
//! in progress. It takes the time and the live measurement as arguments, so it runs without a
//! headset in tests; the pause menu's `VR: BODY` page ([`crate::controllers::body::menu`]) drives
//! it.

mod derive;
mod flow;

pub use derive::*;
pub use flow::*;

/// Fixtures shared by the derivation's and the flow's tests.
#[cfg(test)]
pub(crate) mod tests {
    use crate::controllers::body::calibration::{
        EYE_HEIGHT_RATIO, GRIP_TO_FINGERTIP_SHARE, HAND_RATIO, Measurements,
    };

    /// A player of about 1.80 m with a span equal to their height.
    pub(crate) fn player() -> Measurements {
        let height = 1.80;
        Measurements {
            eye_height: height * EYE_HEIGHT_RATIO,
            grip_span: height - 2.0 * GRIP_TO_FINGERTIP_SHARE * HAND_RATIO * height,
        }
    }

    /// The eye height of a 1.80 m player sitting on an ordinary chair.
    pub(crate) const SEATED_EYE_HEIGHT: f32 = 1.2;
}
