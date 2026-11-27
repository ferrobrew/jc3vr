//! The static light sprites' inverse screen width, staged at one eye's width under the double-wide
//! collapse.
//!
//! [`RenderBlockTypeLights::Setup`](jc3gi::graphics_engine::render_block::RenderBlockTypeLights::Setup)
//! stages the sprite vertex shader's inverse screen width as one over the width of the render
//! context's back-buffer texture, which under the double-wide collapse is both eyes' width. The shader
//! multiplies a sprite's pixel size by it to get the sprite's horizontal extent in clip units. With
//! the sprites routed to the eye halves (the `lightsource*` reprojection and the promotion of their
//! non-indexed draw), clip units span one eye, so left alone every sprite comes out half as wide as
//! in an eye-sized render. For the length of the call, the upload is rewritten to one over the width of the
//! eye half the collapse splits the bound target into.

use std::sync::atomic::{AtomicU32, Ordering};

use jc3vr_view::stereo::layout::double_wide_active;

use crate::stereo::single_pass::{collapse_eye_width, in_gbuffer_range};

/// The constant buffer the light type stages its sprite constants into.
pub(super) const LIGHT_CONSTANTS_CB: i32 = 1;
/// The float4 rows of that upload.
pub(super) const LIGHT_CONSTANT_ROWS: u32 = 4;
/// The component of the upload holding the inverse screen width: `.y` of its first row, as the
/// `lightsource*` vertex shaders read it.
const INVERSE_WIDTH_COMPONENT: usize = 1;

/// The light type's constant upload held at one eye's width for the life of the guard.
pub(crate) struct EyeLightWidth(());

impl EyeLightWidth {
    /// Arm the substitution when the double-wide collapse is drawing its routed range; `None` leaves
    /// the upload untouched.
    pub fn stage() -> Option<Self> {
        if !double_wide_active() || !in_gbuffer_range() {
            return None;
        }
        let eye_width = collapse_eye_width().filter(|width| *width > 0.0)?;
        ARMED_EYE_WIDTH.store(eye_width.to_bits(), Ordering::Release);
        Some(Self(()))
    }
}

impl Drop for EyeLightWidth {
    fn drop(&mut self) {
        ARMED_EYE_WIDTH.store(UNARMED, Ordering::Release);
    }
}

/// The light type's sprite constants with the inverse screen width at one eye's, when a guard is
/// armed and the upload is the type's. Called from the vertex-constant staging detour.
///
/// # Safety
///
/// `data` must point to `count` float4 rows when it is non-null.
pub(super) unsafe fn substitute_upload(
    cb_index: i32,
    start_offset: u32,
    data: *const f32,
    count: u32,
) -> Option<[f32; LIGHT_CONSTANT_ROWS as usize * 4]> {
    let bits = ARMED_EYE_WIDTH.load(Ordering::Acquire);
    if bits == UNARMED
        || data.is_null()
        || (cb_index, start_offset, count) != (LIGHT_CONSTANTS_CB, 0, LIGHT_CONSTANT_ROWS)
    {
        return None;
    }
    let mut rows = [0.0f32; LIGHT_CONSTANT_ROWS as usize * 4];
    // SAFETY: per this function's contract, `data` holds `count` float4 rows, which is `rows.len()`
    // floats.
    unsafe { std::ptr::copy_nonoverlapping(data, rows.as_mut_ptr(), rows.len()) };
    localize_inverse_width(&mut rows, f32::from_bits(bits));
    Some(rows)
}

/// Put one over `eye_width` in the upload's inverse screen width.
fn localize_inverse_width(rows: &mut [f32], eye_width: f32) {
    rows[INVERSE_WIDTH_COMPONENT] = eye_width.recip();
}

/// No guard is armed. Not the bit pattern of any positive width.
const UNARMED: u32 = 0;
static ARMED_EYE_WIDTH: AtomicU32 = AtomicU32::new(UNARMED);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_inverse_width_moves_to_the_eye() {
        let mut rows: Vec<f32> = (0..16).map(|i| i as f32).collect();
        rows[INVERSE_WIDTH_COMPONENT] = 1.0 / 1280.0;
        localize_inverse_width(&mut rows, 640.0);
        assert_eq!(rows[INVERSE_WIDTH_COMPONENT], 1.0 / 640.0);
        for (i, value) in rows.iter().enumerate() {
            if i != INVERSE_WIDTH_COMPONENT {
                assert_eq!(*value, i as f32);
            }
        }
    }

    #[test]
    fn an_unarmed_or_foreign_upload_is_left_alone() {
        let rows = [1.0f32; 16];
        // SAFETY: `rows` holds four float4 rows.
        assert!(unsafe { substitute_upload(1, 0, rows.as_ptr(), 4) }.is_none());
        ARMED_EYE_WIDTH.store(640.0f32.to_bits(), Ordering::Release);
        // SAFETY: as above.
        unsafe {
            assert!(substitute_upload(2, 0, rows.as_ptr(), 4).is_none());
            assert!(substitute_upload(1, 1, rows.as_ptr(), 3).is_none());
            let staged = substitute_upload(1, 0, rows.as_ptr(), 4).expect("armed light upload");
            assert_eq!(staged[INVERSE_WIDTH_COMPONENT], 1.0 / 640.0);
        }
        ARMED_EYE_WIDTH.store(UNARMED, Ordering::Release);
    }
}
