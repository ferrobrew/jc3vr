#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The common steering interface (`CSteering`) passed to gameplay and UI input handlers. Its
/// readers (`IsClicked`, `IsPressed`, `IsReleased`, `GetValue`, and the rest of the vtable) resolve
/// each action through
/// [`InputActionMap::GetActionEffector`](crate::input::input_action_map::InputActionMap::GetActionEffector)
/// on [`ActionMap`](crate::input::steering::Steering::ActionMap) with [`m_DeviceIndex`](crate::input::steering::Steering::m_DeviceIndex) as the device
/// selector. While any of the steering's own blocker bits is set and its blocker override is clear,
/// the readers report nothing for actions outside the steering's exception list.
pub struct Steering {
    /// The common steering vtable.
    pub __vftable: u64,
    /// The action map used by the steering readers to resolve input actions.
    pub ActionMap: *mut crate::input::input_action_map::InputActionMap,
    _field_10: [u8; 32],
    /// The device selector every steering reader passes to the action-map lookup: a packed
    /// selector like the one derived from an
    /// [`InputDeviceId`](crate::input::input_device_manager::InputDeviceId), or `-1`, which the lookup
    /// treats as any device.
    pub m_DeviceIndex: i32,
    _field_34: [u8; 4],
}
fn _Steering_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x38], Steering>([0u8; 0x38]);
    }
    unreachable!()
}
impl Steering {}
impl std::convert::AsRef<Steering> for Steering {
    fn as_ref(&self) -> &Steering {
        self
    }
}
impl std::convert::AsMut<Steering> for Steering {
    fn as_mut(&mut self) -> &mut Steering {
        self
    }
}
