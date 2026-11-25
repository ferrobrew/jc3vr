use std::{cell::Cell, ffi::c_void};

use anyhow::{Context as _, ensure};
use windows::{
    Win32::Graphics::{
        Direct3D11::{
            D3D11_BUFFER_DESC, D3D11_TEXTURE2D_DESC, ID3D11DeviceContext, ID3D11DeviceContext1,
            ID3D11PixelShader, ID3D11Texture2D,
        },
        Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_R11G11B10_FLOAT, DXGI_FORMAT_R32_FLOAT},
    },
    core::Interface,
};

use crate::{
    screenshot::harness::{
        self,
        stages::{
            LakeSampleTargets, STATE, Stage, StageImage, capture_raster_state, empty_stage_record,
            read_texture,
        },
    },
    stereo::{
        dispatch::{self, RenderMode},
        draw_index,
        single_pass::shader_identity,
    },
};
thread_local! {static ORDINAL:Cell<Option<u8>>=const{Cell::new(None)};}

pub(crate) fn scope(draw: impl FnOnce()) {
    let active = harness::ssr_bracket_requested() && harness::diagnostic_capture_id().is_some();
    let old = ORDINAL.replace(active.then_some(0));
    draw();
    ORDINAL.set(old);
}

pub(crate) struct RayGuard {
    ctx: ID3D11DeviceContext,
    shader: ID3D11PixelShader,
    targets: LakeSampleTargets,
    frame: i64,
    eye: usize,
    march_eye: Option<bool>,
    veto_eye: Option<bool>,
    grid_eye: Option<bool>,
    history_center: Option<bool>,
}

impl Drop for RayGuard {
    fn drop(&mut self) {
        unsafe { self.ctx.PSSetShader(&self.shader, None) };
        let mut actual = None;
        unsafe { self.ctx.PSGetShader(&mut actual, None, None) };
        if actual.as_ref() != Some(&self.shader) {
            tracing::error!("SSR trace: shader not restored");
            return;
        }
        if let Err(e) = self.targets.finish_resolve(self.frame, self.eye) {
            tracing::error!("SSR trace: {e:#}");
        } else if let Some(treated) = self.march_eye {
            let mut state = STATE.lock();
            if state.frame == Some(self.frame) {
                state.records.push(empty_stage_record(
                    self.frame,
                    self.eye,
                    if treated {
                        Stage::SsrMarchPacked
                    } else {
                        Stage::SsrMarchNative
                    },
                ));
                if let Some(center) = self.history_center {
                    state.records.push(empty_stage_record(
                        self.frame,
                        self.eye,
                        if center {
                            Stage::SsrHistoryPhysical
                        } else {
                            Stage::SsrHistoryNative
                        },
                    ));
                }
                if let Some(grid) = self.grid_eye {
                    state.records.push(empty_stage_record(
                        self.frame,
                        self.eye,
                        if grid {
                            Stage::SsrGridPhysical
                        } else {
                            Stage::SsrGridNative
                        },
                    ));
                }
                if let Some(veto) = self.veto_eye {
                    state.records.push(empty_stage_record(
                        self.frame,
                        self.eye,
                        if veto {
                            Stage::SsrVetoPacked
                        } else {
                            Stage::SsrVetoNative
                        },
                    ));
                }
            }
        }
    }
}

pub(crate) unsafe fn before_draw(raw: *mut c_void, count: u32, start: u32) -> Option<RayGuard> {
    if ORDINAL.get() != Some(1) || count != 3 || start != 0 {
        return None;
    }
    let frame = harness::frame_identity()?;
    if !harness::ssr_operands_requested(frame) {
        return None;
    }
    let id = harness::diagnostic_capture_id()?;
    // The ray trace binds a rewrite of the pristine ray march for one eye-sized run. Under the
    // single-pass collapse the block runs once over both eyes, with the per-eye march rewrite bound
    // when `single_pass.ssr_per_eye` is on, so there is no eye-sized run to trace.
    if dispatch::mode() == RenderMode::SinglePass {
        return None;
    }
    let eye = draw_index();
    let ctx = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }?;
    match bind_ray(ctx, frame, eye, &id) {
        Ok(guard) => Some(guard),
        Err(e) => {
            tracing::error!(%id,frame,eye,"SSR trace binding: {e:#}");
            None
        }
    }
}

