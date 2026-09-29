//! The lens's D3D11 pipeline: each eye's image copied before the overlay draws, and the lens disc
//! drawn over the sighting eye's final back buffer by [`scope_lens_vs`](../../../shaders/scope_lens_vs.hlsl)
//! and `scope_lens_ps`, sampling one of the copies.
//!
//! No depth target is bound, and the rasterizer does not clip against the near plane: the lens sits
//! a few centimetres from the eye, nearer than the engine's near plane, and is drawn over everything.
//! The draw saves the pipeline state it changes and restores it afterwards, except the viewport,
//! which every mod draw at this seam sets itself (see the overlay's renderer).

use std::time::Instant;

use anyhow::Context as _;
use jc3gi::graphics_engine::texture::Texture;
use windows::{
    Win32::Graphics::{
        Direct3D::{D3D_PRIMITIVE_TOPOLOGY, D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP},
        Direct3D11::{
            D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_SHADER_RESOURCE, D3D11_BLEND_DESC,
            D3D11_BLEND_INV_SRC_ALPHA, D3D11_BLEND_ONE, D3D11_BLEND_OP_ADD, D3D11_BLEND_SRC_ALPHA,
            D3D11_BOX, D3D11_BUFFER_DESC, D3D11_COLOR_WRITE_ENABLE_ALL, D3D11_COMPARISON_NEVER,
            D3D11_CPU_ACCESS_WRITE, D3D11_CULL_NONE, D3D11_DEPTH_STENCIL_DESC, D3D11_FILL_SOLID,
            D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_MAP_WRITE_DISCARD, D3D11_MAPPED_SUBRESOURCE,
            D3D11_RASTERIZER_DESC, D3D11_RENDER_TARGET_BLEND_DESC, D3D11_SAMPLER_DESC,
            D3D11_TEXTURE_ADDRESS_CLAMP, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
            D3D11_USAGE_DYNAMIC, D3D11_VIEWPORT, ID3D11BlendState, ID3D11Buffer,
            ID3D11DepthStencilState, ID3D11DepthStencilView, ID3D11Device, ID3D11DeviceContext,
            ID3D11InputLayout, ID3D11PixelShader, ID3D11RasterizerState, ID3D11RenderTargetView,
            ID3D11Resource, ID3D11SamplerState, ID3D11ShaderResourceView, ID3D11Texture2D,
            ID3D11VertexShader,
        },
        Dxgi::Common::{DXGI_FORMAT, DXGI_SAMPLE_DESC},
    },
    core::Interface as _,
};

use crate::controllers::weapons::scope::lens::{EyeCamera, LensConstants};

/// One eye's copied image and the camera it was rendered with.
pub struct EyeCopy {
    texture: ID3D11Texture2D,
    view: ID3D11ShaderResourceView,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
    /// When the copy was taken.
    pub taken: Instant,
    pub camera: EyeCamera,
}

/// The part of the target that holds one eye's image: `width` pixels from `left`.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct EyeRegion {
    pub left: u32,
    pub width: u32,
}

/// The lens pipeline and the eyes' copies.
pub struct Renderer {
    vertex_shader: ID3D11VertexShader,
    pixel_shader: ID3D11PixelShader,
    blend: ID3D11BlendState,
    rasterizer: ID3D11RasterizerState,
    depth_stencil: ID3D11DepthStencilState,
    sampler: ID3D11SamplerState,
    constants: ID3D11Buffer,
    copies: [Option<EyeCopy>; 2],
    /// The render-target view of the last target, kept with the resource it views.
    target: Option<(ID3D11Resource, ID3D11RenderTargetView)>,
}

