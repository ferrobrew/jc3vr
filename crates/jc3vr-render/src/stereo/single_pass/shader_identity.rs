use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::c_void,
    sync::{
        LazyLock, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use parking_lot::Mutex;
use serde::Serialize;
use sha2::{Digest, Sha256};
use windows::{Win32::Graphics::Direct3D11::ID3D11PixelShader, core::Interface};

use crate::stereo::single_pass::{DetourSlot, com_add_ref, com_release};

const MAX_SHADERS: usize = 4096;
const MAX_BYTECODE: usize = 16 * 1024 * 1024;

pub(super) type CreatePixelShaderFn = unsafe extern "system" fn(
    *mut c_void,
    *const c_void,
    usize,
    *mut c_void,
    *mut *mut c_void,
) -> i32;
pub(super) const CREATE_PIXEL_SHADER_SLOT: usize = 15;
pub(super) static CREATE_PIXEL_SHADER: DetourSlot<CreatePixelShaderFn> = DetourSlot::new();
static ARMED: AtomicBool = AtomicBool::new(false);
static REGISTRY: LazyLock<Mutex<Registry>> = LazyLock::new(|| {
    Mutex::new(Registry {
        generation: 0,
        shaders: HashMap::new(),
        kept: HashMap::new(),
        overflow: 0,
        invalid: 0,
        install_error: false,
    })
});

thread_local! {
    static PENDING: RefCell<Option<Pending>> = const { RefCell::new(None) };
}

#[derive(Clone, Debug, Serialize)]
pub struct Identity {
    pub pristine_sha256: String,
    pub actual_sha256: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum IdentityResult {
    Mapped { identity: Identity },
    Unarmed,
    Unmapped,
    Incomplete,
    InstallFailed,
}

#[derive(Clone, Debug, Serialize)]
pub struct CensusStatus {
    pub requested: bool,
    pub status: &'static str,
    pub registered: usize,
    pub overflow: usize,
    pub invalid: usize,
}

struct Registry {
    generation: u64,
    shaders: HashMap<usize, Identity>,
    /// The bytecode of the shaders in [`KEPT`] created this generation, by pristine hash.
    kept: HashMap<&'static str, Vec<u8>>,
    overflow: usize,
    invalid: usize,
    install_error: bool,
}

impl Registry {
    fn clear_generation(&mut self) -> HashMap<usize, Identity> {
        self.generation = self.generation.wrapping_add(1);
        self.overflow = 0;
        self.invalid = 0;
        self.kept.clear();
        self.install_error = false;
        std::mem::take(&mut self.shaders)
    }

    fn accepts(&self, generation: u64, armed: bool) -> bool {
        armed && self.generation == generation
    }

    fn mark_invalid(&mut self, generation: u64, armed: bool) {
        if self.accepts(generation, armed) {
            self.invalid = self.invalid.saturating_add(1);
        }
    }

    fn classify(
        &mut self,
        generation: u64,
        armed: bool,
        key: usize,
        identity: &Identity,
    ) -> Admission {
        if !self.accepts(generation, armed) {
            return Admission::Stale;
        }
        if let Some(existing) = self.shaders.get(&key) {
            if existing.pristine_sha256 != identity.pristine_sha256
                || existing.actual_sha256 != identity.actual_sha256
            {
                self.mark_invalid(generation, armed);
                return Admission::Conflict;
            }
            return Admission::Duplicate;
        }
        if self.shaders.len() >= MAX_SHADERS {
            self.overflow = self.overflow.saturating_add(1);
            return Admission::Overflow;
        }
        Admission::Insert
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Admission {
    Insert,
    Duplicate,
    Conflict,
    Overflow,
    Stale,
}

struct Pending {
    generation: u64,
    pristine: [u8; 32],
    hits: usize,
}

pub(crate) fn requested() -> bool {
    ARMED.load(Ordering::Acquire)
}

pub fn status() -> CensusStatus {
    let state = REGISTRY.lock();
    let requested = ARMED.load(Ordering::Acquire);
    CensusStatus {
        requested,
        status: if !requested {
            "unarmed"
        } else if state.install_error {
            "install_failed"
        } else if state.invalid != 0 || state.overflow != 0 {
            "incomplete"
        } else if state.shaders.is_empty() {
            "empty"
        } else {
            "complete"
        },
        registered: state.shaders.len(),
        overflow: state.overflow,
        invalid: state.invalid,
    }
}

pub fn lookup(shader: &ID3D11PixelShader) -> IdentityResult {
    if !ARMED.load(Ordering::Acquire) {
        return IdentityResult::Unarmed;
    }
    let state = REGISTRY.lock();
    if !ARMED.load(Ordering::Acquire) {
        return IdentityResult::Unarmed;
    }
    if state.install_error {
        return IdentityResult::InstallFailed;
    }
    if state.invalid != 0 || state.overflow != 0 {
        return IdentityResult::Incomplete;
    }
    state
        .shaders
        .get(&(shader.as_raw() as usize))
        .cloned()
        .map_or(IdentityResult::Unmapped, |identity| {
            IdentityResult::Mapped { identity }
        })
}

/// Arm the census and install the vertex- and pixel-shader creation detours it records through:
/// `Ok(true)` when this call armed it, `Ok(false)` when it already was. When a detour does not
/// install, the census is marked failed and the error names the detour.
pub fn arm_with_detours() -> Result<bool, &'static str> {
    let newly = arm();
    crate::stereo::single_pass::ensure_viewport_detours();
    if !crate::stereo::single_pass::vertex_identity_detour_ready() {
        mark_install_failed();
        return Err("vertex shader detour installation failed");
    }
    if !crate::stereo::single_pass::ensure_pixel_shader_detour() {
        mark_install_failed();
        return Err("pixel shader detour installation failed");
    }
    Ok(newly)
}

fn arm() -> bool {
    if ARMED.load(Ordering::Acquire) {
        return false;
    }
    reset();
    crate::stereo::single_pass::vertex_identity_arm();
    ARMED.store(true, Ordering::Release);
    true
}

pub fn disarm() {
    ARMED.store(false, Ordering::Release);
    crate::stereo::single_pass::vertex_identity_disarm();
    reset();
}

pub(crate) fn reset() {
    crate::stereo::single_pass::vertex_identity_reset();
    let old = {
        let mut state = REGISTRY.lock();
        state.clear_generation()
    };
    for pointer in old.into_keys() {
        unsafe { com_release(pointer as *mut c_void) };
    }
}

fn mark_install_failed() {
    REGISTRY.lock().install_error = true;
}

pub struct PendingGuard(Option<Pending>);

impl Drop for PendingGuard {
    fn drop(&mut self) {
        PENDING.with(|pending| {
            let current = pending.replace(self.0.take());
            if let Some(current) = current
                && current.hits != 1
            {
                let mut state = REGISTRY.lock();
                state.mark_invalid(current.generation, ARMED.load(Ordering::Acquire));
            }
        });
    }
}

/// Hash the shader bytecode at `code` and arm the identity the next pixel-shader creation on this
/// thread records it under, while the registry is armed.
///
/// # Safety
///
/// `code` must point to `len` readable bytes for the duration of the call.
pub unsafe fn begin(code: *const u8, len: u64) -> Option<PendingGuard> {
    if !ARMED.load(Ordering::Acquire) {
        return None;
    }
    let generation = {
        let state = REGISTRY.lock();
        if !ARMED.load(Ordering::Acquire) {
            return None;
        }
        state.generation
    };
    let Ok(len) = usize::try_from(len) else {
        mark_invalid(generation);
        return None;
    };
    if code.is_null() || len == 0 || len > MAX_BYTECODE || len > isize::MAX as usize {
        mark_invalid(generation);
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(code, len) };
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    let state = REGISTRY.lock();
    if !state.accepts(generation, ARMED.load(Ordering::Acquire)) {
        return None;
    }
    drop(state);
    let previous = PENDING.with(|pending| {
        pending.replace(Some(Pending {
            generation,
            pristine: digest,
            hits: 0,
        }))
    });
    if previous.is_some() {
        mark_invalid(generation);
    }
    Some(PendingGuard(previous))
}

pub(super) unsafe extern "system" fn create_pixel_shader_detour(
    device: *mut c_void,
    code: *const c_void,
    length: usize,
    linkage: *mut c_void,
    out: *mut *mut c_void,
) -> i32 {
    let detour = CREATE_PIXEL_SHADER.get().expect("set before enable");
    let generation = PENDING.with(|pending| {
        let mut pending = pending.borrow_mut();
        pending.as_mut().map(|pending| {
            pending.hits = pending.hits.saturating_add(1);
            pending.generation
        })
    });
    let result = unsafe { detour.call(device, code, length, linkage, out) };
    if result < 0 || out.is_null() {
        if let Some(generation) = generation {
            mark_invalid(generation);
        }
        return result;
    }
    let shader = unsafe { *out };
    if shader.is_null() {
        if let Some(generation) = generation {
            mark_invalid(generation);
        }
        return result;
    }
    if !ARMED.load(Ordering::Acquire) {
        return result;
    }
    PENDING.with(|pending| {
        let pending = pending.borrow();
        let Some(pending) = pending.as_ref() else {
            return;
        };
        if code.is_null() || length == 0 || length > MAX_BYTECODE || length > isize::MAX as usize {
            mark_invalid(pending.generation);
            return;
        }
        let actual: [u8; 32] =
            Sha256::digest(unsafe { std::slice::from_raw_parts(code.cast::<u8>(), length) }).into();
        let mut state = REGISTRY.lock();
        let key = shader as usize;
        let identity = Identity {
            pristine_sha256: hex(&pending.pristine),
            actual_sha256: hex(&actual),
        };
        if state.classify(
            pending.generation,
            ARMED.load(Ordering::Acquire),
            key,
            &identity,
        ) != Admission::Insert
        {
            return;
        }
        if let Some(kept) = kept_as(&identity, length) {
            state.kept.insert(
                kept.pristine,
                unsafe { std::slice::from_raw_parts(code.cast::<u8>(), length) }.to_vec(),
            );
        }
        unsafe { com_add_ref(shader) };
        state.shaders.insert(key, identity);
    });
    result
}

/// A shader whose bytecode the census keeps as it is created: the hash of the pristine bytecode the
/// engine creates it from, and the executables (the hash and byte length of the bytecode the device
/// is handed) that count as it.
#[derive(Clone, Copy, Debug)]
pub struct Kept {
    pub pristine: &'static str,
    pub executables: &'static [(&'static str, usize)],
}

/// Registers the shaders whose bytecode the census keeps. Once, at startup; a second registration is
/// refused with an error log.
pub fn keep(shaders: &'static [Kept]) {
    if KEPT.set(shaders).is_err() {
        tracing::error!(
            "shader identity: kept shaders already registered; the second list is ignored"
        );
    }
}

/// Whether the kept shaders are registered, for the install's registration check.
pub fn kept_registered() -> bool {
    KEPT.get().is_some()
}

/// The bytecode the census kept this generation for the shader created from `pristine`.
pub fn kept_bytecode(pristine: &str) -> Option<Vec<u8>> {
    REGISTRY.lock().kept.get(pristine).cloned()
}

/// The lake surface's pixel shader, whose pristine hash the shipped water path also matches.
pub const LAKE_PS: &str = "99dd7036aa192c346c1e23883da83cf4da300dd1190dee40eedb3c2421531531";

static KEPT: OnceLock<&'static [Kept]> = OnceLock::new();

/// The registered shader `identity` (of an executable `length` bytes long) counts as, if any.
fn kept_as(identity: &Identity, length: usize) -> Option<&'static Kept> {
    KEPT.get()?.iter().find(|kept| {
        kept.pristine == identity.pristine_sha256
            && kept
                .executables
                .iter()
                .any(|&(actual, size)| actual == identity.actual_sha256 && size == length)
    })
}

