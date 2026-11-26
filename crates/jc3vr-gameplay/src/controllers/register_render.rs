//! Registers the controller subsystems with the render layer: the world-space overlay's and the
//! scope lens's ticks in the post-draw, the body estimator's GPU pass, what the overlay's draw
//! reads, the presentation layer's edits of the body's palette, and the camera's participants (the
//! grapple's aim matrix and the camera height readout). The overlay's and the lens's draws are the
//! render layer's own, which it registers itself.

use jc3gi::graphics_engine::{device::Device, texture::Texture};
use jc3vr_body::controllers::{aim, body::estimator, overlay, presentation::PlayerBodyEdits};
use jc3vr_core::frame_timing::{self, Slot};
use jc3vr_view::{
    controllers::overlay::inputs::{OverlayInputs, register_overlay_inputs},
    hooks::{
        camera::{CameraParticipants, register_camera_participants},
        graphics_engine::{
            palette_edit::register_player_draw_edits,
            post_draw::{DispatchTick, FramePass, register_dispatch_tick, register_frame_pass},
        },
    },
};
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;

use crate::{controllers::weapons::scope, hooks::camera::height};

pub(super) fn register() {
    register_dispatch_tick(DispatchTick::WorldOverlay, overlay::tick);
    register_dispatch_tick(DispatchTick::ScopeLens, scope::tick);
    register_frame_pass(FramePass::BodyEstimator, body_estimator_pass);
    register_overlay_inputs(OverlayInputs {
        settings: overlay::draw_settings,
        hand_poses: overlay::render_poses,
    });
    register_player_draw_edits(&PlayerBodyEdits);
    register_camera_participants(CameraParticipants {
        matrix_override: aim::camera_matrix_override,
        placed: height::record,
    });
}

/// The learned body estimator's GPU pass, once per frame: it takes the window the game thread
/// posted and collects finished estimates without waiting.
fn body_estimator_pass(context: &ID3D11DeviceContext, device: &Device, _target: &Texture) {
    let _timing = frame_timing::scope(Slot::BodyEstimator);
    estimator::render_tick(context, &device.m_Device);
}
