use std::ffi::c_void;

use anyhow::{Context, Result, ensure};
use jc3gi::graphics_engine::{graphics_engine::GraphicsEngine, render_engine::RenderPassId};
use jc3vr_render::stereo::single_pass::{shader_identity, vertex_identity_lookup};
use jc3vr_view::stereo::draw_index;
use windows::{
    Win32::Graphics::{Direct3D::D3D_SRV_DIMENSION_TEXTURE2D, Direct3D11::*},
    core::Interface,
};

use crate::screenshot::{
    harness,
    harness::{
        bound_buffer::{ShaderStage, read_bound_constant_buffer},
        late_sky_stages_requested,
        stages::{
            HdrStage, STATE, Stage, StageArtifact, StageImage, StageRecord, capture_hdr,
            capture_raster_state, empty_stage_record, read_texture,
        },
    },
};

pub(crate) unsafe fn capture_fog_inputs(context: *mut c_void) {
    let Some(frame) = harness::frame_identity().filter(|&f| harness::fog_inputs_requested(f))
    else {
        return;
    };
    let Some(id) = harness::diagnostic_capture_id() else {
        return;
    };
    let eye = draw_index();
    let mut record = empty_stage_record(frame, eye, Stage::FogInput);
    let artifact = (|| -> Result<StageArtifact> {
        let ctx = unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }
            .context("fog inputs: missing context")?;
        capture_raster_state(ctx, &mut record);
        let (mut vs, mut ps, mut srv, mut rtv, mut sampler, mut blend) =
            (None, None, None, None, None, None);
        let (mut factor, mut mask) = ([0.; 4], 0);
        unsafe {
            ctx.VSGetShader(&mut vs, None, None);
            ctx.PSGetShader(&mut ps, None, None);
            ctx.PSGetShaderResources(0, Some(std::slice::from_mut(&mut srv)));
            ctx.PSGetSamplers(0, Some(std::slice::from_mut(&mut sampler)));
            ctx.OMGetRenderTargets(Some(std::slice::from_mut(&mut rtv)), None);
            ctx.OMGetBlendState(Some(&mut blend), Some(&mut factor), Some(&mut mask));
        }
        let vs = vs.and_then(|v| vertex_identity_lookup(v.as_raw() as usize).ok().flatten());
        let ps = ps.as_ref().map(shader_identity::lookup);
        ensure!(
            vs.as_ref().is_some_and(|v| format!(
                "{:x}",
                sha2::digest::Output::<sha2::Sha256>::from(v.actual_sha256)
            ) == "4d8be233e02ca844ce4e4ac4d88136e7a02bddf62f0ec2b80eb79d767e0d4540")
                && matches!(&ps,Some(shader_identity::IdentityResult::Mapped{identity}) if identity.actual_sha256=="c977581cb8b530d46cd3ddd4510fdfb85d45cd5aaa1384b2d0f5103b18d1faae"),
            "fog inputs: unexpected shaders {vs:?} {ps:?}"
        );
        let target: ID3D11Texture2D =
            unsafe { rtv.context("missing RTV")?.GetResource()? }.cast()?;
        let main: ID3D11Texture2D =
            unsafe { GraphicsEngine::get().and_then(|ge| ge.m_MainColorBuffer.as_ref()) }
                .context("missing MainColor")?
                .m_Texture
                .cast()?;
        ensure!(target == main, "fog inputs: not drawing to MainColor");
        let srv = srv.context("fog inputs: missing t0")?;
        let mut view = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
        unsafe { srv.GetDesc(&mut view) };
        ensure!(
            view.ViewDimension == D3D_SRV_DIMENSION_TEXTURE2D
                && unsafe { view.Anonymous.Texture2D.MostDetailedMip } == 0,
            "fog inputs: unexpected t0 view"
        );
        let tex: ID3D11Texture2D = unsafe { srv.GetResource()? }.cast()?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        let mut sd = D3D11_SAMPLER_DESC::default();
        let mut bd = D3D11_BLEND_DESC::default();
        unsafe {
            tex.GetDesc(&mut desc);
            sampler.context("fog inputs: missing s0")?.GetDesc(&mut sd);
            blend.context("fog inputs: missing blend")?.GetDesc(&mut bd)
        };
        ensure!(
            desc.Width <= 1024
                && desc.Height <= 512
                && desc.ArraySize == 1
                && desc.MipLevels == 1
                && desc.SampleDesc.Count == 1,
            "fog inputs: unexpected texture {desc:?}"
        );
        let rows = |stage, n| -> Result<Vec<f32>> {
            let r = read_bound_constant_buffer(ctx, stage, 1, n)?;
            ensure!(
                r.range_known && r.range_error.is_none(),
                "fog inputs: invalid cb1 range"
            );
            let rows = r.values.context("missing cb1")?;
            ensure!(rows.len() == 4 * n as usize, "fog inputs: incomplete cb1");
            Ok(rows)
        };
        let vertex = rows(ShaderStage::Vertex, 1)?;
        let pixel = rows(ShaderStage::Pixel, 2)?;
        tracing::info!(target:"fog_inputs",%id,frame,eye,?vs,?ps,?vertex,?pixel,format=view.Format.0,?sd,blend=?bd.RenderTarget[0],?factor,mask,"composite binding");
        read_texture(ctx, &tex, &mut record)
    })();
    store(record, artifact);
}

