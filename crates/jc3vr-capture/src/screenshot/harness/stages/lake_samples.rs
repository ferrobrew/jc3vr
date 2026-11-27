use anyhow::{Context, ensure};
use jc3vr_render::readback::{read_mapped, staging_texture};
use windows::{
    Win32::Graphics::{
        Direct3D11::*,
        Dxgi::Common::{DXGI_FORMAT_R11G11B10_FLOAT, DXGI_FORMAT_R32G32B32A32_FLOAT},
    },
    core::Interface,
};

use crate::screenshot::harness::{
    frame_identity,
    stages::{STATE, Source, Stage, StageImage, StageRecord, stage_image},
};

const STAGES: [Stage; 4] = [
    Stage::LakeSampleUv,
    Stage::LakeSampleDepth,
    Stage::LakeSampleColor,
    Stage::LakeSamplePlanar,
];

pub(crate) struct LakeSampleTargets {
    ctx: ID3D11DeviceContext,
    saved: Vec<Option<ID3D11RenderTargetView>>,
    depth: Option<ID3D11DepthStencilView>,
    desc: D3D11_TEXTURE2D_DESC,
    textures: Vec<ID3D11Texture2D>,
    views: Vec<Option<ID3D11RenderTargetView>>,
    blend: Option<ID3D11BlendState>,
    factor: [f32; 4],
    mask: u32,
    stages: &'static [Stage],
    armed: bool,
}

impl LakeSampleTargets {
    pub(crate) fn bind(
        ctx: &ID3D11DeviceContext,
        response: bool,
        lighting: bool,
        displacement: bool,
        normals: bool,
        waves: bool,
        gradients: bool,
    ) -> anyhow::Result<Self> {
        Self::bind_stages(
            ctx,
            if gradients {
                &[
                    Stage::LakeGradientSample,
                    Stage::LakeGradientUv,
                    Stage::LakeGradientCoarseX,
                    Stage::LakeResponseOutput,
                    Stage::LakeGradientCoarseY,
                    Stage::LakeGradientFineX,
                    Stage::LakeGradientFineY,
                ]
            } else if waves {
                &[
                    Stage::LakeWaveSamples78,
                    Stage::LakeWaveSamples910,
                    Stage::LakeWaveUv78,
                    Stage::LakeResponseOutput,
                    Stage::LakeWaveUv910,
                    Stage::LakeWaveWeights,
                    Stage::LakeWaveCombined,
                ]
            } else if normals {
                &[
                    Stage::LakeResponseColor,
                    Stage::LakeResponseUv,
                    Stage::LakeResponseTint,
                    Stage::LakeResponseOutput,
                    Stage::LakeResponseAir,
                    Stage::LakeNormalMesh,
                    Stage::LakeNormalMixed,
                ]
            } else if displacement {
                &[
                    Stage::LakeResponseColor,
                    Stage::LakeResponseUv,
                    Stage::LakeResponseTint,
                    Stage::LakeResponseOutput,
                    Stage::LakeResponseAir,
                    Stage::LakeDisplacementDepth,
                    Stage::LakeDisplacementSurface,
                ]
            } else if response {
                &[
                    Stage::LakeResponseColor,
                    Stage::LakeResponseUv,
                    Stage::LakeResponseTint,
                    Stage::LakeResponseOutput,
                    Stage::LakeResponseAir,
                ]
            } else if lighting {
                &[
                    Stage::LakeLightingBase,
                    Stage::LakeLightingMixed,
                    Stage::LakeLightingLit,
                    Stage::LakeLightingOutput,
                ]
            } else {
                &STAGES
            },
            true,
            1,
        )
    }

    pub(crate) fn bind_resolve(ctx: &ID3D11DeviceContext, shadow: bool) -> anyhow::Result<Self> {
        Self::bind_stages(
            ctx,
            if shadow {
                &[
                    Stage::ResolveHalfSamples,
                    Stage::ResolveFullSamples,
                    Stage::ResolveWorld,
                    Stage::ResolveView,
                    Stage::ResolveShadowTerms,
                    Stage::ResolveShadowTap,
                ]
            } else {
                &[
                    Stage::ResolveHalfSamples,
                    Stage::ResolveFullSamples,
                    Stage::ResolveWorld,
                    Stage::ResolveView,
                ]
            },
            false,
            1,
        )
    }

