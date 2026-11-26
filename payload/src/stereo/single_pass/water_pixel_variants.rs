//! The pixel-shader variants the WaveWorks water surface's per-eye re-issue binds in place of the
//! engine's own.
//!
//! The water surface is drawn once per eye into that eye's half of the double-wide target
//! (`nv_water_per_eye` in `hooks/graphics_engine/scene/water.rs`). Its pixel shaders take every
//! screen-space lookup from the full-target screen UV, which is right for the buffers that are
//! double-wide and wrong for two things that are not:
//!
//! - The lake permutation's refraction displacement is a horizontal UV offset, which spans twice
//!   the eye-relative distance on a double-wide target; its packed variant halves it.
//! - The planar reflection texture covers one eye-sized frame, so each eye reads half of it
//!   stretched over its own half; the per-eye variants look it up by an eye-local coordinate
//!   (`dxbc_stereo::localize_planar_reflection`, behind `single_pass.water_reflection_per_eye`).
//!
//! The variants are built once, when the engine creates the original shader, and hang off it as
//! private data, so binding one during the re-issue is a lookup. The per-eye reflection variants are
//! built on top of the packed one, since only one shader can be bound.

use std::mem::size_of;

use anyhow::Context as _;
use dxbc_stereo::DxbcError;
use sha2::{Digest, Sha256};
use windows::{
    Win32::Graphics::Direct3D11::{ID3D11DeviceContext, ID3D11PixelShader},
    core::{GUID, IUnknown, Interface},
};

use crate::stereo::{
    config::STEREO_CONFIG,
    single_pass::{
        per_eye_reissue_eye,
        shader_identity::{self, LAKE_PS},
    },
};

/// Build and attach the variants of a newly created pixel shader. A shader with neither the lake's
/// refraction displacement nor a planar reflection gets none.
pub(crate) fn register(original: &ID3D11PixelShader, code: &[u8]) -> anyhow::Result<()> {
    let packed =
        if code.len() == LAKE_BYTECODE_SIZE && format!("{:x}", Sha256::digest(code)) == LAKE_PS {
            Some(dxbc_stereo::scale_lake_refraction(code)?)
        } else {
            None
        };
    let base = packed.as_deref().unwrap_or(code);
    let eyes = match [false, true].map(|right| dxbc_stereo::localize_planar_reflection(base, right))
    {
        [Ok(left), Ok(right)] => Some([left, right]),
        [
            Err(DxbcError::NoPlanarReflection),
            Err(DxbcError::NoPlanarReflection),
        ] => None,
        [Err(error), _] | [_, Err(error)] => {
            return Err(error).context("water pixel variants: planar reflection");
        }
    };
    if packed.is_none() && eyes.is_none() {
        return Ok(());
    }
    let device = unsafe { original.GetDevice() }?;
    let attach = |key: &GUID, bytes: &[u8]| -> anyhow::Result<()> {
        // The variant is created as the original's identity, so the shader census attributes it to
        // the engine shader it stands in for.
        let _identity = unsafe { shader_identity::begin(code.as_ptr(), code.len() as u64) };
        let mut variant = None;
        unsafe { device.CreatePixelShader(bytes, None, Some(&mut variant)) }?;
        let variant = variant.context("water pixel variants: missing created shader")?;
        unsafe { original.SetPrivateDataInterface(key, &variant) }?;
        Ok(())
    };
    if let Some(packed) = &packed {
        attach(&PACKED_VARIANT, packed)?;
    }
    if let Some(eyes) = &eyes {
        for (key, bytes) in EYE_VARIANTS.iter().zip(eyes) {
            attach(key, bytes)?;
        }
    }
    Ok(())
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

/// Bind the bound pixel shader's variant for the eye being re-issued: the eye's reflection variant
/// while `single_pass.water_reflection_per_eye` is on, otherwise the packed refraction variant.
/// `None`, binding nothing, when the bound shader has no applicable variant.
pub(super) fn bind(ctx: &ID3D11DeviceContext) -> Option<Restore<'_>> {
    let mut shader = None;
    unsafe { ctx.PSGetShader(&mut shader, None, None) };
    let shader = shader?;
    let variant = per_eye_reissue_eye()
        .filter(|_| STEREO_CONFIG.query(|c| c.single_pass.water_reflection_per_eye))
        .and_then(|eye| EYE_VARIANTS.get(eye))
        .and_then(|key| variant(&shader, key))
        .or_else(|| variant(&shader, &PACKED_VARIANT))?;
    unsafe { ctx.PSSetShader(&variant, None) };
    Some(Restore { ctx, shader })
}

/// The lake permutation's bytecode length, checked ahead of its hash so other shaders skip hashing.
pub(crate) const LAKE_BYTECODE_SIZE: usize = 21_568;

/// The private-data keys the variants hang off the original shader under.
const PACKED_VARIANT: GUID = GUID::from_u128(0x3e04c878_10a7_4e6c_a99f_e708da00cc63);
const EYE_VARIANTS: [GUID; 2] = [
    GUID::from_u128(0x7d0b2f4e_8c61_4a35_9e1d_52c3a1f08b46),
    GUID::from_u128(0x7d0b2f4e_8c61_4a35_9e1d_52c3a1f08b47),
];

fn variant(shader: &ID3D11PixelShader, key: &GUID) -> Option<ID3D11PixelShader> {
    let mut value: Option<IUnknown> = None;
    let mut size = size_of::<Option<IUnknown>>() as u32;
    unsafe { shader.GetPrivateData(key, &mut size, Some((&raw mut value).cast())) }.ok()?;
    value?.cast().ok()
}
