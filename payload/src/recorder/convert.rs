//! The recorder's GPU side: the planar-YUV pack pass and the staging ring the frames are read
//! back through.
//!
//! The pack pass draws a fullscreen triangle (the capture composite's vertex shader) with
//! `i420_pack_ps` into an `R8_UNORM` target of `width x height * 3 / 2`, whose rows are exactly
//! the I420 byte layout of one frame (see the shader for the plane packing). That target is copied
//! into one of [`RING_DEPTH`] staging textures each frame, and the staging texture written
//! [`RING_DEPTH`] - 1 frames earlier is mapped with `D3D11_MAP_FLAG_DO_NOT_WAIT`: by then its copy
//! has long completed, so the map returns at once with no GPU stall, and on the rare frame it has
//! not, the map refuses and the recorder repeats the previous frame rather than waiting.

mod ring;

use anyhow::{Context as _, bail};
use windows::Win32::Graphics::{
    Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
    Direct3D11::{
        D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_BUFFER_DESC,
        D3D11_COMPARISON_NEVER, D3D11_CPU_ACCESS_READ, D3D11_CULL_NONE, D3D11_FILL_SOLID,
        D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_MAP_FLAG_DO_NOT_WAIT, D3D11_MAP_READ,
        D3D11_MAPPED_SUBRESOURCE, D3D11_RASTERIZER_DESC, D3D11_SAMPLER_DESC,
        D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE_ADDRESS_CLAMP, D3D11_TEXTURE2D_DESC,
        D3D11_USAGE_DEFAULT, D3D11_USAGE_IMMUTABLE, D3D11_USAGE_STAGING, D3D11_VIEWPORT,
        ID3D11Buffer, ID3D11Device, ID3D11DeviceContext, ID3D11PixelShader, ID3D11RasterizerState,
        ID3D11RenderTargetView, ID3D11SamplerState, ID3D11ShaderResourceView, ID3D11Texture2D,
        ID3D11VertexShader,
    },
    Dxgi::{
        Common::{DXGI_FORMAT_R8_UNORM, DXGI_SAMPLE_DESC},
        DXGI_ERROR_WAS_STILL_DRAWING,
    },
};

use crate::recorder::{convert::ring::SourceRing, stream::FrameMetadata};

/// The committed, precompiled pack shaders (entry point `main`). The vertex shader is the capture
/// composite's fullscreen triangle.
const VERTEX_DXBC: &[u8] = include_bytes!("../shaders/capture_vs.dxbc");
const PIXEL_DXBC: &[u8] = include_bytes!("../shaders/i420_pack_ps.dxbc");

/// How many staging textures the readback cycles through. The map targets the oldest, so a frame
/// is read back `RING_DEPTH - 1` frames after it was converted.
pub const RING_DEPTH: usize = 3;

/// Whether this conversion supplied pixels and their matching source record.
pub enum Readback {
    Priming,
    Pending,
    Fresh(FrameMetadata),
}

/// The pack pipeline and readback ring, built for one source size and output scale.
pub struct Converter {
    vertex_shader: ID3D11VertexShader,
    pixel_shader: ID3D11PixelShader,
    sampler: ID3D11SamplerState,
    rasterizer: ID3D11RasterizerState,
    params: ID3D11Buffer,
    target_rtv: ID3D11RenderTargetView,
    target: ID3D11Texture2D,
    source_srv: ID3D11ShaderResourceView,
    /// The source texture the SRV was made for, to detect a rebuilt capture texture.
    source: ID3D11Texture2D,
    ring: [ID3D11Texture2D; RING_DEPTH],
    sources: SourceRing<FrameMetadata>,
    /// The luma plane size.
    width: u32,
    height: u32,
}

