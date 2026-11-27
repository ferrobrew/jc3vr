//! FSR's sub-pixel camera jitter: the sequence, the projection offset it becomes, and the frame's
//! phase.
//!
//! Each VR frame takes the next offset of FSR's own Halton(2, 3) sequence, at the length FSR
//! recommends for the render-to-display ratio (eight phases at 1:1, `ffxFsr2GetJitterPhaseCount`).
//! The offset is in the eye's pixels, x right and y down, and means the content moves by that many
//! pixels: a pixel centre then samples the unjittered image at its centre less the offset, which is
//! what FSR's `jitterOffset` describes (`ffx_fsr2_upsample.h`). In clip space it is a translation of
//! `(2 x / width, -2 y / height)` times `w`, applied after the projection, as the FSR documentation's
//! `jitterTranslationMatrix * projectionMatrix` and the engine's own TAA jitter
//! ([`RenderEngine::ApplyJitterTransform`](jc3gi::graphics_engine::render_engine::RenderEngine))
//! do. Both eyes take the same offset in their own pixels; under single-pass each eye's NDC spans
//! its own half of the double-wide target, so the eye's width there is half the target's.
//!
//! The offset is chosen once per VR frame on the game thread ([`frame_offset`]) and travels with
//! the frame's per-eye render parameters ([`crate::vr::EyeRenderParams::jitter_ndc`]), so the
//! projection every eye-image pass reads and the offset FSR is told belong to the same frame.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use glam::{Mat4, Vec2, Vec3};
use jc3gi::types::math::Matrix4;

use crate::fsr::FSR_CONFIG;

/// The frame's jitter, or `None` when the jitter is off: neither FSR nor the mod's own temporal
/// resolve running (the game's SMAA 1x has no temporal resolve to use it), the jitter switch off,
/// or no FSR resolve yet to size the eyes. Advances the phase; call once per VR frame.
pub(crate) fn frame_offset() -> Option<FrameJitter> {
    let enabled = FSR_CONFIG.query(|c| c.antialiasing.temporal() && c.jitter);
    if !enabled {
        return None;
    }
    let (width, height) = eye_size()?;
    let phase_count = phase_count(width);
    let index = PHASE.fetch_add(1, Ordering::Relaxed);
    let pixels = pixel_offset(index, phase_count);
    Some(FrameJitter {
        ndc: ndc_from_pixels(pixels, width, height),
        phase: index % phase_count,
        phase_count,
    })
}

/// One frame's jitter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameJitter {
    /// The clip-space offset (the NDC translation times `w`).
    pub ndc: Vec2,
    /// The offset's index in the sequence, from zero at the last [`reset_phase`].
    pub phase: u32,
    /// The sequence's length.
    pub phase_count: u32,
}

/// Restart the sequence at its first phase, so every configuration the visual harness renders after
/// an FSR history reset takes the same offsets on the same frames.
pub fn reset_phase() {
    PHASE.store(0, Ordering::Relaxed);
}

/// Publish the eye size FSR or the temporal resolve last resolved at, from the render thread, for
/// [`frame_offset`] to scale the next frames' offsets by. Under single-pass it is one eye's half of
/// the double-wide target.
pub fn publish_eye_size(width: u32, height: u32) {
    EYE_SIZE.store(
        (u64::from(width) << 32) | u64::from(height),
        Ordering::Relaxed,
    );
}

/// FSR's sequence length for an eye rendered at the display size, `width` pixels wide.
pub fn phase_count(width: u32) -> u32 {
    let width = i32::try_from(width).unwrap_or(i32::MAX);
    u32::try_from(fsr_sys::jitter_phase_count(width, width).max(1)).unwrap_or(1)
}

/// The sequence's offset for frame `index`, in pixels (x right, y down), each within half a pixel.
pub(crate) fn pixel_offset(index: u32, phase_count: u32) -> Vec2 {
    let phase = index % phase_count.max(1) + 1;
    Vec2::new(halton(phase, 2) - 0.5, halton(phase, 3) - 0.5)
}

/// The clip-space offset that moves the content by `pixels` in a `width` by `height` eye.
pub(crate) fn ndc_from_pixels(pixels: Vec2, width: u32, height: u32) -> Vec2 {
    Vec2::new(
        2.0 * pixels.x / width.max(1) as f32,
        -2.0 * pixels.y / height.max(1) as f32,
    )
}

