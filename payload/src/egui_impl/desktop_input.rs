//! Physical desktop input evidence, independent of egui and the game's device polling.

use std::{ffi::c_void, sync::OnceLock};

use parking_lot::Mutex;
use windows::{
    Win32::{
        Foundation::{FreeLibrary, HMODULE, HWND, LPARAM, WPARAM},
        System::{
            LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
            Threading::GetCurrentThreadId,
        },
        UI::{
            Input::KeyboardAndMouse::{
                GetActiveWindow, GetAsyncKeyState, GetFocus, GetKeyboardState,
            },
            WindowsAndMessaging::{
                GetForegroundWindow, GetWindowThreadProcessId, WM_ACTIVATEAPP, WM_CANCELMODE,
                WM_CAPTURECHANGED, WM_CHAR, WM_CLEAR, WM_COPY, WM_CUT, WM_DEADCHAR, WM_DESTROY,
                WM_IME_COMPOSITION, WM_IME_ENDCOMPOSITION, WM_IME_STARTCOMPOSITION, WM_KEYDOWN,
                WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP,
                WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEWHEEL,
                WM_NCDESTROY, WM_PASTE, WM_RBUTTONDBLCLK, WM_RBUTTONDOWN, WM_RBUTTONUP,
                WM_SETFOCUS, WM_SYSCHAR, WM_SYSDEADCHAR, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_UNICHAR,
                WM_XBUTTONDBLCLK, WM_XBUTTONDOWN, WM_XBUTTONUP,
            },
        },
    },
    core::{s, w},
};

/// An unavailable sample never establishes desktop neutrality.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DesktopInputObservation {
    Unavailable(DesktopInputUnavailable),
    Observed(DesktopInputEvidence),
}

impl DesktopInputObservation {
    pub(crate) fn controls_neutral(self) -> Option<bool> {
        match self {
            Self::Unavailable(_) => None,
            Self::Observed(evidence) => Some(evidence.controls_neutral()),
        }
    }

