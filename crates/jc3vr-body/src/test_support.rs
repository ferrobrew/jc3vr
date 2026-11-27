//! Fixtures the body-bound subsystems' tests share, which move with them to the body crate.
//!
//! The head pose's participants ([`HeadParticipants`]) are registered at install by the crate's
//! composition. A test that drives the head composition without that install registers
//! [`register_head_participants`]'s table instead. No body frame filter is registered, so the head
//! pose passes the body frame through as it does while no grapple runs, which no body test starts.

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
        locomotion::strokes,
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
    // The take-off is the vehicles crate's, which no body test starts.
    take_off_heading: || None,
    take_swim_turn: strokes::take_turn,
    ui_mode: || mode::current().mode == Mode::Ui,
};
