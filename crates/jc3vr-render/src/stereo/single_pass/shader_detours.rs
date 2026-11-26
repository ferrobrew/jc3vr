//! The shader-substitution COM detours: `CreateVertexShader`, which substitutes the rewritten
//! bytecode and records what it produced, and `VSSetShader`, which caches whether the shader now bound
//! is one of them.
//!
//! The draw path needs to know, per draw, whether the bound shader renders both eyes by itself. That
//! question is answered here and read there, which is why the patched-shader set and the bound-shader
//! flag live with the detours that maintain them rather than with the draws that consult them.

use std::sync::LazyLock;

use sha2::{Digest, Sha256};
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceChild;

use crate::stereo::single_pass::*;

/// `ID3D11Device::CreateVertexShader` (device vtable slot 12) and `ID3D11DeviceContext::VSSetShader`
/// (context vtable slot 11), verified against the `windows` vtable structs.
pub(super) const CREATE_VERTEX_SHADER_SLOT: usize = 12;
pub(super) const VS_SET_SHADER_SLOT: usize = 11;

pub(super) type CreateVertexShaderFn = unsafe extern "system" fn(
    *mut c_void,
    *const c_void,
    usize,
    *mut c_void,
    *mut *mut c_void,
) -> i32;
pub(super) type VsSetShaderFn =
    unsafe extern "system" fn(*mut c_void, *mut c_void, *const *mut c_void, u32);

pub(super) static CREATE_VERTEX_SHADER: DetourSlot<CreateVertexShaderFn> = DetourSlot::new();
pub(super) static VS_SET_SHADER: DetourSlot<VsSetShaderFn> = DetourSlot::new();

/// What [`create_vertex_shader_detour`] made of a blob that did not arrive pre-substituted from the
/// `CreateVertexProgram` hook.
pub(super) enum Reacquired {
    /// The hook did see this blob and decided its transform by name ([`remember_vs_transform`]); that
    /// decision was re-applied here. `None` when the decision was to leave the shader pristine, or
    /// when the rewrites it allows all bowed out.
    Decided(VsTransform, Option<Vec<u8>>),
    /// The hook never saw this blob, so there is no name to decide with. Fall back to the `cb0` remap,
    /// the only transform decidable from the bytecode alone.
    Rederived(Result<Vec<u8>, DxbcError>),
}

