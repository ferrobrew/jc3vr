use std::{cell::RefCell, ffi::c_void, marker::PhantomData, rc::Rc};

use jc3vr_render::stereo::single_pass::{
    shader_identity,
    shader_identity::{IdentityResult, LAKE_PS},
};
use windows::{
    Win32::Graphics::{
        Direct3D::D3D_SRV_DIMENSION_TEXTURE2D,
        Direct3D11::{
            D3D11_BIND_SHADER_RESOURCE, D3D11_SAMPLER_DESC, D3D11_SHADER_RESOURCE_VIEW_DESC,
            D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC, D3D11_USAGE_IMMUTABLE, D3D11_VIEWPORT,
            ID3D11DeviceContext, ID3D11PixelShader, ID3D11ShaderResourceView, ID3D11Texture2D,
        },
        Dxgi::Common::{
            DXGI_FORMAT, DXGI_FORMAT_R11G11B10_FLOAT, DXGI_FORMAT_R32_FLOAT, DXGI_SAMPLE_DESC,
        },
    },
    core::Interface,
};

use crate::screenshot::harness::{
    WaterReflection, diagnostic_capture_id, frame_identity, lake_displacement_requested,
    lake_ds_mvp, lake_gradients_requested, lake_inner_edge, lake_normals_requested,
    lake_waves_requested,
    patch_draw::LakeMvpGuard,
    stages::{LakeSampleTargets, capture_water_inputs, record_water_treatment},
    water_gpu_probe::work,
    water_inputs_requested, water_reflection_requested, water_tess_draw,
};

thread_local! {
    static ACTIVE: RefCell<Option<DrawState>> = const { RefCell::new(None) };
}

struct DrawState {
    eye: usize,
    draws: usize,
    tess_draws: usize,
    selected: Option<u8>,
    zero: Option<ID3D11ShaderResourceView>,
}

pub(crate) struct WaterInputCapture {
    armed: bool,
    _not_send: PhantomData<Rc<()>>,
}

pub(crate) fn begin_water_input_capture(eye: usize) -> WaterInputCapture {
    let armed = if water_inputs_requested() {
        ACTIVE.with(|active| {
            let mut active = active.borrow_mut();
            if active.is_some() {
                false
            } else {
                *active = Some(DrawState {
                    eye,
                    draws: 0,
                    tess_draws: 0,
                    selected: water_tess_draw(),
                    zero: None,
                });
                true
            }
        })
    } else {
        false
    };
    WaterInputCapture {
        armed,
        _not_send: PhantomData,
    }
}

impl Drop for WaterInputCapture {
    fn drop(&mut self) {
        if self.armed {
            ACTIVE.with(|active| *active.borrow_mut() = None);
        }
    }
}

const VERIFIED_WATER_PS: &str = "fd508419c18c5fe0c08816017a2a20ed5394aab5fd9d3fe94cce7d67ac0f3dce";

pub(crate) enum ReflectionGuard {
    Pristine,
    Texture { _guard: TextureGuard },
    Planar { _guard: Box<LakeShaderGuard> },
}

pub(crate) struct TextureGuard {
    slot: u32,
    ctx: ID3D11DeviceContext,
    original: Option<ID3D11ShaderResourceView>,
    _replacement: ID3D11ShaderResourceView,
    eye: usize,
    id: String,
}

impl Drop for TextureGuard {
    fn drop(&mut self) {
        unsafe {
            self.ctx
                .PSSetShaderResources(self.slot, Some(std::slice::from_ref(&self.original)))
        };
        let mut bound = None;
        unsafe {
            self.ctx
                .PSGetShaderResources(self.slot, Some(std::slice::from_mut(&mut bound)))
        };
        if bound.as_ref().map(Interface::as_raw) != self.original.as_ref().map(Interface::as_raw) {
            record_water_treatment(
                self.eye,
                &self.id,
                Some("water reflection treatment: original view was not restored"),
            );
        } else {
            record_water_treatment(self.eye, &self.id, None);
        }
    }
}