impl Renderer {
    pub fn new(d3d: &ID3D11Device) -> anyhow::Result<Self> {
        // SAFETY: `d3d` is the live engine device; the descriptors below are valid for these calls.
        unsafe {
            let mut vertex_shader = None;
            d3d.CreateVertexShader(VERTEX_DXBC, None, Some(&mut vertex_shader))
                .context("scope lens: creating the vertex shader")?;
            let mut pixel_shader = None;
            d3d.CreatePixelShader(PIXEL_DXBC, None, Some(&mut pixel_shader))
                .context("scope lens: creating the pixel shader")?;

            // Straight alpha, for the antialiased rim.
            let mut blend_desc = D3D11_BLEND_DESC::default();
            blend_desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: true.into(),
                SrcBlend: D3D11_BLEND_SRC_ALPHA,
                DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOp: D3D11_BLEND_OP_ADD,
                SrcBlendAlpha: D3D11_BLEND_ONE,
                DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOpAlpha: D3D11_BLEND_OP_ADD,
                RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
            };
            let mut blend = None;
            d3d.CreateBlendState(&blend_desc, Some(&mut blend))
                .context("scope lens: creating the blend state")?;

            // Depth clipping off (the default's `DepthClipEnable` is false), so the lens nearer than
            // the near plane still draws.
            let mut rasterizer = None;
            d3d.CreateRasterizerState(
                &D3D11_RASTERIZER_DESC {
                    FillMode: D3D11_FILL_SOLID,
                    CullMode: D3D11_CULL_NONE,
                    ..Default::default()
                },
                Some(&mut rasterizer),
            )
            .context("scope lens: creating the rasterizer state")?;

            let mut depth_stencil = None;
            d3d.CreateDepthStencilState(
                &D3D11_DEPTH_STENCIL_DESC::default(),
                Some(&mut depth_stencil),
            )
            .context("scope lens: creating the depth-stencil state")?;

            let mut sampler = None;
            d3d.CreateSamplerState(
                &D3D11_SAMPLER_DESC {
                    Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                    AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                    MipLODBias: 0.0,
                    MaxAnisotropy: 1,
                    ComparisonFunc: D3D11_COMPARISON_NEVER,
                    BorderColor: [0.0; 4],
                    MinLOD: 0.0,
                    MaxLOD: f32::MAX,
                },
                Some(&mut sampler),
            )
            .context("scope lens: creating the sampler")?;

            let mut constants = None;
            d3d.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: size_of::<LensConstants>() as u32,
                    Usage: D3D11_USAGE_DYNAMIC,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                    MiscFlags: 0,
                    StructureByteStride: 0,
                },
                None,
                Some(&mut constants),
            )
            .context("scope lens: creating the constant buffer")?;

            Ok(Self {
                vertex_shader: vertex_shader.context("scope lens: no vertex shader was created")?,
                pixel_shader: pixel_shader.context("scope lens: no pixel shader was created")?,
                blend: blend.context("scope lens: no blend state was created")?,
                rasterizer: rasterizer.context("scope lens: no rasterizer state was created")?,
                depth_stencil: depth_stencil
                    .context("scope lens: no depth-stencil state was created")?,
                sampler: sampler.context("scope lens: no sampler was created")?,
                constants: constants.context("scope lens: no constant buffer was created")?,
                copies: [None, None],
                target: None,
            })
        }
    }

    /// Copy `region` of `target` as `eye`'s image, rendered with `camera`. The caller must hold the
    /// engine context mutex.
    pub fn capture(
        &mut self,
        d3d: &ID3D11Device,
        context: &ID3D11DeviceContext,
        target: &Texture,
        region: EyeRegion,
        eye: usize,
        camera: EyeCamera,
    ) -> anyhow::Result<()> {
        let source: ID3D11Texture2D = target
            .m_Texture
            .cast()
            .context("scope lens: the back buffer is not a 2D texture")?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: `source` is the engine's live back buffer.
        unsafe { source.GetDesc(&mut desc) };
        if region.width == 0 || region.left + region.width > desc.Width || desc.Height == 0 {
            anyhow::bail!(
                "scope lens: the eye region {region:?} does not fit a {}x{} back buffer",
                desc.Width,
                desc.Height
            );
        }
        let slot = &mut self.copies[eye.min(1)];
        let reusable = slot.as_ref().is_some_and(|c| {
            c.width == region.width && c.height == desc.Height && c.format == desc.Format
        });
        if !reusable {
            *slot = Some(EyeCopy::new(d3d, region.width, desc.Height, desc.Format)?);
        }
        let Some(copy) = slot.as_mut() else {
            return Ok(());
        };
        let bounds = D3D11_BOX {
            left: region.left,
            top: 0,
            front: 0,
            right: region.left + region.width,
            bottom: desc.Height,
            back: 1,
        };
        // SAFETY: both textures are live, of one format, and the box lies within the source.
        unsafe {
            context.CopySubresourceRegion(&copy.texture, 0, 0, 0, 0, &source, 0, Some(&bounds));
        }
        copy.taken = Instant::now();
        copy.camera = camera;
        Ok(())
    }

    /// `eye`'s latest copy.
    pub fn copy(&self, eye: usize) -> Option<&EyeCopy> {
        self.copies[eye.min(1)].as_ref()
    }

    /// Draw the lens onto `target` within `viewport`, reading `source`'s copy. The caller must hold
    /// the engine context mutex.
    pub fn draw(
        &mut self,
        d3d: &ID3D11Device,
        context: &ID3D11DeviceContext,
        target: &Texture,
        viewport: D3D11_VIEWPORT,
        constants: &LensConstants,
        source: usize,
    ) -> anyhow::Result<()> {
        let Some(view) = self.copies[source.min(1)].as_ref().map(|c| c.view.clone()) else {
            return Ok(());
        };
        let rtv = target_view(&mut self.target, d3d, target)?;
        // SAFETY: every bound object is ours or the engine's live resources; the context is the
        // caller-locked immediate context, and the saved state is restored before returning.
        unsafe {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            context
                .Map(
                    &self.constants,
                    0,
                    D3D11_MAP_WRITE_DISCARD,
                    0,
                    Some(&mut mapped),
                )
                .context("scope lens: mapping the constant buffer")?;
            std::ptr::write(mapped.pData.cast::<LensConstants>(), *constants);
            context.Unmap(&self.constants, 0);

            let saved = SavedState::capture(context);
            context.OMSetRenderTargets(Some(&[Some(rtv)]), None);
            match crate::stereo::single_pass::collapse_ui_eye_override() {
                // The collapse detours the plain viewport call; bind this eye's half through the
                // original.
                Some(_) => {
                    crate::stereo::single_pass::set_ui_viewport_raw(context.as_raw(), &viewport)
                }
                None => context.RSSetViewports(Some(&[viewport])),
            }
            context.OMSetBlendState(&self.blend, None, 0xffff_ffff);
            context.OMSetDepthStencilState(&self.depth_stencil, 0);
            context.RSSetState(&self.rasterizer);
            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            let buffers = [Some(self.constants.clone())];
            context.VSSetShader(&self.vertex_shader, None);
            context.VSSetConstantBuffers(0, Some(&buffers));
            context.PSSetShader(&self.pixel_shader, None);
            context.PSSetConstantBuffers(0, Some(&buffers));
            context.PSSetShaderResources(0, Some(&[Some(view)]));
            context.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));

            context.Draw(4, 0);

            context.PSSetShaderResources(0, Some(&[None]));
            saved.restore(context);
        }
        Ok(())
    }
}

