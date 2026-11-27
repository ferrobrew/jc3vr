use std::{cell::Cell, ffi::c_void};

use anyhow::{Context as _, Result, ensure};
use jc3vr_render::{
    probes::state::{patch_camera_sources, vertex_identity_lookup},
    stereo::single_pass::shader_identity,
};
use jc3vr_view::stereo::draw_index;
use sha2::{Digest, Sha256};
use windows::{
    Win32::Graphics::Direct3D11::{
        D3D11_BIND_CONSTANT_BUFFER, D3D11_BUFFER_DESC, D3D11_SUBRESOURCE_DATA,
        D3D11_USAGE_IMMUTABLE, D3D11_VIEWPORT, ID3D11Buffer, ID3D11DeviceChild,
        ID3D11DeviceContext,
    },
    core::{GUID, Interface},
};

use crate::screenshot::{
    harness,
    harness::{
        WaterReflection,
        bound_buffer::{ShaderStage, read_bound_constant_buffer},
    },
};

const TAG: GUID = GUID::from_u128(0x8cb3b9e3_ad4d_41e7_bdc3_cdc6611f1238);
thread_local! { static REMAINING: Cell<[u8;2]> = const { Cell::new([0;2]) }; }

#[derive(Clone)]
struct EyeSource {
    frame: i64,
    id: String,
    xyz: [u32; 3],
    secondary: bool,
}
static CAMERA_SOURCE: parking_lot::Mutex<[Option<EyeSource>; 2]> =
    parking_lot::Mutex::new([None, None]);

pub(crate) fn resolve_camera_source(frame: i64, eye: usize, id: &str) -> Option<[u32; 3]> {
    CAMERA_SOURCE.lock()[eye]
        .as_ref()
        .filter(|s| s.frame == frame && s.id == id && s.secondary)
        .map(|s| s.xyz)
}

pub(crate) fn scope(tess: bool, draw: impl FnOnce()) {
    let requested = tess
        && matches!(
            harness::requested_water_diagnostic(),
            Some(
                WaterReflection::LakePatchDraw
                    | WaterReflection::LakeCameraControl
                    | WaterReflection::LakeDomainCamera
                    | WaterReflection::LakeHullCamera
            )
        );
    let saved = REMAINING.replace(if requested {
        [
            harness::patch_draw_ordinal().unwrap_or(1),
            harness::second_patch_ordinal().unwrap_or(0),
        ]
    } else {
        [0; 2]
    });
    if tess
        && (harness::requested_water_diagnostic() == Some(WaterReflection::LakePatchOwners)
            || harness::frame_identity().is_some_and(harness::tess_owners_requested))
    {
        harness::stages::patch_owners::scope(draw);
    } else {
        draw();
    }
    REMAINING.set(saved);
}

pub(crate) fn select() -> Option<bool> {
    let remaining = REMAINING.get();
    if remaining == [0; 2] {
        return None;
    }
    REMAINING.set(remaining.map(|n| n.saturating_sub(1)));
    if remaining[0] == 1 {
        Some(false)
    } else if remaining[1] == 1 {
        Some(true)
    } else {
        None
    }
}

pub(crate) fn tag(shader: &ID3D11DeviceChild, bytes: &[u8]) {
    let digest = Sha256::digest(bytes);
    if let Err(e) = unsafe { shader.SetPrivateData(&TAG, 32, Some(digest.as_ptr().cast())) } {
        tracing::warn!("patch draw: tagging shader: {e}");
    }
}

pub(crate) fn identity(shader: &ID3D11DeviceChild) -> Option<String> {
    let mut out = [0u8; 32];
    let mut n = 32;
    unsafe { shader.GetPrivateData(&TAG, &mut n, Some(out.as_mut_ptr().cast())) }.ok()?;
    (n == 32).then(|| out.iter().map(|b| format!("{b:02x}")).collect())
}

pub(crate) fn camera_experiment() -> bool {
    matches!(
        harness::requested_water_diagnostic(),
        Some(
            WaterReflection::LakeCameraControl
                | WaterReflection::LakeDomainCamera
                | WaterReflection::LakeHullCamera
        )
    )
}

