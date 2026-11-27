//! The gameplay participants of the head pose ([`jc3vr_view::headpose::HeadParticipants`]): the
//! character's state the head composition reads, and the two inputs to the body's heading that
//! systems above the head pose own: the wingsuit take-off's heading and the arm strokes' turn.
//! [`register`] reports the table to the install's registration check when it is missing. The
//! grapple registers its comfort filter as the head pose's body frame filter itself
//! ([`jc3vr_grapple::register`]).

use jc3vr_body::{
    controllers::{
        body::{
            procedural::{authority::Case, published_status},
            rig::follow_settings,
        },
        body_frame,
        locomotion::strokes,
        mode,
        mode::Mode,
        roomscale::{frame_shared, seat::SeatPhase},
    },
    hooks::input::swim,
};
use jc3vr_core::{published::Stamp, registration::RegistrationCheck};
use jc3vr_vehicles::controllers::vehicles::take_off;
use jc3vr_view::headpose::{
    HeadParticipants, head_participants_registered, register_head_participants,
};

pub(super) fn register(check: &mut RegistrationCheck) {
    register_head_participants(&PARTICIPANTS);
    check.require(
        "head pose",
        "the gameplay participants",
        head_participants_registered(),
    );
}

static PARTICIPANTS: HeadParticipants = HeadParticipants {
    follow_settings,
    on_foot_ran: || body_frame::liveness().on_foot,
    swim_ran: || body_frame::liveness().swimming,
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
    take_off_heading: take_off::held_heading,
    take_swim_turn: strokes::take_turn,
    ui_mode: || mode::current().mode == Mode::Ui,
};
