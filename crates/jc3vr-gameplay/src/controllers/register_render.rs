//! Registers the controller subsystems with the render layer: the world-space overlay's and the
//! scope lens's ticks and draws in the post-draw, the body estimator's GPU pass, what the overlay's
//! draw reads, the presentation layer's edits of the body's palette, and the camera's participants
//! (the grapple's aim matrix and the camera height readout).

use jc3gi::graphics_engine::{device::Device, texture::Texture};
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;

use crate::{
    controllers::{
        aim,
        body::estimator,
        overlay::{
            self,
            pass::{OverlayInputs, register_overlay_inputs},
        },
        presentation::PlayerBodyEdits,
        weapons::scope,
    },
    frame_timing::{self, Slot},
    hooks::{
        camera::{CameraParticipants, height, register_camera_participants},
        graphics_engine::{
            palette_edit::register_player_draw_edits,
            post_draw::{
                DispatchTick, EyeLayer, FramePass, register_dispatch_tick, register_eye_layer,
                register_frame_pass, register_scene_capture,
            },
        },
    },
};

pub(super) fn register() {
    register_dispatch_tick(DispatchTick::WorldOverlay, overlay::tick);
    register_dispatch_tick(DispatchTick::ScopeLens, scope::tick);
    register_scene_capture(scope::capture);
    register_eye_layer(EyeLayer::WorldOverlay, overlay::draw);
    register_eye_layer(EyeLayer::ScopeLens, scope::draw);
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