/// Substitute and record the `ID3D11VertexShader` for a stereo-patched blob, covering both the fresh
/// and the re-created shader-creation paths.
///
/// The `CreateVertexProgram` hook substitutes the blob for a *fresh* shader create and sets
/// [`PATCH_PENDING`] right before the engine calls `CreateVertexShader`, so the shader created under
/// that flag is the patched one and goes straight into [`PATCHED_VS`]. But a bundle reload re-creates
/// an already-loaded shader through `ResourceCacheReCreateResource`, which calls `CreateVertexShader`
/// directly *without* re-running `CreateVertexProgram` -- so a shader first loaded before single-pass
/// (e.g. a character shader from level start, whose resource still holds the original bytecode) would
/// arrive here unsubstituted and render mono/skewed.
///
/// Catch that path with the decision the hook already made for that blob ([`cached_vs_transform`]),
/// which is the only way to get it right: the transform is chosen by shader *name* and the D3D layer
/// has no name, so re-deriving one here can only ever produce the `cb0` remap -- the wrong transform
/// for the reprojection families, and one transform too many for the families a render-block intercept
/// owns and the hook deliberately left pristine. Only a blob the hook never saw is re-derived, and
/// then the remap is the best that can be done: an unpatched-but-patchable blob is substituted in
/// place for the create call, and an already-patched blob (`Cb13AlreadyDeclared` -- a reload of a
/// shader whose resource already holds the patched blob) is recorded as-is. `PATCH_PENDING`
/// short-circuits all of it for the fresh path, whose blob `CreateVertexProgram` already substituted.
pub(super) unsafe extern "system" fn create_vertex_shader_detour(
    device: *mut c_void,
    bytecode: *const c_void,
    length: usize,
    linkage: *mut c_void,
    out: *mut *mut c_void,
) -> i32 {
    let detour = CREATE_VERTEX_SHADER.get().expect("set before enable");
    // `PATCH_PENDING` is only set by callers that have already verified `active()` — specifically
    // `hooks::graphics_engine::shader` gates on `saved.is_some()` which requires `active()` — so the
    // recording path is safe by contract even though `active()` is not re-checked here.
    let pending = PATCH_PENDING.with(Cell::take);
    // Taken unconditionally, so a name left behind by a create that did not reach the record below
    // cannot be attributed to a later, unrelated shader.
    let pending_name = PATCH_PENDING_NAME.with(Cell::take);
    // A pinned-eye blob was substituted by the hook as well, so it short-circuits the re-acquire the
    // same way.
    let pin_pending = PIN_PENDING.with(Cell::take);
    let reacquired = (!pending && !pin_pending && active() && !bytecode.is_null() && length >= 4)
        .then(|| {
            let code = unsafe { std::slice::from_raw_parts(bytecode.cast::<u8>(), length) };
            match cached_vs_transform(code) {
                // No remap output to hand on (this path did not run one) and no name (the whole reason the
                // decision had to be made elsewhere), so the terrain log line here is unnamed.
                Some(transform) => {
                    Reacquired::Decided(transform, apply_vs_transform(transform, code, None, None))
                }
                None => Reacquired::Rederived(remap_vertex_shader(code)),
            }
        });
    // A pinned-eye shader is substituted but must not be recorded as patched: recording it would
    // instance-double its draws, and it has no instance parity to read. It is tagged instead.
    let pinned = pin_pending
        || matches!(
            &reacquired,
            Some(Reacquired::Decided(VsTransform::Pinned, Some(_)))
        );
    let (record, blob, len) = match &reacquired {
        Some(Reacquired::Decided(_, Some(transformed))) => {
            CVS_DECIDED_TRANSFORMED.fetch_add(1, Ordering::Relaxed);
            (
                !pinned,
                transformed.as_ptr().cast::<c_void>(),
                transformed.len(),
            )
        }
        Some(Reacquired::Decided(_, None)) => {
            CVS_DECIDED_PRISTINE.fetch_add(1, Ordering::Relaxed);
            (false, bytecode, length)
        }
        Some(Reacquired::Rederived(Ok(patched))) => {
            CVS_REACQ_PATCHED.fetch_add(1, Ordering::Relaxed);
            (true, patched.as_ptr().cast::<c_void>(), patched.len())
        }
        Some(Reacquired::Rederived(Err(DxbcError::Cb13AlreadyDeclared))) => {
            CVS_REACQ_CB13.fetch_add(1, Ordering::Relaxed);
            (true, bytecode, length)
        }
        Some(Reacquired::Rederived(Err(DxbcError::NoPerEyeReferences))) => {
            CVS_REACQ_NOREFS.fetch_add(1, Ordering::Relaxed);
            (false, bytecode, length)
        }
        Some(Reacquired::Rederived(Err(error))) => {
            report_malformed("cb0 remap", *error);
            CVS_REACQ_ERR.fetch_add(1, Ordering::Relaxed);
            (false, bytecode, length)
        }
        None => {
            if pending {
                CVS_PENDING.fetch_add(1, Ordering::Relaxed);
            }
            (pending && !pinned, bytecode, length)
        }
    };
    let generation = vertex_identity_generation();
    let hr = unsafe { detour.call(device, blob, len, linkage, out) };
    if let Some(generation) = generation
        && hr >= 0
        && !out.is_null()
        && let shader = unsafe { *out }
        && !shader.is_null()
    {
        unsafe { vertex_identity_record(generation, shader, blob, len, pending_name.as_deref()) };
    }
    if hr >= 0
        && !out.is_null()
        && !bytecode.is_null()
        && let shader = unsafe { *out }
        && !shader.is_null()
        && let Err(error) = vegetation_once::variants::attach(
            shader,
            // SAFETY: the engine's bytecode for this create, which outlives the call.
            unsafe { std::slice::from_raw_parts(bytecode.cast::<u8>(), length) },
            |variant| {
                let mut created = std::ptr::null_mut();
                // SAFETY: the trampoline, so the variant's creation is not seen as an engine
                // shader; `variant` is a complete container.
                let hr = unsafe {
                    detour.call(
                        device,
                        variant.as_ptr().cast(),
                        variant.len(),
                        std::ptr::null_mut(),
                        &mut created,
                    )
                };
                (hr >= 0 && !created.is_null()).then_some(created)
            },
        )
    {
        tracing::warn!(target: "single_pass", "{error:#}");
    }
    if pinned
        && hr >= 0
        && !out.is_null()
        && let shader = unsafe { *out }
        && let Some(shader) = unsafe { ID3D11DeviceChild::from_raw_borrowed(&shader) }
    {
        tag_pinned_eye(shader);
    }
    if record
        && hr == 0
        && !out.is_null()
        && let shader = unsafe { *out }
        && !shader.is_null()
        && PATCHED_VS.lock().insert(shader as usize)
    {
        // SAFETY: on success `*out` is the newly-created, live COM object. Only the thread that won the
        // `insert` takes the reference, so records and releases stay balanced.
        unsafe { com_add_ref(shader) };
        if let Some(name) = pending_name {
            PATCHED_VS_NAMES.lock().insert(shader as usize, name);
        }
    }
    hr
}

