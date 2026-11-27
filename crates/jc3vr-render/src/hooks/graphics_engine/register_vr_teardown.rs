//! What the VR runtime's teardown releases on the render side: the flip block the frame top sets
//! while a session presents.

use std::sync::atomic::Ordering;

use jc3vr_view::vr::{TeardownParticipant, register_teardown_participant};

use crate::hooks::graphics_engine::engine::BLOCK_FLIP;

pub fn register() {
    register_teardown_participant(&TEARDOWN);
}

static TEARDOWN: TeardownParticipant = TeardownParticipant {
    release_flip: || BLOCK_FLIP.store(false, Ordering::Relaxed),
};
