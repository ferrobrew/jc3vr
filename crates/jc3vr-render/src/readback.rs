//! Reading GPU resources back to the CPU through staging copies, for the F12 screenshot and the
//! capture harness.
//!
//! A readback creates a CPU-readable staging twin of the resource ([`staging_texture`],
//! [`staging_buffer`]), copies into it on the immediate context, and maps it for reading
//! ([`read_mapped`]), which hands the mapping to a closure and unmaps it afterwards whatever the
//! closure returns.

use std::marker::PhantomData;

use anyhow::{Context as _, bail};
use windows::{
    Win32::Graphics::Direct3D11::{
        D3D11_BUFFER_DESC, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
        D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, ID3D11Buffer, ID3D11Device, ID3D11DeviceContext,
        ID3D11Resource, ID3D11Texture2D,
    },
    core::Param,
};

/// A CPU-readable staging texture matching `desc`'s size, format, and mip and array layout.
///
/// # Safety
///
/// `device` must be the live device, used on the render thread.
pub unsafe fn staging_texture(
    device: &ID3D11Device,
    desc: &D3D11_TEXTURE2D_DESC,
) -> anyhow::Result<ID3D11Texture2D> {
    let staging_desc = D3D11_TEXTURE2D_DESC {
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
        ..*desc
    };
    let mut staging: Option<ID3D11Texture2D> = None;
    // SAFETY: per the caller's contract -- the live device on the render thread.
    unsafe {
        device
            .CreateTexture2D(&staging_desc, None, Some(&mut staging))
            .context("creating the staging texture")?;
    }
    staging.context("the staging texture was not created")
}

/// A CPU-readable staging buffer of `byte_width` bytes.
///
/// # Safety
///
/// `device` must be the live device, used on the render thread.
pub unsafe fn staging_buffer(
    device: &ID3D11Device,
    byte_width: u32,
) -> anyhow::Result<ID3D11Buffer> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: byte_width,
        Usage: D3D11_USAGE_STAGING,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        ..Default::default()
    };
    let mut staging: Option<ID3D11Buffer> = None;
    // SAFETY: per the caller's contract; the descriptor is a valid staging buffer.
    unsafe {
        device
            .CreateBuffer(&desc, None, Some(&mut staging))
            .context("creating the staging buffer")?;
    }
    staging.context("the staging buffer was not created")
}

/// Map subresource 0 of `staging` for reading, hand the mapping to `read`, and unmap it.
///
/// # Safety
///
/// `context` must be the live immediate context, locked by the caller, and `staging` a CPU-readable
/// staging resource on its device whose copies were issued on that context.
pub unsafe fn read_mapped<P, R>(
    context: &ID3D11DeviceContext,
    staging: P,
    read: impl FnOnce(&Mapped<'_>) -> R,
) -> anyhow::Result<R>
where
    P: Param<ID3D11Resource> + Copy,
{
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    // SAFETY: per the caller's contract.
    unsafe { context.Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))? };
    let result = if mapped.pData.is_null() {
        None
    } else {
        Some(read(&Mapped {
            data: mapped.pData.cast_const().cast(),
            row_pitch: mapped.RowPitch as usize,
            lifetime: PhantomData,
        }))
    };
    // SAFETY: the mapping was made above, and nothing borrowed from it outlives `read`.
    unsafe { context.Unmap(staging, 0) };
    match result {
        Some(result) => Ok(result),
        None => bail!("the mapping has no data"),
    }
}

/// A staging resource's subresource mapped for reading, valid for the closure [`read_mapped`] runs.
pub struct Mapped<'a> {
    data: *const u8,
    row_pitch: usize,
    lifetime: PhantomData<&'a [u8]>,
}

impl Mapped<'_> {
    /// The start of the mapping.
    pub fn data(&self) -> *const u8 {
        self.data
    }

    /// The bytes from one row to the next.
    pub fn row_pitch(&self) -> usize {
        self.row_pitch
    }

    /// The mapping's first `len` bytes.
    ///
    /// # Safety
    ///
    /// The mapped subresource spans at least `len` bytes.
    pub unsafe fn bytes(&self, len: usize) -> &[u8] {
        // SAFETY: per the caller's contract; the mapping is live for `self`'s lifetime.
        unsafe { std::slice::from_raw_parts(self.data, len) }
    }

    /// `height` whole rows, `row_pitch` bytes each.
    ///
    /// # Safety
    ///
    /// The mapped subresource is at least `height` rows tall.
    pub unsafe fn rows(&self, height: u32) -> anyhow::Result<&[u8]> {
        let len = self
            .row_pitch
            .checked_mul(height as usize)
            .filter(|len| *len <= isize::MAX as usize)
            .context("the mapped rows overflow")?;
        // SAFETY: per the caller's contract.
        Ok(unsafe { self.bytes(len) })
    }

    /// The first `len` bytes of row `y`.
    ///
    /// # Safety
    ///
    /// The mapped subresource has more than `y` rows, and `len` is at most the row pitch.
    pub unsafe fn row(&self, y: usize, len: usize) -> &[u8] {
        // SAFETY: per the caller's contract.
        unsafe { std::slice::from_raw_parts(self.data.add(y * self.row_pitch), len) }
    }
}
