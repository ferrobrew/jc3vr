//! Registers the controllers' render passes in the post-draw: the scope lens's copy of the
//! dispatch's eye images, and the world-space overlay's and the scope lens's draws onto each eye.

use crate::{
    controllers::{overlay::pass as overlay_pass, weapons::scope::pass as scope_pass},
    hooks::graphics_engine::post_draw::{EyeLayer, register_eye_layer, register_scene_capture},
};

pub fn register() {
    register_scene_capture(scope_pass::capture);
    register_eye_layer(EyeLayer::WorldOverlay, overlay_pass::draw);
    register_eye_layer(EyeLayer::ScopeLens, scope_pass::draw);
}