    /// Native focus is known only for observed samples or an explicit focus failure.
    pub(crate) fn focused(self) -> Option<bool> {
        match self {
            Self::Observed(_) => Some(true),
            Self::Unavailable(DesktopInputUnavailable::Unfocused) => Some(false),
            Self::Unavailable(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DesktopInputUnavailable {
    WindowNotObserved,
    WrongThread,
    Unfocused,
    KeyboardStateUnavailable,
    ImeStateUnavailable,
    BaselinePending,
    ConcurrentActivity,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DesktopModifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub windows: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DesktopInputEvidence {
    pub sequence: u64,
    pub held_virtual_keys: u16,
    pub held_mouse_buttons: u8,
    pub modifiers: DesktopModifiers,
    pub key_activity: bool,
    pub mouse_button_activity: bool,
    pub scroll_activity: bool,
    pub text_activity: bool,
    pub composition_active: bool,
    pub consumed_shortcuts: u16,
}

impl DesktopInputEvidence {
    pub(crate) fn controls_neutral(self) -> bool {
        self.held_virtual_keys == 0
            && self.held_mouse_buttons == 0
            && self.modifiers == DesktopModifiers::default()
            && !self.key_activity
            && !self.mouse_button_activity
            && !self.scroll_activity
            && !self.text_activity
            && !self.composition_active
            && self.consumed_shortcuts == 0
    }
}

/// Sample once before the capture policy, including while editing disables game-device polling.
/// The native calls use the window thread's keyboard state and the asynchronous held-state bit.
pub(crate) fn desktop_input_observation() -> DesktopInputObservation {
    let (window, revision) = {
        let state = DESKTOP_INPUT.lock();
        (state.window, state.revision)
    };
    let native = native_snapshot(window);
    let mut state = DESKTOP_INPUT.lock();
    if state.window != window || state.revision != revision {
        return DesktopInputObservation::Unavailable(DesktopInputUnavailable::ConcurrentActivity);
    }
    state.sample(native)
}

/// Observe before any shortcut, egui, or game filtering. Returns true for a consumed key release.
pub(crate) fn observe_desktop_message(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> bool {
    DESKTOP_INPUT
        .lock()
        .message_with_lparam(hwnd.0 as usize, msg, wparam.0, lparam.0)
}

/// Retain the shortcut episode even when its key-down never reaches egui or the game.
pub(crate) fn consume_desktop_shortcut(virtual_key: usize) {
    DESKTOP_INPUT.lock().consume_shortcut(virtual_key);
}

pub(super) fn focus_loss_generation() -> u64 {
    DESKTOP_INPUT.lock().focus_loss_generation
}

pub(super) fn reset_desktop_input() {
    *DESKTOP_INPUT.lock() = DesktopInputTracker::new();
}

static DESKTOP_INPUT: Mutex<DesktopInputTracker> = Mutex::new(DesktopInputTracker::new());

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Keys([u64; 4]);

impl Keys {
    fn insert(&mut self, key: usize) {
        if key < VIRTUAL_KEY_COUNT {
            self.0[key / u64::BITS as usize] |= 1 << (key % u64::BITS as usize);
        }
    }

    fn remove(&mut self, key: usize) {
        if key < VIRTUAL_KEY_COUNT {
            self.0[key / u64::BITS as usize] &= !(1 << (key % u64::BITS as usize));
        }
    }

    fn contains(self, key: usize) -> bool {
        key < VIRTUAL_KEY_COUNT
            && self.0[key / u64::BITS as usize] & (1 << (key % u64::BITS as usize)) != 0
    }

    fn union(self, other: Self) -> Self {
        Self(std::array::from_fn(|index| self.0[index] | other.0[index]))
    }

    fn intersection(self, other: Self) -> Self {
        Self(std::array::from_fn(|index| self.0[index] & other.0[index]))
    }

    fn count(self) -> u16 {
        self.0.iter().map(|word| word.count_ones() as u16).sum()
    }
}

const VIRTUAL_KEY_COUNT: usize = 256;
const MOUSE_KEYS: [usize; 5] = [0x01, 0x02, 0x04, 0x05, 0x06];

#[derive(Clone, Copy, Debug)]
struct NativeSnapshot {
    held: Keys,
    composition_active: bool,
}

fn native_snapshot(window: Option<usize>) -> Result<NativeSnapshot, DesktopInputUnavailable> {
    let window = window
        .filter(|window| *window != 0)
        .ok_or(DesktopInputUnavailable::WindowNotObserved)?;
    let hwnd = HWND(window as *mut c_void);
    // SAFETY: these APIs validate an HWND; the cached handle is never dereferenced by Rust.
    let thread = unsafe { GetWindowThreadProcessId(hwnd, None) };
    if thread == 0 {
        return Err(DesktopInputUnavailable::WindowNotObserved);
    }
    // SAFETY: the native keyboard queue belongs to the calling window thread.
    if thread != unsafe { GetCurrentThreadId() } {
        return Err(DesktopInputUnavailable::WrongThread);
    }
    if !native_focus(hwnd) {
        return Err(DesktopInputUnavailable::Unfocused);
    }
    let mut keyboard = [0; VIRTUAL_KEY_COUNT];
    // SAFETY: the API receives its required complete virtual-key buffer.
    unsafe { GetKeyboardState(&mut keyboard) }
        .map_err(|_| DesktopInputUnavailable::KeyboardStateUnavailable)?;
    let mut held = Keys::default();
    for (key, state) in keyboard
        .iter()
        .enumerate()
        .skip(1)
        .take(VIRTUAL_KEY_COUNT - 2)
    {
        // The low asynchronous bit is shared process-wide and cannot establish a held key.
        // SAFETY: every queried key is inside the Win32 virtual-key namespace.
        let asynchronous = unsafe { GetAsyncKeyState(key as i32) };
        if state & 0x80 != 0 || asynchronous as u16 & 0x8000 != 0 {
            held.insert(key);
        }
    }
    let composition_active = native_composition(hwnd)?;
    if !native_focus(hwnd) {
        return Err(DesktopInputUnavailable::Unfocused);
    }
    Ok(NativeSnapshot {
        held,
        composition_active,
    })
}

pub(super) fn native_focus(hwnd: HWND) -> bool {
    !hwnd.0.is_null()
        // SAFETY: these calls return handles owned by the OS, without dereferencing game memory.
        && unsafe { GetForegroundWindow() == hwnd && GetActiveWindow() == hwnd && GetFocus() == hwnd }
}

/// IMM is resolved from the system module without adding a new workspace dependency feature.
/// The retained module reference keeps the cached public Win32 API pointers valid.
fn native_composition(hwnd: HWND) -> Result<bool, DesktopInputUnavailable> {
    let api = IMM_API
        .get_or_init(|| {
            let result = load_imm_api();
            if let Err(error) = &result {
                tracing::warn!(
                    target: "desktop_input", api = error.api, error = ?error.source,
                    "desktop input: IMM API initialization is unavailable"
                );
            }
            result
        })
        .as_ref()
        .map_err(|_| DesktopInputUnavailable::ImeStateUnavailable)?;
    // SAFETY: the signatures are the public IMM API; the context is released before returning.
    unsafe {
        let context = (api.get_context)(hwnd);
        if context.is_null() {
            return Ok(false);
        }
        let length = (api.composition_string)(context, GCS_COMPSTR, std::ptr::null_mut(), 0);
        let released = (api.release_context)(hwnd, context) != 0;
        composition_from_native(length, released)
    }
}

fn composition_from_native(length: i32, released: bool) -> Result<bool, DesktopInputUnavailable> {
    if !released {
        return Err(DesktopInputUnavailable::ImeStateUnavailable);
    }
    match length {
        IMM_ERROR_NODATA | 0 => Ok(false),
        length if length > 0 => Ok(true),
        _ => Err(DesktopInputUnavailable::ImeStateUnavailable),
    }
}

const GCS_COMPSTR: u32 = 0x0008;
const IMM_ERROR_NODATA: i32 = -1;
type GetImeContext = unsafe extern "system" fn(HWND) -> *mut c_void;
type ReleaseImeContext = unsafe extern "system" fn(HWND, *mut c_void) -> i32;
type GetCompositionString = unsafe extern "system" fn(*mut c_void, u32, *mut c_void, u32) -> i32;

struct ImmApi {
    get_context: GetImeContext,
    release_context: ReleaseImeContext,
    composition_string: GetCompositionString,
}

#[derive(Debug)]
struct ImmLoadError {
    api: &'static str,
    source: windows::core::Error,
}

struct ImmModuleReference(HMODULE);

impl Drop for ImmModuleReference {
    fn drop(&mut self) {
        // SAFETY: this guard owns one successful LoadLibraryExW reference.
        if let Err(error) = unsafe { FreeLibrary(self.0) } {
            tracing::warn!(target: "desktop_input", api = "FreeLibrary", ?error,
                "desktop input: the failed IMM load could not release its module reference");
        }
    }
}

static IMM_API: OnceLock<Result<ImmApi, ImmLoadError>> = OnceLock::new();

fn load_imm_api() -> Result<ImmApi, ImmLoadError> {
    // SAFETY: only the OS system directory is searched; the guard releases all failed resolutions.
    unsafe {
        let module = ImmModuleReference(
            LoadLibraryExW(w!("imm32.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).map_err(
                |source| ImmLoadError {
                    api: "LoadLibraryExW(imm32.dll)",
                    source,
                },
            )?,
        );
        let get_context =
            GetProcAddress(module.0, s!("ImmGetContext")).ok_or_else(|| ImmLoadError {
                api: "ImmGetContext",
                source: windows::core::Error::from_thread(),
            })?;
        let release_context =
            GetProcAddress(module.0, s!("ImmReleaseContext")).ok_or_else(|| ImmLoadError {
                api: "ImmReleaseContext",
                source: windows::core::Error::from_thread(),
            })?;
        let composition_string = GetProcAddress(module.0, s!("ImmGetCompositionStringW"))
            .ok_or_else(|| ImmLoadError {
                api: "ImmGetCompositionStringW",
                source: windows::core::Error::from_thread(),
            })?;
        let api = ImmApi {
            get_context: std::mem::transmute::<unsafe extern "system" fn() -> isize, GetImeContext>(
                get_context,
            ),
            release_context: std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                ReleaseImeContext,
            >(release_context),
            composition_string: std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                GetCompositionString,
            >(composition_string),
        };
        std::mem::forget(module);
        Ok(api)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Activity {
    key: bool,
    mouse: bool,
    scroll: bool,
    text: bool,
}

struct DesktopInputTracker {
    window: Option<usize>,
    revision: u64,
    sequence: u64,
    focus_loss_generation: u64,
    baseline: bool,
    message_held: Keys,
    consumed: Keys,
    release_to_consume: Keys,
    physically_released: Keys,
    activity: Activity,
    composition_active: bool,
}

impl DesktopInputTracker {
    const fn new() -> Self {
        Self {
            window: None,
            revision: 0,
            sequence: 0,
            focus_loss_generation: 0,
            baseline: false,
            message_held: Keys([0; 4]),
            consumed: Keys([0; 4]),
            release_to_consume: Keys([0; 4]),
            physically_released: Keys([0; 4]),
            activity: Activity {
                key: false,
                mouse: false,
                scroll: false,
                text: false,
            },
            composition_active: false,
        }
    }

    fn consume_shortcut(&mut self, virtual_key: usize) {
        self.consumed.insert(virtual_key);
        self.release_to_consume.insert(virtual_key);
        self.physically_released.remove(virtual_key);
        self.activity.key = true;
        self.revision = self.revision.wrapping_add(1);
    }

    #[cfg(test)]
    fn message(&mut self, window: usize, msg: u32, wparam: usize) -> bool {
        self.message_with_lparam(window, msg, wparam, 0)
    }

    fn message_with_lparam(
        &mut self,
        window: usize,
        msg: u32,
        wparam: usize,
        lparam: isize,
    ) -> bool {
        if self.window != Some(window) {
            *self = Self {
                window: Some(window),
                revision: self.revision.wrapping_add(1),
                focus_loss_generation: self.focus_loss_generation.wrapping_add(1),
                ..Self::new()
            };
        }
        if msg == WM_KILLFOCUS || (msg == WM_ACTIVATEAPP && wparam == 0) {
            self.focus_loss_generation = self.focus_loss_generation.wrapping_add(1);
        }
        let consumed_release =
            matches!(msg, WM_KEYUP | WM_SYSKEYUP) && self.release_to_consume.contains(wparam);
        if consumed_release {
            self.release_to_consume.remove(wparam);
        }
        match msg {
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let first_press = lparam & 0x4000_0000 == 0;
                if first_press && self.physically_released.contains(wparam) {
                    self.release_to_consume.remove(wparam);
                    self.consumed.remove(wparam);
                }
                self.physically_released.remove(wparam);
                self.message_held.insert(wparam);
                self.activity.key = true;
            }
            WM_KEYUP | WM_SYSKEYUP => {
                self.message_held.remove(wparam);
                self.physically_released.insert(wparam);
                self.activity.key = true;
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => self.mouse_button(0x01, true),
            WM_LBUTTONUP => self.mouse_button(0x01, false),
            WM_RBUTTONDOWN | WM_RBUTTONDBLCLK => self.mouse_button(0x02, true),
            WM_RBUTTONUP => self.mouse_button(0x02, false),
            WM_MBUTTONDOWN | WM_MBUTTONDBLCLK => self.mouse_button(0x04, true),
            WM_MBUTTONUP => self.mouse_button(0x04, false),
            WM_XBUTTONDOWN | WM_XBUTTONDBLCLK | WM_XBUTTONUP => {
                let key = if wparam >> 16 & 0xffff == 1 {
                    0x05
                } else {
                    0x06
                };
                self.mouse_button(key, msg != WM_XBUTTONUP);
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => self.activity.scroll = true,
            WM_IME_STARTCOMPOSITION | WM_IME_COMPOSITION => {
                self.composition_active = true;
                self.activity.text = true;
            }
            WM_IME_ENDCOMPOSITION => {
                self.composition_active = false;
                self.activity.text = true;
            }
            WM_CHAR | WM_SYSCHAR | WM_DEADCHAR | WM_SYSDEADCHAR | WM_UNICHAR | WM_PASTE
            | WM_CUT | WM_COPY | WM_CLEAR => self.activity.text = true,
            WM_SETFOCUS | WM_KILLFOCUS | WM_ACTIVATEAPP | WM_CANCELMODE | WM_CAPTURECHANGED => {
                self.baseline = false;
            }
            WM_DESTROY | WM_NCDESTROY => {
                self.window = None;
                self.baseline = false;
            }
            _ => return consumed_release,
        }
        self.revision = self.revision.wrapping_add(1);
        consumed_release
    }

    fn mouse_button(&mut self, key: usize, pressed: bool) {
        if pressed {
            self.message_held.insert(key);
        } else {
            self.message_held.remove(key);
        }
        self.activity.mouse = true;
    }

    fn sample(
        &mut self,
        native: Result<NativeSnapshot, DesktopInputUnavailable>,
    ) -> DesktopInputObservation {
        self.sequence = self.sequence.wrapping_add(1);
        let native = match native {
            Ok(native) => native,
            Err(reason) => {
                self.baseline = false;
                return DesktopInputObservation::Unavailable(reason);
            }
        };
        for key in 1..VIRTUAL_KEY_COUNT - 1 {
            if !native.held.contains(key) {
                self.physically_released.insert(key);
            }
        }
        if !self.baseline {
            self.baseline = true;
            self.message_held = Keys::default();
            return DesktopInputObservation::Unavailable(DesktopInputUnavailable::BaselinePending);
        }
        let held = native.held.union(self.message_held);
        let mouse = MOUSE_KEYS
            .into_iter()
            .filter(|key| held.contains(*key))
            .count() as u8;
        let evidence = DesktopInputEvidence {
            sequence: self.sequence,
            held_virtual_keys: held.count() - u16::from(mouse),
            held_mouse_buttons: mouse,
            modifiers: DesktopModifiers {
                shift: [0x10, 0xa0, 0xa1].into_iter().any(|key| held.contains(key)),
                control: [0x11, 0xa2, 0xa3].into_iter().any(|key| held.contains(key)),
                alt: [0x12, 0xa4, 0xa5].into_iter().any(|key| held.contains(key)),
                windows: [0x5b, 0x5c].into_iter().any(|key| held.contains(key)),
            },
            key_activity: self.activity.key,
            mouse_button_activity: self.activity.mouse,
            scroll_activity: self.activity.scroll,
            text_activity: self.activity.text,
            composition_active: self.composition_active || native.composition_active,
            consumed_shortcuts: self.consumed.count(),
        };
        self.activity = Activity::default();
        self.message_held = self.message_held.intersection(native.held);
        self.consumed = self.consumed.intersection(held);
        DesktopInputObservation::Observed(evidence)
    }
}

#[cfg(test)]
#[path = "desktop_input_tests.rs"]
mod tests;