pub(crate) unsafe fn capture_fog_generation(context: *mut c_void, draw: &dyn Fn()) -> bool {
    let Some(frame) = harness::frame_identity().filter(|&f| harness::fog_generation_requested(f))
    else {
        return false;
    };
    let Some(id) = harness::diagnostic_capture_id() else {
        return false;
    };
    let Some(ctx) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }) else {
        return false;
    };
    let eye = draw_index();
    let mut input = empty_stage_record(frame, eye, Stage::FogGenDepth);
    let mut output = empty_stage_record(frame, eye, Stage::FogGenerated);
    capture_raster_state(ctx, &mut input);
    capture_raster_state(ctx, &mut output);
    let textures = (|| -> Result<_> {
        let (mut vs, mut ps, mut srv, mut rtv) = (None, None, None, None);
        unsafe {
            ctx.VSGetShader(&mut vs, None, None);
            ctx.PSGetShader(&mut ps, None, None);
            ctx.PSGetShaderResources(0, Some(std::slice::from_mut(&mut srv)));
            ctx.OMGetRenderTargets(Some(std::slice::from_mut(&mut rtv)), None);
        }
        let vs = vs.and_then(|v| vertex_identity_lookup(v.as_raw() as usize).ok().flatten());
        let ps = ps.as_ref().map(shader_identity::lookup);
        ensure!(
            vs.as_ref().is_some_and(|v| format!(
                "{:x}",
                sha2::digest::Output::<sha2::Sha256>::from(v.actual_sha256)
            ) == "246a3f64ab0529f96ea440341c2e87ad03422ec4c3f1f29fba19915776207c0c")
                && matches!(&ps,Some(shader_identity::IdentityResult::Mapped{identity}) if matches!(identity.actual_sha256.as_str(),"e139d6336eea7c3c312fd761002cd4ed6efd95df60a07f9d719e081cd65aac0e"|"67abe8dfe932319f1829db778bd46c2179eec85b6a0b717581a617e5d29f5fdc"|"28b0112a7da45e59a41158913528a6564d8a19685312b40ce2b930c300373536"|"a10dbcd176ceeaa9b289a3c6ae0ea5935e75d0685e22fabc9aaa0b7042a6db4e")),
            "fog generation: unexpected programs {vs:?} {ps:?}"
        );
        let srv = srv.context("fog generation: missing t0")?;
        let mut view = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
        unsafe { srv.GetDesc(&mut view) };
        ensure!(
            view.ViewDimension == D3D_SRV_DIMENSION_TEXTURE2D
                && unsafe { view.Anonymous.Texture2D.MostDetailedMip } == 0,
            "fog generation: unexpected depth view"
        );
        let source: ID3D11Texture2D = unsafe { srv.GetResource()? }.cast()?;
        let target: ID3D11Texture2D =
            unsafe { rtv.context("fog generation: missing RTV")?.GetResource()? }.cast()?;
        for tex in [&source, &target] {
            let mut d = D3D11_TEXTURE2D_DESC::default();
            unsafe { tex.GetDesc(&mut d) };
            ensure!(
                d.Width <= 640 && d.Height <= 360 && d.ArraySize == 1 && d.SampleDesc.Count == 1,
                "fog generation: invalid texture {d:?}"
            );
        }
        let mut constants = Vec::new();
        for (slot, n) in [(0, 10), (1, 11)] {
            let r = read_bound_constant_buffer(ctx, ShaderStage::Pixel, slot, n)?;
            ensure!(
                r.range_known && r.range_error.is_none(),
                "fog generation: invalid PS b{slot}"
            );
            constants.push(r.values.context("fog generation: missing constants")?);
        }
        tracing::info!(target:"fog_generation",%id,frame,eye,?ps,view_format=view.Format.0,?constants,"producer binding");
        let replacement = if harness::fog_local_uv(frame) == Some(true) {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            unsafe { target.GetDesc(&mut desc) };
            ensure!(
                jc3vr_view::stereo::dispatch::mode()
                    == jc3vr_view::stereo::dispatch::RenderMode::SinglePass
                    && input.viewports.len() == 2
                    && input
                        .viewports
                        .iter()
                        .all(|v| (v.x, v.y, v.width, v.height) == (0., 0., 320., 180.))
                    && input.scissor_enabled == Some(false)
                    && (desc.Width, desc.Height, desc.Format.0) == (320, 180, 28)
                    && view.Format.0 == 21
                    && matches!(&ps,Some(shader_identity::IdentityResult::Mapped{identity}) if identity.actual_sha256==shader_identity::FOG_PS && identity.pristine_sha256==shader_identity::FOG_PS),
                "fog UV: unexpected producer"
            );
            let bytes = dxbc_stereo::local_fog_reconstruction(
                &shader_identity::fog_bytecode().context("fog UV: missing bytecode")?,
            )?;
            let mut shader = None;
            unsafe {
                ctx.GetDevice()?
                    .CreatePixelShader(&bytes, None, Some(&mut shader))?
            };
            Some(shader.context("fog UV: missing replacement")?)
        } else {
            None
        };
        Ok((source, target, replacement))
    })();
    match textures {
        Ok((source, target, replacement)) => {
            let depth = read_texture(ctx, &source, &mut input);
            let treated = if let Some(replacement) = replacement {
                let mut original = None;
                let mut bound = None;
                unsafe {
                    ctx.PSGetShader(&mut original, None, None);
                    ctx.PSSetShader(&replacement, None);
                    ctx.PSGetShader(&mut bound, None, None)
                };
                let valid = bound.as_ref() == Some(&replacement) && original.is_some();
                if !valid {
                    unsafe { ctx.PSSetShader(original.as_ref(), None) };
                }
                draw();
                unsafe {
                    ctx.PSSetShader(original.as_ref(), None);
                    ctx.PSGetShader(&mut bound, None, None)
                };
                let mut treatment = empty_stage_record(frame, eye, Stage::FogLocalUv);
                let ok = valid && bound == original;
                if !ok {
                    treatment.error = Some("fog UV: bind or restore failed".into());
                }
                let mut state = STATE.lock();
                if state.frame == Some(frame) {
                    state.records.push(treatment);
                }
                ok
            } else {
                draw();
                true
            };
            let result = if treated {
                read_texture(ctx, &target, &mut output)
            } else {
                Err(anyhow::anyhow!("fog UV: invalid treatment"))
            };
            store(input, depth);
            store(output, result);
        }
        Err(e) => {
            draw();
            store(input, Err(anyhow::anyhow!("{e:#}")));
            store(output, Err(e));
        }
    }
    true
}