fn bind_ray(
    ctx: &ID3D11DeviceContext,
    frame: i64,
    eye: usize,
    id: &str,
) -> anyhow::Result<RayGuard> {
    let (w, h) = crate::stereo::render_size().context("missing SSR size")?;
    ensure!(
        eye < 2 && u64::from(w) * u64::from(h) <= 1024 * 1024,
        "unexpected SSR target size"
    );
    let mut shader = None;
    let mut count = 0;
    unsafe { ctx.PSGetShader(&mut shader, None, Some(&mut count)) };
    let shader = shader.context("missing ray shader")?;
    ensure!(
        count == 0
            && matches!(shader_identity::lookup(&shader),shader_identity::IdentityResult::Mapped{identity} if identity.actual_sha256==shader_identity::SSR_RAY && identity.pristine_sha256==shader_identity::SSR_RAY),
        "unexpected ray shader"
    );
    let bytes = shader_identity::ssr_bytecode().context("missing ray bytecode")?;
    let march_eye = harness::march_eye(frame);
    let veto_eye = harness::veto_eye(frame);
    let grid_eye = harness::grid_eye(frame);
    let history_center = harness::history_center(frame);
    let history = harness::ssr_history_requested(frame);
    ensure!(
        march_eye.is_none(),
        "the march trial needs a per-eye run over the double-wide depth, which no mode takes"
    );
    let bytes = dxbc_stereo::trace_ssr_ray(
        &bytes,
        None,
        veto_eye == Some(true),
        grid_eye == Some(true),
        history,
        history_center == Some(true),
    )?;
    let mut trace = None;
    unsafe {
        ctx.GetDevice()?
            .CreatePixelShader(&bytes, None, Some(&mut trace))?
    };
    let trace = trace.context("missing ray trace shader")?;
    log_rows(ctx, frame, eye, id, 1, &[(0, 5), (1, 11), (2, 15)])?;
    let mut srvs = [const { None }; 4];
    unsafe { ctx.PSGetShaderResources(0, Some(&mut srvs)) };
    for (slot, srv) in srvs.iter().enumerate() {
        let srv = srv.as_ref().context("unbound ray input")?;
        let resource = unsafe { srv.GetResource()? };
        let tex = resource.cast::<ID3D11Texture2D>()?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { tex.GetDesc(&mut desc) };
        if (march_eye.is_some() && slot == 2) || (veto_eye.is_some() && slot == 0) {
            let mut view = Default::default();
            unsafe { srv.GetDesc(&mut view) };
            ensure!(
                desc.Width * 2 == w
                    && desc.Height * 2 == h
                    && desc.Format == DXGI_FORMAT_R32_FLOAT
                    && desc.MipLevels == 1
                    && view.Format == DXGI_FORMAT_R32_FLOAT
                    && view.ViewDimension
                        == windows::Win32::Graphics::Direct3D::D3D_SRV_DIMENSION_TEXTURE2D,
                "unexpected march depth layout"
            );
        }
        if history && slot == 3 {
            let mut view = Default::default();
            unsafe { srv.GetDesc(&mut view) };
            ensure!(
                desc.Width * 2 == w
                    && desc.Height * 2 == h
                    && desc.Format == DXGI_FORMAT_R11G11B10_FLOAT
                    && desc.MipLevels == 1
                    && desc.ArraySize == 1
                    && view.Format == desc.Format
                    && view.ViewDimension
                        == windows::Win32::Graphics::Direct3D::D3D_SRV_DIMENSION_TEXTURE2D,
                "unexpected previous-scene layout"
            );
            let mut record = empty_stage_record(frame, eye, Stage::SsrHistorySource);
            capture_raster_state(ctx, &mut record);
            let artifact = read_texture(ctx, &tex, &mut record)?;
            let mut state = STATE.lock();
            ensure!(state.frame == Some(frame), "stale SSR history frame");
            let index = state.records.len();
            state.images.push(StageImage {
                record: index,
                artifact,
            });
            state.records.push(record);
        }
        tracing::info!(target:"ssr_operands",%id,frame,eye,slot,resource=resource.as_raw() as usize,width=desc.Width,height=desc.Height,format=desc.Format.0,"SSR source");
    }
    let targets = LakeSampleTargets::bind_ssr(ctx, history)?;
    let guard = RayGuard {
        ctx: ctx.clone(),
        shader,
        targets,
        march_eye,
        veto_eye,
        grid_eye,
        history_center,
        frame,
        eye,
    };
    unsafe { ctx.PSSetShader(&trace, None) };
    let mut observed = None;
    unsafe { ctx.PSGetShader(&mut observed, None, None) };
    ensure!(observed.as_ref() == Some(&trace), "SSR trace not bound");
    Ok(guard)
}

