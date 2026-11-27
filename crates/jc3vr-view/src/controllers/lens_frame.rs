//! The frame's scope lens as the scopes plan it and the lens pass reads it: the lens's geometry,
//! the constants of the lens shaders derived from it, and the pass's readout.
//!
//! The scopes, in the gameplay layer, hand the frame's [`LensFrame`] over once per frame with
//! [`set_frame`]; the lens pass reads it with [`frame`] on every dispatch and records what it drew
//! here.

use glam::{Mat4, Vec3};
use parking_lot::Mutex;

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
    pub(crate) source_view_projection: [f32; 16],
    pub(crate) eyepiece_from_eye: [f32; 3],
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
    *FRAME.lock() = frame;
    if frame.sighting.is_none() {
        STATUS.lock().eyes = None;
    }
}

pub fn draw_status() -> LensDrawStatus {
    STATUS.lock().clone()
}

/// The frame's lens, as last handed to the pass.
pub fn frame() -> LensFrame {
    *FRAME.lock()
}

/// Record a draw of the lens on `eye`, reading the image of `source`.
pub fn record_drawn(eye: usize, source: usize) {
    let mut status = STATUS.lock();
    status.eyes = Some((eye, source));
    status.frames += 1;
}

/// Record a failure of the pass, logging it when it differs from the last one.
pub fn report_error(message: String) {
    let mut status = STATUS.lock();
    if status.error.as_deref() != Some(message.as_str()) {
        tracing::error!("{message}");
    }
    status.error = Some(message);
}

static FRAME: Mutex<LensFrame> = Mutex::new(LensFrame {
    armed: false,
    sighting: None,
    lens: None,
});

static STATUS: Mutex<LensDrawStatus> = Mutex::new(LensDrawStatus {
    eyes: None,
    frames: 0,
    error: None,
});

/// The nearest an eye is taken to be to the lens, so the field stays finite.
const MIN_EYE_DISTANCE_M: f32 = 0.01;

#[cfg(test)]
mod tests {
    use super::*;

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