impl Converter {
    /// Build the pipeline against the engine device for `source` at `scale`.
    pub fn new(
        device: &ID3D11Device,
        source: &ID3D11Texture2D,
        scale: f32,
    ) -> anyhow::Result<Self> {
        // SAFETY: the engine device and a live capture texture on it; the descriptors below are
        // valid for these calls.
        unsafe {
            let mut source_desc = D3D11_TEXTURE2D_DESC::default();
            source.GetDesc(&mut source_desc);
            let (width, height) = output_size(source_desc.Width, source_desc.Height, scale);
            if width < 2 || height < 2 {
                bail!(
                    "the source is too small to record: {}x{} at scale {scale}",
                    source_desc.Width,
                    source_desc.Height
                );
            }

            let mut vertex_shader: Option<ID3D11VertexShader> = None;
            device
                .CreateVertexShader(VERTEX_DXBC, None, Some(&mut vertex_shader))
                .context("creating the pack vertex shader")?;
            let vertex_shader = vertex_shader.context("the pack vertex shader was not created")?;

            let mut pixel_shader: Option<ID3D11PixelShader> = None;
            device
                .CreatePixelShader(PIXEL_DXBC, None, Some(&mut pixel_shader))
                .context("creating the pack pixel shader")?;
            let pixel_shader = pixel_shader.context("the pack pixel shader was not created")?;

            let mut sampler: Option<ID3D11SamplerState> = None;
            device
                .CreateSamplerState(
                    &D3D11_SAMPLER_DESC {
                        Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                        AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                        AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                        AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                        MinLOD: f32::MIN,
                        MaxLOD: f32::MAX,
                        MipLODBias: 0.0,
                        MaxAnisotropy: 0,
                        ComparisonFunc: D3D11_COMPARISON_NEVER,
                        BorderColor: [0.0; 4],
                    },
                    Some(&mut sampler),
                )
                .context("creating the pack sampler")?;
            let sampler = sampler.context("the pack sampler was not created")?;

            let mut rasterizer: Option<ID3D11RasterizerState> = None;
            device
                .CreateRasterizerState(
                    &D3D11_RASTERIZER_DESC {
                        FillMode: D3D11_FILL_SOLID,
                        CullMode: D3D11_CULL_NONE,
                        ..Default::default()
                    },
                    Some(&mut rasterizer),
                )
                .context("creating the pack rasterizer state")?;
            let rasterizer = rasterizer.context("the pack rasterizer state was not created")?;

            // The pack parameters never change for the converter's lifetime, so an immutable
            // constant buffer with the size baked in.
            let params_data: [u32; 4] = [width, height, 0, 0];
            let mut params: Option<ID3D11Buffer> = None;
            device
                .CreateBuffer(
                    &D3D11_BUFFER_DESC {
                        ByteWidth: std::mem::size_of_val(&params_data) as u32,
                        Usage: D3D11_USAGE_IMMUTABLE,
                        BindFlags: windows::Win32::Graphics::Direct3D11::D3D11_BIND_CONSTANT_BUFFER
                            .0 as u32,
                        ..Default::default()
                    },
                    Some(&D3D11_SUBRESOURCE_DATA {
                        pSysMem: params_data.as_ptr().cast(),
                        SysMemPitch: 0,
                        SysMemSlicePitch: 0,
                    }),
                    Some(&mut params),
                )
                .context("creating the pack parameter buffer")?;
            let params = params.context("the pack parameter buffer was not created")?;

            let packed_desc = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height * 3 / 2,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_R8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };
            let mut target: Option<ID3D11Texture2D> = None;
            device
                .CreateTexture2D(&packed_desc, None, Some(&mut target))
                .context("creating the packed target")?;
            let target = target.context("the packed target was not created")?;
            let mut target_rtv: Option<ID3D11RenderTargetView> = None;
            device
                .CreateRenderTargetView(&target, None, Some(&mut target_rtv))
                .context("creating the packed target view")?;
            let target_rtv = target_rtv.context("the packed target view was not created")?;

            let staging_desc = D3D11_TEXTURE2D_DESC {
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                ..packed_desc
            };
            let mut ring: Vec<ID3D11Texture2D> = Vec::with_capacity(RING_DEPTH);
            for _ in 0..RING_DEPTH {
                let mut staging: Option<ID3D11Texture2D> = None;
                device
                    .CreateTexture2D(&staging_desc, None, Some(&mut staging))
                    .context("creating a readback staging texture")?;
                ring.push(staging.context("a readback staging texture was not created")?);
            }
            let Ok(ring) = <[ID3D11Texture2D; RING_DEPTH]>::try_from(ring) else {
                bail!("the staging ring is the wrong size");
            };

            let source_srv = source_view(device, source)?;

            Ok(Self {
                vertex_shader,
                pixel_shader,
                sampler,
                rasterizer,
                params,
                target_rtv,
                target,
                source_srv,
                source: source.clone(),
                ring,
                sources: SourceRing::new(),
                width,
                height,
            })
        }
    }

    /// The luma plane size of the stream.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The byte length of one packed frame.
    pub fn frame_len(&self) -> usize {
        self.width as usize * (self.height as usize * 3 / 2)
    }

    /// Convert `source` into the ring with its metadata and read the oldest slot into `buffer`.
    /// Returns that slot's source metadata on a fresh readback. Priming and pending readbacks
    /// leave `buffer` untouched. The caller must hold the engine context mutex.
    ///
    /// # Safety
    /// `context` must be the live engine immediate context, and `source` a live texture on the
    /// same device whose GPU writes for this frame are complete.
    pub unsafe fn convert(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        source: &ID3D11Texture2D,
        buffer: &mut [u8],
        metadata: FrameMetadata,
    ) -> anyhow::Result<Readback> {
        unsafe {
            // The capture texture is rebuilt across a resize; a stale SRV would sample the old one.
            if self.source != *source {
                self.source_srv = source_view(device, source)?;
                self.source = source.clone();
            }

            context.OMSetRenderTargets(Some(&[Some(self.target_rtv.clone())]), None);
            context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: self.width as f32,
                Height: (self.height * 3 / 2) as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&self.vertex_shader, None);
            context.PSSetShader(&self.pixel_shader, None);
            context.PSSetConstantBuffers(0, Some(&[Some(self.params.clone())]));
            context.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            context.PSSetShaderResources(0, Some(&[Some(self.source_srv.clone())]));
            context.RSSetState(&self.rasterizer);
            context.Draw(3, 0);
            context.PSSetShaderResources(0, Some(&[None]));
            context.OMSetRenderTargets(Some(&[None]), None);

            context.CopyResource(&self.ring[self.sources.copy_slot()], &self.target);
            let Some((slot, metadata)) = self.sources.copied(metadata) else {
                return Ok(Readback::Priming);
            };

            let staging = &self.ring[slot];
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            match context.Map(
                staging,
                0,
                D3D11_MAP_READ,
                D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32,
                Some(&mut mapped),
            ) {
                Ok(()) => {}
                Err(e) if e.code() == DXGI_ERROR_WAS_STILL_DRAWING => return Ok(Readback::Pending),
                Err(e) => return Err(e).context("mapping the readback staging texture"),
            }
            let rows = self.height as usize * 3 / 2;
            let row_len = self.width as usize;
            let pitch = mapped.RowPitch as usize;
            if buffer.len() < rows * row_len {
                context.Unmap(staging, 0);
                bail!(
                    "the frame buffer is too small: {} bytes for {} rows of {}",
                    buffer.len(),
                    rows,
                    row_len
                );
            }
            let base = mapped.pData.cast::<u8>();
            for row in 0..rows {
                let src = std::slice::from_raw_parts(base.add(row * pitch), row_len);
                buffer[row * row_len..(row + 1) * row_len].copy_from_slice(src);
            }
            context.Unmap(staging, 0);
            Ok(Readback::Fresh(metadata))
        }
    }
}

