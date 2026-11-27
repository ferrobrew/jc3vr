use std::{cell::Cell, ffi::c_void};

use anyhow::{Context as _, ensure};
use jc3gi::{
    graphics_engine::{
        graphics_engine::RenderContext, render_block::RenderBlockWaterReflectionPlane,
        render_pass::CalculateOffsetWorldViewProjectionMatrix,
    },
    types::math::Matrix4,
};
use jc3vr_render::{
    probes::state::{last_frame_diagnostics, vertex_identity_lookup},
    stereo::single_pass::{shader_identity, shader_identity::IdentityResult},
};
use windows::{
    Win32::Graphics::{Direct3D11::*, Dxgi::Common::DXGI_FORMAT_R32_FLOAT},
    core::Interface,
};

use crate::screenshot::{
    harness,
    harness::{
        bound_buffer::{ShaderStage, read_bound_constant_buffer},
        stages::{STATE, Stage, empty_stage_record},
    },
};

#[derive(Clone, Copy)]
struct Proxy {
    world: Matrix4,
    native: Matrix4,
    treated: bool,
}
thread_local! {static PROXY:Cell<Option<Proxy>>=const{Cell::new(None)};}

pub(crate) fn scope(
    this: *const RenderBlockWaterReflectionPlane,
    rc: *const RenderContext,
    draw: impl FnOnce(),
) {
    let probe = (|| {
        if !harness::screenshot_wants_eyes() {
            return None;
        }
        let frame = harness::frame_identity()?;
        let treated = harness::proxy_eye(frame)?;
        let world = unsafe { this.as_ref() }?.m_WorldTransform;
        let mut native = Matrix4::default();
        unsafe { CalculateOffsetWorldViewProjectionMatrix(rc, &world, &mut native) };
        Some(Proxy {
            world,
            native,
            treated,
        })
    })();
    let previous = PROXY.replace(probe);
    draw();
    PROXY.set(previous);
}

pub(crate) unsafe fn on_draw(raw: *mut c_void, submit: &impl Fn()) -> bool {
    let Some(proxy) = PROXY.get() else {
        return false;
    };
    let Some(ctx) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }) else {
        return false;
    };
    let Some(id) = harness::diagnostic_capture_id() else {
        return false;
    };
    let Some(frame) = harness::frame_identity() else {
        return false;
    };
    let mut issued = false;
    if let Err(e) = trial(ctx, &id, frame, proxy, &mut || {
        submit();
        issued = true;
    }) {
        tracing::error!(%id,frame,"proxy trial: {e:#}");
    }
    issued
}