fn mark_invalid(generation: u64) {
    let mut state = REGISTRY.lock();
    state.mark_invalid(generation, ARMED.load(Ordering::Acquire));
}

fn hex(digest: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(64);
    for &byte in digest {
        result.push(DIGITS[(byte >> 4) as usize] as char);
        result.push(DIGITS[(byte & 15) as usize] as char);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_invalidates_inflight_generations_and_clears_state() {
        let mut state = Registry {
            generation: 7,
            shaders: HashMap::new(),
            kept: HashMap::new(),
            overflow: 1,
            invalid: 2,
            install_error: true,
        };
        state.shaders.insert(
            1,
            Identity {
                pristine_sha256: "a".into(),
                actual_sha256: "b".into(),
            },
        );
        let old = state.clear_generation();
        assert_eq!(old.len(), 1);
        assert!(state.shaders.is_empty());
        assert_eq!(
            (state.overflow, state.invalid, state.install_error),
            (0, 0, false)
        );
        assert!(!state.accepts(7, true));
        assert!(state.accepts(8, true));
        assert!(!state.accepts(8, false));
    }

    #[test]
    fn stale_failures_and_disarmed_completions_do_not_change_the_census() {
        let mut state = Registry {
            generation: 4,
            shaders: HashMap::new(),
            kept: HashMap::new(),
            overflow: 0,
            invalid: 0,
            install_error: false,
        };
        state.mark_invalid(3, true);
        state.mark_invalid(4, false);
        assert_eq!(state.invalid, 0);
        state.mark_invalid(4, true);
        assert_eq!(state.invalid, 1);
        state.clear_generation();
        state.mark_invalid(4, true);
        assert_eq!(state.invalid, 0);
    }

    #[test]
    fn admission_bounds_duplicates_conflicts_and_generations() {
        let mut state = Registry {
            generation: 7,
            shaders: HashMap::new(),
            kept: HashMap::new(),
            overflow: 0,
            invalid: 0,
            install_error: false,
        };
        let identity = Identity {
            pristine_sha256: "a".into(),
            actual_sha256: "b".into(),
        };
        assert_eq!(state.classify(6, true, 1, &identity), Admission::Stale);
        assert_eq!(state.classify(7, false, 1, &identity), Admission::Stale);
        assert_eq!(state.classify(7, true, 1, &identity), Admission::Insert);
        state.shaders.insert(1, identity.clone());
        assert_eq!(state.classify(7, true, 1, &identity), Admission::Duplicate);
        let other = Identity {
            pristine_sha256: "other".into(),
            actual_sha256: "b".into(),
        };
        assert_eq!(state.classify(7, true, 1, &other), Admission::Conflict);
        assert_eq!(state.invalid, 1);
        for key in 2..=MAX_SHADERS {
            state.shaders.insert(key, identity.clone());
        }
        assert_eq!(
            state.classify(7, true, MAX_SHADERS + 1, &identity),
            Admission::Overflow
        );
        assert_eq!(state.overflow, 1);
        assert_eq!(
            state.classify(6, true, MAX_SHADERS + 2, &identity),
            Admission::Stale
        );
        assert_eq!(state.overflow, 1);
    }

    #[test]
    fn sha256_is_stable_and_hex_encoded() {
        let digest: [u8; 32] = Sha256::digest(b"abc").into();
        assert_eq!(
            hex(&digest),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
