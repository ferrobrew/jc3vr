//! Registers the controller subsystems with the render layer: the world-space overlay's and the
//! scope lens's ticks in the post-draw ([`DISPATCH_TICKS`]), the body estimator's GPU pass, what the
//! overlay's draw reads, the presentation layer's edits of the body's palette, and the camera's
//! participants (the grapple's aim matrix and the camera height readout). The overlay's and the
//! lens's draws are the render layer's own, which its post-draw runs itself. [`register`] reports
//! any of these still missing to the install's registration check.

use jc3gi::graphics_engine::{device::Device, texture::Texture};
use jc3vr_body::controllers::{aim, body::estimator, overlay, presentation::PlayerBodyEdits};
use jc3vr_core::{
    frame_timing::{self, Slot},
    registration::RegistrationCheck,
};
use jc3vr_view::{
    controllers::overlay::inputs::{
        OverlayInputs, register_overlay_inputs, registered as overlay_inputs,
    },
    hooks::{
        camera::{
            CameraParticipants, camera_participants_registered, register_camera_participants,
        },
        graphics_engine::{
            palette_edit::{player_draw_edits, register_player_draw_edits},
            post_draw::{
                DispatchTick, FramePass, dispatch_ticks_registered, frame_pass_registered,
                register_dispatch_ticks, register_frame_pass,
            },
        },
    },
};
use jc3vr_weapons::controllers::weapons::scope;
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;

use crate::hooks::camera::height;

pub(super) fn register(check: &mut RegistrationCheck) {
    register_dispatch_ticks(&DISPATCH_TICKS);
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

    check.require(
        "post-draw",
        "the overlay's and the scope lens's dispatch ticks",
        dispatch_ticks_registered(),
    );
    check.require(
        "post-draw",
        "the body estimator's frame pass",
        frame_pass_registered(FramePass::BodyEstimator),
    );
    check.require(
        "overlay",
        "the overlay's draw inputs",
        overlay_inputs().is_some(),
    );
    check.require(
        "render block",
        "the presentation layer's player draw edits",
        player_draw_edits().is_some(),
    );
    check.require(
        "camera",
        "the grapple's aim matrix and the camera height readout",
        camera_participants_registered(),
    );
}

/// The post-draw's dispatch ticks, in the order they run: the world-space overlay's producers, then
/// the scope lens's plan.
static DISPATCH_TICKS: [DispatchTick; 2] = [overlay::tick, scope::tick];

/// The learned body estimator's GPU pass, once per frame: it takes the window the game thread
/// posted and collects finished estimates without waiting.
fn body_estimator_pass(context: &ID3D11DeviceContext, device: &Device, _target: &Texture) {
    let _timing = frame_timing::scope(Slot::BodyEstimator);
    estimator::render_tick(context, &device.m_Device);
}
