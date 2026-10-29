//! The atlas's aim part drawn at the aim point: the weapon reticle, the grapple reticle, and the
//! hit marker, gathered into one cell around the aim point ([`crate::hud::atlas`]) and drawn as a
//! billboard toward the aim point at the aim depth ([`crate::hud::aim`]).
//!
//! The billboard keeps the angular size the reticles have on the panel, and stands toward the aim
//! point the game's grapple reticle projects, which is where the camera's aim lands; while the
//! grapple reticle is held hidden, that point follows the grapple's hand aim instead of the gaze,
//! so the billboard stands along the camera's forward. Like every HUD quad it is drawn without a
//! depth test, after the panel, so the reticle is never hidden behind the surface it marks.
//!
//! On eye 0 the corners are placed from the head pose; both eyes then draw the same corners.

use std::sync::atomic::{AtomicBool, Ordering};

use glam::{Quat, Vec3, Vec4};
use jc3gi::graphics_engine::{device::Device, texture::Texture};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;

use crate::hud::{
    aim,
    atlas::{self, Part},
    quad::HudQuad,
    scaleform,
    world_markers::layout::{Billboard, billboard_corners, radians_per_texel},
};

/// The eye-0 inputs for placing the aim part.
pub(crate) struct FrameInputs {
    /// The head (cyclopean eye) pose the panel was placed from.
    pub head: Vec3,
    pub head_rotation: Quat,
    /// The panel's height over its distance: the angular size the HUD texture is shown at.
    pub panel_height_over_distance: f32,
}

/// Place the aim part for this frame (eye 0), after the atlas chose the frame's capture. Draws
/// nothing for the frame when the atlas did not move the aim part.
pub(crate) fn prepare_frame(device: &Device, inputs: &FrameInputs) {
    let mut state = STATE.lock();
    state.corners = None;
    if crate::director::clean_view::hides_everything() {
        return;
    }
    let Some((cell, anchor, hud_texture, size)) = atlas::part_texture(Part::Aim, |part| {
        (part.cell, part.anchor, part.hud_texture, part.target.size())
    }) else {
        return;
    };
    if state.quad.is_none() {
        match HudQuad::new(device) {
            Ok(quad) => state.quad = Some(quad),
            Err(e) => {
                log_once(&e);
                return;
            }
        }
    }
    let point = aim::point().filter(|_| !scaleform::grapple_reticle_hidden());
    let billboard = Billboard {
        head: inputs.head,
        head_rotation: inputs.head_rotation,
        direction: aim_direction(point, inputs.head, inputs.head_rotation),
        distance: aim::current(aim::REST_DISTANCE),
    };
    // The cell holds the reticles at its scale, so each of its texels covers that much less of the
    // stage than a panel texel does.
    let per_texel =
        radians_per_texel(inputs.panel_height_over_distance, hud_texture.1) / cell.scale.max(1e-3);
    state.corners = billboard_corners(&billboard, size, anchor, per_texel);
}

/// Drop the frame's placement (eye 0, when no panel pose exists).
pub(crate) fn clear_frame() {
    STATE.lock().corners = None;
}

/// Draw the aim part for this eye over `target`. Call on every eye after the atlas copied the
/// part's cell and after the panel.
pub(crate) fn draw(context: &ID3D11DeviceContext, device: &Device, target: &Texture) {
    let state = STATE.lock();
    let (Some(quad), Some(corners)) = (state.quad.as_ref(), state.corners) else {
        return;
    };
    atlas::part_texture(Part::Aim, |part| {
        quad.draw(context, device, target, part.target.color_srv(), &corners)
    });
}

/// The direction the aim part stands in from the head: toward the aim point when there is one,
/// otherwise the head's forward.
fn aim_direction(point: Option<Vec3>, head: Vec3, head_rotation: Quat) -> Vec3 {
    point
        .and_then(|point| (point - head).try_normalize())
        .unwrap_or(head_rotation * Vec3::NEG_Z)
}

/// The quad pass, created on the first frame with an aim part, and the frame's corners.
struct State {
    quad: Option<HudQuad>,
    corners: Option<[Vec4; 4]>,
}

static STATE: Mutex<State> = Mutex::new(State {
    quad: None,
    corners: None,
});

/// Log a resource failure once per session; the next frame retries regardless.
fn log_once(error: &anyhow::Error) {
    static LOGGED: AtomicBool = AtomicBool::new(false);
    if !LOGGED.swap(true, Ordering::Relaxed) {
        tracing::error!("hud reticle: {error:#}");
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;

    #[test]
    fn the_part_stands_toward_the_aim_point() {
        let head = Vec3::new(10.0, 2.0, -5.0);
        let direction = aim_direction(Some(head + Vec3::new(3.0, 0.0, 4.0)), head, Quat::IDENTITY);
        assert!(direction.abs_diff_eq(Vec3::new(0.6, 0.0, 0.8), 1e-6));
    }

    /// Without an aim point, or with one at the head, the part stands along the head's forward.
    #[test]
    fn without_a_point_the_part_stands_ahead() {
        let turned = Quat::from_rotation_y(FRAC_PI_2);
        let ahead = turned * Vec3::NEG_Z;
        assert!(aim_direction(None, Vec3::ZERO, turned).abs_diff_eq(ahead, 1e-6));
        let head = Vec3::new(1.0, 2.0, 3.0);
        assert!(aim_direction(Some(head), head, turned).abs_diff_eq(ahead, 1e-6));
    }
}
