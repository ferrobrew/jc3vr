//! The gameplay participants of the VR frame and its recenters ([`crate::vr::FrameParticipants`]).

use crate::{
    controllers::roomscale,
    grapple,
    vr::{FrameParticipants, register_frame_participants},
};

pub(super) fn register() {
    register_frame_participants(&PARTICIPANTS);
}

static PARTICIPANTS: FrameParticipants = FrameParticipants {
    advance_grapple: grapple::advance,
    filter_anchor: grapple::filter_anchor,
    compose_cockpit: roomscale::compose_frame,
    publish_composition: roomscale::publish_composition,
    log_grapple_frame: grapple::telemetry::log_frame,
    on_recenter: roomscale::on_recenter,
    manual_recenter_kind: roomscale::manual_recenter_kind,
};
