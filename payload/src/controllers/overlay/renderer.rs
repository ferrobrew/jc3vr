//! The overlay's D3D11 pipeline: one instanced draw per eye over a structured buffer of
//! primitives ([`overlay_vs`](../../shaders/overlay_vs.hlsl) and `overlay_ps`), alpha-blended onto
//! the eye's final back buffer, with the scene depth read in the pixel shader.
//!
//! No depth target is bound: the pixel shader tests against the main depth texture by hand, with a
//! tolerance in metres, so the engine's depth-stencil view (and the stencil bits it carries) is
//! never touched and the render and display sizes may differ.
//!
//! The draw saves the pipeline state it changes and restores it afterwards, except the viewport:
//! under the single-pass collapse the plain viewport call is detoured to widen a viewport to the
//! whole target, so restoring through it would not reproduce the saved state, and every mod draw
//! at this seam sets its own viewport anyway.

use anyhow::Context as _;
use jc3gi::graphics_engine::texture::Texture;
use windows::{
    Win32::Graphics::{
        Direct3D::{D3D_PRIMITIVE_TOPOLOGY, D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP},
        Direct3D11::{
            D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_SHADER_RESOURCE, D3D11_BLEND_DESC,
            D3D11_BLEND_INV_SRC_ALPHA, D3D11_BLEND_ONE, D3D11_BLEND_OP_ADD, D3D11_BLEND_SRC_ALPHA,
            D3D11_BUFFER_DESC, D3D11_COLOR_WRITE_ENABLE_ALL, D3D11_CPU_ACCESS_WRITE,
            D3D11_CULL_NONE, D3D11_DEPTH_STENCIL_DESC, D3D11_FILL_SOLID, D3D11_MAP_WRITE_DISCARD,
            D3D11_MAPPED_SUBRESOURCE, D3D11_RASTERIZER_DESC, D3D11_RENDER_TARGET_BLEND_DESC,
            D3D11_RESOURCE_MISC_BUFFER_STRUCTURED, D3D11_USAGE_DYNAMIC, D3D11_VIEWPORT,
            ID3D11BlendState, ID3D11Buffer, ID3D11DepthStencilState, ID3D11DepthStencilView,
            ID3D11Device, ID3D11DeviceContext, ID3D11InputLayout, ID3D11PixelShader,
            ID3D11RasterizerState, ID3D11RenderTargetView, ID3D11Resource,
            ID3D11ShaderResourceView, ID3D11VertexShader,
        },
    },
    core::Interface as _,
};

use crate::controllers::overlay::resolve::GpuPrimitive;

/// The per-eye constants, matching the HLSL `cbuffer Eye` of both overlay shaders.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct EyeConstants {
    pub offset_view_projection: [f32; 16],
    /// `xyz`: the frame's origin relative to the eye.
    pub origin_to_eye: [f32; 4],
    /// Viewport width and height, pixels per metre at unit distance, and the minimum line width in
    /// pixels.
    pub viewport_size: [f32; 2],
    pub pixel_scale: f32,
    pub min_line_pixels: f32,
    /// Depth texels per target pixel, and the projection's z row.
    pub depth_scale: [f32; 2],
    pub depth_z_row: [f32; 2],
    /// The last depth texel, and the absolute and relative depth tolerances.
    pub depth_max: [f32; 2],
    pub depth_tolerance_abs: f32,
    pub depth_tolerance_rel: f32,
}

/// What one eye's draw needs besides the renderer.
pub struct EyeDraw<'a> {
    pub target: &'a Texture,
    pub scene_depth: &'a ID3D11ShaderResourceView,
    pub viewport: D3D11_VIEWPORT,
    pub constants: EyeConstants,
}

/// The overlay pipeline and its buffers.
pub struct Renderer {
    vertex_shader: ID3D11VertexShader,
    pixel_shader: ID3D11PixelShader,
    blend: ID3D11BlendState,
    rasterizer: ID3D11RasterizerState,
    depth_stencil: ID3D11DepthStencilState,
    constants: ID3D11Buffer,
    primitives: Option<PrimitiveBuffer>,
    /// The primitives uploaded by the last [`upload`](Self::upload).
    count: u32,
    /// The render-target view of the last target, kept with the resource it views.
    target: Option<(ID3D11Resource, ID3D11RenderTargetView)>,
}

