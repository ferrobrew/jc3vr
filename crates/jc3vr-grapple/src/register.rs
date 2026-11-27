//! What the grapple contributes to the layers below it, registered in one place: the comfort
//! filter on the head pose's body frame ([`jc3vr_view::headpose::BodyFrameFilter`]) and the pump
//! of the hang swing the hands' anchor drives ([`BodyContributions::hang_pump`]).
//!
//! The crate above that composes the install calls [`register`] before any hook is installed, and
//! the readers report what they find missing to the install's registration check.

use jc3vr_body::contributions::BodyContributions;
use jc3vr_view::headpose::{BodyFrameFilter, register_body_frame_filter};

use crate::grapple::{self, physical::swing};

/// Register the grapple's contributions to the layers below it, filling its fields of `body`.
pub fn register(body: &mut BodyContributions) {
    register_body_frame_filter(&COMFORT_FILTER);
    body.hang_pump = Some(swing::HANG_PUMP);
}

/// The grapple's comfort filter, as the head pose's body frame filter (`crate::grapple`).
static COMFORT_FILTER: BodyFrameFilter = BodyFrameFilter {
    advance: grapple::advance,
    filter_rotation: grapple::filter_body_rotation,
    filter_anchor: grapple::filter_anchor,
    engaged: || grapple::blend_factor() > 0.0,
    take_body_yaw_retarget: grapple::take_body_yaw_retarget,
    observe_tick: grapple::telemetry::log_tick,
    observe_frame: grapple::telemetry::log_frame,
};
