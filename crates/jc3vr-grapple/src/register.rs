//! What the grapple contributes to the layers below it, registered in one place: the comfort
//! filter on the head pose's body frame ([`jc3vr_view::headpose::BodyFrameFilter`]) and the pump
//! of the hang swing the hands' anchor drives ([`BodyContributions::hang_pump`]).
//!
//! The payload's composition calls [`register`] once, before any hook is installed. [`register`]
//! reports the body frame filter to the install's registration check if it is missing, and the
//! body's registration reports the hang pump.

use jc3vr_body::contributions::BodyContributions;
use jc3vr_core::registration::RegistrationCheck;
use jc3vr_view::headpose::{
    BodyFrameFilter, body_frame_filter_registered, register_body_frame_filter,
};

use crate::grapple::{self, physical::swing};

/// Register the grapple's contributions to the layers below it, filling its fields of `body`, and
/// report what is still missing to `check`.
pub fn register(body: &mut BodyContributions, check: &mut RegistrationCheck) {
    register_body_frame_filter(&COMFORT_FILTER);
    body.hang_pump = Some(swing::HANG_PUMP);
    check.require(
        "head pose",
        "the grapple's body frame filter",
        body_frame_filter_registered(),
    );
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
