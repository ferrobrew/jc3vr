//! The pipeline state the mod's own post-draw passes change on the engine's immediate context, saved
//! before a pass binds its own and put back when the pass is done, so the engine's next draw finds
//! the context as it left it.

use windows::Win32::Graphics::{
    Direct3D::D3D_PRIMITIVE_TOPOLOGY,
    Direct3D11::{
        ID3D11BlendState, ID3D11Buffer, ID3D11DepthStencilState, ID3D11DepthStencilView,
        ID3D11DeviceContext, ID3D11InputLayout, ID3D11PixelShader, ID3D11RasterizerState,
        ID3D11RenderTargetView, ID3D11SamplerState, ID3D11ShaderResourceView, ID3D11VertexShader,
    },
};

/// The output-merger, rasterizer, input-assembler, and shader state an overlay or scope lens pass
/// binds, other than the viewport (which each pass binds through the collapse's own path), restored
/// when the guard drops.
///
/// It covers the slots those passes use: vertex shader resource and constant buffer 0, pixel shader
/// resources 0 and 1, pixel constant buffer 0, and pixel sampler 0.
pub(crate) struct DrawStateGuard {
    context: ID3D11DeviceContext,
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
    vs_resources: [Option<ID3D11ShaderResourceView>; 1],
    vs_constants: [Option<ID3D11Buffer>; 1],
    ps_resources: [Option<ID3D11ShaderResourceView>; 2],
    ps_constants: [Option<ID3D11Buffer>; 1],
    ps_samplers: [Option<ID3D11SamplerState>; 1],
}

impl DrawStateGuard {
    /// Save the state the passes change.
    ///
    /// # Safety
    ///
    /// `context` must be the caller-locked immediate context, and stay locked until the guard drops.
    pub(crate) unsafe fn capture(context: &ID3D11DeviceContext) -> Self {
        let mut s = Self {
            context: context.clone(),
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
            vs_resources: Default::default(),
            vs_constants: Default::default(),
            ps_resources: Default::default(),
            ps_constants: Default::default(),
            ps_samplers: Default::default(),
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
            context.VSGetShaderResources(0, Some(&mut s.vs_resources));
            context.VSGetConstantBuffers(0, Some(&mut s.vs_constants));
            context.PSGetShaderResources(0, Some(&mut s.ps_resources));
            context.PSGetConstantBuffers(0, Some(&mut s.ps_constants));
            context.PSGetSamplers(0, Some(&mut s.ps_samplers));
        }
        s
    }
}

impl Drop for DrawStateGuard {
    fn drop(&mut self) {
        let context = &self.context;
        // SAFETY: rebinding objects the context itself returned, on the context still locked per
        // `capture`'s contract.
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
            context.VSSetShaderResources(0, Some(&self.vs_resources));
            context.VSSetConstantBuffers(0, Some(&self.vs_constants));
            context.PSSetShaderResources(0, Some(&self.ps_resources));
            context.PSSetConstantBuffers(0, Some(&self.ps_constants));
            context.PSSetSamplers(0, Some(&self.ps_samplers));
        }
    }
}
