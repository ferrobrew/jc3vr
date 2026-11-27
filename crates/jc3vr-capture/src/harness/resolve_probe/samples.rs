use anyhow::{Context as _, ensure};
use jc3vr_render::stereo::single_pass::shader_identity::IdentityResult;
use windows::Win32::Graphics::Direct3D11::{ID3D11DeviceContext, ID3D11PixelShader};

use crate::harness::{
    kept_shaders, resolve_probe::Observation, rewrites, stages::LakeSampleTargets,
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
            matches!(&obs.pixel_shader.identity, Some(IdentityResult::Mapped{identity}) if identity.pristine_sha256 == kept_shaders::RESOLVE_PRISTINE && [kept_shaders::RESOLVE_ACTUAL, kept_shaders::RESOLVE_EYE_CAMERA, kept_shaders::RESOLVE_EYE_LIGHTS].contains(&identity.actual_sha256.as_str())),
            "resolve samples: unexpected shader"
        );
        let bytes =
            kept_shaders::resolve_bytecode().context("resolve samples: missing executable")?;
        let shadow = crate::harness::resolve_shadow_requested(frame);
        let traced = rewrites::trace_deferred_samples(&bytes, shadow)?;
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
