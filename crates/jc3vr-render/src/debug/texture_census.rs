//! A census of the live 2D textures on the game's device, for finding what a resize leaves behind.
//!
//! Armed by `testing.texture_census`, it detours `ID3D11Device::CreateTexture2D` to record every
//! texture created (its size, format, bind flags, and the call stack that created it) and the
//! textures' `Release` to drop one once its last public reference goes. [`report`] logs the live
//! textures grouped by size, format, and creator, so two reports either side of a resize round trip
//! show which of the old size's textures outlived it and which code created them.
//!
//! Textures created before the census is armed are not in it. The detours stay installed for the
//! rest of the process: an eject is refused once the census has been armed, and the game must be
//! restarted, as with the shader identity census.

use std::{
    collections::HashMap,
    ffi::c_void,
    sync::{
        LazyLock, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use jc3gi::graphics_engine::graphics_engine::GraphicsEngine;
use jc3vr_core::config::engine::ConfigSlot;
use parking_lot::Mutex;
use retour::GenericDetour;
use windows::{
    Win32::{
        Foundation::HMODULE,
        Graphics::{
            Direct3D11::{
                D3D11_BIND_SHADER_RESOURCE, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC,
                D3D11_USAGE_DEFAULT, ID3D11Texture2D,
            },
            Dxgi::Common::{DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC},
        },
        System::{
            Diagnostics::Debug::RtlCaptureStackBackTrace,
            LibraryLoader::{
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, GetModuleFileNameW,
                GetModuleHandleExW,
            },
        },
    },
    core::{HRESULT, Interface as _, PCWSTR},
};

/// Whether to keep the census, `testing.texture_census` in the settings file. The payload binds the
/// slot to its field in the capture harness's settings at startup.
pub static TEXTURE_CENSUS: ConfigSlot<bool> = ConfigSlot::new("testing.texture_census");

/// The log line that marks the payload as pinned by the census's detours.
pub(crate) const PINNED_BY_TEXTURE_CENSUS: &str =
    "texture census: the texture detours were enabled, which pins the payload";

/// Arm the census if `testing.texture_census` asks for it and it is not armed yet. Any thread, once
/// the engine's device exists.
pub(crate) fn ensure_armed() {
    if ARMED.load(Ordering::Acquire) || !TEXTURE_CENSUS.query(|on| *on) {
        return;
    }
    let _install = INSTALL.lock();
    if ARMED.load(Ordering::Acquire) || jc3vr_core::is_shutting_down() {
        return;
    }
    match unsafe { arm() } {
        Ok(()) => {
            ARMED.store(true, Ordering::Release);
            tracing::warn!(
                "{PINNED_BY_TEXTURE_CENSUS}: an eject will be refused until the game restarts"
            );
        }
        Err(error) => tracing::warn!("texture census: could not arm ({error})"),
    }
}

/// Whether the census's detours were ever enabled, which pins the payload in the process.
pub(crate) fn armed() -> bool {
    ARMED.load(Ordering::Acquire)
}

/// Log the live textures grouped by size, format, bind flags, and creator, largest groups first,
/// under `label`.
pub(crate) fn report(label: &str) {
    if !armed() {
        return;
    }
    let mut groups: HashMap<GroupKey, (u32, u64)> = HashMap::new();
    {
        let live = LIVE.lock();
        for entry in live.values() {
            let slot = groups.entry(entry.key).or_default();
            slot.0 += 1;
            slot.1 += entry.bytes;
        }
    }
    let mut groups: Vec<_> = groups.into_iter().collect();
    groups.sort_by_key(|(_, (_, bytes))| std::cmp::Reverse(*bytes));
    let total: u64 = groups.iter().map(|(_, (_, bytes))| bytes).sum();
    tracing::info!(
        label,
        textures = groups.iter().map(|(_, (count, _))| count).sum::<u32>(),
        mib = total / (1 << 20),
        "texture census"
    );
    for (key, (count, bytes)) in groups.iter().take(40) {
        tracing::info!(
            label,
            width = key.width,
            height = key.height,
            format = key.format,
            bind = key.bind,
            count,
            mib = bytes / (1 << 20),
            creator = %creator(&key.stack),
            "texture census group"
        );
    }
}

/// The creation detour's and the release detour's install lock and the armed flag.
static INSTALL: Mutex<()> = Mutex::new(());
static ARMED: AtomicBool = AtomicBool::new(false);

type CreateTexture2dFn = unsafe extern "system" fn(
    *mut c_void,
    *const D3D11_TEXTURE2D_DESC,
    *const D3D11_SUBRESOURCE_DATA,
    *mut *mut c_void,
) -> HRESULT;
type ReleaseFn = unsafe extern "system" fn(*mut c_void) -> u32;

static CREATE: OnceLock<GenericDetour<CreateTexture2dFn>> = OnceLock::new();
static RELEASE: OnceLock<GenericDetour<ReleaseFn>> = OnceLock::new();

/// The live textures the census has seen created, by object address.
static LIVE: LazyLock<Mutex<HashMap<usize, Entry>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// The frames of the creating call stack kept, after the detour's own.
const STACK_FRAMES: usize = 6;

/// The `ID3D11Device` vtable slot of `CreateTexture2D`, and the `IUnknown` slot of `Release`.
const CREATE_TEXTURE_2D_SLOT: usize = 5;
const RELEASE_SLOT: usize = 2;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GroupKey {
    width: u32,
    height: u32,
    format: i32,
    bind: u32,
    stack: [usize; STACK_FRAMES],
}

struct Entry {
    key: GroupKey,
    bytes: u64,
}

unsafe fn arm() -> Result<(), String> {
    // SAFETY: the engine singleton and its device are live once the engine is initialized.
    let device = unsafe { GraphicsEngine::get() }
        .and_then(|ge| unsafe { ge.m_Device.as_ref() })
        .ok_or("the device is not available")?;
    let device = &device.m_Device;
    // A texture to read the textures' `Release` from.
    let desc = D3D11_TEXTURE2D_DESC {
        Width: 1,
        Height: 1,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        ..Default::default()
    };
    let mut probe: Option<ID3D11Texture2D> = None;
    // SAFETY: a valid description and no initial data.
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut probe)) }
        .map_err(|error| error.to_string())?;
    let probe = probe.ok_or("the probe texture was not created")?;
    // SAFETY: the vtables of a live device and a live texture.
    let (create_target, release_target): (CreateTexture2dFn, ReleaseFn) = unsafe {
        let device_vtable = *(device.as_raw() as *const *const usize);
        let texture_vtable = *(probe.as_raw() as *const *const usize);
        (
            std::mem::transmute::<usize, CreateTexture2dFn>(
                *device_vtable.add(CREATE_TEXTURE_2D_SLOT),
            ),
            std::mem::transmute::<usize, ReleaseFn>(*texture_vtable.add(RELEASE_SLOT)),
        )
    };
    drop(probe);
    // SAFETY: the targets are the live functions read above, with the signatures declared.
    let create = unsafe { GenericDetour::new(create_target, create_texture_2d_detour) }
        .map_err(|error| error.to_string())?;
    let release = unsafe { GenericDetour::new(release_target, release_detour) }
        .map_err(|error| error.to_string())?;
    // The detours live for the rest of the process.
    let create = CREATE.get_or_init(|| create);
    let release = RELEASE.get_or_init(|| release);
    jc3vr_core::hooks::suspend::for_block(|| {
        // SAFETY: the detours were built from these targets; other threads are suspended.
        unsafe {
            create.enable()?;
            release.enable()?;
        }
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .map_err(|error| error.to_string())
}

unsafe extern "system" fn create_texture_2d_detour(
    device: *mut c_void,
    desc: *const D3D11_TEXTURE2D_DESC,
    initial: *const D3D11_SUBRESOURCE_DATA,
    out: *mut *mut c_void,
) -> HRESULT {
    let detour = CREATE.get().expect("set before enable");
    // SAFETY: forwarding the caller's arguments to the original.
    let result = unsafe { detour.call(device, desc, initial, out) };
    if result.is_ok()
        && !out.is_null()
        && let (Some(texture), Some(desc)) = (unsafe { (*out).as_ref() }, unsafe { desc.as_ref() })
    {
        let mut frames = [std::ptr::null_mut(); STACK_FRAMES];
        // SAFETY: the buffer holds `STACK_FRAMES` pointers.
        unsafe { RtlCaptureStackBackTrace(1, &mut frames, None) };
        let key = GroupKey {
            width: desc.Width,
            height: desc.Height,
            format: desc.Format.0,
            bind: desc.BindFlags,
            stack: frames.map(|frame| frame as usize),
        };
        LIVE.lock().insert(
            std::ptr::from_ref(texture) as usize,
            Entry {
                key,
                bytes: estimated_bytes(desc),
            },
        );
    }
    result
}

unsafe extern "system" fn release_detour(object: *mut c_void) -> u32 {
    let detour = RELEASE.get().expect("set before enable");
    // SAFETY: forwarding to the original.
    let remaining = unsafe { detour.call(object) };
    if remaining == 0 {
        LIVE.lock().remove(&(object as usize));
    }
    remaining
}

/// A rough size: four bytes a texel (eight for the 64-bit formats), times the array and the mips.
fn estimated_bytes(desc: &D3D11_TEXTURE2D_DESC) -> u64 {
    let texel = match desc.Format.0 {
        10..=14 | 19..=22 => 8,
        2..=4 => 16,
        _ => 4,
    };
    let mips = if desc.MipLevels == 0 {
        1.34
    } else {
        (1..desc.MipLevels).fold(1.0, |sum, level| sum + 0.25f64.powi(level as i32))
    };
    (f64::from(desc.Width)
        * f64::from(desc.Height)
        * texel as f64
        * mips
        * f64::from(desc.ArraySize.max(1))) as u64
}

/// The creating stack as `module+offset` frames.
fn creator(stack: &[usize; STACK_FRAMES]) -> String {
    stack
        .iter()
        .filter(|frame| **frame != 0)
        .map(|frame| {
            let mut module = HMODULE::default();
            // SAFETY: looks up the module containing a code address without taking a reference.
            let found = unsafe {
                GetModuleHandleExW(
                    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                        | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                    PCWSTR(*frame as *const u16),
                    &mut module,
                )
            }
            .is_ok();
            if !found {
                return format!("{frame:#x}");
            }
            let mut name = [0u16; 260];
            // SAFETY: a live module handle and a sized buffer.
            let length = unsafe { GetModuleFileNameW(Some(module), &mut name) } as usize;
            let name = String::from_utf16_lossy(&name[..length]);
            let name = name
                .rsplit(['\\', '/'])
                .next()
                .unwrap_or_default()
                .to_owned();
            format!("{name}+{:#x}", frame - module.0 as usize)
        })
        .collect::<Vec<_>>()
        .join(" < ")
}