fn log_rows(
    ctx: &ID3D11DeviceContext,
    frame: i64,
    eye: usize,
    id: &str,
    n: u8,
    selection: &[(u32, u32)],
) -> anyhow::Result<()> {
    let ctx1 = ctx.cast::<ID3D11DeviceContext1>()?;
    for &(slot, rows) in selection {
        let mut buffer = None;
        let mut first = 0;
        let mut count = 0;
        unsafe {
            ctx1.PSGetConstantBuffers1(
                slot,
                1,
                Some(&mut buffer),
                Some(&mut first),
                Some(&mut count),
            )
        };
        let buffer = buffer.context("missing SSR constants")?;
        let mut desc = D3D11_BUFFER_DESC::default();
        unsafe { buffer.GetDesc(&mut desc) };
        let end = first
            .checked_add(rows)
            .and_then(|n| n.checked_mul(16))
            .context("invalid SSR constant range")?;
        ensure!(
            rows <= count && end <= desc.ByteWidth && desc.ByteWidth <= 65536,
            "unsupported SSR constant range"
        );
        let bytes = crate::screenshot::harness::resolve_probe::copy_content(
            ctx,
            &buffer,
            desc.ByteWidth,
            (first * 16) as usize..end as usize,
        )?;
        let words: Vec<_> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|w| u32::from_le_bytes(*w))
            .collect();
        if n == 1 && slot == 2 && harness::grid_eye(frame).is_some() {
            let (w, h) = crate::stereo::render_size().context("missing grid extent")?;
            let (w, h) = ((w / 2) as f32, (h / 2) as f32);
            ensure!(
                words.get(48..52)
                    == Some(&[
                        w.to_bits(),
                        h.to_bits(),
                        w.recip().to_bits(),
                        h.recip().to_bits()
                    ]),
                "unexpected SSR grid dimensions"
            );
        }
        tracing::info!(target:"ssr_operands",%id,frame,eye,n,slot,first,count,?words,"SSR constants");
    }
    Ok(())
}