impl EyeCopy {
    fn new(
        d3d: &ID3D11Device,
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
    ) -> anyhow::Result<EyeCopy> {
        // SAFETY: `d3d` is the live engine device; the descriptor is valid for a sampled texture.
        unsafe {
            let mut texture = None;
            d3d.CreateTexture2D(
                &D3D11_TEXTURE2D_DESC {
                    Width: width,
                    Height: height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: format,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                },
                None,
                Some(&mut texture),
            )
            .with_context(|| {
                format!("scope lens: creating a {width}x{height} copy of format {format:?}")
            })?;
            let texture: ID3D11Texture2D = texture.context("scope lens: no copy was created")?;
            let mut view = None;
            d3d.CreateShaderResourceView(&texture, None, Some(&mut view))
                .context("scope lens: creating the copy's view")?;
            Ok(EyeCopy {
                texture,
                view: view.context("scope lens: no copy view was created")?,
                width,
                height,
                format,
                taken: Instant::now(),
                camera: EyeCamera {
                    position: glam::Vec3::ZERO,
                    offset_view_projection: glam::Mat4::IDENTITY,
                },
            })
        }
    }
}

/// The committed, precompiled lens shaders (entry point `main`).
const VERTEX_DXBC: &[u8] = include_bytes!("../../../shaders/scope_lens_vs.dxbc");
const PIXEL_DXBC: &[u8] = include_bytes!("../../../shaders/scope_lens_ps.dxbc");

/// The render-target view of `target`, created when the target changes.
fn target_view(
    cache: &mut Option<(ID3D11Resource, ID3D11RenderTargetView)>,
    d3d: &ID3D11Device,
    target: &Texture,
) -> anyhow::Result<ID3D11RenderTargetView> {
    if let Some((resource, rtv)) = cache
        && resource.as_raw() == target.m_Texture.as_raw()
    {
        return Ok(rtv.clone());
    }
    let mut rtv = None;
    // SAFETY: `target.m_Texture` is the engine's render-target-capable back buffer.
    unsafe { d3d.CreateRenderTargetView(&target.m_Texture, None, Some(&mut rtv)) }
        .context("scope lens: creating the back buffer's render-target view")?;
    let rtv = rtv.context("scope lens: no render-target view was created")?;
    *cache = Some((target.m_Texture.clone(), rtv.clone()));
    Ok(rtv)
}

