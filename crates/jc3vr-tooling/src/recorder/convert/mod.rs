//! The recorder's GPU side: the pass that draws both eyes into one planar-YUV frame, and the
//! staging ring the frames are read back through.
//!
//! The pack pass draws a fullscreen triangle (the capture composite's vertex shader) with
//! `i420_pack_ps` into an `R8_UNORM` target of `width x height * 3 / 2`, whose rows are exactly the
//! I420 byte layout of one frame (see the shader for the plane packing). The shader samples each
//! eye's capture into its rectangle of the [`StreamLayout`], so the scaling, the arrangement, and
//! the colour conversion are one pass. That target is copied into one of [`RING_DEPTH`] staging
//! textures each captured frame, and the staging texture written [`RING_DEPTH`] - 1 captures
//! earlier is mapped with `D3D11_MAP_FLAG_DO_NOT_WAIT`: by then its copy has long completed, so the
//! map returns at once with no GPU stall, and on the rare frame it has not, the map refuses and the
//! recorder repeats the previous image rather than waiting.

mod ring;

use anyhow::{Context as _, bail};
use windows::Win32::Graphics::{
    Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
    Direct3D11::{
        D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE,
        D3D11_BUFFER_DESC, D3D11_COMPARISON_NEVER, D3D11_CPU_ACCESS_READ, D3D11_CULL_NONE,
        D3D11_FILL_SOLID, D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_MAP_FLAG_DO_NOT_WAIT,
        D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_RASTERIZER_DESC, D3D11_SAMPLER_DESC,
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

use crate::recorder::{convert::ring::SourceRing, layout::StreamLayout};

/// How many staging textures the readback cycles through. The map targets the oldest, so a frame
/// is read back `RING_DEPTH - 1` captures after it was converted.
pub const RING_DEPTH: usize = 3;

/// What a conversion read back.
pub enum Readback {
    /// The ring is still filling; nothing to read yet.
    Priming,
    /// The oldest copy has not completed, so the map refused.
    Pending,
    /// The buffer holds the frame converted from the source with this take frame ordinal.
    Fresh(u32),
}

/// The pack pipeline and readback ring, built for one layout.
pub(crate) struct Converter {
    vertex_shader: ID3D11VertexShader,
    pixel_shader: ID3D11PixelShader,
    sampler: ID3D11SamplerState,
    rasterizer: ID3D11RasterizerState,
    params: ID3D11Buffer,
    target_rtv: ID3D11RenderTargetView,
    target: ID3D11Texture2D,
    sources: [Source; 2],
    ring: [ID3D11Texture2D; RING_DEPTH],
    frames: SourceRing<u32>,
    layout: StreamLayout,
}

impl Converter {
    /// Build the pipeline against the engine device for `eyes` (the left eye's capture, then the
    /// right's) drawn into `layout`.
    pub fn new(
        device: &ID3D11Device,
        eyes: [&ID3D11Texture2D; 2],
        layout: StreamLayout,
    ) -> anyhow::Result<Self> {
        // SAFETY: the engine device and live capture textures on it; the descriptors below are
        // valid for these calls.
        unsafe {
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

            // The layout never changes for the converter's lifetime, so an immutable constant
            // buffer with it baked in.
            let params_data = PackParams::new(&layout);
            let mut params: Option<ID3D11Buffer> = None;
            device
                .CreateBuffer(
                    &D3D11_BUFFER_DESC {
                        ByteWidth: std::mem::size_of::<PackParams>() as u32,
                        Usage: D3D11_USAGE_IMMUTABLE,
                        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                        ..Default::default()
                    },
                    Some(&D3D11_SUBRESOURCE_DATA {
                        pSysMem: std::ptr::from_ref(&params_data).cast(),
                        SysMemPitch: 0,
                        SysMemSlicePitch: 0,
                    }),
                    Some(&mut params),
                )
                .context("creating the pack parameter buffer")?;
            let params = params.context("the pack parameter buffer was not created")?;

            let packed_desc = D3D11_TEXTURE2D_DESC {
                Width: layout.width,
                Height: layout.height * 3 / 2,
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

            Ok(Self {
                vertex_shader,
                pixel_shader,
                sampler,
                rasterizer,
                params,
                target_rtv,
                target,
                sources: [Source::new(device, eyes[0])?, Source::new(device, eyes[1])?],
                ring,
                frames: SourceRing::new(),
                layout,
            })
        }
    }

    pub fn layout(&self) -> &StreamLayout {
        &self.layout
    }

    /// Draw `eyes` into the ring, tagged with the take frame ordinal `frame`, and read the oldest
    /// slot into `buffer`. Returns that slot's frame ordinal on a fresh readback; priming and
    /// pending readbacks leave `buffer` untouched.
    ///
    /// # Safety
    /// `context` must be the live engine immediate context, held under the engine's context
    /// critical section, and `eyes` live textures on `device` whose GPU writes for this frame are
    /// submitted.
    pub unsafe fn convert(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        eyes: [&ID3D11Texture2D; 2],
        buffer: &mut [u8],
        frame: u32,
    ) -> anyhow::Result<Readback> {
        let frame_len = self.layout.frame_len();
        if buffer.len() < frame_len {
            bail!(
                "the frame buffer is too small: {} bytes for a {frame_len}-byte frame",
                buffer.len()
            );
        }
        // SAFETY: forwarded from the caller's contract; every resource bound below belongs to the
        // engine device.
        unsafe {
            for (source, eye) in self.sources.iter_mut().zip(eyes) {
                source.refresh(device, eye)?;
            }

            let packed_height = self.layout.height * 3 / 2;
            context.OMSetRenderTargets(Some(&[Some(self.target_rtv.clone())]), None);
            context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: self.layout.width as f32,
                Height: packed_height as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            // Opaque and unconditional: the pack shader writes only the red channel, so a blend
            // state another pass left bound weighs it by an undefined alpha and can leave the
            // target unchanged frame after frame.
            context.OMSetBlendState(None, None, u32::MAX);
            context.OMSetDepthStencilState(None, 0);
            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&self.vertex_shader, None);
            context.GSSetShader(None, None);
            context.HSSetShader(None, None);
            context.DSSetShader(None, None);
            context.PSSetShader(&self.pixel_shader, None);
            context.PSSetConstantBuffers(0, Some(&[Some(self.params.clone())]));
            context.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            context.PSSetShaderResources(
                0,
                Some(&[
                    Some(self.sources[0].view.clone()),
                    Some(self.sources[1].view.clone()),
                ]),
            );
            context.RSSetState(&self.rasterizer);
            context.Draw(3, 0);
            context.PSSetShaderResources(0, Some(&[None, None]));
            context.OMSetRenderTargets(Some(&[None]), None);

            context.CopyResource(&self.ring[self.frames.copy_slot()], &self.target);
            let Some((slot, frame)) = self.frames.copied(frame) else {
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
            let rows = packed_height as usize;
            let row_len = self.layout.width as usize;
            let pitch = mapped.RowPitch as usize;
            let base = mapped.pData.cast::<u8>();
            for row in 0..rows {
                let src = std::slice::from_raw_parts(base.add(row * pitch), row_len);
                buffer[row * row_len..(row + 1) * row_len].copy_from_slice(src);
            }
            context.Unmap(staging, 0);
            Ok(Readback::Fresh(frame))
        }
    }
}

/// The committed, precompiled pack shaders (entry point `main`). The vertex shader is the capture
/// composite's fullscreen triangle.
const VERTEX_DXBC: &[u8] = jc3vr_view::CAPTURE_VS;
const PIXEL_DXBC: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/i420_pack_ps.dxbc"
));

/// The pack shader's constant buffer, laid out as `PackParams` in `i420_pack_ps.hlsl`.
#[repr(C)]
struct PackParams {
    size: [u32; 2],
    padding: [u32; 2],
    eye_rects: [[f32; 4]; 2],
}

impl PackParams {
    fn new(layout: &StreamLayout) -> Self {
        Self {
            size: [layout.width, layout.height],
            padding: [0; 2],
            eye_rects: layout.eyes.map(|rect| {
                [
                    rect.x as f32,
                    rect.y as f32,
                    rect.width as f32,
                    rect.height as f32,
                ]
            }),
        }
    }
}

/// One eye's capture and the view the pack pass samples it through.
struct Source {
    /// The capture texture the view was made for, to detect a rebuilt capture.
    texture: ID3D11Texture2D,
    view: ID3D11ShaderResourceView,
}

impl Source {
    fn new(device: &ID3D11Device, texture: &ID3D11Texture2D) -> anyhow::Result<Self> {
        Ok(Self {
            texture: texture.clone(),
            view: source_view(device, texture)?,
        })
    }

    /// Follow a capture texture rebuilt across a resize; a stale view would sample the old one.
    fn refresh(&mut self, device: &ID3D11Device, texture: &ID3D11Texture2D) -> anyhow::Result<()> {
        if self.texture != *texture {
            *self = Self::new(device, texture)?;
        }
        Ok(())
    }
}

/// A shader-resource view over a capture texture.
fn source_view(
    device: &ID3D11Device,
    source: &ID3D11Texture2D,
) -> anyhow::Result<ID3D11ShaderResourceView> {
    let mut srv: Option<ID3D11ShaderResourceView> = None;
    // SAFETY: a live capture texture created with shader-resource binding on this device.
    unsafe {
        device
            .CreateShaderResourceView(source, None, Some(&mut srv))
            .context("creating the source view")?;
    }
    srv.context("the source view was not created")
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::recorder::layout::Arrangement;

    #[test]
    fn the_pack_parameters_match_the_shaders_constant_buffer() {
        // Two 16-byte registers of header, then one register per eye rectangle.
        assert_eq!(std::mem::size_of::<PackParams>(), 48);
        let layout =
            StreamLayout::new(2016, 2240, 1920, Arrangement::SideBySide).expect("a layout");
        let params = PackParams::new(&layout);
        assert_eq!(params.size, [3840, 2134]);
        assert_eq!(
            params.eye_rects,
            [[0.0, 0.0, 1920.0, 2134.0], [1920.0, 0.0, 1920.0, 2134.0]]
        );
    }
}