const MAX_VERTEX_IDENTITIES: usize = 4096;
const MAX_VERTEX_BYTECODE: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VertexIdentity {
    pub name: Option<String>,
    pub actual_sha256: [u8; 32],
}

struct VertexIdentityState {
    armed: bool,
    generation: u64,
    entries: BTreeMap<usize, VertexIdentity>,
    incomplete: bool,
}

impl VertexIdentityState {
    fn reset(&mut self, armed: bool) -> BTreeMap<usize, VertexIdentity> {
        self.armed = armed;
        self.generation = self.generation.wrapping_add(1);
        self.incomplete = false;
        std::mem::take(&mut self.entries)
    }

    fn admit(&mut self, generation: u64, pointer: usize, identity: &VertexIdentity) -> bool {
        if !vertex_identity_accepts(self.armed, self.generation, generation) {
            return false;
        }
        if let Some(previous) = self.entries.get(&pointer) {
            if previous.actual_sha256 != identity.actual_sha256 {
                self.incomplete = true;
            }
            return false;
        }
        if self.entries.len() >= MAX_VERTEX_IDENTITIES {
            self.incomplete = true;
            return false;
        }
        true
    }
}

static VERTEX_IDENTITY_ARMED: AtomicBool = AtomicBool::new(false);
static VERTEX_IDENTITIES: LazyLock<Mutex<VertexIdentityState>> = LazyLock::new(|| {
    Mutex::new(VertexIdentityState {
        armed: false,
        generation: 0,
        entries: BTreeMap::new(),
        incomplete: false,
    })
});

pub(crate) fn vertex_identity_arm() {
    vertex_identity_clear(true);
    VERTEX_IDENTITY_ARMED.store(true, Ordering::Release);
}

pub(crate) fn vertex_identity_reset() {
    let old = {
        let mut state = VERTEX_IDENTITIES.lock();
        let armed = state.armed;
        state.reset(armed)
    };
    for pointer in old.into_keys() {
        unsafe { com_release(pointer as *mut c_void) };
    }
}

pub(crate) fn vertex_identity_disarm() {
    VERTEX_IDENTITY_ARMED.store(false, Ordering::Release);
    vertex_identity_clear(false);
}

fn vertex_identity_clear(armed: bool) {
    let old = {
        let mut state = VERTEX_IDENTITIES.lock();
        state.reset(armed)
    };
    for pointer in old.into_keys() {
        unsafe { com_release(pointer as *mut c_void) };
    }
}

fn vertex_identity_generation() -> Option<u64> {
    if !VERTEX_IDENTITY_ARMED.load(Ordering::Acquire) {
        return None;
    }
    let state = VERTEX_IDENTITIES.lock();
    state.armed.then_some(state.generation)
}

