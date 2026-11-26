//! The glare chain's filters, kept to each eye's half of the double-wide target under the collapse.
//!
//! `GlareEffect::Apply` builds the bloom from the half-resolution scene in about two dozen
//! fullscreen draws: a bright pass, an optional morphological opening, box downsamples to a quarter,
//! an eighth, and a sixteenth of the screen, a separable Gaussian blur at each level accumulated
//! back up by bilinear upsampling, an optional temporal blend, and a separable saw blur at the
//! eighth level for the secondary bloom. Every one of those targets is double-wide under the
//! collapse, with the eyes side by side, and every draw reads its source by the screen UV, so each
//! pixel already reads its own eye, except at the seam: the blurs' taps, the downsample's far corner,
//! the upsample's bilinear footprint, and the opening's neighbourhood reach across the middle of the
//! source into the other eye, where an eye-sized target clamps to its own edge. A seventeen-texel
//! blur at the sixteenth level spans a large share of an eye, and the levels accumulate, so a bright
//! sun near one eye's inner edge bleeds into the other eye's whole inner half.
//!
//! [`dxbc_stereo::localize_post_filter`] rewrites a filter so every read stays in the eye's half the
//! pixel lies in. Two of the programs the chain draws with, the plain copy it upsamples with and the
//! box downsample, are shared with passes that draw elsewhere, so the rewrite cannot replace them at
//! creation. Instead each qualifying program gets its rewrite built alongside it as a variant, and
//! the draw detour binds the variant in place of the original only while the glare chain runs
//! ([`Scope`]). The chain keeps its draw count; nothing is re-issued.
//!
//! The final composite reads the chain's two results by the screen UV too, and its bilinear footprint
//! crosses the seam the same way. Those lookups are rewritten at creation
//! ([`dxbc_stereo::localize_composite_bloom`], from `hooks/graphics_engine/shader.rs`), since the
//! composite always draws over the whole target.

use std::{
    mem::size_of,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use anyhow::Context as _;
use windows::{
    Win32::Graphics::Direct3D11::{ID3D11DeviceContext, ID3D11PixelShader},
    core::{GUID, IUnknown, Interface},
};

use crate::stereo::single_pass::shader_identity;

/// Build and attach the eye-half variant of a newly created pixel shader. A shader that is not one
/// of the glare chain's filters gets none.
pub(crate) fn register(original: &ID3D11PixelShader, code: &[u8]) -> anyhow::Result<()> {
    let Ok(rewritten) = dxbc_stereo::localize_post_filter(code) else {
        return Ok(());
    };
    let device = unsafe { original.GetDevice() }?;
    // The variant is created as the original's identity, so the shader census attributes it to the
    // engine shader it stands in for.
    let _identity = unsafe { shader_identity::begin(code.as_ptr(), code.len() as u64) };
    let mut variant = None;
    unsafe { device.CreatePixelShader(&rewritten, None, Some(&mut variant)) }?;
    let variant = variant.context("glare filter variants: missing created shader")?;
    unsafe { original.SetPrivateDataInterface(&VARIANT, &variant) }?;
    REGISTERED.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// How many filter variants have been built since injection.
pub fn registered_count() -> usize {
    REGISTERED.load(Ordering::Relaxed)
}

/// Marks the glare chain as running for its lifetime, when the collapse is live. Entered on the render thread around `GlareEffect::Apply`.
pub(crate) struct Scope {
    entered: bool,
}

impl Scope {
    pub(crate) fn enter() -> Scope {
        let entered = jc3vr_view::stereo::layout::collapse_active();
        if entered {
            IN_GLARE.store(true, Ordering::Relaxed);
        }
        Scope { entered }
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        if self.entered {
            IN_GLARE.store(false, Ordering::Relaxed);
        }
    }
}

/// Puts the engine's shader back when the variant's draw is done.
pub(super) struct Restore<'a> {
    ctx: &'a ID3D11DeviceContext,
    shader: ID3D11PixelShader,
}

impl Drop for Restore<'_> {
    fn drop(&mut self) {
        unsafe { self.ctx.PSSetShader(&self.shader, None) };
    }
}

/// Inside the glare chain, bind the bound pixel shader's eye-half variant for one draw. `None`,
/// binding nothing, outside the chain or when the bound shader has no variant.
pub(super) fn bind(ctx: &ID3D11DeviceContext) -> Option<Restore<'_>> {
    if !IN_GLARE.load(Ordering::Relaxed) {
        return None;
    }
    let mut shader = None;
    unsafe { ctx.PSGetShader(&mut shader, None, None) };
    let shader = shader?;
    let variant = variant(&shader)?;
    unsafe { ctx.PSSetShader(&variant, None) };
    Some(Restore { ctx, shader })
}

/// Whether the glare chain is running under the eye-half treatment.
static IN_GLARE: AtomicBool = AtomicBool::new(false);
static REGISTERED: AtomicUsize = AtomicUsize::new(0);

/// The private-data key the variant hangs off the original shader under.
const VARIANT: GUID = GUID::from_u128(0x5b9e6a1c_2f4d_4c7e_8a13_96d0e4b7c251);

fn variant(shader: &ID3D11PixelShader) -> Option<ID3D11PixelShader> {
    let mut value: Option<IUnknown> = None;
    let mut size = size_of::<Option<IUnknown>>() as u32;
    unsafe { shader.GetPrivateData(&VARIANT, &mut size, Some((&raw mut value).cast())) }.ok()?;
    value?.cast().ok()
}