pub(crate) unsafe fn observe(
    context: *mut c_void,
    offset: u32,
    expected: Option<usize>,
    secondary: bool,
) -> Option<CameraGuard> {
    let id = harness::diagnostic_capture_id()?;
    let ctx = unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }?;
    let (mut vs, mut hs, mut ds, mut ps) = (None, None, None, None);
    let mut vp = [D3D11_VIEWPORT::default(); 2];
    let mut n = 2;
    unsafe {
        ctx.VSGetShader(&mut vs, None, None);
        ctx.HSGetShader(&mut hs, None, None);
        ctx.DSGetShader(&mut ds, None, None);
        ctx.PSGetShader(&mut ps, None, None);
        ctx.RSGetViewports(&mut n, Some(vp.as_mut_ptr()));
    }
    let vs = vs
        .as_ref()
        .and_then(|s| vertex_identity_lookup(s.as_raw() as usize).ok().flatten())
        .map(|s| {
            s.actual_sha256
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        });
    let hs = hs.as_ref().and_then(|s| identity(s));
    let ds = ds.as_ref().and_then(|s| identity(s));
    let ps = ps.as_ref().map(shader_identity::lookup);
    let frame = harness::frame_identity();
    let eye = draw_index();
    tracing::info!(target:"patch_draw",%id,?frame,eye,offset,?vs,?hs,?ds,?ps,n, x=vp[0].TopLeftX,width=vp[0].Width,"submission");
    let camera = camera_experiment();
    for (stage, slot, rows) in [
        (ShaderStage::Domain, 0, 2),
        (ShaderStage::Domain, 1, if camera { 10 } else { 6 }),
        (ShaderStage::Domain, 13, 18),
        (ShaderStage::Vertex, 3, 1),
        (ShaderStage::Hull, 0, 1),
        (ShaderStage::Hull, 1, 10),
    ] {
        if stage == ShaderStage::Hull && !camera {
            continue;
        }
        match read_bound_constant_buffer(ctx, stage, slot, rows) {
            Ok(r) => {
                tracing::info!(target:"patch_draw",%id,?frame,eye,offset,x=vp[0].TopLeftX,stage=stage.name(),slot,pointer=r.pointer,first=r.first_constant,count=r.num_constants,known=r.range_known,err=?r.range_error,bits=?r.values.map(|vs|vs.into_iter().map(f32::to_bits).collect::<Vec<_>>()),"constants")
            }
            Err(e) => {
                tracing::warn!(target:"patch_draw",%id,?frame,eye,offset,stage=stage.name(),slot,"constant read: {e:#}")
            }
        }
    }
    if !camera {
        return None;
    }
    let eye = expected?;
    let condition = harness::requested_water_diagnostic()?;
    let verified = vs.as_deref()
        == Some("f55b68229567994446a0f9c3f13074e125e3237e75b3c2956286b86f7265e43b")
        && hs.as_deref()
            == Some("50607f399ea4023cdc6ad3f942d52728dc622de7a454d393501a9ae83a5143a1")
        && ds.as_deref()
            == Some("a58152c0cdef733943e99b2d4f9ac82eacaa89484e5c093668962b5379f6a5ba")
        && matches!(ps, Some(shader_identity::IdentityResult::Mapped { identity }) if identity.actual_sha256 == "38987576f35ad34008eb3321ebe4e531a75f840d8b40eb25081d287c0cd19470");
    let result = (|| {
        let (viewport, source) =
            patch_camera_sources(eye).context("collapse sources unavailable")?;
        ensure!(
            verified && n == 2 && vp == [viewport; 2],
            "unexpected shaders or viewport"
        );
        CameraGuard::bind(ctx, &id, eye, source, condition, secondary)
    })();
    match result {
        Ok(guard) => Some(guard),
        Err(e) => {
            tracing::warn!(target:"patch_draw",%id,eye,offset,"camera setup: {e:#}");
            None
        }
    }
}

