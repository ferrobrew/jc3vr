use std::{cell::Cell, ffi::c_void};

use anyhow::{Context, Result, ensure};
use jc3gi::graphics_engine::{
    graphics_engine::GraphicsEngine,
    post_effects::{PostEffectContext, PostEffectsManager},
    texture::Texture,
};
use windows::{
    Win32::Graphics::{Direct3D::D3D_SRV_DIMENSION_TEXTURE2D, Direct3D11::*},
    core::Interface,
};

use crate::{
    screenshot::harness::{
        self,
        bound_buffer::{ShaderStage, read_bound_constant_buffer},
        stages::{
            STATE, Stage, StageArtifact, StageImage, StageRecord, capture, capture_raster_state,
            empty_stage_record, read_texture,
        },
    },
    stereo::{
        draw_index,
        single_pass::{shader_identity, vertex_identity_lookup},
    },
};

#[derive(Clone, Copy)]
pub(crate) struct Capture {
    frame: i64,
    eye: usize,
}

thread_local! {
    static ACTIVE: Cell<Option<Capture>> = const { Cell::new(None) };
    static TARGET: Cell<Option<usize>> = const { Cell::new(None) };
}

pub(crate) fn begin(pec: *const PostEffectContext, input: u32) -> Option<Capture> {
    if !harness::wants_eyes() {
        return None;
    }
    let frame = harness::frame_identity().filter(|&f| harness::dof_inputs_requested(f))?;
    let eye = draw_index();
    let id = harness::diagnostic_capture_id()?;
    let p = unsafe { pec.as_ref() }?;
    tracing::info!(target:"dof_inputs", %id, frame, eye, input, exposure=p.m_Exposure, technique=?p.m_ToneMappingTechnique, "composite entry");
    let probe = Capture { frame, eye };
    TARGET.set(None);
    ACTIVE.set(Some(probe));
    Some(probe)
}

impl Capture {
    pub(crate) fn finish(self, mgr: *mut c_void, slot: u32) {
        ACTIVE.set(None);
        let target = TARGET.take();
        let id = harness::diagnostic_capture_id();
        capture(Stage::DofReturned, || {
            ensure!(
                slot < PostEffectsManager::FULLSCREEN_TEMP_TEXTURE_COUNT && !mgr.is_null(),
                "DoF: invalid result slot {slot}"
            );
            let result = unsafe {
                (*(mgr.cast::<PostEffectsManager>())).m_FullscreenLinearTempTexture[slot as usize]
                    .cast::<Texture>()
            };
            let tex: ID3D11Texture2D = unsafe { result.as_ref() }
                .context("DoF: missing returned texture")?
                .m_Texture
                .cast()?;
            ensure!(
                target == Some(tex.as_raw() as usize),
                "DoF: returned resource differs from the observed draw target"
            );
            tracing::info!(target:"dof_inputs", ?id, frame=self.frame, eye=self.eye, slot, ?target, "verified composite return");
            Ok(result)
        });
    }
}