pub(crate) unsafe fn reflection_treatment(context: *mut c_void) -> Option<ReflectionGuard> {
    let (eye, selected, tess_draws) = ACTIVE.with_borrow(|active| {
        active
            .as_ref()
            .map(|state| (state.eye, state.selected, state.tess_draws))
    })?;
    let treatment = water_reflection_requested()?;
    let id = diagnostic_capture_id()?;
    let Some(ctx) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }) else {
        record_water_treatment(
            eye,
            &id,
            Some("water reflection treatment: context unavailable"),
        );
        return None;
    };
    let bound = unsafe { work::bound_state(ctx) };
    if !bound.dsv_bound
        || selected.is_some_and(|index| {
            bound.scene != work::SceneKind::Tess || usize::from(index) != tess_draws
        })
    {
        return None;
    }
    let mut shader: Option<ID3D11PixelShader> = None;
    unsafe { ctx.PSGetShader(&mut shader, None, None) };
    let Some(shader) = shader else {
        record_water_treatment(
            eye,
            &id,
            Some("water reflection treatment: pixel shader is unbound"),
        );
        return None;
    };
    let target = if selected.is_some() {
        LAKE_PS
    } else {
        VERIFIED_WATER_PS
    };
    match treatment_identity(shader_identity::lookup(&shader), target) {
        TreatmentIdentity::Target => {}
        TreatmentIdentity::Unrelated => return None,
        TreatmentIdentity::Changed => {
            record_water_treatment(
                eye,
                &id,
                Some("water reflection treatment: target pixel shader was modified"),
            );
            return None;
        }
        TreatmentIdentity::Unknown => {
            record_water_treatment(
                eye,
                &id,
                Some("water reflection treatment: pixel shader identity is unavailable"),
            );
            return None;
        }
    }
    if treatment == WaterReflection::PristineLake {
        record_water_treatment(eye, &id, None);
        return Some(ReflectionGuard::Pristine);
    }
    if matches!(
        treatment,
        WaterReflection::PlanarUv
            | WaterReflection::LakeSamples
            | WaterReflection::LakeLighting
            | WaterReflection::LakeSky
            | WaterReflection::LakeSkyBuffer
            | WaterReflection::LakeResponse
            | WaterReflection::EyeWidthPlanar
            | WaterReflection::MirroredPlanar
            | WaterReflection::ProjectedPlanar
            | WaterReflection::CenterPlanar
            | WaterReflection::PackedRefraction
    ) {
        let packed = jc3vr_view::stereo::layout::collapse_active();
        if (!packed
            && !matches!(
                treatment,
                WaterReflection::PlanarUv
                    | WaterReflection::LakeSamples
                    | WaterReflection::LakeLighting
                    | WaterReflection::LakeSky
                    | WaterReflection::LakeSkyBuffer
                    | WaterReflection::LakeResponse
            ))
            || (packed && jc3vr_render::stereo::single_pass::per_eye_reissue_eye() != Some(eye))
        {
            return None;
        }
        return match LakeShaderGuard::bind(ctx, eye, shader, &id, treatment) {
            Ok(guard) => Some(ReflectionGuard::Planar {
                _guard: Box::new(guard),
            }),
            Err(err) => {
                record_water_treatment(eye, &id, Some(&format!("lake shader: {err:#}")));
                None
            }
        };
    }
    let (slot, format) = match treatment {
        WaterReflection::ZeroWeight => (12, DXGI_FORMAT_R32_FLOAT),
        WaterReflection::ZeroPlanar => (2, DXGI_FORMAT_R11G11B10_FLOAT),
        WaterReflection::PristineLake
        | WaterReflection::PlanarUv
        | WaterReflection::LakeSamples
        | WaterReflection::LakeLighting
        | WaterReflection::LakeSky
        | WaterReflection::LakeSkyBuffer
        | WaterReflection::LakeResponse
        | WaterReflection::LakeNearPatch
        | WaterReflection::LakeTessPatch
        | WaterReflection::LakePatchDraw
        | WaterReflection::LakeCameraControl
        | WaterReflection::LakePatchOwners
        | WaterReflection::LakeDomainCamera
        | WaterReflection::LakeHullCamera
        | WaterReflection::EyeWidthPlanar
        | WaterReflection::MirroredPlanar
        | WaterReflection::ProjectedPlanar
        | WaterReflection::CenterPlanar
        | WaterReflection::PackedRefraction => unreachable!(),
    };
    let mut original: Option<ID3D11ShaderResourceView> = None;
    unsafe { ctx.PSGetShaderResources(slot, Some(std::slice::from_mut(&mut original))) };
    let valid = original.as_ref().is_some_and(|view| {
        let mut desc = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
        unsafe { view.GetDesc(&mut desc) };
        desc.Format == format && desc.ViewDimension == D3D_SRV_DIMENSION_TEXTURE2D
    });
    if !valid {
        record_water_treatment(
            eye,
            &id,
            Some("water reflection treatment: unsupported or unbound target view"),
        );
        return None;
    }
    let zero = ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        let state = active
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("water draw scope disappeared"))?;
        if state.zero.is_none() {
            state.zero = Some(create_zero(ctx, format)?);
        }
        state
            .zero
            .clone()
            .ok_or_else(|| anyhow::anyhow!("zero view unavailable"))
    });
    let replacement = match zero {
        Ok(replacement) => replacement,
        Err(error) => {
            record_water_treatment(
                eye,
                &id,
                Some(&format!("water reflection treatment: {error:#}")),
            );
            return None;
        }
    };
    unsafe {
        ctx.PSSetShaderResources(slot, Some(std::slice::from_ref(&Some(replacement.clone()))))
    };
    let mut bound = None;
    unsafe { ctx.PSGetShaderResources(slot, Some(std::slice::from_mut(&mut bound))) };
    if bound.as_ref().map(Interface::as_raw) != Some(replacement.as_raw()) {
        unsafe { ctx.PSSetShaderResources(slot, Some(std::slice::from_ref(&original))) };
        let mut restored = None;
        unsafe { ctx.PSGetShaderResources(slot, Some(std::slice::from_mut(&mut restored))) };
        let error = if restored.as_ref().map(Interface::as_raw)
            != original.as_ref().map(Interface::as_raw)
        {
            "water reflection treatment: zero view was not bound and original view was not restored"
        } else {
            "water reflection treatment: zero view was not bound"
        };
        record_water_treatment(eye, &id, Some(error));
        return None;
    }
    Some(ReflectionGuard::Texture {
        _guard: TextureGuard {
            slot,
            ctx: ctx.clone(),
            original,
            _replacement: replacement,
            eye,
            id,
        },
    })
}