fn trial(
    ctx: &ID3D11DeviceContext,
    id: &str,
    frame: i64,
    proxy: Proxy,
    submit: &mut impl FnMut(),
) -> anyhow::Result<()> {
    let mut vs = None;
    let mut ps = None;
    unsafe {
        ctx.VSGetShader(&mut vs, None, None);
        ctx.PSGetShader(&mut ps, None, None)
    };
    let vs = vertex_identity_lookup(vs.context("proxy VS missing")?.as_raw() as usize)
        .map_err(anyhow::Error::msg)?
        .context("proxy VS unmapped")?;
    ensure!(
        format!(
            "{:x}",
            sha2::digest::Output::<sha2::Sha256>::from(vs.actual_sha256)
        ) == "d5887c8d73722e74905ca3026de2e8f9fe0c514407217ffc68c8f3a957577fcc"
            && matches!(shader_identity::lookup(&ps.context("proxy PS missing")?),IdentityResult::Mapped{identity} if identity.actual_sha256=="f541bd3d285df926398001b27874a83e23bcf02e4fafa80880fb4e706768d249"),
        "proxy executable mismatch"
    );
    let mut rt = None;
    unsafe { ctx.OMGetRenderTargets(Some(std::slice::from_mut(&mut rt)), None) };
    let tex: ID3D11Texture2D = unsafe { rt.context("proxy RTV missing")?.GetResource()? }.cast()?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { tex.GetDesc(&mut desc) };
    let (w, h) = jc3vr_view::stereo::render_size().context("proxy size missing")?;
    let vps = || {
        let mut n = 2;
        let mut v = [D3D11_VIEWPORT::default(); 2];
        unsafe { ctx.RSGetViewports(&mut n, Some(v.as_mut_ptr())) };
        (n, v)
    };
    let (n, saved) = vps();
    let full = saved[0];
    ensure!(
        desc.Format == DXGI_FORMAT_R32_FLOAT
            && desc.Width * 2 == w
            && desc.Height * 2 == h
            && n == 2
            && saved[1] == full
            && full.TopLeftX == 0.
            && full.TopLeftY == 0.
            && full.Width == desc.Width as f32
            && full.Height == desc.Height as f32
            && desc.Width.is_multiple_of(2),
        "unexpected proxy viewport"
    );
    let mut original = None;
    unsafe { ctx.VSGetConstantBuffers(1, Some(std::slice::from_mut(&mut original))) };
    let original = original.context("proxy cb1 missing")?;
    let check = |buffer: &ID3D11Buffer, data: &[f32; 16]| -> anyhow::Result<()> {
        let rb = read_bound_constant_buffer(ctx, ShaderStage::Vertex, 1, 4)?;
        ensure!(
            rb.range_known
                && rb.first_constant == 0
                && rb.num_constants == 4
                && rb.byte_width == 64
                && rb.pointer == buffer.as_raw() as usize
                && rb.values.as_deref() == Some(data.as_slice()),
            "proxy cb1 mismatch: {}+{} bytes {}",
            rb.first_constant,
            rb.num_constants,
            rb.byte_width
        );
        Ok(())
    };
    check(&original, &proxy.native.data)?;
    let record = |eye, stage| {
        let mut state = STATE.lock();
        if state.frame == Some(frame) {
            state.records.push(empty_stage_record(frame, eye, stage));
        }
    };
    if !proxy.treated {
        submit();
        record(0, Stage::ProxyNativeObserved);
        return Ok(());
    }
    let diag = last_frame_diagnostics().context("proxy eye inputs missing")?;
    let mut trials = Vec::new();
    for (eye, params) in diag.eyes.iter().enumerate() {
        let xyz = harness::patch_draw::resolve_camera_source(frame, eye, id)
            .context("missing same-frame physical camera")?;
        ensure!(
            xyz == params.cb13_camera_position[..3]
                .iter()
                .map(|x| x.to_bits())
                .collect::<Vec<_>>()
                .as_slice(),
            "stale proxy eye source"
        );
        let mut world = proxy.world;
        for (dst, src) in world.data[12..15].iter_mut().zip(xyz) {
            *dst -= f32::from_bits(src);
        }
        world.data[15] = 1.;
        let mut matrix = Matrix4::default();
        unsafe {
            Matrix4::Multiply4x4(
                &world,
                &Matrix4 {
                    data: params.cb13_view_projection,
                },
                &mut matrix,
            )
        };
        let mut buffer = None;
        unsafe {
            ctx.GetDevice()?.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: 64,
                    Usage: D3D11_USAGE_IMMUTABLE,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    ..Default::default()
                },
                Some(&D3D11_SUBRESOURCE_DATA {
                    pSysMem: matrix.data.as_ptr().cast(),
                    ..Default::default()
                }),
                Some(&mut buffer),
            )?
        };
        let vp = D3D11_VIEWPORT {
            Width: full.Width * 0.5,
            TopLeftX: eye as f32 * full.Width * 0.5,
            ..full
        };
        trials.push((matrix, buffer.context("proxy override missing")?, vp));
    }
    let result = (|| {
        for (eye, (matrix, buffer, vp)) in trials.iter().enumerate() {
            unsafe {
                ctx.VSSetConstantBuffers(1, Some(&[Some(buffer.clone())]));
                ctx.RSSetViewports(Some(&[*vp, *vp]))
            };
            check(buffer, &matrix.data)?;
            ensure!(vps() == (2, [*vp, *vp]), "proxy half viewport not bound");
            submit();
            tracing::info!(target:"proxy_trial",%id,frame,eye,world=?proxy.world.data,native=?proxy.native.data,physical=?matrix.data,resource=tex.as_raw() as usize,"physical proxy draw");
        }
        Ok(())
    })();
    unsafe {
        ctx.VSSetConstantBuffers(1, Some(&[Some(original.clone())]));
        ctx.RSSetViewports(Some(&saved))
    };
    check(&original, &proxy.native.data)?;
    ensure!(vps() == (2, saved), "proxy viewport not restored");
    result?;
    for eye in 0..2 {
        record(eye, Stage::ProxyEyeApplied);
    }
    Ok(())
}