pub(crate) struct CameraGuard {
    ds: BufferOverride,
    hs: Option<BufferOverride>,
    secondary: bool,
    id: String,
    eye: usize,
}

struct BufferOverride {
    ctx: ID3D11DeviceContext,
    original: ID3D11Buffer,
    before: Vec<u32>,
    stage: ShaderStage,
    restored: bool,
}

pub(crate) fn read_rows(
    ctx: &ID3D11DeviceContext,
    stage: ShaderStage,
    slot: u32,
    count: u32,
) -> Result<(usize, Vec<u32>)> {
    let r = read_bound_constant_buffer(ctx, stage, slot, count)?;
    ensure!(
        r.range_known
            && r.first_constant == 0
            && r.num_constants == count
            && r.byte_width == count * 16
            && r.range_error.is_none(),
        "unexpected DS cb{slot} range: {}+{} width {}",
        r.first_constant,
        r.num_constants,
        r.byte_width
    );
    let bits: Vec<_> = r
        .values
        .context("missing DS constants")?
        .into_iter()
        .map(f32::to_bits)
        .collect();
    ensure!(bits.len() == count as usize * 4, "incomplete DS constants");
    Ok((r.pointer, bits))
}

impl CameraGuard {
    fn bind(
        ctx: &ID3D11DeviceContext,
        id: &str,
        eye: usize,
        source: usize,
        condition: WaterReflection,
        secondary: bool,
    ) -> Result<Self> {
        let (bound_source, stereo) = read_rows(ctx, ShaderStage::Domain, 13, 18)?;
        ensure!(
            source == bound_source && stereo[16..19] != stereo[36..39],
            "unexpected stereo camera source"
        );
        let start = (eye * 5 + 4) * 4;
        let xyz = &stereo[start..start + 3];
        let corrected = condition != WaterReflection::LakeCameraControl;
        let ds = BufferOverride::bind(ctx, ShaderStage::Domain, xyz, corrected, id, eye)?;
        let hs = corrected
            .then(|| {
                BufferOverride::bind(
                    ctx,
                    ShaderStage::Hull,
                    xyz,
                    condition == WaterReflection::LakeHullCamera,
                    id,
                    eye,
                )
            })
            .transpose()?;
        if let Some(frame) = harness::frame_identity()
            && harness::resolve_eye_camera(frame).is_some()
        {
            let xyz: [u32; 3] = xyz.try_into()?;
            let mut sources = CAMERA_SOURCE.lock();
            ensure!(
                !secondary
                    || sources[eye].as_ref().is_some_and(|s| s.frame == frame
                        && s.id == id
                        && s.xyz == xyz
                        && !s.secondary),
                "resolve camera: inconsistent selected writer sources"
            );
            sources[eye] = Some(EyeSource {
                frame,
                id: id.to_owned(),
                xyz,
                secondary,
            });
        }
        Ok(Self {
            ds,
            hs,
            secondary,
            id: id.to_owned(),
            eye,
        })
    }
}

pub(crate) struct WriterDomainGuard(BufferOverride);

impl WriterDomainGuard {
    pub(crate) fn bind(
        ctx: &ID3D11DeviceContext,
        id: &str,
        eye: usize,
        viewport: D3D11_VIEWPORT,
        treated: bool,
        stage: ShaderStage,
    ) -> Result<Self> {
        let (expected, source) =
            patch_camera_sources(eye).context("terrain writer: absent physical camera")?;
        let (bound, stereo) = read_rows(ctx, ShaderStage::Domain, 13, 18)?;
        let start = 16 + 20 * eye;
        ensure!(
            expected == viewport && source == bound && stereo[16..19] != stereo[36..39],
            "terrain writer: unexpected physical camera source"
        );
        Ok(Self(BufferOverride::bind(
            ctx,
            stage,
            &stereo[start..start + 3],
            treated,
            id,
            eye,
        )?))
    }

    pub(crate) fn restore(mut self) -> Result<()> {
        ensure!(
            self.0.restore(),
            "terrain writer: original domain binding not restored"
        );
        Ok(())
    }
}

