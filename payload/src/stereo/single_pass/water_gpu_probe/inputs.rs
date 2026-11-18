use std::{cell::RefCell, ffi::c_void, marker::PhantomData, rc::Rc};

use windows::{
    Win32::Graphics::{
        Direct3D::D3D_SRV_DIMENSION_TEXTURE2D,
        Direct3D11::{
            D3D11_BIND_SHADER_RESOURCE, D3D11_SHADER_RESOURCE_VIEW_DESC, D3D11_SUBRESOURCE_DATA,
            D3D11_TEXTURE2D_DESC, D3D11_USAGE_IMMUTABLE, ID3D11DeviceContext, ID3D11PixelShader,
            ID3D11ShaderResourceView, ID3D11Texture2D,
        },
        Dxgi::Common::{DXGI_FORMAT_R32_FLOAT, DXGI_SAMPLE_DESC},
    },
    core::Interface,
};

use crate::{
    screenshot::harness::{
        WaterReflection, diagnostic_capture_id,
        stages::{capture_water_inputs, record_water_treatment},
        water_inputs_requested, water_reflection_requested, water_tess_draw,
    },
    stereo::single_pass::{
        shader_identity::{self, IdentityResult, LAKE_PS},
        water_gpu_probe::work,
    },
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
    Weight { _guard: WeightGuard },
    Planar { _guard: PlanarGuard },
}

pub(crate) struct WeightGuard {
    ctx: ID3D11DeviceContext,
    original: Option<ID3D11ShaderResourceView>,
    _replacement: ID3D11ShaderResourceView,
    eye: usize,
    id: String,
}

impl Drop for WeightGuard {
    fn drop(&mut self) {
        unsafe {
            self.ctx
                .PSSetShaderResources(12, Some(std::slice::from_ref(&self.original)))
        };
        let mut bound = None;
        unsafe {
            self.ctx
                .PSGetShaderResources(12, Some(std::slice::from_mut(&mut bound)))
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
    if treatment == WaterReflection::CenterPlanar {
        if !crate::stereo::single_pass::collapse_active()
            || crate::stereo::single_pass::per_eye_reissue_eye() != Some(eye)
        {
            return None;
        }
        return match PlanarGuard::bind(ctx, eye, shader, &id) {
            Ok(guard) => Some(ReflectionGuard::Planar { _guard: guard }),
            Err(err) => {
                record_water_treatment(eye, &id, Some(&format!("lake shader: {err:#}")));
                None
            }
        };
    }
    let mut original: Option<ID3D11ShaderResourceView> = None;
    unsafe { ctx.PSGetShaderResources(12, Some(std::slice::from_mut(&mut original))) };
    let valid = original.as_ref().is_some_and(|view| {
        let mut desc = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
        unsafe { view.GetDesc(&mut desc) };
        desc.Format == DXGI_FORMAT_R32_FLOAT && desc.ViewDimension == D3D_SRV_DIMENSION_TEXTURE2D
    });
    if !valid {
        record_water_treatment(
            eye,
            &id,
            Some("water reflection treatment: unsupported or unbound alpha view"),
        );
        return None;
    }
    let zero = ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        let state = active
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("water draw scope disappeared"))?;
        if state.zero.is_none() {
            state.zero = Some(create_zero(ctx)?);
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
    unsafe { ctx.PSSetShaderResources(12, Some(std::slice::from_ref(&Some(replacement.clone())))) };
    let mut bound = None;
    unsafe { ctx.PSGetShaderResources(12, Some(std::slice::from_mut(&mut bound))) };
    if bound.as_ref().map(Interface::as_raw) != Some(replacement.as_raw()) {
        unsafe { ctx.PSSetShaderResources(12, Some(std::slice::from_ref(&original))) };
        let mut restored = None;
        unsafe { ctx.PSGetShaderResources(12, Some(std::slice::from_mut(&mut restored))) };
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
    Some(ReflectionGuard::Weight {
        _guard: WeightGuard {
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

pub(crate) struct PlanarGuard {
    ctx: ID3D11DeviceContext,
    original: ID3D11PixelShader,
    eye: usize,
    id: String,
}

impl PlanarGuard {
    fn bind(
        ctx: &ID3D11DeviceContext,
        eye: usize,
        original: ID3D11PixelShader,
        id: &str,
    ) -> anyhow::Result<Self> {
        let blob = shader_identity::lake_bytecode()
            .ok_or_else(|| anyhow::anyhow!("missing lake bytecode"))?;
        let patched = dxbc_stereo::offset_lake_reflection(&blob, eye == 1)?;
        let _identity = unsafe { shader_identity::begin(blob.as_ptr(), blob.len() as u64) }
            .ok_or_else(|| anyhow::anyhow!("lake shader: census unavailable"))?;
        let mut shader = None;
        unsafe {
            ctx.GetDevice()?
                .CreatePixelShader(&patched, None, Some(&mut shader))?;
        }
        let shader = shader.ok_or_else(|| anyhow::anyhow!("missing offset shader"))?;
        unsafe { ctx.PSSetShader(&shader, None) };
        let mut bound = None;
        unsafe { ctx.PSGetShader(&mut bound, None, None) };
        if bound.as_ref() != Some(&shader) {
            unsafe { ctx.PSSetShader(&original, None) };
            anyhow::bail!("offset shader not bound");
        }
        tracing::info!(target:"water_probe", "lake planar treatment id={id} eye={eye} original={:#x} actual={:#x}",original.as_raw() as usize,shader.as_raw() as usize);
        Ok(Self {
            ctx: ctx.clone(),
            original,
            eye,
            id: id.to_owned(),
        })
    }
}

impl Drop for PlanarGuard {
    fn drop(&mut self) {
        unsafe { self.ctx.PSSetShader(&self.original, None) };
        let mut bound = None;
        unsafe { self.ctx.PSGetShader(&mut bound, None, None) };
        record_water_treatment(
            self.eye,
            &self.id,
            (bound.as_ref() != Some(&self.original))
                .then_some("lake shader: original shader not restored"),
        );
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

fn create_zero(ctx: &ID3D11DeviceContext) -> anyhow::Result<ID3D11ShaderResourceView> {
    let device = unsafe { ctx.GetDevice()? };
    let zero = 0.0f32;
    let desc = D3D11_TEXTURE2D_DESC {
        Width: 1,
        Height: 1,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_R32_FLOAT,
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
    use super::*;
    use crate::stereo::single_pass::shader_identity::Identity;

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
