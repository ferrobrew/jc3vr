use anyhow::{Context as _, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use windows::{
    Win32::Graphics::Direct3D11::{
        D3D11_BIND_CONSTANT_BUFFER, D3D11_BUFFER_DESC, D3D11_SUBRESOURCE_DATA,
        D3D11_USAGE_IMMUTABLE, ID3D11Buffer, ID3D11DeviceContext, ID3D11DeviceContext1,
    },
    core::Interface,
};

use crate::{
    screenshot::harness::{
        CascadeScale,
        resolve_probe::{BufferContent, Observation, STATE, copy_content},
    },
    stereo::single_pass::shader_identity::IdentityResult,
};

const ROWS: u32 = 95;
const BYTE_WIDTH: u32 = ROWS * 16;
const SCALE_X: usize = 54 * 16;
const PRISTINE_PS: &str = "06f43db0887b0038147e273a3879a9b7f48c3b630955a94d695898fd7d0e52fc";
const BOUND_PS: &str = "6a82533e6458166ea3d04789c266ec127baf59931882955c99c4603e47f11c9b";

#[derive(Clone, Serialize)]
pub(super) struct CascadeTrial {
    #[serde(skip_serializing_if = "Option::is_none")]
    eye_camera: Option<[u32; 3]>,
    original: u32,
    applied: u32,
    bound_sha256: String,
    restored: bool,
}

pub(crate) struct CascadeGuard {
    ctx: ID3D11DeviceContext1,
    original: ID3D11Buffer,
    eye: usize,
    frame: i64,
}

pub(super) enum Change {
    Cascade(CascadeScale),
    Camera([u32; 3], bool),
}

impl Drop for CascadeGuard {
    fn drop(&mut self) {
        unsafe {
            self.ctx
                .PSSetConstantBuffers(0, Some(&[Some(self.original.clone())]))
        };
        let (bound, first, count) = binding(&self.ctx);
        let restored = bound.as_ref() == Some(&self.original) && first == 0 && count == ROWS;
        let mut s = STATE.lock();
        if s.frame == Some(self.frame)
            && let Some(t) = s.observations[self.eye]
                .as_mut()
                .and_then(|o| o.cascade.as_mut().or(o.camera.as_mut()))
        {
            t.restored = restored;
        }
    }
}

fn binding(ctx: &ID3D11DeviceContext1) -> (Option<ID3D11Buffer>, u32, u32) {
    let mut bound = None;
    let mut first = 0;
    let mut count = 0;
    unsafe {
        ctx.PSGetConstantBuffers1(0, 1, Some(&mut bound), Some(&mut first), Some(&mut count))
    };
    (bound, first, count)
}

pub(super) fn bind(
    ctx: &ID3D11DeviceContext,
    obs: &Observation,
    selection: Change,
    eye: usize,
    frame: i64,
) -> anyhow::Result<(CascadeGuard, CascadeTrial)> {
    ensure!(
        matches!(&obs.pixel_shader.identity, Some(IdentityResult::Mapped { identity }) if identity.pristine_sha256 == PRISTINE_PS && identity.actual_sha256 == BOUND_PS),
        "resolve shader mismatch"
    );
    let ctx1 = ctx.cast::<ID3D11DeviceContext1>()?;
    let (original, first, count) = binding(&ctx1);
    let original = original.context("missing cb0")?;
    let buffer = obs
        .constant_buffers
        .first()
        .filter(|cb| cb.slot == 0)
        .context("cascade: missing slot zero")?;
    ensure!(
        first == 0
            && count == ROWS
            && buffer.byte_width == BYTE_WIDTH
            && buffer.pointer == original.as_raw() as usize,
        "not the full captured cb0"
    );
    let Some(BufferContent::Captured { bytes, .. }) = &buffer.content else {
        anyhow::bail!("missing cb0 content")
    };
    ensure!(bytes.len() == BYTE_WIDTH as usize, "cb0 length");
    let mut patched = bytes.clone();
    let (start, applied, eye_camera) = match selection {
        Change::Cascade(s) => (SCALE_X, s.bits(), None),
        Change::Camera(xyz, treated) => {
            let start = 4 * 16;
            ensure!(
                xyz.iter().all(|b| f32::from_bits(*b).is_finite()),
                "resolve camera: nonfinite eye"
            );
            ensure!(
                xyz.into_iter()
                    .flat_map(u32::to_le_bytes)
                    .ne(bytes[start..start + 12].iter().copied()),
                "resolve camera: original already physical"
            );
            if treated {
                for (dest, word) in patched[start..start + 12]
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(xyz)
                {
                    dest.copy_from_slice(&word.to_le_bytes());
                }
            }
            (
                start,
                u32::from_le_bytes(patched[start..start + 4].try_into()?),
                Some(xyz),
            )
        }
    };
    let previous = u32::from_le_bytes(bytes[start..start + 4].try_into()?);
    patched[start..start + 4].copy_from_slice(&applied.to_le_bytes());
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: BYTE_WIDTH,
        Usage: D3D11_USAGE_IMMUTABLE,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        ..Default::default()
    };
    let data = D3D11_SUBRESOURCE_DATA {
        pSysMem: patched.as_ptr().cast(),
        ..Default::default()
    };
    let mut replacement = None;
    unsafe {
        ctx.GetDevice()?
            .CreateBuffer(&desc, Some(&data), Some(&mut replacement))?
    };
    let replacement = replacement.context("missing replacement cb0")?;
    unsafe { ctx.PSSetConstantBuffers(0, Some(&[Some(replacement.clone())])) };
    let verification = (|| {
        let (bound, f, n) = binding(&ctx1);
        ensure!(
            bound.as_ref() == Some(&replacement) && f == 0 && n == ROWS,
            "replacement binding mismatch"
        );
        let bytes = copy_content(ctx, &replacement, BYTE_WIDTH, 0..BYTE_WIDTH as usize)?;
        ensure!(bytes == patched, "replacement content mismatch");
        Ok::<_, anyhow::Error>(format!("{:x}", Sha256::digest(bytes)))
    })();
    let digest = match verification {
        Ok(digest) => digest,
        Err(error) => {
            unsafe { ctx.PSSetConstantBuffers(0, Some(&[Some(original.clone())])) };
            ensure!(
                binding(&ctx1) == (Some(original), 0, ROWS),
                "cascade: binding verification and restoration failed: {error}"
            );
            return Err(error);
        }
    };
    Ok((
        CascadeGuard {
            ctx: ctx1,
            original,
            eye,
            frame,
        },
        CascadeTrial {
            original: previous,
            applied,
            eye_camera,
            bound_sha256: digest,
            restored: false,
        },
    ))
}
