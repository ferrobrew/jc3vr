#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The supply-drop store's screen (`CCommStoreUI`). Its layout is unmapped.
pub struct CommStoreUI {}
impl CommStoreUI {
    pub const OnInactive_ADDRESS: usize = 0x140F035E0;
    /// The store's close (a virtual override): when the store delivered something while the
    /// planted explosive is in use, it stops using the planted explosive and selects a valid weapon
    /// ([`Inventory::SelectValidWeapon`](crate::character::inventory::Inventory::SelectValidWeapon)).
    pub unsafe fn OnInactive(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::OnInactive_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<CommStoreUI> for CommStoreUI {
    fn as_ref(&self) -> &CommStoreUI {
        self
    }
}
impl std::convert::AsMut<CommStoreUI> for CommStoreUI {
    fn as_mut(&mut self) -> &mut CommStoreUI {
        self
    }
}
