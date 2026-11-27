//! Shaders that read whichever eye `cb13` is pinned to, and the pin their draws take.
//!
//! Some pipelines have no way to learn which eye they are drawing. The terrain patch system's
//! tessellated draws build clip in the domain shader, and their vertex shaders fill the whole
//! `TEXCOORD3` interpolant, so there is no free lane to ride an eye index through the hull stage; its
//! GPU-indirect basemesh draws use vertex shaders that already consume `SV_InstanceID`. Neither can be
//! instance-doubled, so both are drawn once per eye, and with their shaders left alone both eyes of
//! that re-issue would be drawn from the centred camera. `dxbc_stereo::reproject_pinned_eye` rewrites such a shader to read
//! `cb13`'s first eye slot unconditionally, and every per-eye re-issue that finds one bound pins both
//! slots to the eye it is drawing, the same pin the already-instanced re-issue uses.
//!
//! A pinned shader is identified by a private-data tag on the D3D object, set where the shader is
//! created, so the mark lives and dies with the shader and needs no pointer bookkeeping.

use std::{ffi::c_void, sync::atomic::Ordering};

use jc3gi::types::math::Vector4;
use jc3vr_view::stereo::engine_context::EngineContext;
use windows::{
    Win32::Graphics::Direct3D11::{ID3D11DeviceChild, ID3D11DeviceContext},
    core::{GUID, Interface},
};

use crate::stereo::single_pass::{
    BOUND_VS, CB13, STEREO_CB_TOTAL_ROWS, pin_rows_to_eye, write_cb13_rows,
};

/// Mark `shader` as created from a pinned-eye rewrite.
pub(crate) fn tag_pinned_eye(shader: &ID3D11DeviceChild) {
    let marker = 1u8;
    // SAFETY: `shader` is a live device child; the data is one byte copied by the call.
    if let Err(e) = unsafe {
        shader.SetPrivateData(&PINNED_EYE_TAG, 1, Some(std::ptr::from_ref(&marker).cast()))
    } {
        tracing::warn!(target: "single_pass", "pinned eye: tagging a shader: {e}");
    }
}

/// `cb13` pinned to one eye at a time for a per-eye re-issue, restored to the dual-eye rows on drop.
pub(super) struct Cb13EyePin {
    d3d: EngineContext,
    rows: [Vector4; STEREO_CB_TOTAL_ROWS],
}

impl Cb13EyePin {
    /// Start a pin if the vertex or domain shader bound on `context` carries the pinned-eye rewrite
    /// and `cb13` holds rows to restore. `None` means the draw needs no pin.
    pub(super) fn begin(context: *mut c_void) -> Option<Self> {
        // SAFETY: `context` is the live immediate context the draw detour was called on.
        let ctx = unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }?;
        if !pinned_program_bound(ctx) {
            return None;
        }
        let rows = CB13.lock().live_rows()?;
        Some(Self {
            d3d: EngineContext::get()?,
            rows,
        })
    }

    /// Fill both `cb13` eye slots with `eye`'s rows, reporting whether the write landed.
    pub(super) fn pin(&self, eye: usize) -> bool {
        write_cb13_rows(self.d3d, &pin_rows_to_eye(&self.rows, eye))
    }
}

impl Drop for Cb13EyePin {
    fn drop(&mut self) {
        if !write_cb13_rows(self.d3d, &self.rows) {
            tracing::warn!(
                target: "single_pass",
                "cb13 restore failed after a pinned-eye re-issue; geometry may render from one eye \
                 for the rest of the pass"
            );
        }
    }
}

/// The private-data key [`tag_pinned_eye`] sets.
const PINNED_EYE_TAG: GUID = GUID::from_u128(0x5f1d_2c7a_93e4_4b8e_a6d1_0c37_b2e9_f418);

/// Whether the bound vertex or domain shader carries [`PINNED_EYE_TAG`].
fn pinned_program_bound(ctx: &ID3D11DeviceContext) -> bool {
    let vs = BOUND_VS.load(Ordering::Relaxed) as *mut c_void;
    // SAFETY: the pointer is the shader the last `VSSetShader` bound, which the context keeps alive
    // while it stays bound.
    if let Some(vs) = unsafe { ID3D11DeviceChild::from_raw_borrowed(&vs) }
        && is_pinned(vs)
    {
        return true;
    }
    let mut ds = None;
    // SAFETY: `DSGetShader` writes an owned reference (or null), released when `ds` drops.
    unsafe { ctx.DSGetShader(&mut ds, None, None) };
    ds.and_then(|ds| ds.cast::<ID3D11DeviceChild>().ok())
        .is_some_and(|ds| is_pinned(&ds))
}

fn is_pinned(shader: &ID3D11DeviceChild) -> bool {
    let mut marker = 0u8;
    let mut size = 1u32;
    // SAFETY: the buffer is one byte, and `size` says so.
    unsafe {
        shader.GetPrivateData(
            &PINNED_EYE_TAG,
            &mut size,
            Some(std::ptr::from_mut(&mut marker).cast()),
        )
    }
    .is_ok()
        && size == 1
        && marker == 1
}
