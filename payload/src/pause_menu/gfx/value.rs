//! The owned Scaleform value.

use std::{
    ffi::{CStr, c_void},
    marker::PhantomData,
};

use jc3gi::ui::{
    scaleform::{Value, ValueObjectInterface},
    ui_base::UIBase,
};

use crate::pause_menu::{gfx::CaptureThread, model::command::Arg};

/// An AS3 value the mod owns: a heap-pinned [`Value`] that is released through its movie's object
/// interface when dropped.
///
/// A managed value is a node on its movie's external-references list, so it must not move once the
/// engine has filled it; the box pins it. The `'t` borrow of a [`CaptureThread`] keeps the value
/// on the capture thread and inside the hook call that made it, so no managed value is held across
/// frames (the movie may be unloaded in between).
pub(crate) struct GfxValue<'t> {
    value: Box<Value>,
    _thread: PhantomData<&'t CaptureThread>,
}

/// An unmanaged value passed into the movie: the movie copies it, and nothing needs releasing.
#[derive(Copy, Clone, Debug)]
pub(crate) enum Param<'a> {
    Int(i32),
    Bool(bool),
    Str(&'a CStr),
}

impl<'t> GfxValue<'t> {
    /// An undefined value, ready to be filled by the engine.
    pub(crate) fn undefined(_thread: &'t CaptureThread) -> Self {
        Self::wrap(unmanaged(0, 0))
    }

    /// A new reference to the managed value `source` (the engine's `GFx::Value` copy): the copy
    /// adds a reference, and dropping it releases only that reference. An unmanaged `source` is
    /// copied as is.
    ///
    /// # Safety
    /// `source` must be a live value of a movie that outlives `'t`, such as a loaded screen's
    /// [`UIBase::m_Clip`].
    pub(crate) unsafe fn copy_of(_thread: &'t CaptureThread, source: &Value) -> Self {
        let mut copy = unmanaged(source.Type, source.mValue);
        copy.pObjectInterface = source.pObjectInterface;
        copy.DataAux = source.DataAux;
        let mut this = Self::wrap(copy);
        if this.is_managed() {
            // SAFETY: the copy carries the source's interface and payload, as `ObjectAddRef`
            // requires; the box pins it before the engine links it.
            unsafe {
                let data = this.value.mValue as *mut c_void;
                (*this.value.pObjectInterface).ObjectAddRef(this.value.as_mut(), data);
            }
        }
        this
    }

    /// A new empty AS3 `Object` in `ui`'s movie.
    pub(crate) fn create_object(thread: &'t CaptureThread, ui: &mut UIBase) -> Option<Self> {
        let mut value = Self::undefined(thread);
        // SAFETY: `ui` is a live screen whose movie exists (the caller holds its clip); the
        // capture-thread token makes this the engine's own thread for movie calls.
        unsafe { ui.CreateObject(value.value.as_mut()) };
        value.is_object().then_some(value)
    }

    /// A new empty AS3 `Array` in `ui`'s movie.
    pub(crate) fn create_array(thread: &'t CaptureThread, ui: &mut UIBase) -> Option<Self> {
        let mut value = Self::undefined(thread);
        // SAFETY: as `create_object`.
        unsafe { ui.CreateArray(value.value.as_mut()) };
        value.is_object().then_some(value)
    }

    /// Whether this is a managed object (an object, array, or display object) the object interface
    /// can operate on.
    pub(crate) fn is_object(&self) -> bool {
        self.is_managed() && self.value.mValue != 0
    }

