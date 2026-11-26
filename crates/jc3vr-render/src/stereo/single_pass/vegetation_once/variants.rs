//! The reprojected vertex-shader variants a single-drawn bark or foliage block binds in place of the
//! engine's own.
//!
//! The engine's vegetation shaders stay pristine: the whole-block re-issue, the shadow and
//! reflective-shadow passes, and every pass outside the eye split draw with them. Each eye-pass
//! permutation instead gets a variant, built from its bytecode when the engine creates it and hung
//! off the engine's shader as private data, so the draw path finds it with one lookup on whatever
//! shader the block's setup bound. A permutation that reads `SV_InstanceID` (the GPU-indirect ones,
//! which fetch each instance from a structured buffer by it) takes
//! [`dxbc_stereo::reproject_instanced_vertex_shader`], which halves the id; the others take
//! [`dxbc_stereo::reproject_vertex_shader`].
//!
//! The bytecode is decided by name in the `CreateVertexProgram` hook, the only place the name exists,
//! and remembered against the pristine blob, so the shader created from it -- through that hook or
//! through the D3D-level re-create path, which skips it -- gets its variant in the
//! `CreateVertexShader` detour.

use std::{
    collections::BTreeMap,
    ffi::c_void,
    mem::size_of,
    sync::atomic::{AtomicUsize, Ordering},
};

use anyhow::Context as _;
use dxbc_stereo::DxbcError;
use parking_lot::Mutex;
use windows::{
    Win32::Graphics::Direct3D11::ID3D11VertexShader,
    core::{GUID, IUnknown, Interface},
};

use crate::stereo::single_pass::shader_policy::vs_blob_key;

/// Decide and remember the variant of a vertex shader the engine is about to create from `code`.
/// Returns whether one was remembered, so the caller can make sure the `CreateVertexShader` detour
/// that attaches it is installed.
pub(crate) fn remember(name: Option<&str>, code: &[u8]) -> bool {
    if !is_eye_pass_vegetation(name) {
        return false;
    }
    let variant = match dxbc_stereo::reproject_vertex_shader(code) {
        Err(DxbcError::InstanceIdAlreadyDeclared) => {
            dxbc_stereo::reproject_instanced_vertex_shader(code)
        }
        other => other,
    };
    match variant {
        Ok(bytes) => {
            BLOBS.lock().insert(vs_blob_key(code), bytes);
            true
        }
        Err(error) => {
            FAILED.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(
                target: "single_pass",
                name = ?name,
                "vegetation once: no variant for this vertex shader ({error}); its blocks keep the \
                 per-eye re-issue"
            );
            false
        }
    }
}

/// Create and attach the variant of a vertex shader just created from `code`, if one was remembered
/// for that bytecode. `create` creates a vertex shader from bytecode without passing through the
/// mod's own creation detour.
pub(crate) fn attach(
    shader: *mut c_void,
    code: &[u8],
    create: impl FnOnce(&[u8]) -> Option<*mut c_void>,
) -> anyhow::Result<()> {
    let Some(bytes) = BLOBS.lock().get(&vs_blob_key(code)).cloned() else {
        return Ok(());
    };
    // SAFETY: `shader` is the live vertex shader the engine's creation just returned; borrowing it
    // takes no reference.
    let original = unsafe { ID3D11VertexShader::from_raw_borrowed(&shader) }
        .context("vegetation once: the created vertex shader is null")?;
    let raw = create(&bytes).context("vegetation once: creating the variant failed")?;
    // SAFETY: `raw` is the variant just created, whose one reference this takes over.
    let variant = unsafe { ID3D11VertexShader::from_raw(raw) };
    // SAFETY: both are live; private data keeps its own reference to the variant.
    unsafe { original.SetPrivateDataInterface(&VARIANT, &variant) }
        .context("vegetation once: attaching the variant")?;
    ATTACHED.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// The variant attached to `shader`, if it has one.
pub(super) fn variant(shader: &ID3D11VertexShader) -> Option<ID3D11VertexShader> {
    let mut value: Option<IUnknown> = None;
    let mut size = size_of::<Option<IUnknown>>() as u32;
    // SAFETY: the buffer is one interface pointer, which is what an interface was stored as.
    unsafe { shader.GetPrivateData(&VARIANT, &mut size, Some((&raw mut value).cast())) }.ok()?;
    value?.cast().ok()
}

/// Forget the remembered variant bytecode, on a shader reload: the reload re-decides every shader.
pub(crate) fn reset() {
    BLOBS.lock().clear();
    ATTACHED.store(0, Ordering::Relaxed);
    FAILED.store(0, Ordering::Relaxed);
}

/// The variants attached since the last reset, and the eye-pass permutations that got none.
pub(super) fn counts() -> (usize, usize) {
    (
        ATTACHED.load(Ordering::Relaxed),
        FAILED.load(Ordering::Relaxed),
    )
}

/// The private-data key the variant hangs off the engine's shader under.
const VARIANT: GUID = GUID::from_u128(0x5b2e9d41_63a7_4c0f_8e15_9d0a6c2f7b31);

/// The bark and foliage permutations their blocks draw with in the eye passes. The shadow and
/// reflective-shadow permutations draw from a light's view, outside the eye split, and the billboard
/// ones feed a geometry shader the variant could not route through.
fn is_eye_pass_vegetation(name: Option<&str>) -> bool {
    name.is_some_and(|name| {
        (name.starts_with("vegetationbark") || name.starts_with("vegetationfoliage"))
            && !["shadow", "rsm", "billboard"]
                .iter()
                .any(|pass| name.contains(pass))
    })
}

/// The variant bytecode remembered for each pristine blob, keyed by its length and hash.
static BLOBS: Mutex<BTreeMap<(usize, u64), Vec<u8>>> = Mutex::new(BTreeMap::new());
static ATTACHED: AtomicUsize = AtomicUsize::new(0);
static FAILED: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eye_pass_permutations_are_the_colour_depth_and_velocity_ones() {
        for name in [
            "vegetationbark",
            "vegetationbarkinstanced",
            "vegetationbarkprezhwinstanced",
            "vegetationbarkvelocityinstanced",
            "vegetationfoliage_osnormalmap",
            "vegetationfoliageinstanced",
            "vegetationfoliagevelocityhwinstanced",
        ] {
            assert!(is_eye_pass_vegetation(Some(name)), "{name}");
        }
        for name in [
            "vegetationbarkshadowinstanced",
            "vegetationfoliagersmhwinstanced",
            "vegetationfoliagebillboard",
            "vegetationbarkbillboard",
            "treeimpostor",
            "generaljc3",
        ] {
            assert!(!is_eye_pass_vegetation(Some(name)), "{name}");
        }
        assert!(!is_eye_pass_vegetation(None));
    }
}
