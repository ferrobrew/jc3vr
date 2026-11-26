//! The gameplay participants of the VR frame and its recenters ([`jc3vr_render::vr::FrameParticipants`]).

use jc3vr_render::vr::{FrameParticipants, register_frame_participants};

use crate::controllers::roomscale;

pub(super) fn register() {
    register_frame_participants(&PARTICIPANTS);
}

static PARTICIPANTS: FrameParticipants = FrameParticipants {
    compose_cockpit: roomscale::compose_frame,
    publish_composition: roomscale::publish_composition,
    on_recenter: roomscale::on_recenter,
    manual_recenter_kind: roomscale::manual_recenter_kind,
};
