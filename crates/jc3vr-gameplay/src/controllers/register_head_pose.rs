//! The gameplay participants of the head pose ([`jc3vr_view::headpose::HeadParticipants`]): the
//! grapple's comfort filter and the character's state the head composition reads.

use std::sync::atomic::Ordering;

use jc3vr_body::{
    controllers::{
        body::{
            procedural::{authority::Case, published_status},
            rig::follow_settings,
        },
        mode,
        mode::Mode,
        roomscale::{frame_shared, seat::SeatPhase},
    },
    hooks::input::{locomotion::ORIENTATION_EVAL_CALLS, swim},
};
use jc3vr_core::published::Stamp;
use jc3vr_grapple::grapple;
use jc3vr_view::headpose::{HeadParticipants, register_head_participants};

pub(super) fn register() {
    register_head_participants(&PARTICIPANTS);
}

static PARTICIPANTS: HeadParticipants = HeadParticipants {
    advance_grapple: grapple::advance,
    filter_body_rotation: grapple::filter_body_rotation,
    filter_anchor: grapple::filter_anchor,
    grapple_blend: grapple::blend_factor,
    take_body_yaw_retarget: grapple::take_body_yaw_retarget,
    log_grapple_tick: grapple::telemetry::log_tick,
    log_grapple_frame: grapple::telemetry::log_frame,
    follow_settings,
    orientation_evals: || ORIENTATION_EVAL_CALLS.load(Ordering::Relaxed),
    swim_evals: || swim::SWIM_EVAL_CALLS.load(Ordering::Relaxed),
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
