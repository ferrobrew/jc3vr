#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The common steering interface passed to gameplay and UI input handlers. Its action map stores
/// the device effectors that the steering readers query.
pub struct Steering {
    /// The common steering vtable.
    pub __vftable: u64,
    /// The action map used by the steering readers to resolve input actions.
    pub ActionMap: *mut crate::input::input_action_map::InputActionMap,
}
fn _Steering_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], Steering>([0u8; 0x10]);
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