    /// The member `name`, whatever its type.
    pub(crate) fn member(&self, name: &CStr) -> Option<GfxValue<'t>> {
        let (interface, data, display) = self.object()?;
        let mut out = Self::wrap(unmanaged(0, 0));
        // SAFETY: `object` checked this is a live managed object; `out` is pinned and unmanaged.
        let found = unsafe {
            (*interface).GetMember(data, name.as_ptr().cast(), out.value.as_mut(), display)
        };
        found.then_some(out)
    }

    /// The object at the dot-separated member path `path`, such as
    /// `"MCI_panel.MCI_container.MCI_root"`. Every step must be an object.
    pub(crate) fn path(&self, path: &str) -> Option<GfxValue<'t>> {
        let mut current: Option<GfxValue<'t>> = None;
        for segment in path.split('.') {
            let name = std::ffi::CString::new(segment).ok()?;
            let next = current.as_ref().unwrap_or(self).member(&name)?;
            if !next.is_object() {
                return None;
            }
            current = Some(next);
        }
        current
    }

    /// Set the member `name` to the unmanaged `param`.
    pub(crate) fn set(&self, name: &CStr, param: Param<'_>) -> bool {
        let value = param_value(param);
        self.set_raw(name, &value)
    }

    /// Set the member `name` to `value` (the movie adds its own reference).
    pub(crate) fn set_value(&self, name: &CStr, value: &GfxValue<'_>) -> bool {
        self.set_raw(name, &value.value)
    }

    /// Call the method `name` with `params`. Returns the result (undefined for a void method), or
    /// `None` when the call failed.
    pub(crate) fn invoke(&self, name: &CStr, params: &[Param<'_>]) -> Option<GfxValue<'t>> {
        let (interface, data, display) = self.object()?;
        let args: Vec<Value> = params.iter().copied().map(param_value).collect();
        let mut result = Self::wrap(unmanaged(0, 0));
        // SAFETY: a live managed object; the arguments are unmanaged values the movie copies; the
        // result is pinned and unmanaged until the engine fills it.
        let ok = unsafe {
            (*interface).Invoke(
                data,
                result.value.as_mut(),
                name.as_ptr().cast(),
                args.as_ptr(),
                args.len() as u64,
                display,
            )
        };
        ok.then_some(result)
    }

    /// Element `index` of this array.
    pub(crate) fn element(&self, index: u32) -> Option<GfxValue<'t>> {
        let (interface, data, _) = self.object()?;
        let mut out = Self::wrap(unmanaged(0, 0));
        // SAFETY: as `member`.
        let found = unsafe { (*interface).GetElement(data, index, out.value.as_mut()) };
        found.then_some(out)
    }

    /// Set element `index` of this array to `value`.
    pub(crate) fn set_element(&self, index: u32, value: &GfxValue<'_>) -> bool {
        let Some((interface, data, _)) = self.object() else {
            return false;
        };
        // SAFETY: as `member`; the movie adds its own reference to `value`.
        unsafe { (*interface).SetElement(data, index, value.value.as_ref()) }
    }

    /// Append the unmanaged `param` to this array.
    pub(crate) fn push(&self, param: Param<'_>) -> bool {
        let value = param_value(param);
        self.push_raw(&value)
    }

    /// Append `value` to this array.
    pub(crate) fn push_value(&self, value: &GfxValue<'_>) -> bool {
        self.push_raw(&value.value)
    }

    /// The value as an integer: an int, or a number with an exact integer value.
    pub(crate) fn as_int(&self) -> Option<i32> {
        decode_arg(&self.value).as_int()
    }

    /// The value as a boolean.
    pub(crate) fn as_bool(&self) -> Option<bool> {
        self.value.as_bool()
    }

    /// The value as a string, for either a managed or an unmanaged string.
    pub(crate) fn as_str(&self) -> Option<&CStr> {
        let pointer = self.value.as_string()?;
        let pointer = if self.is_managed() {
            // SAFETY: a managed string's payload points at the movie's string-data pointer, as
            // the engine's own command handlers read it.
            unsafe { *(pointer as *const *const std::ffi::c_char) }
        } else {
            pointer
        };
        // SAFETY: a string payload points at NUL-terminated character data that lives as long as
        // this value holds its reference.
        (!pointer.is_null()).then(|| unsafe { CStr::from_ptr(pointer) })
    }

    fn wrap(value: Value) -> Self {
        Self {
            value: Box::new(value),
            _thread: PhantomData,
        }
    }

    fn is_managed(&self) -> bool {
        self.value.Type & Value::VTC_MANAGED_BIT != 0 && !self.value.pObjectInterface.is_null()
    }

    /// The object interface, payload, and display-object flag the interface calls take.
    /// The interface is its movie's, alive while this value holds a reference (and the token
    /// keeps the value inside the hook call), so callers may dereference it.
    fn object(&self) -> Option<(*mut ValueObjectInterface, *mut c_void, bool)> {
        if !self.is_object() {
            return None;
        }
        let interface = self.value.pObjectInterface;
        let display = self.value.value_type() == Value::VT_DISPLAY_OBJECT;
        Some((interface, self.value.mValue as *mut c_void, display))
    }

    fn set_raw(&self, name: &CStr, value: &Value) -> bool {
        let Some((interface, data, display)) = self.object() else {
            return false;
        };
        // SAFETY: a live managed object; the movie copies `value`.
        unsafe { (*interface).SetMember(data, name.as_ptr().cast(), value, display) }
    }

    fn push_raw(&self, value: &Value) -> bool {
        let Some((interface, data, _)) = self.object() else {
            return false;
        };
        // SAFETY: as `set_raw`.
        unsafe { (*interface).PushBack(data, value) }
    }
}

impl Drop for GfxValue<'_> {
    fn drop(&mut self) {
        if self.is_managed() {
            // SAFETY: the value is pinned, managed, and still holds the reference it was filled or
            // copied with; the token keeps the drop on the capture thread.
            unsafe {
                let data = self.value.mValue as *mut c_void;
                (*self.value.pObjectInterface).ObjectRelease(self.value.as_mut(), data);
            }
        }
    }
}

/// Decode a command argument from the movie.
pub(crate) fn decode_arg(value: &Value) -> Arg {
    if let Some(int) = value.as_int() {
        Arg::Int(int)
    } else if let Some(number) = value.as_number() {
        Arg::Number(number)
    } else if let Some(boolean) = value.as_bool() {
        Arg::Bool(boolean)
    } else {
        Arg::Other
    }
}

/// An unmanaged value of type `value_type` with payload `bits`, laid out as the engine lays out its
/// own stack values (list pointers set to the not-linked sentinel, no interface).
fn unmanaged(value_type: u32, bits: u64) -> Value {
    // SAFETY: all-zero is a valid unmanaged undefined value; every field tolerates zero.
    let mut value: Value = unsafe { std::mem::zeroed() };
    value.pPrev = usize::MAX as *mut Value;
    value.pNext = usize::MAX as *mut Value;
    value.Type = value_type;
    value.mValue = bits;
    value
}

/// The unmanaged value for `param`.
fn param_value(param: Param<'_>) -> Value {
    match param {
        Param::Int(value) => unmanaged(Value::VT_INT, value as u32 as u64),
        Param::Bool(value) => unmanaged(Value::VT_BOOLEAN, u64::from(value)),
        Param::Str(value) => unmanaged(Value::VT_STRING, value.as_ptr() as u64),
    }
}
