//! Fixtures the body-bound subsystems' tests share, which move with them to the body crate.
//!
//! The head pose's participants ([`HeadParticipants`]) are registered at install by the crate's
//! composition, which names the grapple above the body. A test that drives the head composition
//! without that install registers [`register_head_participants`]'s table instead: the body-bound
//! participants themselves, and stand-ins for the grapple's that act as the grapple's own do while
//! no grapple runs, which no body test starts.

use std::sync::Once;

use jc3vr_core::published::Stamp;
use jc3vr_view::{headpose, headpose::HeadParticipants};

use crate::{
    controllers::{
        body::{
            procedural::{authority::Case, published_status},
            rig::follow_settings,
        },
        body_frame,
        mode::{self, Mode},
        roomscale::{frame_shared, seat::SeatPhase},
    },
    hooks::input::swim,
};

/// Register the head pose's participants once, for a test that drives the head composition
/// without the install that registers them.
pub(crate) fn register_head_participants() {
    static REGISTERED: Once = Once::new();
    REGISTERED.call_once(|| headpose::register_head_participants(&PARTICIPANTS));
}

static PARTICIPANTS: HeadParticipants = HeadParticipants {
    advance_grapple: |_, _, _, _| {},
    filter_body_rotation: |body| body,
    filter_anchor: |anchor| anchor,
    grapple_blend: || 0.0,
    take_body_yaw_retarget: || None,
    log_grapple_tick: |_, _| {},
    log_grapple_frame: |_| {},
    follow_settings,
    orientation_evals: || body_frame::liveness().on_foot.map_or(0, Stamp::as_nanos),
    swim_evals: || body_frame::liveness().swimming.map_or(0, Stamp::as_nanos),
    swimming: swim::swimming,
    vehicle_transition: || {
        frame_shared()
            .filter(|shared| shared.fresh(Stamp::now()))
            .is_some_and(|shared| matches!(shared.seat, SeatPhase::Entering | SeatPhase::Exiting))
    },
    body_thrown: || {
        matches!(
            published_status().case,
            Case::Ragdoll | Case::GettingUp | Case::Dead
        )
    },
    ui_mode: || mode::current().mode == Mode::Ui,
};