    pub(crate) fn bind_ssr(ctx: &ID3D11DeviceContext, history: bool) -> anyhow::Result<Self> {
        Self::bind_stages(
            ctx,
            if history {
                &[
                    Stage::SsrRayInput,
                    Stage::SsrRayNormal,
                    Stage::SsrHistoryUv,
                    Stage::SsrHistorySample,
                    Stage::SsrRayMarch,
                    Stage::SsrRayMask,
                ]
            } else {
                &[
                    Stage::SsrRayInput,
                    Stage::SsrRayNormal,
                    Stage::SsrRaySegment,
                    Stage::SsrRayWorld,
                    Stage::SsrRayMarch,
                    Stage::SsrRayMask,
                ]
            },
            false,
            2,
        )
    }

    fn bind_stages(
        ctx: &ID3D11DeviceContext,
        stages: &'static [Stage],
        require_depth: bool,
        prefix: usize,
    ) -> anyhow::Result<Self> {
        let mut views = [const { None }; 8];
        let mut depth = None;
        let mut uavs = [const { None }; 8];
        unsafe {
            ctx.OMGetRenderTargets(Some(&mut views), Some(&mut depth));
            ctx.OMGetRenderTargetsAndUnorderedAccessViews(None, None, 0, Some(&mut uavs));
        }
        ensure!(
            views[..prefix].iter().all(Option::is_some)
                && views[prefix..].iter().all(Option::is_none)
                && (!require_depth || depth.is_some())
                && uavs.iter().all(Option::is_none),
            "unsupported lake output bindings"
        );
        let mut blend = None;
        let mut factor = [0.; 4];
        let mut mask = 0;
        unsafe { ctx.OMGetBlendState(Some(&mut blend), Some(&mut factor), Some(&mut mask)) };
        let plain = D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: false.into(),
            SrcBlend: D3D11_BLEND_ONE,
            DestBlend: D3D11_BLEND_ZERO,
            BlendOp: D3D11_BLEND_OP_ADD,
            SrcBlendAlpha: D3D11_BLEND_ONE,
            DestBlendAlpha: D3D11_BLEND_ZERO,
            BlendOpAlpha: D3D11_BLEND_OP_ADD,
            RenderTargetWriteMask: 15,
        };
        let mut b = D3D11_BLEND_DESC {
            RenderTarget: [plain; 8],
            ..Default::default()
        };
        if let Some(original) = &blend {
            unsafe { original.GetDesc(&mut b) };
        }
        if !b.IndependentBlendEnable.as_bool() {
            let original = b.RenderTarget[0];
            b.RenderTarget[..prefix].fill(original);
        }
        b.IndependentBlendEnable = true.into();
        b.RenderTarget[prefix..].fill(plain);
        let target = views[0].as_ref().unwrap();
        let tex: ID3D11Texture2D = unsafe { target.GetResource()? }.cast()?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { tex.GetDesc(&mut desc) };
        ensure!(
            desc.Format == DXGI_FORMAT_R11G11B10_FLOAT
                && desc.ArraySize == 1
                && desc.SampleDesc.Count == 1
                && desc.MipLevels == 1
                && u64::from(desc.Width) * u64::from(desc.Height) <= 1024 * 1024,
            "unsupported lake output texture"
        );
        if prefix == 2 {
            let tex: ID3D11Texture2D =
                unsafe { views[1].as_ref().unwrap().GetResource()? }.cast()?;
            let mut alpha = D3D11_TEXTURE2D_DESC::default();
            unsafe { tex.GetDesc(&mut alpha) };
            let (w, h) = jc3vr_view::stereo::render_size().context("SSR render size missing")?;
            ensure!(
                desc.Width * 2 == w
                    && desc.Height * 2 == h
                    && alpha.Width == desc.Width
                    && alpha.Height == desc.Height
                    && alpha.Format
                        == windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R32_FLOAT,
                "SSR target layout mismatch"
            );
        }
        let desc = D3D11_TEXTURE2D_DESC {
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
            ..desc
        };
        let device = unsafe { ctx.GetDevice()? };
        let mut trace_blend = None;
        unsafe { device.CreateBlendState(&b, Some(&mut trace_blend))? };
        let trace_blend = trace_blend.context("missing trace blend state")?;
        let mut trace = Self {
            blend,
            factor,
            mask,
            ctx: ctx.clone(),
            saved: views[..prefix].to_vec(),
            depth,
            desc,
            textures: Vec::new(),
            views: views[..prefix].to_vec(),
            stages,
            armed: false,
        };
        for _ in trace.stages {
            let mut tex = None;
            unsafe { device.CreateTexture2D(&desc, None, Some(&mut tex))? };
            let tex = tex.context("missing trace texture")?;
            let mut view = None;
            unsafe { device.CreateRenderTargetView(&tex, None, Some(&mut view))? };
            let view = view.context("missing trace RTV")?;
            unsafe { ctx.ClearRenderTargetView(&view, &[0.; 4]) };
            trace.textures.push(tex);
            trace.views.push(Some(view));
        }
        unsafe { ctx.OMSetRenderTargets(Some(&trace.views), trace.depth.as_ref()) };
        trace.armed = true;
        unsafe { ctx.OMSetBlendState(&trace_blend, Some(&trace.factor), trace.mask) };
        let mut observed = vec![None; trace.views.len()];
        let mut dsv = None;
        unsafe { ctx.OMGetRenderTargets(Some(&mut observed), Some(&mut dsv)) };
        ensure!(
            observed[..] == trace.views && dsv == trace.depth,
            "trace RTVs not bound"
        );
        Ok(trace)
    }

    fn restore(&mut self) {
        if self.armed {
            unsafe {
                self.ctx
                    .OMSetBlendState(self.blend.as_ref(), Some(&self.factor), self.mask);
                self.ctx
                    .OMSetRenderTargets(Some(&self.saved), self.depth.as_ref())
            };
            self.armed = false;
        }
    }

    pub(crate) fn finish(&mut self, eye: usize, id: &str) -> anyhow::Result<()> {
        self.restore();
        self.verify_restoration()?;
        let frame = frame_identity().context("missing sample frame")?;
        let mut state = STATE.lock();
        ensure!(state.frame == Some(frame), "invalid sample frame");
        let record = state
            .records
            .iter()
            .find(|r| {
                r.stage == Stage::WaterInputRefraction
                    && r.eye == eye
                    && r.frame == frame
                    && r.water_input
                        .as_ref()
                        .is_some_and(|i| i.capture_id == id && i.tess_index == Some(7))
            })
            .cloned()
            .context("missing selected refraction source")?;
        self.write_images(&mut state, record)
    }

    fn verify_restoration(&self) -> anyhow::Result<()> {
        let mut views = [const { None }; 8];
        let mut depth = None;
        unsafe {
            self.ctx
                .OMGetRenderTargets(Some(&mut views), Some(&mut depth))
        };
        ensure!(
            views[..self.saved.len()] == self.saved
                && views[self.saved.len()..].iter().all(Option::is_none)
                && depth == self.depth,
            "lake output bindings not restored"
        );
        let mut observed = None;
        let mut factor = [0.; 4];
        let mut mask = 0;
        unsafe {
            self.ctx
                .OMGetBlendState(Some(&mut observed), Some(&mut factor), Some(&mut mask))
        };
        ensure!(
            observed == self.blend
                && factor.map(f32::to_bits) == self.factor.map(f32::to_bits)
                && mask == self.mask,
            "lake blend state not restored"
        );
        Ok(())
    }

    pub(crate) fn finish_resolve(&mut self, frame: i64, eye: usize) -> anyhow::Result<()> {
        self.restore();
        self.verify_restoration()?;
        let mut state = STATE.lock();
        ensure!(
            frame_identity() == Some(frame) && state.frame == Some(frame),
            "resolve trace: frame changed"
        );
        let mut record = crate::screenshot::harness::stages::empty_stage_record(
            frame,
            eye,
            Stage::ResolveFullSamples,
        );
        crate::screenshot::harness::stages::capture_raster_state(&self.ctx, &mut record);
        self.write_images(&mut state, record)
    }

    fn write_images(
        &self,
        state: &mut crate::screenshot::harness::stages::State,
        record: StageRecord,
    ) -> anyhow::Result<()> {
        let device = unsafe { self.ctx.GetDevice()? };
        let staging = unsafe { staging_texture(&device, &self.desc) }?;
        for (stage, tex) in self.stages.iter().copied().zip(&self.textures) {
            let mut r = StageRecord {
                stage,
                source: Some(Source {
                    width: self.desc.Width,
                    height: self.desc.Height,
                    format: format!("{:?}", self.desc.Format),
                    format_id: self.desc.Format.0,
                    bytes_per_pixel: Some(16),
                }),
                ..record.clone()
            };
            r.water_input = None;
            // SAFETY: the context is locked, and the staging copy is the sample targets' size.
            let image = unsafe {
                self.ctx.CopyResource(&staging, tex);
                read_mapped(&self.ctx, &staging, |m| {
                    stage_image(stage, m.rows(self.desc.Height)?, m.row_pitch(), &self.desc)
                })?
            };
            let ix = state.records.len();
            state.records.push(r);
            state.images.push(StageImage {
                record: ix,
                artifact: image?,
            });
        }
        Ok(())
    }
}
impl Drop for LakeSampleTargets {
    fn drop(&mut self) {
        self.restore();
    }
}