#[derive(Debug, PartialEq, Eq)]
enum TreatmentIdentity {
    Target,
    Unrelated,
    Changed,
    Unknown,
}

pub(crate) struct LakeShaderGuard {
    mvp: Option<LakeMvpGuard>,
    sky: Option<SkyViews>,
    targets: Option<LakeSampleTargets>,
    ctx: ID3D11DeviceContext,
    original: ID3D11PixelShader,
    eye: usize,
    id: String,
}

fn verify_inner_edge(ctx: &ID3D11DeviceContext, depth: bool) -> anyhow::Result<()> {
    let mut v = [D3D11_VIEWPORT::default(); 2];
    let mut count = 2;
    let mut sampler = [None];
    let mut view = [None];
    unsafe {
        ctx.RSGetViewports(&mut count, Some(v.as_mut_ptr()));
        ctx.PSGetShaderResources(if depth { 0 } else { 3 }, Some(&mut view));
        ctx.PSGetSamplers(if depth { 0 } else { 2 }, Some(&mut sampler));
    }
    let mut sd = D3D11_SAMPLER_DESC::default();
    let mut vd = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
    let mut td = D3D11_TEXTURE2D_DESC::default();
    let s = sampler[0]
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("lake edge: missing sampler"))?;
    let view = view[0]
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("lake edge: missing refraction texture"))?;
    unsafe {
        s.GetDesc(&mut sd);
        view.GetDesc(&mut vd);
        view.GetResource()?
            .cast::<ID3D11Texture2D>()?
            .GetDesc(&mut td);
    }
    anyhow::ensure!(
        count == 2
            && v[0] == v[1]
            && v[0].TopLeftX == 0.
            && v[0].TopLeftY == 0.
            && v[0].Width == 320.
            && v[0].Height == 360.
            && sd.Filter.0 == if depth { 0 } else { 20 }
            && [sd.AddressU.0, sd.AddressV.0, sd.AddressW.0] == [if depth { 3 } else { 2 }; 3]
            && vd.ViewDimension == D3D_SRV_DIMENSION_TEXTURE2D
            && vd.Format
                == if depth {
                    DXGI_FORMAT_R32_FLOAT
                } else {
                    DXGI_FORMAT_R11G11B10_FLOAT
                }
            && td.Width == 640
            && td.Height == 360
            && td.SampleDesc.Count == 1
            && unsafe {
                vd.Anonymous.Texture2D.MostDetailedMip == 0 && vd.Anonymous.Texture2D.MipLevels == 1
            },
        "lake edge: unexpected viewport, color view or sampler"
    );
    Ok(())
}

