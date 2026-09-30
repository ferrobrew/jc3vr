#![cfg_attr(any(), rustfmt::skip)]
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The ordering used by [`InputDeviceManager::GetDevice`](crate::input::input_device_manager::InputDeviceManager::GetDevice). Appearance order counts devices of the
/// requested class in the manager's device vector. Hardware-index order matches the index encoded in
/// [`InputDevice::ID`](crate::input::input_device_manager::InputDevice::ID).
pub enum DeviceOrder {
    Appearance = 0isize as _,
    HardwareIndex = 1isize as _,
}
fn _DeviceOrder_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], DeviceOrder>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// An input device class (`Input::EDeviceType`), as passed to the device manager's device lookup
/// and action mapping.
pub enum DeviceType {
    Default = 0isize as _,
    Keyboard = 1isize as _,
    Mouse = 2isize as _,
    Joystick = 3isize as _,
    Gamepad = 4isize as _,
    Memorycard = 5isize as _,
    Microphone = 6isize as _,
    Headphone = 7isize as _,
    Unknown = 8isize as _,
    DeviceTypeCount = 16isize as _,
}
fn _DeviceType_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], DeviceType>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// The common input-device object (`Input::IInputDevice`). Only the identity field is modeled; the
/// concrete device state and object collections are intentionally left opaque.
pub struct InputDevice {
    pub __vftable: u64,
    _field_8: [u8; 40],
    /// The packed device identity used by device lookup and action-effector selection.
    pub ID: crate::input::input_device_manager::InputDeviceId,
    _field_34: [u8; 4],
}
fn _InputDevice_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x38], InputDevice>([0u8; 0x38]);
    }
    unreachable!()
}
impl InputDevice {}
impl std::convert::AsRef<InputDevice> for InputDevice {
    fn as_ref(&self) -> &InputDevice {
        self
    }
}
impl std::convert::AsMut<InputDevice> for InputDevice {
    fn as_mut(&mut self) -> &mut InputDevice {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// The packed identifier stored by every input device. The low nibble is the [`DeviceType`](crate::input::input_device_manager::DeviceType), and the
/// higher bits carry the hardware index and the component that
/// [`InputActionMap::GetActionEffector`](crate::input::input_action_map::InputActionMap::GetActionEffector)
/// takes as its device selector. The remaining bits are device-specific identity data.
pub struct InputDeviceId {
    pub raw: u32,
}
fn _InputDeviceId_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], InputDeviceId>([0u8; 0x4]);
    }
    unreachable!()
}
impl InputDeviceId {}
impl std::convert::AsRef<InputDeviceId> for InputDeviceId {
    fn as_ref(&self) -> &InputDeviceId {
        self
    }
}
impl std::convert::AsMut<InputDeviceId> for InputDeviceId {
    fn as_mut(&mut self) -> &mut InputDeviceId {
        self
    }
}
#[repr(C, align(8))]
pub struct InputDeviceManager {
    vftable: *const crate::input::input_device_manager::InputDeviceManagerVftable,
    _field_8: [u8; 4],
    pub m_Enabled: bool,
    _field_d: [u8; 107],
}
fn _InputDeviceManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x78], InputDeviceManager>([0u8; 0x78]);
    }
    unreachable!()
}
impl InputDeviceManager {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5417121528usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl InputDeviceManager {
    pub fn vftable(
        &self,
    ) -> *const crate::input::input_device_manager::InputDeviceManagerVftable {
        self.vftable
            as *const crate::input::input_device_manager::InputDeviceManagerVftable
    }
    pub const GetDevice_ADDRESS: usize = 0x1402F1F00;
    /// Finds a device of `device_type`. With [`DeviceOrder::Appearance`](crate::input::input_device_manager::DeviceOrder::Appearance), `index` selects the
    /// matching device's position among devices of that class. With [`DeviceOrder::HardwareIndex`](crate::input::input_device_manager::DeviceOrder::HardwareIndex),
    /// `index` selects the hardware index packed into [`InputDevice::ID`](crate::input::input_device_manager::InputDevice::ID). [`DeviceType::Default`](crate::input::input_device_manager::DeviceType::Default)
    /// ignores `order` and returns the `index`-th device in the manager's device vector regardless
    /// of class, falling back to the class search only when `index` is past the end. Returns null
    /// when no device matches.
    pub unsafe fn GetDevice(
        &mut self,
        device_type: crate::input::input_device_manager::DeviceType,
        index: i32,
        order: crate::input::input_device_manager::DeviceOrder,
    ) -> *mut crate::input::input_device_manager::InputDevice {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                device_type: crate::input::input_device_manager::DeviceType,
                index: i32,
                order: crate::input::input_device_manager::DeviceOrder,
            ) -> *mut crate::input::input_device_manager::InputDevice = ::std::mem::transmute(
                Self::GetDevice_ADDRESS,
            );
            f(self as *mut Self as _, device_type, index, order)
        }
    }
    pub const Update_ADDRESS: usize = 0x1402F20C0;
    /// The per-frame device poll: reads all devices and refreshes their effector states (see
    /// [`InputDeviceEffector`](crate::input::input_action_map::InputDeviceEffector)). `CGameState::PreUpdate`
    /// calls it, then [`UpdateForceClicks`](crate::input::input_device_manager::InputDeviceManager::UpdateForceClicks), before the frame's world
    /// update. The poll overwrites effector values written before it runs, and skips the devices when
    /// the clock's real time is unchanged since the previous poll.
    pub unsafe fn Update(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const UpdateForceClicks_ADDRESS: usize = 0x1402F2050;
    /// Applies the force-click flags latched on effectors so an injected click survives the poll.
    pub unsafe fn UpdateForceClicks(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::UpdateForceClicks_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub unsafe fn Destructor(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).Destructor).read();
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<InputDeviceManager> for InputDeviceManager {
    fn as_ref(&self) -> &InputDeviceManager {
        self
    }
}
impl std::convert::AsMut<InputDeviceManager> for InputDeviceManager {
    fn as_mut(&mut self) -> &mut InputDeviceManager {
        self
    }
}
#[repr(C, align(8))]
pub struct InputDeviceManagerVftable {
    pub Destructor: unsafe extern "system" fn(
        this: *mut crate::input::input_device_manager::InputDeviceManager,
    ),
}
fn _InputDeviceManagerVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], InputDeviceManagerVftable>([0u8; 0x8]);
    }
    unreachable!()
}
impl InputDeviceManagerVftable {}
impl std::convert::AsRef<InputDeviceManagerVftable> for InputDeviceManagerVftable {
    fn as_ref(&self) -> &InputDeviceManagerVftable {
        self
    }
}
impl std::convert::AsMut<InputDeviceManagerVftable> for InputDeviceManagerVftable {
    fn as_mut(&mut self) -> &mut InputDeviceManagerVftable {
        self
    }
}
impl InputDeviceId {
    /// Returns the signed hardware index used by the manager's hardware-index lookup. The engine
    /// computes it as `(ID << 2) >> 19` with an arithmetic right shift: the identity's bits 17
    /// through 29, sign-extended from bit 29. Bits 30 and 31 do not take part.
    pub fn hardware_index(&self) -> i32 {
        ((self.raw << 2) as i32) >> 19
    }
    /// Returns the device selector passed to an action-map effector lookup. The high part identifies
    /// the device class, and the low part is the signed packed-ID component `(ID << 15) >> 19`.
    pub fn action_device_index(&self) -> i32 {
        ((self.raw & 0xF) << 16) as i32 | (((self.raw << 15) as i32) >> 19)
    }
}