unsafe fn vertex_identity_record(
    generation: u64,
    shader: *mut c_void,
    blob: *const c_void,
    len: usize,
    name: Option<&str>,
) {
    if blob.is_null() || len == 0 || len > MAX_VERTEX_BYTECODE || len > isize::MAX as usize {
        let mut state = VERTEX_IDENTITIES.lock();
        if state.armed && state.generation == generation {
            state.incomplete = true;
        }
        return;
    }
    let digest =
        Sha256::digest(unsafe { std::slice::from_raw_parts(blob.cast::<u8>(), len) }).into();
    let mut state = VERTEX_IDENTITIES.lock();
    if !vertex_identity_accepts(state.armed, state.generation, generation) {
        return;
    }
    let identity = VertexIdentity {
        name: name.map(str::to_owned),
        actual_sha256: digest,
    };
    if !state.admit(generation, shader as usize, &identity) {
        return;
    }
    unsafe { com_add_ref(shader) };
    state.entries.insert(shader as usize, identity);
}

fn vertex_identity_accepts(armed: bool, current: u64, observed: u64) -> bool {
    armed && current == observed
}

#[cfg(test)]
mod vertex_identity_tests {
    use std::collections::BTreeMap;

    use super::{
        MAX_VERTEX_IDENTITIES, VertexIdentity, VertexIdentityState, vertex_identity_accepts,
    };

    #[test]
    fn registry_bounds_conflicts_and_reset() {
        let mut state = VertexIdentityState {
            armed: true,
            generation: 1,
            entries: BTreeMap::new(),
            incomplete: false,
        };
        let identity = VertexIdentity {
            name: None,
            actual_sha256: [1; 32],
        };
        assert!(state.admit(1, 1, &identity));
        state.entries.insert(1, identity.clone());
        assert!(!state.admit(1, 1, &identity));
        assert!(!state.incomplete);
        let conflict = VertexIdentity {
            name: None,
            actual_sha256: [2; 32],
        };
        assert!(!state.admit(1, 1, &conflict));
        assert!(state.incomplete);
        assert_eq!(state.reset(true).len(), 1);
        assert!(!state.incomplete);
        assert!(!state.admit(1, 2, &identity));
        for pointer in 0..MAX_VERTEX_IDENTITIES {
            state.entries.insert(pointer, identity.clone());
        }
        assert!(!state.admit(2, MAX_VERTEX_IDENTITIES, &identity));
        assert!(state.incomplete);
        assert_eq!(state.reset(false).len(), MAX_VERTEX_IDENTITIES);
        assert!(!state.armed);
        assert!(!state.incomplete);
    }

    #[test]
    fn old_creations_cannot_enter_a_new_generation() {
        assert!(vertex_identity_accepts(true, 7, 7));
        assert!(!vertex_identity_accepts(true, 8, 7));
        assert!(!vertex_identity_accepts(false, 7, 7));
    }
}

pub fn vertex_identity_lookup(shader: usize) -> Result<Option<VertexIdentity>, &'static str> {
    let state = VERTEX_IDENTITIES.lock();
    if !state.armed {
        return Err("unarmed");
    }
    if state.incomplete {
        return Err("incomplete");
    }
    Ok(state.entries.get(&shader).cloned())
}

/// Cache whether the vertex shader now bound is a patched one, so [`draw_indexed_detour`] can gate
/// without a per-draw set lookup, and its pointer, so the already-instanced exposure measurement can
/// attribute a draw to the shader that issued it.
pub(super) unsafe extern "system" fn vs_set_shader_detour(
    context: *mut c_void,
    shader: *mut c_void,
    instances: *const *mut c_void,
    num_instances: u32,
) {
    let patched = !shader.is_null() && PATCHED_VS.lock().contains(&(shader as usize));
    BOUND_VS_PATCHED.store(patched, Ordering::Relaxed);
    BOUND_VS.store(shader as usize, Ordering::Relaxed);
    let detour = VS_SET_SHADER.get().expect("set before enable");
    unsafe { detour.call(context, shader, instances, num_instances) };
}