/// The pixel offset a clip-space offset moves the content by in a `width` by `height` eye: the
/// inverse of [`ndc_from_pixels`], in FSR's `jitterOffset` convention.
pub fn pixels_from_ndc(ndc: Vec2, width: u32, height: u32) -> Vec2 {
    Vec2::new(ndc.x * width as f32 * 0.5, -ndc.y * height as f32 * 0.5)
}

/// The clip-space translation by `ndc`, to pre-multiply onto a projection or a view-projection
/// (column vectors): `x' = x + ndc.x * w`, `y' = y + ndc.y * w`.
pub fn translation(ndc: Vec2) -> Mat4 {
    Mat4::from_translation(Vec3::new(ndc.x, ndc.y, 0.0))
}

/// The clip-to-previous-clip matrix of an eye that rasterizes with `current` jittered by `jitter`
/// (a clip-space offset), into the unjittered `previous`, that yields motion vectors free of the
/// jitter, as FSR wants them (its documentation: motion vectors carry no jitter unless it is told to
/// cancel it, which the mod does not ask).
///
/// With `T(j)` the clip-space translation by `j`, a pixel at clip `x` of the jittered render is the
/// point `C⁻¹ T(-j) x`; its jitter-free motion is where that point lies in the previous frame less
/// where it lies in the unjittered current one, `ndc(P C⁻¹ T(-j) x) - (ndc(x) - j)`. The decode takes
/// `ndc(M x) - ndc(x)`, so `M = T(j) P C⁻¹ T(-j)`: the previous view-projection with this frame's
/// jitter, which is the engine's own construction for its TAA. The matrices are taken unjittered and
/// the jitter applied here in `f64`, since a jitter folded into an `f32` view-projection rounds its
/// world-scale translation differently every frame, which reads as motion at a still camera.
pub fn jitter_free_reprojection(previous: Mat4, current: Mat4, jitter: Vec2) -> Matrix4 {
    let shift = translation(jitter).as_dmat4();
    let back = translation(-jitter).as_dmat4();
    Matrix4::from((shift * previous.as_dmat4() * current.as_dmat4().inverse() * back).as_mat4())
}

/// The clip-to-previous-clip matrix from an eye that rasterizes with `current` jittered by `jitter`
/// into `previous` as it stands: what a velocity encoding that takes the current position from the
/// rasterized pixel and the previous one through `previous` records, jitter and all. Formed in
/// `f64` from the unjittered `current`, as [`jitter_free_reprojection`] is.
pub fn reprojection_from_jittered(previous: Mat4, current: Mat4, jitter: Vec2) -> Matrix4 {
    let back = translation(-jitter).as_dmat4();
    Matrix4::from((previous.as_dmat4() * current.as_dmat4().inverse() * back).as_mat4())
}

/// The Halton low-discrepancy sequence's `index`th element in `base`, in `[0, 1)`.
fn halton(mut index: u32, base: u32) -> f32 {
    let mut fraction = 1.0;
    let mut result = 0.0;
    while index > 0 {
        fraction /= base as f32;
        result += fraction * (index % base) as f32;
        index /= base;
    }
    result
}

/// The eye size FSR last resolved at, when it has resolved one.
fn eye_size() -> Option<(u32, u32)> {
    let packed = EYE_SIZE.load(Ordering::Relaxed);
    let (width, height) = ((packed >> 32) as u32, packed as u32);
    (width > 0 && height > 0).then_some((width, height))
}

