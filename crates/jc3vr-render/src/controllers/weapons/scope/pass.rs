//! The scope lens's draw: the dispatch's eye images copied while a scoped weapon is held, and the
//! lens drawn on the sighting eye, with the constants of the lens shaders.
//!
//! The lens's plan is the scopes' ([`crate::controllers::weapons::scope`]): once per frame they hand
//! this pass the frame's [`LensFrame`], which the capture and the draw of every dispatch read.

use std::time::Duration;

use glam::{Mat4, Vec3};
use jc3gi::graphics_engine::{device::Device, texture::Texture};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::{D3D11_VIEWPORT, ID3D11DeviceContext};

use crate::{
    controllers::{
        overlay::view::{self, EyeView},
        weapons::scope::renderer::{EyeRegion, Renderer},
    },
    stereo::{STEREO_STATE, layout::collapse_active},
};

/// The lens of the frame, as its draw needs it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct LensDraw {
    /// The eyepiece's centre.
    pub eyepiece: Vec3,
    /// The sight line's direction, a unit vector.
    pub direction: Vec3,
    /// The distance along the sight line to the aim point, metres.
    pub focus_m: f32,
    /// The lens's radius, metres.
    pub radius_m: f32,
    /// The lens's magnification.
    pub zoom: f32,
    /// Where the dark rim starts, as a fraction of the lens radius.
    pub vignette_start: f32,
}

impl LensDraw {
    /// The lens draw's constants for the sighting eye `eye`, reading the image of `source`.
    pub fn constants(&self, eye: &EyeCamera, source: &EyeCamera) -> LensConstants {
        let (right, up) = lens_basis(self.direction);
        let eyepiece_from_eye = self.eyepiece - eye.position;
        LensConstants {
            view_projection: eye.offset_view_projection.to_cols_array(),
            source_view_projection: source.offset_view_projection.to_cols_array(),
            eyepiece_from_eye: eyepiece_from_eye.to_array(),
            radius: self.radius_m,
            right: right.to_array(),
            tangent: field_tangent(self.radius_m, eyepiece_from_eye.length(), self.zoom),
            up: up.to_array(),
            focus: self.focus_m,
            direction: self.direction.to_array(),
            vignette_start: self.vignette_start.clamp(0.0, 0.99),
            eyepiece_from_source: (self.eyepiece - source.position).to_array(),
            reticle: 1.0,
        }
    }
}

/// The constants of the lens shaders' `cbuffer Lens`.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct LensConstants {
    /// The sighting eye's view-projection of a position relative to it.
    pub view_projection: [f32; 16],
    /// The source eye's view-projection of a position relative to it.
    pub source_view_projection: [f32; 16],
    pub eyepiece_from_eye: [f32; 3],
    pub radius: f32,
    pub right: [f32; 3],
    pub tangent: f32,
    pub up: [f32; 3],
    pub focus: f32,
    pub direction: [f32; 3],
    pub vignette_start: f32,
    pub eyepiece_from_source: [f32; 3],
    /// 1 to draw the reticle, 0 not to.
    pub reticle: f32,
}

/// An eye's camera as the lens needs it: its position and the view-projection of a position
/// relative to it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct EyeCamera {
    pub position: Vec3,
    pub offset_view_projection: Mat4,
}

/// Two unit vectors that span the lens's plane with `direction`, right and up as seen looking along
/// it. The lens is round and its image is mapped through the same vectors, so their roll about the
/// sight line does not change what the lens shows.
pub fn lens_basis(direction: Vec3) -> (Vec3, Vec3) {
    let right = direction
        .cross(Vec3::Y)
        .try_normalize()
        .unwrap_or_else(|| direction.cross(Vec3::Z).normalize());
    let up = right.cross(direction);
    (right, up)
}

/// The tangent off the sight line that the lens's rim shows: the rim's tangent from an eye
/// `eye_distance` from the lens, divided by the zoom.
pub fn field_tangent(radius: f32, eye_distance: f32, zoom: f32) -> f32 {
    radius / eye_distance.max(MIN_EYE_DISTANCE_M) / zoom.max(1.0)
}

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

/// The frame's lens, as the scopes planned it.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct LensFrame {
    /// Whether a scoped weapon is held and aimed, so the eyes' images are copied.
    pub armed: bool,
    /// The eye at the eyepiece.
    pub sighting: Option<usize>,
    /// The lens, while armed.
    pub lens: Option<LensDraw>,
}