/// The stream's luma size for a source size and scale: scaled, then rounded down to even, since
/// 4:2:0 chroma needs even dimensions.
pub fn output_size(width: u32, height: u32, scale: f32) -> (u32, u32) {
    let scale = if scale.is_finite() {
        scale.clamp(0.0, 1.0)
    } else {
        1.0
    };
    let even = |v: u32| v & !1;
    (
        even((width as f32 * scale).round() as u32),
        even((height as f32 * scale).round() as u32),
    )
}

/// A shader-resource view over the capture texture.
unsafe fn source_view(
    device: &ID3D11Device,
    source: &ID3D11Texture2D,
) -> anyhow::Result<ID3D11ShaderResourceView> {
    let mut srv: Option<ID3D11ShaderResourceView> = None;
    // SAFETY: the caller passes a live capture texture created with shader-resource binding.
    unsafe {
        device
            .CreateShaderResourceView(source, None, Some(&mut srv))
            .context("creating the source view")?;
    }
    srv.context("the source view was not created")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_size_scales_and_rounds_to_even() {
        assert_eq!(output_size(2468, 2740, 1.0), (2468, 2740));
        assert_eq!(output_size(2468, 2740, 0.5), (1234, 1370));
        assert_eq!(output_size(2467, 2741, 1.0), (2466, 2740));
        assert_eq!(output_size(1000, 1000, 0.333), (332, 332));
        assert_eq!(output_size(1000, 1000, f32::NAN), (1000, 1000));
    }
}
