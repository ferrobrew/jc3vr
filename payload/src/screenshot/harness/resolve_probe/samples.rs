use anyhow::{Context as _, ensure};
use windows::Win32::Graphics::Direct3D11::{ID3D11DeviceContext, ID3D11PixelShader};

use crate::{
    screenshot::harness::{resolve_probe::Observation, stages::LakeSampleTargets},
    stereo::single_pass::shader_identity::{self, IdentityResult},
};

pub(crate) struct Guard {
    ctx: ID3D11DeviceContext,
    original: ID3D11PixelShader,
    targets: LakeSampleTargets,
    frame: i64,
    eye: usize,
}

impl Guard {
    pub(super) fn bind(
        ctx: &ID3D11DeviceContext,
        obs: &Observation,
        frame: i64,
        eye: usize,
    ) -> anyhow::Result<Self> {
        ensure!(
            matches!(&obs.pixel_shader.identity, Some(IdentityResult::Mapped{identity}) if identity.pristine_sha256 == shader_identity::RESOLVE_PRISTINE && [shader_identity::RESOLVE_ACTUAL, shader_identity::RESOLVE_EYE_CAMERA, shader_identity::RESOLVE_EYE_LIGHTS].contains(&identity.actual_sha256.as_str())),
            "resolve samples: unexpected shader"
        );
        let bytes =
            shader_identity::resolve_bytecode().context("resolve samples: missing executable")?;
        let shadow = crate::screenshot::harness::resolve_shadow_requested(frame);
        let traced = dxbc_stereo::trace_deferred_samples(&bytes, shadow)?;
        let mut shader = None;
        unsafe {
            ctx.GetDevice()?
                .CreatePixelShader(&traced, None, Some(&mut shader))?;
        }
        let shader = shader.context("resolve samples: missing shader")?;
        let mut original = None;
        let mut count = 0;
        unsafe { ctx.PSGetShader(&mut original, None, Some(&mut count)) };
        ensure!(count == 0, "resolve samples: dynamic shader linkage");
        let original = original.context("resolve samples: missing original")?;
        let targets = LakeSampleTargets::bind_resolve(ctx, shadow)?;
        unsafe { ctx.PSSetShader(&shader, None) };
        let mut bound = None;
        unsafe { ctx.PSGetShader(&mut bound, None, None) };
        if bound.as_ref() != Some(&shader) {
            unsafe { ctx.PSSetShader(&original, None) };
            anyhow::bail!("resolve samples: shader not bound");
        }
        Ok(Self {
            ctx: ctx.clone(),
            original,
            targets,
            frame,
            eye,
        })
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        unsafe { self.ctx.PSSetShader(&self.original, None) };
        let mut bound = None;
        unsafe { self.ctx.PSGetShader(&mut bound, None, None) };
        if bound.as_ref() != Some(&self.original) {
            tracing::warn!("resolve samples: shader not restored");
            return;
        }
        if let Err(e) = self.targets.finish_resolve(self.frame, self.eye) {
            tracing::warn!("resolve samples: {e:#}");
        }
    }
}
