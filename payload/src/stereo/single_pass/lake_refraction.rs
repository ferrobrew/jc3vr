use std::mem::size_of;

use anyhow::Context as _;
use sha2::{Digest, Sha256};
use windows::{
    Win32::Graphics::Direct3D11::{ID3D11DeviceContext, ID3D11PixelShader},
    core::{GUID, IUnknown, Interface},
};

use crate::stereo::single_pass::shader_identity::{self, LAKE_PS};

const PACKED_VARIANT: GUID = GUID::from_u128(0x3e04c878_10a7_4e6c_a99f_e708da00cc63);
pub(crate) const BYTECODE_SIZE: usize = 21_568;

pub(crate) fn register(original: &ID3D11PixelShader, code: &[u8]) -> anyhow::Result<()> {
    if code.len() != BYTECODE_SIZE || format!("{:x}", Sha256::digest(code)) != LAKE_PS {
        return Ok(());
    }
    let bytes = dxbc_stereo::scale_lake_refraction(code)?;
    let _identity = unsafe { shader_identity::begin(code.as_ptr(), code.len() as u64) };
    let mut packed = None;
    unsafe {
        original
            .GetDevice()?
            .CreatePixelShader(&bytes, None, Some(&mut packed))
    }?;
    let packed = packed.context("lake refraction: missing packed shader")?;
    unsafe { original.SetPrivateDataInterface(&PACKED_VARIANT, &packed) }?;
    Ok(())
}

pub(super) struct Restore<'a> {
    ctx: &'a ID3D11DeviceContext,
    shader: ID3D11PixelShader,
}

impl Drop for Restore<'_> {
    fn drop(&mut self) {
        unsafe { self.ctx.PSSetShader(&self.shader, None) };
    }
}

pub(super) fn bind(ctx: &ID3D11DeviceContext) -> Option<Restore<'_>> {
    let mut shader = None;
    unsafe { ctx.PSGetShader(&mut shader, None, None) };
    let shader = shader?;
    let mut value: Option<IUnknown> = None;
    let mut size = size_of::<Option<IUnknown>>() as u32;
    unsafe { shader.GetPrivateData(&PACKED_VARIANT, &mut size, Some((&raw mut value).cast())) }
        .ok()?;
    let packed: ID3D11PixelShader = value?.cast().ok()?;
    unsafe { ctx.PSSetShader(&packed, None) };
    Some(Restore { ctx, shader })
}