impl LakeShaderGuard {
    fn bind(
        ctx: &ID3D11DeviceContext,
        eye: usize,
        original: ID3D11PixelShader,
        id: &str,
        treatment: WaterReflection,
    ) -> anyhow::Result<Self> {
        let blob = shader_identity::lake_bytecode()
            .ok_or_else(|| anyhow::anyhow!("missing lake bytecode"))?;
        let displacement = treatment == WaterReflection::LakeResponse
            && frame_identity().is_some_and(lake_displacement_requested);
        let patched = match treatment {
            WaterReflection::LakeSamples
            | WaterReflection::LakeLighting
            | WaterReflection::LakeSky
            | WaterReflection::LakeSkyBuffer
            | WaterReflection::LakeResponse => {
                let source = if jc3vr_view::stereo::layout::collapse_active() {
                    dxbc_stereo::isolate_lake_reflection(
                        &dxbc_stereo::scale_lake_refraction(&blob)?,
                        eye == 1,
                    )?
                } else {
                    blob.clone()
                };
                if matches!(
                    treatment,
                    WaterReflection::LakeLighting
                        | WaterReflection::LakeSky
                        | WaterReflection::LakeSkyBuffer
                        | WaterReflection::LakeResponse
                ) {
                    if treatment == WaterReflection::LakeResponse {
                        if eye == 0 && frame_identity().and_then(lake_inner_edge) == Some(true) {
                            verify_inner_edge(ctx, false)?;
                            dxbc_stereo::trace_lake_inner_edge(&source)?
                        } else {
                            dxbc_stereo::trace_lake_response(&source)?
                        }
                    } else {
                        dxbc_stereo::trace_lake_lighting(&source)?
                    }
                } else {
                    dxbc_stereo::trace_lake_samples(&source)?
                }
            }
            WaterReflection::PlanarUv => {
                let source = if jc3vr_view::stereo::layout::collapse_active() {
                    dxbc_stereo::reproject_lake_reflection(
                        &dxbc_stereo::scale_lake_refraction(&blob)?,
                        eye == 1,
                    )?
                } else {
                    blob.clone()
                };
                dxbc_stereo::capture_lake_planar_uv(&source)?
            }
            WaterReflection::EyeWidthPlanar => dxbc_stereo::isolate_lake_reflection(
                &dxbc_stereo::scale_lake_refraction(&blob)?,
                eye == 1,
            )?,
            WaterReflection::MirroredPlanar => dxbc_stereo::mirror_lake_reflection(
                &dxbc_stereo::scale_lake_refraction(&blob)?,
                eye == 1,
            )?,
            WaterReflection::ProjectedPlanar => dxbc_stereo::reproject_lake_reflection(
                &dxbc_stereo::scale_lake_refraction(&blob)?,
                eye == 1,
            )?,
            WaterReflection::CenterPlanar => dxbc_stereo::offset_lake_reflection(&blob, eye == 1)?,
            WaterReflection::PackedRefraction => dxbc_stereo::scale_lake_refraction(&blob)?,
            _ => anyhow::bail!("lake shader: expected a shader treatment"),
        };
        let normals = displacement && frame_identity().is_some_and(lake_normals_requested);
        let waves = displacement && frame_identity().is_some_and(lake_waves_requested);
        let gradients = waves && frame_identity().is_some_and(lake_gradients_requested);
        let patched = if gradients {
            dxbc_stereo::trace_lake_gradients(&patched)?
        } else if waves {
            dxbc_stereo::trace_lake_waves(&patched)?
        } else if normals {
            dxbc_stereo::trace_lake_normals(&patched)?
        } else if displacement {
            dxbc_stereo::trace_lake_displacement(&patched)?
        } else {
            patched
        };
        let patched = if eye == 0
            && let Some(treated) =
                frame_identity().and_then(crate::screenshot::harness::lake_depth_edge)
        {
            verify_inner_edge(ctx, true)?;
            if treated {
                let depth = dxbc_stereo::bound_lake_candidate_depth(&patched)?;
                if frame_identity().and_then(crate::screenshot::harness::lake_color_mirror)
                    == Some(true)
                {
                    dxbc_stereo::mirror_lake_color_overflow(&depth)?
                } else {
                    depth
                }
            } else {
                patched
            }
        } else {
            patched
        };
        let _identity = unsafe { shader_identity::begin(blob.as_ptr(), blob.len() as u64) }
            .ok_or_else(|| anyhow::anyhow!("lake shader: census unavailable"))?;
        let mut shader = None;
        unsafe {
            ctx.GetDevice()?
                .CreatePixelShader(&patched, None, Some(&mut shader))?;
        }
        let shader = shader.ok_or_else(|| anyhow::anyhow!("missing offset shader"))?;
        let targets = matches!(
            treatment,
            WaterReflection::LakeSamples
                | WaterReflection::LakeLighting
                | WaterReflection::LakeSky
                | WaterReflection::LakeSkyBuffer
                | WaterReflection::LakeResponse
        )
        .then(|| {
            LakeSampleTargets::bind(
                ctx,
                treatment == WaterReflection::LakeResponse,
                matches!(
                    treatment,
                    WaterReflection::LakeLighting
                        | WaterReflection::LakeSky
                        | WaterReflection::LakeSkyBuffer
                        | WaterReflection::LakeResponse
                ),
                displacement,
                normals,
                waves,
                gradients,
            )
        })
        .transpose()?;
        let mvp = frame_identity()
            .and_then(lake_ds_mvp)
            .map(|treated| {
                anyhow::ensure!(
                    displacement
                        && eye < 2
                        && water_tess_draw() == Some(7)
                        && jc3vr_view::stereo::layout::collapse_active()
                        && jc3vr_render::stereo::single_pass::per_eye_reissue_eye() == Some(eye)
                        && jc3vr_render::stereo::single_pass::current_pass_id()
                            == jc3gi::graphics_engine::render_engine::RenderPassId::RP_WATER as u8,
                    "lake MVP: submission outside selected draw"
                );
                LakeMvpGuard::bind(ctx, eye, treated)
            })
            .transpose()?;
        let mut sky = None;
        if matches!(
            treatment,
            WaterReflection::LakeLighting
                | WaterReflection::LakeSky
                | WaterReflection::LakeSkyBuffer
                | WaterReflection::LakeResponse
        ) && (treatment != WaterReflection::LakeResponse
            || jc3vr_view::stereo::layout::collapse_active())
        {
            sky = SkyViews::prepare(ctx, eye, id, treatment)?;
        }
        unsafe { ctx.PSSetShader(&shader, None) };
        let mut bound = None;
        unsafe { ctx.PSGetShader(&mut bound, None, None) };
        if bound.as_ref() != Some(&shader) {
            unsafe { ctx.PSSetShader(&original, None) };
            anyhow::bail!("offset shader not bound");
        }
        tracing::info!(target:"water_probe", "lake shader treatment={treatment:?} id={id} eye={eye} original={:#x} actual={:#x}",original.as_raw() as usize,shader.as_raw() as usize);
        Ok(Self {
            ctx: ctx.clone(),
            original,
            eye,
            targets,
            mvp,
            sky,
            id: id.to_owned(),
        })
    }
}