impl Renderer {
    pub fn new(d3d: &ID3D11Device) -> anyhow::Result<Self> {
        // SAFETY: `d3d` is the live engine device; the descriptors below are valid for these calls.
        unsafe {
            let mut vertex_shader = None;
            d3d.CreateVertexShader(VERTEX_DXBC, None, Some(&mut vertex_shader))
                .context("overlay: creating the vertex shader")?;
            let mut pixel_shader = None;
            d3d.CreatePixelShader(PIXEL_DXBC, None, Some(&mut pixel_shader))
                .context("overlay: creating the pixel shader")?;

            // Straight alpha, as the HUD panel's.
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
                .context("overlay: creating the blend state")?;

            // Depth clipping off: nothing is depth-tested in hardware, and a line's near end is
            // clipped by the vertex shader.
            let mut rasterizer = None;
            d3d.CreateRasterizerState(
                &D3D11_RASTERIZER_DESC {
                    FillMode: D3D11_FILL_SOLID,
                    CullMode: D3D11_CULL_NONE,
                    ..Default::default()
                },
                Some(&mut rasterizer),
            )
            .context("overlay: creating the rasterizer state")?;

            let mut depth_stencil = None;
            d3d.CreateDepthStencilState(
                &D3D11_DEPTH_STENCIL_DESC::default(),
                Some(&mut depth_stencil),
            )
            .context("overlay: creating the depth-stencil state")?;

            let mut constants = None;
            d3d.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: size_of::<EyeConstants>() as u32,
                    Usage: D3D11_USAGE_DYNAMIC,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                    MiscFlags: 0,
                    StructureByteStride: 0,
                },
                None,
                Some(&mut constants),
            )
            .context("overlay: creating the constant buffer")?;

            Ok(Self {
                vertex_shader: vertex_shader.context("overlay: no vertex shader was created")?,
                pixel_shader: pixel_shader.context("overlay: no pixel shader was created")?,
                blend: blend.context("overlay: no blend state was created")?,
                rasterizer: rasterizer.context("overlay: no rasterizer state was created")?,
                depth_stencil: depth_stencil
                    .context("overlay: no depth-stencil state was created")?,
                constants: constants.context("overlay: no constant buffer was created")?,
                primitives: None,
                count: 0,
                target: None,
            })
        }
    }

    /// The primitives uploaded by the last [`upload`](Self::upload).
    pub fn count(&self) -> u32 {
        self.count
    }

    /// Upload the frame's primitives, growing the buffer when they do not fit. The caller must
    /// hold the engine context mutex.
    pub fn upload(
        &mut self,
        d3d: &ID3D11Device,
        context: &ID3D11DeviceContext,
        primitives: &[GpuPrimitive],
    ) -> anyhow::Result<()> {
        self.count = 0;
        if primitives.is_empty() {
            return Ok(());
        }
        if self
            .primitives
            .as_ref()
            .is_none_or(|b| b.capacity < primitives.len())
        {
            let capacity = primitives.len().next_power_of_two().max(MIN_CAPACITY);
            self.primitives = Some(PrimitiveBuffer::new(d3d, capacity)?);
        }
        let Some(buffer) = &self.primitives else {
            return Ok(());
        };
        // SAFETY: the buffer is ours, dynamic, and CPU-writable, and holds at least
        // `primitives.len()` elements; the context is the caller-locked immediate context.
        unsafe {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            context
                .Map(
                    &buffer.buffer,
                    0,
                    D3D11_MAP_WRITE_DISCARD,
                    0,
                    Some(&mut mapped),
                )
                .context("overlay: mapping the primitive buffer")?;
            std::ptr::copy_nonoverlapping(
                primitives.as_ptr(),
                mapped.pData.cast::<GpuPrimitive>(),
                primitives.len(),
            );
            context.Unmap(&buffer.buffer, 0);
        }
        self.count = primitives.len() as u32;
        Ok(())
    }

    /// Draw the uploaded primitives for one eye. The caller must hold the engine context mutex.
    pub fn draw(
        &mut self,
        d3d: &ID3D11Device,
        context: &ID3D11DeviceContext,
        eye: &EyeDraw<'_>,
    ) -> anyhow::Result<()> {
        let Some(buffer) = self.primitives.as_ref().filter(|_| self.count > 0) else {
            return Ok(());
        };
        let rtv = target_view(&mut self.target, d3d, eye.target)?;
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
                .context("overlay: mapping the constant buffer")?;
            std::ptr::write(mapped.pData.cast::<EyeConstants>(), eye.constants);
            context.Unmap(&self.constants, 0);

            let saved = SavedState::capture(context);

            // The render target first, so the depth texture is no longer bound as a depth target
            // when it is bound as a shader resource.
            context.OMSetRenderTargets(Some(&[Some(rtv)]), None);
            match crate::stereo::single_pass::collapse_ui_eye_override() {
                // The collapse detours the plain viewport call; bind this eye's half through the
                // original.
                Some(_) => {
                    crate::stereo::single_pass::set_ui_viewport_raw(context.as_raw(), &eye.viewport)
                }
                None => context.RSSetViewports(Some(&[eye.viewport])),
            }
            context.OMSetBlendState(&self.blend, None, 0xffff_ffff);
            context.OMSetDepthStencilState(&self.depth_stencil, 0);
            context.RSSetState(&self.rasterizer);
            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            let constants = [Some(self.constants.clone())];
            context.VSSetShader(&self.vertex_shader, None);
            context.VSSetShaderResources(0, Some(&[Some(buffer.view.clone())]));
            context.VSSetConstantBuffers(0, Some(&constants));
            context.PSSetShader(&self.pixel_shader, None);
            context.PSSetShaderResources(1, Some(&[Some(eye.scene_depth.clone())]));
            context.PSSetConstantBuffers(0, Some(&constants));

            context.DrawInstanced(4, self.count, 0, 0);

            context.VSSetShaderResources(0, Some(&[None]));
            context.PSSetShaderResources(1, Some(&[None]));
            saved.restore(context);
        }
        Ok(())
    }
}