/// The next frame's index into the sequence.
static PHASE: AtomicU32 = AtomicU32::new(0);
/// The eye size FSR last resolved at, width in the high half.
static EYE_SIZE: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sequence_is_fsrs_own() {
        let count = phase_count(3584);
        assert_eq!(count, 8, "FSR's length at 1:1");
        for index in 0..24 {
            let (x, y) = fsr_sys::jitter_offset(index as i32, count as i32);
            let ours = pixel_offset(index, count);
            assert!(
                (ours.x - x).abs() < 1e-6 && (ours.y - y).abs() < 1e-6,
                "{index}"
            );
        }
    }

    #[test]
    fn the_sequence_stays_within_half_a_pixel_and_never_rests_at_the_centre() {
        let count = phase_count(1000);
        let offsets: Vec<Vec2> = (0..count).map(|i| pixel_offset(i, count)).collect();
        for offset in &offsets {
            assert!(offset.abs().max_element() < 0.5);
            assert!(offset.length() > 1e-3, "FSR requires a nonzero offset");
        }
        let mean = offsets.iter().sum::<Vec2>() / count as f32;
        assert!(
            mean.abs().max_element() < 0.07,
            "the phases cover the pixel: {mean}"
        );
        assert_eq!(
            pixel_offset(count, count),
            offsets[0],
            "the sequence repeats"
        );
    }

    #[test]
    fn the_offset_moves_the_content_by_the_pixels() {
        let (width, height) = (3584, 3561);
        let pixels = Vec2::new(0.25, -0.375);
        let ndc = ndc_from_pixels(pixels, width, height);
        assert!((pixels_from_ndc(ndc, width, height) - pixels).length() < 1e-6);
        let projection = Mat4::perspective_infinite_reverse_rh(1.5, 1.0, 0.1);
        let point = glam::Vec4::new(0.3, -0.2, -4.0, 1.0);
        let still = projection * point;
        let moved = translation(ndc) * projection * point;
        let screen = |clip: glam::Vec4| {
            let ndc = clip.truncate().truncate() / clip.w;
            Vec2::new(
                (ndc.x * 0.5 + 0.5) * width as f32,
                (0.5 - ndc.y * 0.5) * height as f32,
            )
        };
        assert!((screen(moved) - screen(still) - pixels).length() < 1e-3);
        assert!(
            (moved.z / moved.w - still.z / still.w).abs() < 1e-7,
            "the depth is untouched"
        );
    }

    #[test]
    fn removing_the_offset_restores_the_projection() {
        let projection = Mat4::perspective_infinite_reverse_rh(1.2, 0.9, 0.1);
        let ndc = Vec2::new(1.3e-4, -2.1e-4);
        let restored = translation(-ndc) * translation(ndc) * projection;
        assert!(restored.abs_diff_eq(projection, 1e-7));
    }

    /// The decode's motion for the pixel at `clip` through the clip-to-previous-clip `matrix`, in
    /// NDC: the current position less the previous one.
    fn decoded_motion(matrix: &Matrix4, clip: glam::Vec4) -> glam::Vec2 {
        let previous = glam::Mat4::from(*matrix) * clip;
        clip.truncate().truncate() - previous.truncate().truncate() / previous.w
    }

    #[test]
    fn a_still_jittered_eye_decodes_to_no_motion() {
        let projection = glam::Mat4::perspective_infinite_reverse_rh(1.5, 1.0, 0.1);
        let view = glam::Mat4::from_translation(glam::Vec3::new(-5528.0, -12.0, 1926.0)).inverse();
        let matrix = jitter_free_reprojection(
            projection * view,
            projection * view,
            glam::Vec2::new(-2.3e-4, 0.4e-4),
        );
        for clip in [
            glam::Vec4::new(0.3, -0.4, 0.02, 1.0),
            glam::Vec4::new(-0.8, 0.7, 0.5, 1.0),
            glam::Vec4::new(0.0, 0.0, 0.0, 1.0),
        ] {
            let motion = decoded_motion(&matrix, clip);
            assert!(motion.abs().max_element() < 1e-6, "{motion}");
        }
    }

    #[test]
    fn a_moving_jittered_eye_decodes_to_the_unjittered_motion() {
        let projection = glam::Mat4::perspective_infinite_reverse_rh(1.5, 1.0, 0.1);
        let current_view = glam::Mat4::IDENTITY;
        let previous_view = glam::Mat4::from_rotation_translation(
            glam::Quat::from_rotation_y(0.02),
            glam::Vec3::new(0.03, 0.0, 0.01),
        )
        .inverse();
        let jitter = glam::Vec2::new(-1.0e-4, -2.5e-4);
        let current = projection * current_view;
        let previous = projection * previous_view;
        let matrix = jitter_free_reprojection(previous, current, jitter);
        let point = glam::Vec4::new(0.7, -0.3, -5.0, 1.0);
        let ndc = |clip: glam::Vec4| clip.truncate().truncate() / clip.w;
        let expected = ndc(current * point) - ndc(previous * point);
        let rendered = translation(jitter) * current * point;
        let motion = decoded_motion(&matrix, rendered / rendered.w);
        assert!(
            (motion - expected).abs().max_element() < 1e-5,
            "{motion} {expected}"
        );
        // The encoding's own reprojection keeps the jitter, as the rasterized position does.
        let encoded = decoded_motion(
            &reprojection_from_jittered(previous, current, jitter),
            rendered / rendered.w,
        );
        assert!(
            (encoded - expected - jitter).abs().max_element() < 1e-5,
            "{encoded}"
        );
    }
}
