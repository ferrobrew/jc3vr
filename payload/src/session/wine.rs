//! Wine's path conversion between the host's Unix namespace and the Windows one the payload opens
//! files in. Both directions are exports of wine's `kernel32` (`wine_get_dos_file_name` and
//! `wine_get_unix_file_name`), resolved at run time so the payload still loads on Windows, where
//! they do not exist.

use std::{
    ffi::{CString, OsString, c_char, c_void},
    os::windows::ffi::{OsStrExt as _, OsStringExt as _},
    path::{Path, PathBuf},
};

use windows::{
    Win32::System::{
        LibraryLoader::{GetModuleHandleW, GetProcAddress},
        Memory::{GetProcessHeap, HEAP_FLAGS, HeapFree},
    },
    core::{PCSTR, s, w},
};

/// Why a path could not be converted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConversionError {
    /// The process is not running under wine (the export is missing).
    NotWine,
    /// The path holds a NUL, which neither namespace allows.
    InteriorNul,
    /// Wine refused the path, or named it in bytes that are not UTF-8.
    Refused,
}

impl std::fmt::Display for ConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NotWine => "wine's path conversion is unavailable (not running under wine)",
            Self::InteriorNul => "the path holds a NUL",
            Self::Refused => "wine could not convert the path",
        })
    }
}

impl std::error::Error for ConversionError {}

/// The Windows path wine maps the Unix path `unix` to.
pub(crate) fn dos_file_name(unix: &str) -> Result<PathBuf, ConversionError> {
    // SAFETY: the export's signature is `WCHAR *CDECL wine_get_dos_file_name(LPCSTR)`.
    let convert: DosFileName =
        unsafe { std::mem::transmute(export(s!("wine_get_dos_file_name"))?) };
    let unix = CString::new(unix).map_err(|_| ConversionError::InteriorNul)?;
    // SAFETY: a NUL-terminated string; the result is null or a NUL-terminated wide string on the
    // process heap, which is ours to free.
    let wide = unsafe { convert(unix.as_ptr()) };
    if wide.is_null() {
        return Err(ConversionError::Refused);
    }
    // SAFETY: `wide` is NUL-terminated, per the export's contract.
    let path = unsafe {
        let len = (0..).take_while(|&i| *wide.add(i) != 0).count();
        OsString::from_wide(std::slice::from_raw_parts(wide, len))
    };
    // SAFETY: allocated by wine on the process heap and not used after this.
    unsafe { free(wide.cast()) };
    Ok(PathBuf::from(path))
}

/// The Unix path wine maps the Windows path `windows` to. Every component but the last must exist.
pub(crate) fn unix_file_name(windows: &Path) -> Result<String, ConversionError> {
    // SAFETY: the export's signature is `char *CDECL wine_get_unix_file_name(LPCWSTR)`.
    let convert: UnixFileName =
        unsafe { std::mem::transmute(export(s!("wine_get_unix_file_name"))?) };
    let wide: Vec<u16> = windows.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return Err(ConversionError::InteriorNul);
    }
    let wide: Vec<u16> = wide.into_iter().chain([0]).collect();
    // SAFETY: a NUL-terminated wide string; the result is null or a NUL-terminated byte string on
    // the process heap, which is ours to free.
    let bytes = unsafe { convert(wide.as_ptr()) };
    if bytes.is_null() {
        return Err(ConversionError::Refused);
    }
    // SAFETY: `bytes` is NUL-terminated, per the export's contract.
    let path = unsafe { std::ffi::CStr::from_ptr(bytes) }
        .to_str()
        .map(str::to_owned);
    // SAFETY: allocated by wine on the process heap and not used after this.
    unsafe { free(bytes.cast()) };
    path.map_err(|_| ConversionError::Refused)
}

type DosFileName = unsafe extern "C" fn(*const c_char) -> *mut u16;
type UnixFileName = unsafe extern "C" fn(*const u16) -> *mut c_char;
type Export = unsafe extern "system" fn() -> isize;

fn export(name: PCSTR) -> Result<Export, ConversionError> {
    // SAFETY: plain lookups; `kernel32` is mapped into every process for its whole life.
    let module =
        unsafe { GetModuleHandleW(w!("kernel32.dll")) }.map_err(|_| ConversionError::NotWine)?;
    unsafe { GetProcAddress(module, name) }.ok_or(ConversionError::NotWine)
}

/// # Safety
/// `ptr` must be a live allocation on the process heap.
unsafe fn free(ptr: *mut c_void) {
    if let Ok(heap) = unsafe { GetProcessHeap() } {
        // A failed free leaks a path's worth of bytes; nothing to recover.
        let _ = unsafe { HeapFree(heap, HEAP_FLAGS(0), Some(ptr)) };
    }
}