pub(crate) struct TerrainCameraGuard {
    ds: BufferOverride,
    hs: BufferOverride,
}

impl TerrainCameraGuard {
    pub(crate) fn bind(
        ctx: &ID3D11DeviceContext,
        id: &str,
        treated: bool,
        viewport: D3D11_VIEWPORT,
    ) -> Result<Self> {
        let (expected, source) =
            patch_camera_sources(0).context("terrain camera: left source unavailable")?;
        let (bound, stereo) = read_rows(ctx, ShaderStage::Domain, 13, 18)?;
        ensure!(
            viewport == expected && source == bound && stereo[16..19] != stereo[36..39],
            "terrain camera: unexpected physical camera source"
        );
        let xyz = &stereo[16..19];
        Ok(Self {
            ds: BufferOverride::bind(ctx, ShaderStage::Domain, xyz, treated, id, 0)?,
            hs: BufferOverride::bind(ctx, ShaderStage::Hull, xyz, treated, id, 0)?,
        })
    }

    pub(crate) fn restore(mut self) -> Result<()> {
        let hs = self.hs.restore();
        ensure!(
            self.ds.restore() && hs,
            "terrain camera: original buffers not restored"
        );
        Ok(())
    }
}

impl BufferOverride {
    fn bind(
        ctx: &ID3D11DeviceContext,
        stage: ShaderStage,
        xyz: &[u32],
        treated: bool,
        id: &str,
        eye: usize,
    ) -> Result<Self> {
        let (original_ptr, before) = read_rows(ctx, stage, 1, 10)?;
        let start = match stage {
            ShaderStage::Domain => 20,
            ShaderStage::Hull => 24,
            _ => anyhow::bail!("unsupported camera stage"),
        };
        ensure!(
            xyz.iter().all(|v| f32::from_bits(*v).is_finite()) && before[start..start + 3] != *xyz,
            "invalid camera operand"
        );
        let mut data = before.clone();
        if treated {
            data[start..start + 3].copy_from_slice(xyz);
        }
        let guard = Self::clone_rows(ctx, stage, original_ptr, before, &data)?;
        tracing::info!(target:"patch_draw",id,eye,stage=stage.name(),treated,original_ptr,?xyz,before=?guard.before,?data,"camera bound");
        Ok(guard)
    }

    fn clone_rows(
        ctx: &ID3D11DeviceContext,
        stage: ShaderStage,
        original_ptr: usize,
        before: Vec<u32>,
        data: &[u32],
    ) -> Result<Self> {
        let mut buffers = [None];
        unsafe {
            match stage {
                ShaderStage::Hull => ctx.HSGetConstantBuffers(1, Some(&mut buffers)),
                _ => ctx.DSGetConstantBuffers(1, Some(&mut buffers)),
            }
        };
        let original = buffers[0]
            .take()
            .context("missing original camera buffer")?;
        ensure!(
            original.as_raw() as usize == original_ptr,
            "camera buffer changed during readback"
        );
        let mut cloned = None;
        unsafe {
            ctx.GetDevice()?.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: 160,
                    Usage: D3D11_USAGE_IMMUTABLE,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    ..Default::default()
                },
                Some(&D3D11_SUBRESOURCE_DATA {
                    pSysMem: data.as_ptr().cast(),
                    ..Default::default()
                }),
                Some(&mut cloned),
            )?
        };
        let cloned = cloned.context("missing diagnostic DS cb1")?;
        let ptr = cloned.as_raw() as usize;
        let guard = Self {
            ctx: ctx.clone(),
            original,
            before,
            stage,
            restored: false,
        };
        guard.set(cloned);
        ensure!(
            read_rows(ctx, stage, 1, 10)? == (ptr, data.to_vec()),
            "diagnostic camera binding differs"
        );
        Ok(guard)
    }

    fn set(&self, buffer: ID3D11Buffer) {
        let buffers = [Some(buffer)];
        unsafe {
            match self.stage {
                ShaderStage::Hull => self.ctx.HSSetConstantBuffers(1, Some(&buffers)),
                _ => self.ctx.DSSetConstantBuffers(1, Some(&buffers)),
            }
        }
    }

    fn restore(&mut self) -> bool {
        self.set(self.original.clone());
        let ptr = self.original.as_raw() as usize;
        self.restored = read_rows(&self.ctx, self.stage, 1, 10)
            .is_ok_and(|(p, rows)| p == ptr && rows == self.before);
        self.restored
    }
}