fn store(mut record: StageRecord, artifact: Result<StageArtifact>) {
    let mut state = STATE.lock();
    if state.frame == Some(record.frame) {
        match artifact {
            Ok(artifact) => {
                let index = state.records.len();
                state.images.push(StageImage {
                    record: index,
                    artifact,
                });
            }
            Err(e) => record.error = Some(format!("fog inputs: {e:#}")),
        }
        state.records.push(record);
    }
}

pub(crate) fn capture_before_fog_apply(pass: i16) {
    if pass == RenderPassId::RP_FOG_VOLUME_APPLY as i16 && late_sky_stages_requested() {
        capture_hdr(HdrStage::BeforeFogVolumeApply);
    }
}

pub(crate) fn capture_late_sky(pass: i16) {
    let stage = match pass {
        id if id == RenderPassId::RP_APPLY_CLOUDS as i16 => HdrStage::AfterApplyClouds,
        id if id == RenderPassId::RP_FOG_VOLUME_APPLY as i16 => HdrStage::AfterFogVolumeApply,
        _ => return,
    };
    if late_sky_stages_requested() {
        capture_hdr(stage);
    }
}

pub(crate) fn complete_late_sky(records: &mut Vec<StageRecord>, frame: i64, eyes: [bool; 2]) {
    for (eye, required) in eyes.into_iter().enumerate() {
        if !required {
            continue;
        }
        for stage in [
            Stage::AfterApplyClouds,
            Stage::BeforeFogVolumeApply,
            Stage::AfterFogVolumeApply,
        ] {
            if !records
                .iter()
                .any(|r| (r.frame, r.eye, r.stage) == (frame, eye, stage))
            {
                let mut record = empty_stage_record(frame, eye, stage);
                record.error = Some("late sky stages: missing requested frame/eye boundary".into());
                records.push(record);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_records_cannot_substitute_another_frame_or_eye() {
        let mut records = vec![
            empty_stage_record(1, 1, Stage::BeforeFogVolumeApply),
            empty_stage_record(2, 0, Stage::BeforeFogVolumeApply),
            empty_stage_record(2, 1, Stage::AfterFogVolumeApply),
        ];
        for _ in 0..2 {
            complete_late_sky(&mut records, 2, [false, true]);
        }
        assert_eq!(records.len(), 5);
        assert!(
            records[3..]
                .iter()
                .all(|r| r.frame == 2 && r.eye == 1 && r.error.is_some())
        );
    }
}