/// The draw's readout: what the lens drew and its last failure.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LensDrawStatus {
    /// The eye that sights through the lens and the eye whose image it reads.
    pub eyes: Option<(usize, usize)>,
    /// Frames the lens was drawn in, since the payload loaded.
    pub frames: u64,
    /// The last failure to build the pipeline, copy an eye, or draw.
    pub error: Option<String>,
}

/// Hand the pass the frame's lens. Called once per frame, before the frame's first capture; a
/// frame without a sighting eye clears the readout's eyes.
pub fn set_frame(frame: LensFrame) {
    STATE.lock().frame = frame;
    if frame.sighting.is_none() {
        STATUS.lock().eyes = None;
    }
}

pub fn draw_status() -> LensDrawStatus {
    STATUS.lock().clone()
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
    if crate::stereo::far_phase() {
        return;
    }
    let mut state = STATE.lock();
    if !state.frame.armed {
        return;
    }
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
    if crate::stereo::far_phase() || jc3vr_core::director::clean_view::hides_everything() {
        return;
    }
    let mut state = STATE.lock();
    let (Some(lens), Some(sighting)) = (state.frame.lens, state.frame.sighting) else {
        return;
    };
    if sighting != eye {
        return;
    }
    let Some((v, viewport)) = view::current(eye) else {
        return;
    };
    let Some(renderer) = state.renderer(device) else {
        return;
    };
    let fresh = |e: usize| {
        renderer
            .copy(e)
            .filter(|c| c.taken.elapsed() < MAX_COPY_AGE)
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
        Ok(()) => {
            let mut status = STATUS.lock();
            status.eyes = Some((eye, source));
            status.frames += 1;
        }
        Err(error) => report(error),
    }
}

struct State {
    renderer: Option<Renderer>,
    /// Set when the pipeline could not be built, so that it is not retried every frame.
    failed: bool,
    frame: LensFrame,
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
    frame: LensFrame {
        armed: false,
        sighting: None,
        lens: None,
    },
});

static STATUS: Mutex<LensDrawStatus> = Mutex::new(LensDrawStatus {
    eyes: None,
    frames: 0,
    error: None,
});

/// How old an eye's copy may be and still be read: a frame or two at any usable frame rate. The
/// other eye's copy is the previous frame's when the first eye sights in two-pass stereo.
const MAX_COPY_AGE: Duration = Duration::from_millis(100);

/// The nearest an eye is taken to be to the lens, so the field stays finite.
const MIN_EYE_DISTANCE_M: f32 = 0.01;

/// An eye's camera under the collapse, which keeps the render camera centred.
fn collapse_camera(eye: usize) -> Option<EyeCamera> {
    let center = STEREO_STATE.lock().center_transform?;
    let params = crate::vr::render_params(eye)?;
    let eye_world = params.resolve(Mat4::from(center));
    // The lens is drawn after FSR has resolved the eye, so it takes the projection without FSR's
    // jitter, as `view::current` does.
    Some(camera_of(&EyeView::from_eye_world(
        eye_world,
        params.unjittered_projection_reverse_z(),
    )))
}

fn camera_of(view: &EyeView) -> EyeCamera {
    EyeCamera {
        position: view.position,
        offset_view_projection: view.offset_view_projection,
    }
}

fn report(error: anyhow::Error) {
    let message = format!("{error:#}");
    let mut status = STATUS.lock();
    if status.error.as_deref() != Some(message.as_str()) {
        tracing::error!("{message}");
    }
    status.error = Some(message);
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

    #[test]
    fn the_basis_is_orthonormal_even_looking_straight_up() {
        for direction in [Vec3::NEG_Z, Vec3::Y, Vec3::new(0.3, -0.2, -0.9).normalize()] {
            let (right, up) = lens_basis(direction);
            assert!((right.length() - 1.0).abs() < 1e-5);
            assert!((up.length() - 1.0).abs() < 1e-5);
            assert!(right.dot(up).abs() < 1e-5);
            assert!(right.dot(direction).abs() < 1e-5);
            assert!(up.dot(direction).abs() < 1e-5);
        }
        let (right, up) = lens_basis(Vec3::NEG_Z);
        assert!(right.distance(Vec3::X) < 1e-5);
        assert!(up.distance(Vec3::Y) < 1e-5);
    }
}