impl Drop for LakeShaderGuard {
    fn drop(&mut self) {
        unsafe { self.ctx.PSSetShader(&self.original, None) };
        let mut bound = None;
        unsafe { self.ctx.PSGetShader(&mut bound, None, None) };
        if let Some(mvp) = self.mvp.take()
            && !mvp.restore()
        {
            record_water_treatment(
                self.eye,
                &self.id,
                Some("lake MVP: original binding not restored"),
            );
        }
        if let Some(mut sky) = self.sky.take()
            && !sky.restore()
        {
            record_water_treatment(
                self.eye,
                &self.id,
                Some("sky lighting: bindings not restored"),
            );
            return;
        }
        if let Some(mut targets) = self.targets.take()
            && let Err(error) = targets.finish(self.eye, &self.id)
        {
            record_water_treatment(
                self.eye,
                &self.id,
                Some(&format!("lake samples: {error:#}")),
            );
            return;
        }
        record_water_treatment(
            self.eye,
            &self.id,
            (bound.as_ref() != Some(&self.original))
                .then_some("lake shader: original shader not restored"),
        );
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LightBuffer {
    buffer: Option<windows::Win32::Graphics::Direct3D11::ID3D11Buffer>,
    first: u32,
    count: u32,
}
impl LightBuffer {
    fn current(ctx: &windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext1) -> Self {
        let mut b = [None];
        let (mut first, mut count) = (0, 0);
        unsafe {
            ctx.PSGetConstantBuffers1(
                3,
                1,
                Some(b.as_mut_ptr()),
                Some(&mut first),
                Some(&mut count),
            )
        };
        Self {
            buffer: b[0].take(),
            first,
            count,
        }
    }
    fn bind(&self, ctx: &windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext1) {
        if self.buffer.is_some() {
            unsafe {
                ctx.PSSetConstantBuffers1(
                    3,
                    1,
                    Some(&self.buffer),
                    Some(&self.first),
                    Some(&self.count),
                )
            }
        } else {
            unsafe { ctx.PSSetConstantBuffers(3, Some(&[None])) }
        }
    }
}
type SkyTriplet = [Option<ID3D11ShaderResourceView>; 3];
thread_local! { static SKY_SOURCE: RefCell<Option<(String, i64, SkyTriplet, LightBuffer)>> = const { RefCell::new(None) }; }
struct SkyViews {
    ctx: ID3D11DeviceContext,
    original: SkyTriplet,
    light: Option<(
        windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext1,
        LightBuffer,
    )>,
    armed: bool,
}
impl SkyViews {
    fn current(ctx: &ID3D11DeviceContext) -> SkyTriplet {
        let mut views = [const { None }; 3];
        unsafe { ctx.PSGetShaderResources(22, Some(&mut views)) };
        views
    }
    fn prepare(
        ctx: &ID3D11DeviceContext,
        eye: usize,
        id: &str,
        treatment: WaterReflection,
    ) -> anyhow::Result<Option<Self>> {
        let ctx1 = ctx.cast::<windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext1>()?;
        let light = LightBuffer::current(&ctx1);
        let views = Self::current(ctx);
        let ptrs = views
            .each_ref()
            .map(|v| v.as_ref().map_or(0, |v| v.as_raw() as usize));
        let frame = crate::screenshot::harness::frame_identity()
            .ok_or_else(|| anyhow::anyhow!("sky lighting: missing frame"))?;
        tracing::info!(target:"water_probe", "lake sky id={id} frame={frame} eye={eye} treatment={treatment:?} views={ptrs:?} cb3={light:?}");
        if !matches!(
            treatment,
            WaterReflection::LakeSky
                | WaterReflection::LakeSkyBuffer
                | WaterReflection::LakeResponse
        ) {
            return Ok(None);
        }
        if eye == 0 {
            SKY_SOURCE.with_borrow_mut(|s| *s = None);
            for view in &views {
                let v = view
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("sky lighting: missing source view"))?;
                let mut desc = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
                unsafe { v.GetDesc(&mut desc) };
                anyhow::ensure!(
                    desc.ViewDimension
                        == windows::Win32::Graphics::Direct3D::D3D_SRV_DIMENSION_TEXTURE1D,
                    "sky lighting: unexpected view dimension"
                );
            }
            SKY_SOURCE.with_borrow_mut(|s| *s = Some((id.to_owned(), frame, views, light)));
            return Ok(None);
        }
        let (saved_id, saved_frame, source, source_light) = SKY_SOURCE
            .take()
            .ok_or_else(|| anyhow::anyhow!("sky lighting: no matching first draw"))?;
        anyhow::ensure!(
            id == saved_id && frame == saved_frame && views.iter().all(Option::is_none),
            "sky lighting: right inputs not all missing or unmatched frame"
        );
        let restore_light = matches!(
            treatment,
            WaterReflection::LakeSkyBuffer | WaterReflection::LakeResponse
        );
        if restore_light {
            let buffer = source_light
                .buffer
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("sky lighting: source CB3 unbound"))?;
            let mut d = windows::Win32::Graphics::Direct3D11::D3D11_BUFFER_DESC::default();
            unsafe { buffer.GetDesc(&mut d) };
            anyhow::ensure!(
                light.buffer.is_none()
                    && source_light.count >= 2332
                    && u64::from(d.ByteWidth) >= 16 * (u64::from(source_light.first) + 2332),
                "sky lighting: CB3 not missing or source range does not contain saturation"
            );
        }
        let guard = Self {
            ctx: ctx.clone(),
            original: views,
            light: restore_light.then(|| (ctx1.clone(), light)),
            armed: true,
        };
        if restore_light {
            source_light.bind(&ctx1);
            anyhow::ensure!(
                LightBuffer::current(&ctx1) == source_light,
                "sky lighting: CB3 range not bound"
            );
        }
        unsafe { ctx.PSSetShaderResources(22, Some(&source)) };
        anyhow::ensure!(
            Self::current(ctx) == source,
            "sky lighting: triplet not bound"
        );
        Ok(Some(guard))
    }
    fn restore(&mut self) -> bool {
        if self.armed {
            if let Some((ctx, light)) = &self.light {
                light.bind(ctx);
            }
            unsafe { self.ctx.PSSetShaderResources(22, Some(&self.original)) };
            self.armed = false;
        }
        Self::current(&self.ctx) == self.original
            && self
                .light
                .as_ref()
                .is_none_or(|(ctx, light)| LightBuffer::current(ctx) == *light)
    }
}
impl Drop for SkyViews {
    fn drop(&mut self) {
        if self.armed {
            self.restore();
        }
    }
}

