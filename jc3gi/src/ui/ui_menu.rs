#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// `CUIMenu`: the menu screens' shared helpers. Only its static helpers are mapped.
pub struct UIMenu {}
impl UIMenu {
    pub const GetWeaponNameById_ADDRESS: usize = 0x140F1E250;
    /// Returns the display name of the weapon type whose name hash is `name_hash`
    /// ([`WeaponBase::m_WeaponNameHash`](crate::equipment::weapon::WeaponBase::m_WeaponNameHash)): the
    /// localisation store's string for that hash in the current language
    /// ([`Locator::GetLocalizedStringRaw`](crate::ui::locator::Locator::GetLocalizedStringRaw)), or an
    /// empty string when the store has none. The HUD names weapons through it, as
    /// [`HUDUI::OnPickupWeapon`](crate::ui::hud_ui::HUDUI::OnPickupWeapon) does.
    pub unsafe fn GetWeaponNameById(name_hash: u32) -> *const u8 {
        unsafe {
            let f: unsafe extern "system" fn(name_hash: u32) -> *const u8 = ::std::mem::transmute(
                Self::GetWeaponNameById_ADDRESS,
            );
            f(name_hash)
        }
    }
}
impl std::convert::AsRef<UIMenu> for UIMenu {
    fn as_ref(&self) -> &UIMenu {
        self
    }
}
impl std::convert::AsMut<UIMenu> for UIMenu {
    fn as_mut(&mut self) -> &mut UIMenu {
        self
    }
}