/// The committed, precompiled overlay shaders (entry point `main`).
const VERTEX_DXBC: &[u8] = include_bytes!("../../shaders/overlay_vs.dxbc");
const PIXEL_DXBC: &[u8] = include_bytes!("../../shaders/overlay_ps.dxbc");

/// The smallest primitive buffer, in primitives.
const MIN_CAPACITY: usize = 64;

/// A dynamic structured buffer of primitives and its view.
struct PrimitiveBuffer {
    buffer: ID3D11Buffer,
    view: ID3D11ShaderResourceView,
    capacity: usize,
}

impl PrimitiveBuffer {
    fn new(d3d: &ID3D11Device, capacity: usize) -> anyhow::Result<Self> {
        let stride = size_of::<GpuPrimitive>();
        // SAFETY: `d3d` is the live engine device; the descriptor is valid for a structured buffer.
        unsafe {
            let mut buffer = None;
            d3d.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: (capacity * stride) as u32,
                    Usage: D3D11_USAGE_DYNAMIC,
                    BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                    CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                    MiscFlags: D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32,
                    StructureByteStride: stride as u32,
                },
                None,
                Some(&mut buffer),
            )
            .with_context(|| format!("overlay: creating a buffer for {capacity} primitives"))?;
            let buffer = buffer.context("overlay: no primitive buffer was created")?;
            let mut view = None;
            d3d.CreateShaderResourceView(&buffer, None, Some(&mut view))
                .context("overlay: creating the primitive buffer's view")?;
            Ok(Self {
                buffer,
                view: view.context("overlay: no primitive buffer view was created")?,
                capacity,
            })
        }
    }
}

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
        .context("overlay: creating the back buffer's render-target view")?;
    let rtv = rtv.context("overlay: no render-target view was created")?;
    *cache = Some((target.m_Texture.clone(), rtv.clone()));
    Ok(rtv)
}

/// The pipeline state the draw changes, other than the viewport (see the module docs).
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
    vs_resource: [Option<ID3D11ShaderResourceView>; 1],
    vs_constants: [Option<ID3D11Buffer>; 1],
    ps_resource: [Option<ID3D11ShaderResourceView>; 1],
    ps_constants: [Option<ID3D11Buffer>; 1],
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
            vs_resource: Default::default(),
            vs_constants: Default::default(),
            ps_resource: Default::default(),
            ps_constants: Default::default(),
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
            context.VSGetShaderResources(0, Some(&mut s.vs_resource));
            context.VSGetConstantBuffers(0, Some(&mut s.vs_constants));
            context.PSGetShaderResources(1, Some(&mut s.ps_resource));
            context.PSGetConstantBuffers(0, Some(&mut s.ps_constants));
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
            context.VSSetShaderResources(0, Some(&self.vs_resource));
            context.VSSetConstantBuffers(0, Some(&self.vs_constants));
            context.PSSetShaderResources(1, Some(&self.ps_resource));
            context.PSSetConstantBuffers(0, Some(&self.ps_constants));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_constants_fill_whole_registers() {
        assert_eq!(size_of::<EyeConstants>() % 16, 0);
        assert_eq!(size_of::<EyeConstants>(), 128);
    }

    #[test]
    fn a_primitive_is_six_registers() {
        assert_eq!(size_of::<GpuPrimitive>(), 96);
    }
}