fn treatment_identity(identity: IdentityResult, target: &str) -> TreatmentIdentity {
    match identity {
        IdentityResult::Mapped { identity } if identity.pristine_sha256 != target => {
            TreatmentIdentity::Unrelated
        }
        IdentityResult::Mapped { identity } if identity.actual_sha256 == target => {
            TreatmentIdentity::Target
        }
        IdentityResult::Mapped { .. } => TreatmentIdentity::Changed,
        IdentityResult::Unarmed
        | IdentityResult::Unmapped
        | IdentityResult::Incomplete
        | IdentityResult::InstallFailed => TreatmentIdentity::Unknown,
    }
}

fn create_zero(
    ctx: &ID3D11DeviceContext,
    format: DXGI_FORMAT,
) -> anyhow::Result<ID3D11ShaderResourceView> {
    let device = unsafe { ctx.GetDevice()? };
    let zero = 0.0f32;
    let desc = D3D11_TEXTURE2D_DESC {
        Width: 1,
        Height: 1,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_IMMUTABLE,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let initial = D3D11_SUBRESOURCE_DATA {
        pSysMem: (&zero as *const f32).cast(),
        SysMemPitch: 4,
        SysMemSlicePitch: 4,
    };
    let mut texture: Option<ID3D11Texture2D> = None;
    unsafe { device.CreateTexture2D(&desc, Some(&initial), Some(&mut texture))? };
    let texture =
        texture.ok_or_else(|| anyhow::anyhow!("creating zero texture returned no object"))?;
    let mut view = None;
    unsafe { device.CreateShaderResourceView(&texture, None, Some(&mut view))? };
    view.ok_or_else(|| anyhow::anyhow!("creating zero shader resource view returned no object"))
}

pub(super) unsafe fn on_raw_draw(context: *mut c_void) {
    let draw = ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        let state = active.as_mut()?;
        let index = state.draws;
        state.draws += 1;
        Some((state.eye, index))
    });
    let Some((eye, index)) = draw else { return };
    let Some(ctx) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }) else {
        return;
    };
    let bound = unsafe { work::bound_state(ctx) };
    if !bound.dsv_bound {
        return;
    }
    let Some(id) = diagnostic_capture_id() else {
        return;
    };
    let tess_index = ACTIVE.with_borrow_mut(|active| {
        let state = active.as_mut()?;
        let tess_index = (bound.scene == work::SceneKind::Tess).then(|| {
            let n = state.tess_draws;
            state.tess_draws += 1;
            n
        });
        (!state
            .selected
            .is_some_and(|n| Some(usize::from(n)) != tess_index))
        .then_some(tess_index)
    });
    if let Some(tess_index) = tess_index {
        capture_water_inputs(ctx, eye, &id, index, tess_index);
    }
}

