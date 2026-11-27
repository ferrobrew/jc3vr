//! The gameplay participants of the VR frame and its recenters ([`jc3vr_view::vr::FrameParticipants`]).
//! [`register`] reports them to the install's registration check when they are missing.

use jc3vr_body::controllers::roomscale;
use jc3vr_core::registration::RegistrationCheck;
use jc3vr_view::vr::{
    FrameParticipants, frame_participants_registered, register_frame_participants,
};

pub(super) fn register(check: &mut RegistrationCheck) {
    register_frame_participants(&PARTICIPANTS);
    check.require(
        "VR frame",
        "the roomscale's frame participants",
        frame_participants_registered(),
    );
}

static PARTICIPANTS: FrameParticipants = FrameParticipants {
    compose_cockpit: roomscale::compose_frame,
    on_recenter: roomscale::on_recenter,
    manual_recenter_kind: roomscale::manual_recenter_kind,
};