pub(crate) unsafe fn after_draw(raw: *mut c_void, count: u32, start: u32) {
    let Some(n) = ORDINAL.get() else { return };
    ORDINAL.set(Some(n.saturating_add(1)));
    let Some(frame) = harness::frame_identity() else {
        return;
    };
    let stages = match n {
        0 if harness::ssr_prepare_requested(frame) => [Stage::SsrPrepDepth, Stage::SsrPrepNormal],
        1 => [Stage::SsrRayColor, Stage::SsrRayAlpha],
        2 => [Stage::SsrBlurColor, Stage::SsrBlurAlpha],
        _ => return,
    };
    if count != 3 || start != 0 {
        return;
    }
    let Some(id) = harness::diagnostic_capture_id() else {
        return;
    };
    // Under the single-pass collapse the block runs once over the double-wide target, so each eye's
    // record holds the whole target and the comparison takes that eye's half of it, as it does for
    // every other stage the collapse renders in one run.
    let packed = dispatch::mode() == RenderMode::SinglePass;
    let eyes: &[usize] = if packed { &[0, 1] } else { &[draw_index()] };
    let Some(ctx) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }) else {
        return;
    };
    let mut views = [None, None];
    let mut ps = None;
    unsafe {
        ctx.OMGetRenderTargets(Some(&mut views), None);
        ctx.PSGetShader(&mut ps, None, None)
    };
    let Some(shader_identity::IdentityResult::Mapped { identity }) =
        ps.as_ref().map(shader_identity::lookup)
    else {
        return;
    };
    let prep_hash = "e54f7846ad37c7094c9594483d62ab935baee018f284f077380bd7b8dd005e9d";
    if n == 0 && (identity.actual_sha256 != prep_hash || identity.pristine_sha256 != prep_hash) {
        tracing::error!(%id,frame,?eyes,ps=%identity.actual_sha256,"unexpected SSR preparation shader");
        return;
    }
    let formats = if n == 0 {
        [
            DXGI_FORMAT_R32_FLOAT,
            windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM,
        ]
    } else {
        [DXGI_FORMAT_R11G11B10_FLOAT, DXGI_FORMAT_R32_FLOAT]
    };
    let mut targets = stages
        .into_iter()
        .zip(views)
        .zip(formats)
        .map(|((stage, v), fmt)| {
            (
                stage,
                v.and_then(|v| unsafe { v.GetResource().ok() })
                    .and_then(|r| r.cast::<ID3D11Texture2D>().ok()),
                Some(fmt),
            )
        })
        .collect::<Vec<_>>();
    if n == 0 {
        if let Err(e) = log_rows(ctx, frame, eyes[0], &id, 0, &[(1, 1)]) {
            tracing::error!(%id,frame,?eyes,"SSR prep constants: {e:#}");
            return;
        }
        for (stage, slot) in [(Stage::SsrPrepPlane, 0), (Stage::SsrPrepScene, 2)] {
            let mut srv = None;
            unsafe { ctx.PSGetShaderResources(slot, Some(std::slice::from_mut(&mut srv))) };
            let tex=srv.and_then(|v|{let mut d=Default::default();unsafe{v.GetDesc(&mut d)}; if d.ViewDimension!=windows::Win32::Graphics::Direct3D::D3D_SRV_DIMENSION_TEXTURE2D || unsafe{d.Anonymous.Texture2D.MostDetailedMip}!=0 {return None;} tracing::info!(target:"ssr_prepare",%id,frame,?eyes,slot,view_format=d.Format.0,"SSR preparation source");unsafe{v.GetResource().ok()} }).and_then(|r|r.cast::<ID3D11Texture2D>().ok());
            targets.push((
                stage,
                tex,
                if slot == 0 {
                    Some(DXGI_FORMAT_R32_FLOAT)
                } else {
                    None
                },
            ));
        }
    }
    let producer = Producer {
        ctx,
        frame,
        id: &id,
        n,
        ps: &identity.actual_sha256,
    };
    for (stage, tex, format) in targets {
        for &eye in eyes {
            producer.record(eye, stage, tex.as_ref(), format);
        }
    }
}

/// One SSR producer draw whose targets are being recorded.
struct Producer<'a> {
    ctx: &'a ID3D11DeviceContext,
    frame: i64,
    id: &'a str,
    n: u8,
    ps: &'a str,
}

impl Producer<'_> {
    fn record(
        &self,
        eye: usize,
        stage: Stage,
        tex: Option<&ID3D11Texture2D>,
        format: Option<DXGI_FORMAT>,
    ) {
        let Self {
            ctx,
            frame,
            id,
            n,
            ps,
        } = *self;
        let mut record = empty_stage_record(frame, eye, stage);
        let artifact = (|| {
            let tex = tex.context("SSR texture unbound or unsupported view")?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            unsafe { tex.GetDesc(&mut desc) };
            let (w, h) = crate::stereo::render_size().context("SSR render size missing")?;
            ensure!(
                format.is_none_or(|f| desc.Format == f)
                    && ((desc.Width * 2 == w && desc.Height * 2 == h)
                        || (stage == Stage::SsrPrepScene && desc.Width == w && desc.Height == h))
                    && desc.MipLevels == 1
                    && u64::from(w) * u64::from(h) <= 1024 * 1024,
                "SSR output layout mismatch"
            );
            capture_raster_state(ctx, &mut record);
            tracing::info!(target:"ssr_bracket",%id,frame,eye,n,?stage,resource=tex.as_raw() as usize,%ps,"SSR producer target");
            read_texture(ctx, tex, &mut record)
        })();
        let mut state = STATE.lock();
        if state.frame != Some(frame) {
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
            Err(e) => record.error = Some(format!("SSR bracket: {e:#}")),
        };
        state.records.push(record);
    }
}
