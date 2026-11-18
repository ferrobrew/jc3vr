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
        diagnostic_capture_id,
        stages::{capture_water_inputs, record_water_treatment},
        water_inputs_requested, water_reflection_zero_weight_requested,
    },
    stereo::single_pass::{
        shader_identity::{self, IdentityResult},
        water_gpu_probe::work,
    },
};

thread_local! {
    static ACTIVE: RefCell<Option<DrawState>> = const { RefCell::new(None) };
}

struct DrawState {
    eye: usize,
    draws: usize,
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

pub(crate) struct ReflectionGuard {
    ctx: ID3D11DeviceContext,
    original: Option<ID3D11ShaderResourceView>,
    _replacement: ID3D11ShaderResourceView,
    eye: usize,
    id: String,
}

impl Drop for ReflectionGuard {
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
    if !water_reflection_zero_weight_requested() {
        return None;
    }
    let eye = ACTIVE.with(|active| active.borrow().as_ref().map(|state| state.eye))?;
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
    if !bound.dsv_bound {
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
    match treatment_identity(shader_identity::lookup(&shader)) {
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
    Some(ReflectionGuard {
        ctx: ctx.clone(),
        original,
        _replacement: replacement,
        eye,
        id,
    })
}

#[derive(Debug, PartialEq, Eq)]
enum TreatmentIdentity {
    Target,
    Unrelated,
    Changed,
    Unknown,
}

fn treatment_identity(identity: IdentityResult) -> TreatmentIdentity {
    match identity {
        IdentityResult::Mapped { identity } if identity.pristine_sha256 != VERIFIED_WATER_PS => {
            TreatmentIdentity::Unrelated
        }
        IdentityResult::Mapped { identity } if identity.actual_sha256 == VERIFIED_WATER_PS => {
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
    capture_water_inputs(ctx, eye, &id, index, bound.scene == work::SceneKind::Tess);
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
            treatment_identity(mapped(VERIFIED_WATER_PS, VERIFIED_WATER_PS)),
            TreatmentIdentity::Target
        );
        assert_eq!(
            treatment_identity(mapped("unrelated", "unrelated")),
            TreatmentIdentity::Unrelated
        );
        assert_eq!(
            treatment_identity(mapped("unrelated", VERIFIED_WATER_PS)),
            TreatmentIdentity::Unrelated
        );
        assert_eq!(
            treatment_identity(mapped(VERIFIED_WATER_PS, "modified")),
            TreatmentIdentity::Changed
        );
        for identity in [
            IdentityResult::Unarmed,
            IdentityResult::Unmapped,
            IdentityResult::Incomplete,
            IdentityResult::InstallFailed,
        ] {
            assert_eq!(treatment_identity(identity), TreatmentIdentity::Unknown);
        }
    }
}