impl Drop for BufferOverride {
    fn drop(&mut self) {
        if !self.restored {
            self.restore();
        }
    }
}

pub(crate) struct LakeMvpGuard {
    buffer: BufferOverride,
    other_bindings: [usize; 5],
}

fn other_b1(ctx: &ID3D11DeviceContext) -> [usize; 5] {
    let mut buffers = [const { None }; 5];
    unsafe {
        ctx.VSGetConstantBuffers(1, Some(&mut buffers[0..1]));
        ctx.HSGetConstantBuffers(1, Some(&mut buffers[1..2]));
        ctx.GSGetConstantBuffers(1, Some(&mut buffers[2..3]));
        ctx.PSGetConstantBuffers(1, Some(&mut buffers[3..4]));
        ctx.CSGetConstantBuffers(1, Some(&mut buffers[4..5]));
    }
    buffers.map(|b| b.map_or(0, |b| b.as_raw() as usize))
}

pub(crate) fn water_mvp(eye: usize) -> Result<[u32; 16]> {
    let vp = jc3vr_render::probes::state::full_eye_view_projection(eye)
        .context("lake MVP: missing physical VP")?
        .data;
    ensure!(
        vp.iter().all(|v| v.is_finite()),
        "lake MVP: nonfinite eye matrix"
    );
    Ok(std::array::from_fn(|i| {
        vp[4 * [0, 2, 1, 3][i / 4] + i % 4].to_bits()
    }))
}

impl LakeMvpGuard {
    pub(crate) fn bind(ctx: &ID3D11DeviceContext, eye: usize, treated: bool) -> Result<Self> {
        ensure!(
            unsafe { ctx.GetType() }
                == windows::Win32::Graphics::Direct3D11::D3D11_DEVICE_CONTEXT_IMMEDIATE,
            "lake MVP: deferred context"
        );
        let mut ds = None;
        unsafe { ctx.DSGetShader(&mut ds, None, None) };
        ensure!(
            ds.as_ref().and_then(|s| identity(s)).as_deref()
                == Some("71d536bcb178edec429ce8f6a593b8ac38fd0c159861171347b586cc6bdd4fbc"),
            "lake MVP: unexpected DS"
        );
        let (ptr, before) = read_rows(ctx, ShaderStage::Domain, 1, 10)?;
        let other_bindings = other_b1(ctx);
        ensure!(
            other_bindings[0] == ptr && before[..16].iter().all(|v| f32::from_bits(*v).is_finite()),
            "lake MVP: unexpected source"
        );
        let vp = water_mvp(eye)?;
        let mut data = before.clone();
        if treated {
            data[..16].copy_from_slice(&vp);
        }
        let buffer = BufferOverride::clone_rows(ctx, ShaderStage::Domain, ptr, before, &data)?;
        ensure!(
            other_b1(ctx) == other_bindings,
            "lake MVP: non-domain binding changed"
        );
        Ok(Self {
            buffer,
            other_bindings,
        })
    }
    pub(crate) fn restore(mut self) -> bool {
        self.buffer.restore() && other_b1(&self.buffer.ctx) == self.other_bindings
    }
}

impl Drop for CameraGuard {
    fn drop(&mut self) {
        let hull_restored = self.hs.as_mut().is_none_or(BufferOverride::restore);
        let restored = self.ds.restore() && hull_restored;
        tracing::info!(target:"patch_draw",id=%self.id,eye=self.eye,hull_restored,restored,"camera restored");
        if restored {
            harness::stages::record_camera_submission(self.eye, self.secondary);
        }
    }
}
