//! What the VR runtime's teardown releases on the render side: the flip block the frame top sets
//! while a session presents, and the far-field share pipeline's resources.

use std::sync::atomic::Ordering;

use crate::{
    hooks::graphics_engine::graphics_engine::BLOCK_FLIP,
    vr::{TeardownParticipant, register_teardown_participant},
};

pub fn register() {
    register_teardown_participant(&TEARDOWN);
}

static TEARDOWN: TeardownParticipant = TeardownParticipant {
    release_flip: || BLOCK_FLIP.store(false, Ordering::Relaxed),
    release_resources: crate::far_field::share::teardown,
};
