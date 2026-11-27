//! The mod-owned constant buffers the rewritten shaders read their per-eye rows from.
//!
//! Each rewrite that moves a shader's constants per eye reads them from a dynamic constant buffer
//! the mod owns at a register the game's shaders leave free: `cb13` for the vertex and domain
//! stages, and the ambient-occlusion, ambient-occlusion volume, screen-space reflection, and eye
//! camera buffers for the pixel stage. [`PerEyeCb`] is that buffer for a fixed row count and
//! register: created on the first upload, rewritten with a `WRITE_DISCARD` map, and released at
//! eject so it does not outlive the mod.

use jc3gi::types::math::Vector4;
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BUFFER_DESC, D3D11_CPU_ACCESS_WRITE, D3D11_MAP_WRITE_DISCARD,
    D3D11_MAPPED_SUBRESOURCE, D3D11_USAGE_DYNAMIC, ID3D11Buffer, ID3D11DeviceContext,
};

use crate::stereo::single_pass::EngineContext;

/// A mod-owned dynamic constant buffer of `ROWS` float4 rows, bound at register `REG`.
pub(crate) struct PerEyeCb<const ROWS: usize, const REG: u32> {
    buffer: Option<ID3D11Buffer>,
}

impl<const ROWS: usize, const REG: u32> PerEyeCb<ROWS, REG> {
    pub(crate) const fn new() -> Self {
        Self { buffer: None }
    }

    /// The live buffer, or `None` before the first upload.
    pub(crate) fn buffer(&self) -> Option<&ID3D11Buffer> {
        self.buffer.as_ref()
    }

    /// Drop the buffer, so it does not outlive an eject. The caller unbinds it first.
    pub(crate) fn release(&mut self) {
        self.buffer = None;
    }

    /// Write `rows` into the buffer, creating it on first use, and return it for binding.
    ///
    /// # Safety
    ///
    /// `ctx` is the live immediate context, used on the render thread under the engine's context
    /// mutex.
    pub(crate) unsafe fn upload(
        &mut self,
        ctx: &ID3D11DeviceContext,
        rows: &[Vector4; ROWS],
    ) -> windows::core::Result<&ID3D11Buffer> {
        let buffer = match self.buffer.take() {
            Some(buffer) => buffer,
            None => {
                // SAFETY: the context belongs to the live device, which creates the buffer.
                let device = unsafe { ctx.GetDevice()? };
                let mut created = None;
                // SAFETY: a valid dynamic constant-buffer description with no initial data.
                unsafe {
                    device.CreateBuffer(
                        &D3D11_BUFFER_DESC {
                            ByteWidth: std::mem::size_of_val(rows) as u32,
                            Usage: D3D11_USAGE_DYNAMIC,
                            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                            ..Default::default()
                        },
                        None,
                        Some(&mut created),
                    )?
                };
                created.ok_or_else(windows::core::Error::empty)?
            }
        };
        let buffer = self.buffer.insert(buffer);
        // SAFETY: per the caller's contract; the buffer is dynamic and CPU-writable.
        unsafe { map_rows(ctx, buffer, rows)? };
        Ok(buffer)
    }

    /// Overwrite the live buffer's contents without binding it; a no-op before the first upload.
    /// A `WRITE_DISCARD` map renames a bound dynamic buffer, so the binding stays live.
    ///
    /// # Safety
    ///
    /// As for [`upload`](Self::upload).
    #[inline]
    pub(crate) unsafe fn write(
        &self,
        ctx: &ID3D11DeviceContext,
        rows: &[Vector4; ROWS],
    ) -> windows::core::Result<()> {
        let Some(buffer) = &self.buffer else {
            return Ok(());
        };
        // SAFETY: per the caller's contract; the buffer is dynamic and CPU-writable.
        unsafe { map_rows(ctx, buffer, rows) }
    }

    /// Bind `buffer` at `REG` on the pixel stage.
    ///
    /// # Safety
    ///
    /// As for [`upload`](Self::upload).
    pub(crate) unsafe fn bind_pixel(ctx: &ID3D11DeviceContext, buffer: &ID3D11Buffer) {
        // SAFETY: per the caller's contract.
        unsafe { ctx.PSSetConstantBuffers(REG, Some(&[Some(buffer.clone())])) };
    }

    /// Clear `REG` on the pixel stage.
    ///
    /// # Safety
    ///
    /// As for [`upload`](Self::upload).
    pub(crate) unsafe fn unbind_pixel(ctx: &ID3D11DeviceContext) {
        // SAFETY: per the caller's contract.
        unsafe { ctx.PSSetConstantBuffers(REG, Some(&[None])) };
    }

    /// Upload `rows` into the buffer behind `cell` and bind it on the pixel stage for as long as the
    /// returned guard lives, taking the engine's context mutex before the buffer's lock.
    pub(crate) fn bind_pixel_scoped(
        cell: &Mutex<Self>,
        d3d: EngineContext,
        rows: &[Vector4; ROWS],
    ) -> windows::core::Result<PixelRowsBound<REG>> {
        d3d.with_lock(|ctx| {
            let mut cb = cell.lock();
            // SAFETY: on the render thread under the engine's context mutex, as every other write to
            // the immediate context.
            unsafe {
                let buffer = cb.upload(ctx, rows)?;
                Self::bind_pixel(ctx, buffer);
            }
            Ok::<_, windows::core::Error>(())
        })?;
        Ok(PixelRowsBound(d3d))
    }
}

/// The scope opened by [`PerEyeCb::bind_pixel_scoped`]; clears `REG` on the pixel stage when dropped.
pub(crate) struct PixelRowsBound<const REG: u32>(EngineContext);

impl<const REG: u32> Drop for PixelRowsBound<REG> {
    fn drop(&mut self) {
        // SAFETY: as in `bind_pixel_scoped`.
        self.0
            .with_lock(|ctx| unsafe { ctx.PSSetConstantBuffers(REG, Some(&[None])) });
    }
}

/// Both eyes' blocks of `EYE` rows laid out one after the other, or `None` when `block` has none for
/// an eye or a row is not finite.
pub(crate) fn per_eye_rows<const EYE: usize, const ROWS: usize>(
    mut block: impl FnMut(usize) -> Option<[Vector4; EYE]>,
) -> Option<[Vector4; ROWS]> {
    const { assert!(ROWS == 2 * EYE) };
    let mut rows = [Vector4::default(); ROWS];
    for eye in 0..2 {
        let block = block(eye)?;
        if !finite(&block) {
            return None;
        }
        rows[eye * EYE..(eye + 1) * EYE].copy_from_slice(&block);
    }
    Some(rows)
}

/// Whether every value in `rows` is finite.
pub(crate) fn finite(rows: &[Vector4]) -> bool {
    rows.iter()
        .all(|row| row.data.iter().all(|v| v.is_finite()))
}

/// Write `rows` through a `WRITE_DISCARD` map of `buffer`.
///
/// # Safety
///
/// `ctx` is the live immediate context under the engine's context mutex, and `buffer` is a dynamic,
/// CPU-writable buffer of at least `ROWS` rows.
#[inline]
unsafe fn map_rows<const ROWS: usize>(
    ctx: &ID3D11DeviceContext,
    buffer: &ID3D11Buffer,
    rows: &[Vector4; ROWS],
) -> windows::core::Result<()> {
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    // SAFETY: per the caller's contract; the mapping is `size_of_val(rows)` bytes.
    unsafe {
        ctx.Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))?;
        std::ptr::copy_nonoverlapping(rows.as_ptr(), mapped.pData.cast(), ROWS);
        ctx.Unmap(buffer, 0);
    }
    Ok(())
}
