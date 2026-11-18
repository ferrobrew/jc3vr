use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::c_void,
    sync::{
        LazyLock,
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
        lake: None,
        overflow: 0,
        invalid: 0,
        install_error: false,
    })
});

thread_local! {
    static PENDING: RefCell<Option<Pending>> = const { RefCell::new(None) };
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Identity {
    pub pristine_sha256: String,
    pub actual_sha256: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum IdentityResult {
    Mapped { identity: Identity },
    Unarmed,
    Unmapped,
    Incomplete,
    InstallFailed,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct CensusStatus {
    pub requested: bool,
    pub status: &'static str,
    pub registered: usize,
    pub overflow: usize,
    pub invalid: usize,
}

struct Registry {
    generation: u64,
    shaders: HashMap<usize, Identity>,
    lake: Option<Vec<u8>>,
    overflow: usize,
    invalid: usize,
    install_error: bool,
}

impl Registry {
    fn clear_generation(&mut self) -> HashMap<usize, Identity> {
        self.generation = self.generation.wrapping_add(1);
        self.overflow = 0;
        self.invalid = 0;
        self.lake = None;
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

pub(crate) fn status() -> CensusStatus {
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

pub(crate) fn lookup(shader: &ID3D11PixelShader) -> IdentityResult {
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

pub(crate) fn arm() -> bool {
    if ARMED.load(Ordering::Acquire) {
        return false;
    }
    reset();
    crate::stereo::single_pass::vertex_identity_arm();
    ARMED.store(true, Ordering::Release);
    true
}

pub(crate) fn disarm() {
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

pub(crate) fn mark_install_failed() {
    REGISTRY.lock().install_error = true;
}

pub(crate) struct PendingGuard(Option<Pending>);

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

pub(crate) unsafe fn begin(code: *const u8, len: u64) -> Option<PendingGuard> {
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
        if identity.actual_sha256 == LAKE_PS
            && identity.pristine_sha256 == LAKE_PS
            && length == 21568
        {
            state.lake =
                Some(unsafe { std::slice::from_raw_parts(code.cast::<u8>(), length) }.to_vec());
        }
        unsafe { com_add_ref(shader) };
        state.shaders.insert(key, identity);
    });
    result
}

pub(crate) const LAKE_PS: &str = "99dd7036aa192c346c1e23883da83cf4da300dd1190dee40eedb3c2421531531";

pub(crate) fn lake_bytecode() -> Option<Vec<u8>> {
    REGISTRY.lock().lake.clone()
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
            lake: None,
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
            lake: None,
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
            lake: None,
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
