#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A connected XInput gamepad (`Input::CGamepad`), as returned by
/// [`InputDeviceManager::GetDevice`](crate::input::input_device_manager::InputDeviceManager::GetDevice)
/// with [`DeviceType::Gamepad`](crate::input::input_device_manager::DeviceType::Gamepad). Its layout
/// beyond the common device header is unmapped.
pub struct Gamepad {
    pub base: crate::input::input_device_manager::InputDevice,
}
fn _Gamepad_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x38], Gamepad>([0u8; 0x38]);
    }
    unreachable!()
}
impl Gamepad {
    pub const SetMotorValue_ADDRESS: usize = 0x1402EC1C0;
    /// Sets one vibration motor's strength (`0..=1`, scaled to the 16-bit XInput range) and pushes
    /// the gamepad's full vibration state to XInput. `motor` 0 is the low-frequency motor and 1 is
    /// the high-frequency motor; any other value is unsupported. Does nothing on a disconnected
    /// gamepad.
    ///
    /// [`RumbleManager::Update`](crate::rumble_manager::RumbleManager::Update) calls it once per motor
    /// every frame with the manager's summed, clamped strength for that motor, so a frame with
    /// nothing rumbling still calls it, with zero.
    pub unsafe fn SetMotorValue(&mut self, motor: i32, value: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, motor: i32, value: f32) = ::std::mem::transmute(
                Self::SetMotorValue_ADDRESS,
            );
            f(self as *mut Self as _, motor, value)
        }
    }
}
impl std::convert::AsRef<crate::input::input_device_manager::InputDevice> for Gamepad {
    fn as_ref(&self) -> &crate::input::input_device_manager::InputDevice {
        &self.base
    }
}
impl std::convert::AsMut<crate::input::input_device_manager::InputDevice> for Gamepad {
    fn as_mut(&mut self) -> &mut crate::input::input_device_manager::InputDevice {
        &mut self.base
    }
}
impl std::convert::AsRef<Gamepad> for Gamepad {
    fn as_ref(&self) -> &Gamepad {
        self
    }
}
impl std::convert::AsMut<Gamepad> for Gamepad {
    fn as_mut(&mut self) -> &mut Gamepad {
        self
    }
}