/// The pipeline state the draw changes, other than the viewport.
struct SavedState {
    render_targets: [Option<ID3D11RenderTargetView>; 8],
    depth_target: Option<ID3D11DepthStencilView>,
    blend: Option<ID3D11BlendState>,
    blend_factor: [f32; 4],
    sample_mask: u32,
    depth_stencil: Option<ID3D11DepthStencilState>,
    stencil_ref: u32,
    rasterizer: Option<ID3D11RasterizerState>,
    input_layout: Option<ID3D11InputLayout>,
    topology: D3D_PRIMITIVE_TOPOLOGY,
    vertex_shader: Option<ID3D11VertexShader>,
    pixel_shader: Option<ID3D11PixelShader>,
    vs_constants: [Option<ID3D11Buffer>; 1],
    ps_constants: [Option<ID3D11Buffer>; 1],
    ps_resource: [Option<ID3D11ShaderResourceView>; 1],
    ps_sampler: [Option<ID3D11SamplerState>; 1],
}

impl SavedState {
    /// # Safety
    ///
    /// `context` must be the caller-locked immediate context.
    unsafe fn capture(context: &ID3D11DeviceContext) -> Self {
        let mut s = SavedState {
            render_targets: Default::default(),
            depth_target: None,
            blend: None,
            blend_factor: [0.0; 4],
            sample_mask: 0,
            depth_stencil: None,
            stencil_ref: 0,
            rasterizer: None,
            input_layout: None,
            topology: D3D_PRIMITIVE_TOPOLOGY::default(),
            vertex_shader: None,
            pixel_shader: None,
            vs_constants: Default::default(),
            ps_constants: Default::default(),
            ps_resource: Default::default(),
            ps_sampler: Default::default(),
        };
        // SAFETY: every out-pointer is a live local; the getters AddRef what they return, which the
        // `Option` wrappers release.
        unsafe {
            context.OMGetRenderTargets(Some(&mut s.render_targets), Some(&mut s.depth_target));
            context.OMGetBlendState(
                Some(&mut s.blend),
                Some(&mut s.blend_factor),
                Some(&mut s.sample_mask),
            );
            context.OMGetDepthStencilState(Some(&mut s.depth_stencil), Some(&mut s.stencil_ref));
            s.rasterizer = context.RSGetState().ok();
            s.input_layout = context.IAGetInputLayout().ok();
            s.topology = context.IAGetPrimitiveTopology();
            context.VSGetShader(&mut s.vertex_shader, None, None);
            context.PSGetShader(&mut s.pixel_shader, None, None);
            context.VSGetConstantBuffers(0, Some(&mut s.vs_constants));
            context.PSGetConstantBuffers(0, Some(&mut s.ps_constants));
            context.PSGetShaderResources(0, Some(&mut s.ps_resource));
            context.PSGetSamplers(0, Some(&mut s.ps_sampler));
        }
        s
    }

    /// # Safety
    ///
    /// `context` must be the caller-locked immediate context the state was captured from.
    unsafe fn restore(self, context: &ID3D11DeviceContext) {
        // SAFETY: rebinding objects the context itself returned.
        unsafe {
            context.OMSetRenderTargets(Some(&self.render_targets), self.depth_target.as_ref());
            context.OMSetBlendState(
                self.blend.as_ref(),
                Some(&self.blend_factor),
                self.sample_mask,
            );
            context.OMSetDepthStencilState(self.depth_stencil.as_ref(), self.stencil_ref);
            context.RSSetState(self.rasterizer.as_ref());
            context.IASetInputLayout(self.input_layout.as_ref());
            context.IASetPrimitiveTopology(self.topology);
            context.VSSetShader(self.vertex_shader.as_ref(), None);
            context.PSSetShader(self.pixel_shader.as_ref(), None);
            context.VSSetConstantBuffers(0, Some(&self.vs_constants));
            context.PSSetConstantBuffers(0, Some(&self.ps_constants));
            context.PSSetShaderResources(0, Some(&self.ps_resource));
            context.PSSetSamplers(0, Some(&self.ps_sampler));
        }
    }
}
