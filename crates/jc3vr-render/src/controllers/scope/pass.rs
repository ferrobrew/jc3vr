//! The scope lens's draw: the dispatch's eye images copied while a scoped weapon is held, and the
//! lens drawn on the sighting eye, with the constants of the lens shaders.
//!
//! The lens's plan is the scopes': once per frame they hand over the frame's [`LensFrame`]
//! ([`jc3vr_view::controllers::lens_frame`]), which the capture and the draw of every
//! dispatch read.

use std::time::Duration;

use jc3gi::graphics_engine::{device::Device, texture::Texture};
use jc3vr_view::{
    controllers::{
        lens_frame,
        lens_frame::{EyeCamera, LensFrame},
    },
    stereo::{collapse_eye, layout::collapse_active},
};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::{D3D11_VIEWPORT, ID3D11DeviceContext};

use crate::{
    controllers::{
        overlay::{view, view::EyeView},
        scope::renderer::{EyeRegion, Renderer},
    },
    tuning::RENDER,
};

/// The eye whose image the lens reads: the other eye when its image is available, since the scope
/// does not block it (in two-pass stereo the second eye's image is the previous frame's when the
/// first eye sights, reprojected by its own camera), and the sighting eye's own otherwise, whose
/// magnified region shows the scope's own model.
pub fn source_eye(sighting: usize, other_available: bool) -> usize {
    if other_available {
        1 - sighting.min(1)
    } else {
        sighting
    }
}

/// Release the lens pipeline, for eject: it holds each eye's image copy and a render-target view
/// over the engine's back buffer, which keeps that texture alive after the engine resizes off it.
pub fn release_on_eject() {
    STATE.lock().renderer = None;
}

/// Copy the dispatch's eye images for the lens while a scoped weapon is held. Called under the
/// engine's context lock, before the world-space overlay draws, with `target` the dispatch's final
/// back buffer.
pub fn capture(context: &ID3D11DeviceContext, device: &Device, target: &Texture, eye: usize) {
    if !lens_frame::frame().armed {
        return;
    }
    let mut state = STATE.lock();
    let Some(renderer) = state.renderer(device) else {
        return;
    };
    let width = u32::from(target.m_Width);
    let result = if collapse_active() {
        let half = width / 2;
        (0..2).try_for_each(|e| {
            let Some(camera) = collapse_camera(e) else {
                return Ok(());
            };
            let region = EyeRegion {
                left: e as u32 * half,
                width: half,
            };
            renderer.capture(&device.m_Device, context, target, region, e, camera)
        })
    } else {
        match view::current(eye) {
            Some((v, _)) => renderer.capture(
                &device.m_Device,
                context,
                target,
                EyeRegion { left: 0, width },
                eye,
                camera_of(&v),
            ),
            None => Ok(()),
        }
    };
    if let Err(error) = result {
        report(error);
    }
}

/// Draw the lens onto `target` for `eye`, when `eye` sights through it. Called under the engine's
/// context lock after the world-space overlay; under the collapse, the caller has selected the eye
/// with `set_collapse_ui_eye`.
pub fn draw(context: &ID3D11DeviceContext, device: &Device, target: &Texture, eye: usize) {
    if jc3vr_core::clean_view::hides_everything() {
        return;
    }
    let LensFrame {
        lens: Some(lens),
        sighting: Some(sighting),
        ..
    } = lens_frame::frame()
    else {
        return;
    };
    let mut state = STATE.lock();
    if sighting != eye {
        return;
    }
    let Some((v, viewport)) = view::current(eye) else {
        return;
    };
    let Some(renderer) = state.renderer(device) else {
        return;
    };
    let max_copy_age = Duration::from_millis(RENDER.get().scope_lens.max_copy_age_ms);
    let fresh = |e: usize| {
        renderer
            .copy(e)
            .filter(|c| c.taken.elapsed() < max_copy_age)
            .map(|c| c.camera)
    };
    let other = fresh(1 - eye.min(1));
    let source = source_eye(eye, other.is_some());
    let source_camera = if source == eye {
        // The own image is this dispatch's copy, taken with this eye's camera.
        fresh(eye).map(|_| camera_of(&v))
    } else {
        other
    };
    let Some(source_camera) = source_camera else {
        return;
    };
    let constants = lens.constants(&camera_of(&v), &source_camera);
    let viewport = viewport.unwrap_or(D3D11_VIEWPORT {
        TopLeftX: 0.0,
        TopLeftY: 0.0,
        Width: f32::from(target.m_Width),
        Height: f32::from(target.m_Height),
        MinDepth: 0.0,
        MaxDepth: 1.0,
    });
    let drawn = renderer.draw(
        &device.m_Device,
        context,
        target,
        viewport,
        &constants,
        source,
    );
    drop(state);
    match drawn {
        Ok(()) => lens_frame::record_drawn(eye, source),
        Err(error) => report(error),
    }
}

struct State {
    renderer: Option<Renderer>,
    /// Set when the pipeline could not be built, so that it is not retried every frame.
    failed: bool,
}

impl State {
    /// The pipeline, built on first use.
    fn renderer(&mut self, device: &Device) -> Option<&mut Renderer> {
        if self.failed {
            return None;
        }
        if self.renderer.is_none() {
            match Renderer::new(&device.m_Device) {
                Ok(renderer) => self.renderer = Some(renderer),
                Err(error) => {
                    self.failed = true;
                    report(error);
                    return None;
                }
            }
        }
        self.renderer.as_mut()
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    renderer: None,
    failed: false,
});

/// An eye's camera under the collapse, which keeps the render camera centred.
fn collapse_camera(eye: usize) -> Option<EyeCamera> {
    let eye = collapse_eye(eye)?;
    // The lens is drawn after FSR has resolved the eye, so it takes the projection without FSR's
    // jitter, as `view::current` does.
    Some(camera_of(&EyeView::from_eye_world(
        eye.world(),
        eye.params.unjittered_projection_reverse_z(),
    )))
}

fn camera_of(view: &EyeView) -> EyeCamera {
    EyeCamera {
        position: view.position,
        offset_view_projection: view.offset_view_projection,
    }
}

fn report(error: anyhow::Error) {
    lens_frame::report_error(format!("{error:#}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_source_is_the_other_eye_only_when_available() {
        assert_eq!(source_eye(0, true), 1);
        assert_eq!(source_eye(1, true), 0);
        assert_eq!(source_eye(1, false), 1);
    }
}