/// Reset the patched-shader set, releasing this module's reference to each recorded shader (on a
/// shader reload, where the game drops the old shaders and their addresses become reusable). Also
/// zeroes the `CreateVertexShader`-path tallies so [`substitution_stats`] reflects one clean pass over
/// the reloaded shader set.
pub fn reset_patched_vs() {
    for shader in std::mem::take(&mut *PATCHED_VS.lock()) {
        // SAFETY: every entry was `com_add_ref`'d exactly once when it was recorded.
        unsafe { com_release(shader as *mut c_void) };
    }
    // Both are keyed by shader pointer, and those addresses become reusable the moment the references
    // above are dropped, so an entry that survived the reset would be attributed to a different shader.
    PATCHED_VS_NAMES.lock().clear();
    // The transform decisions go with them: this runs mid-bounce, so the pass that follows re-decides
    // every shader under the current config and repopulates the map. Keeping the old entries would let
    // a decision made under config that has since changed outlive the shader it was made for, and
    // would accumulate the previous bundle's blobs across every reload.
    VS_TRANSFORM_CACHE.lock().clear();
    vegetation_once::variants::reset();
    reset_instanced_exposure();
    CVS_PENDING.store(0, Ordering::Relaxed);
    CVS_DECIDED_TRANSFORMED.store(0, Ordering::Relaxed);
    CVS_DECIDED_PRISTINE.store(0, Ordering::Relaxed);
    CVS_REACQ_PATCHED.store(0, Ordering::Relaxed);
    CVS_REACQ_CB13.store(0, Ordering::Relaxed);
    CVS_REACQ_NOREFS.store(0, Ordering::Relaxed);
    CVS_REACQ_ERR.store(0, Ordering::Relaxed);
}

/// Whether single-pass has substituted any patched vertex shaders this session (they are still held
/// by the game). If so, eject must re-create the originals, else the game keeps rendering with the
/// mod's `cb13`-reading shaders after the mod is gone.
pub fn has_patched_shaders() -> bool {
    !PATCHED_VS.lock().is_empty()
}

/// Warn if any shader was created from bytecode that already carried the mod's rewrite (see
/// [`CVS_REACQ_CB13`]). The eject bounce cannot restore those: it re-creates from the resource's own
/// bytecode, which in this case is the patched blob, and the rewriter has no inverse. Called from the
/// eject restore so the condition is on the record instead of only in a live counter.
pub fn warn_if_shaders_hold_patched_bytecode() {
    let count = CVS_REACQ_CB13.load(Ordering::Relaxed);
    if count > 0 {
        tracing::warn!(
            "single-pass: {count} shader(s) were created from bytecode that already carried the \
             stereo rewrite, so some resource holds a patched blob; the eject bounce re-creates \
             those still patched"
        );
    }
}

/// A snapshot of the shader-substitution tallies, for the bring-up log and the screenshot JSON sidecar.
/// `recorded_vs` is the live size of [`PATCHED_VS`] (patched shaders the draw gating will double); the
/// `cvs_*` fields are the [`create_vertex_shader_detour`] outcome buckets; the `census_*` fields are the
/// [`record_patch_outcome`] buckets from the `CreateVertexProgram` hook. Comparing the two paths shows
/// whether the re-create path (which skips `CreateVertexProgram`) is reaching the D3D-level substitution.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct SubstitutionStats {
    pub recorded_vs: usize,
    pub cvs_pending: usize,
    pub cvs_decided_transformed: usize,
    pub cvs_decided_pristine: usize,
    pub cvs_reacq_patched: usize,
    pub cvs_reacq_cb13: usize,
    pub cvs_reacq_no_refs: usize,
    pub cvs_reacq_err: usize,
    pub census_patched: usize,
    pub census_no_refs: usize,
    pub census_deferred: usize,
    pub census_errored: usize,
}

/// Snapshot the current shader-substitution tallies. See [`SubstitutionStats`].
pub fn substitution_stats() -> SubstitutionStats {
    SubstitutionStats {
        recorded_vs: PATCHED_VS.lock().len(),
        cvs_pending: CVS_PENDING.load(Ordering::Relaxed),
        cvs_decided_transformed: CVS_DECIDED_TRANSFORMED.load(Ordering::Relaxed),
        cvs_decided_pristine: CVS_DECIDED_PRISTINE.load(Ordering::Relaxed),
        cvs_reacq_patched: CVS_REACQ_PATCHED.load(Ordering::Relaxed),
        cvs_reacq_cb13: CVS_REACQ_CB13.load(Ordering::Relaxed),
        cvs_reacq_no_refs: CVS_REACQ_NOREFS.load(Ordering::Relaxed),
        cvs_reacq_err: CVS_REACQ_ERR.load(Ordering::Relaxed),
        census_patched: patched_count(),
        census_no_refs: no_refs_count(),
        census_deferred: deferred_count(),
        census_errored: errored_count(),
    }
}