pub(crate) unsafe fn on_draw(context: *mut c_void, draw: &dyn Fn()) -> bool {
    let Some(probe) = ACTIVE.get() else {
        return false;
    };
    if draw_index() != probe.eye
        || !harness::dof_inputs_requested(probe.frame)
        || harness::frame_identity() != Some(probe.frame)
    {
        return false;
    }
    let Some(ctx) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }) else {
        return false;
    };
    let mut scene = empty_stage_record(probe.frame, probe.eye, Stage::DofScene);
    let mut output = empty_stage_record(probe.frame, probe.eye, Stage::DofComposite);
    capture_raster_state(ctx, &mut scene);
    capture_raster_state(ctx, &mut output);
    let textures = (|| -> Result<_> {
        let (mut vs, mut ps, mut rtv) = (None, None, None);
        let mut srvs: [Option<ID3D11ShaderResourceView>; 15] = Default::default();
        let mut samplers: [Option<ID3D11SamplerState>; 15] = Default::default();
        unsafe {
            ctx.VSGetShader(&mut vs, None, None);
            ctx.PSGetShader(&mut ps, None, None);
            ctx.PSGetShaderResources(0, Some(&mut srvs));
            ctx.PSGetSamplers(0, Some(&mut samplers));
            ctx.OMGetRenderTargets(Some(std::slice::from_mut(&mut rtv)), None);
        }
        let vs = vs
            .and_then(|v| vertex_identity_lookup(v.as_raw() as usize).ok().flatten())
            .context("DoF: unknown VS")?;
        let original = ps.context("DoF: missing PS")?;
        let ps = shader_identity::lookup(&original);
        ensure!(
            matches!(ps, shader_identity::IdentityResult::Mapped { .. }),
            "DoF: unknown PS {ps:?}"
        );
        let id = harness::diagnostic_capture_id();
        let mut views = Vec::new();
        for (slot, srv) in srvs.iter().enumerate() {
            if let Some(srv) = srv {
                let resource = unsafe { srv.GetResource()? };
                let mut v = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
                unsafe { srv.GetDesc(&mut v) };
                let desc = resource.cast::<ID3D11Texture2D>().ok().map(|tex| {
                    let mut d = D3D11_TEXTURE2D_DESC::default();
                    unsafe { tex.GetDesc(&mut d) };
                    (d.Width, d.Height, d.Format.0, d.MipLevels)
                });
                let sampler = samplers[slot].as_ref().map(|s| {
                    let mut d = D3D11_SAMPLER_DESC::default();
                    unsafe { s.GetDesc(&mut d) };
                    (d.Filter.0, d.AddressU.0, d.AddressV.0, d.AddressW.0)
                });
                views.push((
                    slot,
                    resource.as_raw() as usize,
                    v.Format.0,
                    v.ViewDimension.0,
                    desc,
                    sampler,
                ));
            }
        }
        let rtv = rtv.context("DoF: missing target")?;
        let mut rtv_desc = D3D11_RENDER_TARGET_VIEW_DESC::default();
        unsafe { rtv.GetDesc(&mut rtv_desc) };
        let target: ID3D11Texture2D = unsafe { rtv.GetResource()? }.cast()?;
        let srv = srvs[0].as_ref().context("DoF: missing scene t0")?;
        let mut view = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
        unsafe { srv.GetDesc(&mut view) };
        ensure!(
            view.ViewDimension == D3D_SRV_DIMENSION_TEXTURE2D
                && unsafe { view.Anonymous.Texture2D.MostDetailedMip } == 0
                && rtv_desc.ViewDimension == D3D11_RTV_DIMENSION_TEXTURE2D
                && unsafe { rtv_desc.Anonymous.Texture2D.MipSlice } == 0,
            "DoF: unexpected scene/target view"
        );
        let source: ID3D11Texture2D = unsafe { srv.GetResource()? }.cast()?;
        let main: ID3D11Texture2D =
            unsafe { GraphicsEngine::get().and_then(|ge| ge.m_MainColorBuffer.as_ref()) }
                .context("DoF: missing MainColor")?
                .m_Texture
                .cast()?;
        for tex in [&source, &target] {
            let mut d = D3D11_TEXTURE2D_DESC::default();
            unsafe { tex.GetDesc(&mut d) };
            ensure!(
                d.Width <= 1024
                    && d.Height <= 512
                    && d.MipLevels == 1
                    && d.ArraySize == 1
                    && d.SampleDesc.Count == 1,
                "DoF: unexpected composite resource {d:?}"
            );
        }
        let cb = read_bound_constant_buffer(ctx, ShaderStage::Pixel, 1, 17)?;
        ensure!(
            cb.range_known && cb.range_error.is_none(),
            "DoF: unresolved b1 range"
        );
        let rows = cb.values.context("DoF: missing PS b1")?;
        tracing::info!(target:"dof_inputs",?id,frame=probe.frame,eye=probe.eye,?vs,?ps,?views,?rows, main_color=source==main,rtv_format=rtv_desc.Format.0,rtv_resource=target.as_raw() as usize,"composite draw");
        let replacement = if harness::dof_edge_local(probe.frame) == Some(true) {
            ensure!(
                crate::stereo::dispatch::mode() == crate::stereo::dispatch::RenderMode::SinglePass
                    && probe.eye == 0
                    && source == main
                    && rtv_desc.Format.0 == 29
                    && scene.viewports.len() == 2
                    && scene
                        .viewports
                        .iter()
                        .all(|v| (v.x, v.y, v.width, v.height) == (0., 0., 640., 360.))
                    && scene.scissor_enabled == Some(false),
                "DoF edge: unexpected target or raster"
            );
            ensure!(
                vs.actual_sha256
                    == [
                        36, 144, 175, 160, 219, 84, 239, 223, 87, 100, 193, 225, 215, 152, 72, 191,
                        159, 241, 127, 141, 175, 35, 151, 179, 36, 89, 58, 94, 83, 26, 94, 68
                    ]
                    && matches!(&ps,shader_identity::IdentityResult::Mapped{identity} if identity.actual_sha256==shader_identity::DOF_PS && identity.pristine_sha256==shader_identity::DOF_PS)
                    && rows.len() == 68
                    && rows[13 * 4 + 2] == 0.
                    && rows[2 * 4 + 1] == 1.
                    && views.iter().any(|v| v.0 == 9
                        && v.2 == 28
                        && v.4 == Some((256, 128, 28, 1))
                        && v.5 == Some((20, 3, 3, 3))),
                "DoF edge: unexpected program, branch or t9 binding"
            );
            let bytes = dxbc_stereo::local_dof_edge(
                &shader_identity::dof_bytecode().context("DoF edge: no bytecode")?,
            )?;
            let mut shader = None;
            unsafe {
                ctx.GetDevice()?
                    .CreatePixelShader(&bytes, None, Some(&mut shader))?
            };
            Some(shader.context("DoF edge: missing replacement")?)
        } else {
            None
        };
        Ok((source, target, original, replacement))
    })();
    match textures {
        Ok((source, target, original, replacement)) => {
            let input = read_texture(ctx, &source, &mut scene);
            if let Some(replacement) = replacement {
                let mut bound = None;
                unsafe {
                    ctx.PSSetShader(&replacement, None);
                    ctx.PSGetShader(&mut bound, None, None)
                };
                let applied = bound.as_ref() == Some(&replacement);
                if !applied {
                    unsafe { ctx.PSSetShader(&original, None) }
                }
                draw();
                unsafe {
                    ctx.PSSetShader(&original, None);
                    ctx.PSGetShader(&mut bound, None, None)
                };
                let mut record = empty_stage_record(probe.frame, probe.eye, Stage::DofEdgeLocal);
                if !applied || bound.as_ref() != Some(&original) {
                    record.error = Some("DoF edge: bind or restore failed".into());
                }
                let mut state = STATE.lock();
                if state.frame == Some(probe.frame) {
                    state.records.push(record);
                }
            } else {
                draw();
            }
            TARGET.set(Some(target.as_raw() as usize));
            let result = read_texture(ctx, &target, &mut output);
            store(scene, input);
            store(output, result);
        }
        Err(e) => {
            draw();
            store(scene, Err(anyhow::anyhow!("{e:#}")));
            store(output, Err(e));
        }
    }
    true
}

pub(super) fn store(mut record: StageRecord, artifact: Result<StageArtifact>) {
    let mut state = STATE.lock();
    if state.frame != Some(record.frame) {
        return;
    }
    match artifact {
        Ok(artifact) => {
            let index = state.records.len();
            state.images.push(StageImage {
                record: index,
                artifact,
            });
        }
        Err(e) => record.error = Some(format!("{:?} capture: {e:#}", record.stage)),
    }
    state.records.push(record);
}