#[cfg(test)]
mod tests {
    use jc3vr_render::stereo::single_pass::shader_identity::Identity;

    use super::*;

    fn mapped(pristine: &str, actual: &str) -> IdentityResult {
        IdentityResult::Mapped {
            identity: Identity {
                pristine_sha256: pristine.to_owned(),
                actual_sha256: actual.to_owned(),
            },
        }
    }

    #[test]
    fn treatment_targets_only_the_verified_unmodified_shader() {
        assert_eq!(
            treatment_identity(
                mapped(VERIFIED_WATER_PS, VERIFIED_WATER_PS),
                VERIFIED_WATER_PS
            ),
            TreatmentIdentity::Target
        );
        assert_eq!(
            treatment_identity(mapped("unrelated", "unrelated"), VERIFIED_WATER_PS),
            TreatmentIdentity::Unrelated
        );
        assert_eq!(
            treatment_identity(mapped("unrelated", VERIFIED_WATER_PS), VERIFIED_WATER_PS),
            TreatmentIdentity::Unrelated
        );
        assert_eq!(
            treatment_identity(mapped(VERIFIED_WATER_PS, "modified"), VERIFIED_WATER_PS),
            TreatmentIdentity::Changed
        );
        for identity in [
            IdentityResult::Unarmed,
            IdentityResult::Unmapped,
            IdentityResult::Incomplete,
            IdentityResult::InstallFailed,
        ] {
            assert_eq!(
                treatment_identity(identity, VERIFIED_WATER_PS),
                TreatmentIdentity::Unknown
            );
        }
    }
}
