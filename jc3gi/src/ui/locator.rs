#![cfg_attr(any(), rustfmt::skip)]
#[repr(u32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// `CLocator::ELocale`: a game language, indexing the per-language blocks of each loaded string
/// table. `CGame::SetGameLanguage` maps the two-letter language code onto it, falling back to
/// [`LOCALE_ENGLISH`](crate::ui::locator::Locale::LOCALE_ENGLISH) for a code it does not know.
pub enum Locale {
    LOCALE_ENGLISH = 0isize as _,
    LOCALE_FRENCH = 1isize as _,
    LOCALE_GERMAN = 2isize as _,
    LOCALE_ITALIAN = 3isize as _,
    LOCALE_SPANISH = 4isize as _,
    LOCALE_RUSSIAN = 5isize as _,
    LOCALE_POLISH = 6isize as _,
    LOCALE_JAPANESE = 7isize as _,
    LOCALE_BRAZILIAN = 8isize as _,
    LOCALE_MEXICAN = 9isize as _,
    LOCALE_ARABIC = 10isize as _,
}
fn _Locale_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], Locale>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// The localisation string store (`CLocator`, a `Base::CSingle` singleton): owns the loaded string
/// lookup tables (`.stringlookup` resources), each holding a sorted array of `(hash, offset)`
/// pairs per language over one UTF-8 string blob, plus a map of dynamically registered strings.
/// Strings are keyed by the `hashlittle` hash of their key name (`HashString`, as
/// [`HashString`](crate::hash::HashString) stores it); the UI resolves its `buttonhint_*` and other
/// labels through [`GetLocalizedStringRaw`](crate::ui::locator::Locator::GetLocalizedStringRaw).
pub struct Locator {
    _field_0: [u8; 224],
    /// The language whose block every lookup reads. `CGame::SetGameLanguage` writes it from the
    /// language code before switching the UI's language.
    pub m_CurrentLocale: crate::ui::locator::Locale,
    _field_e4: [u8; 4],
}
fn _Locator_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xE8], Locator>([0u8; 0xE8]);
    }
    unreachable!()
}
impl Locator {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5417317624usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl Locator {
    pub const GetLocalizedStringRaw_ADDRESS: usize = 0x140076E00;
    /// Looks up the string for a key hash in the [current language](crate::ui::locator::Locator::m_CurrentLocale):
    /// under the lookups' reader lock it binary-searches each loaded table's block for that
    /// language, then falls back to the dynamic map. Returns a null-terminated UTF-8 string owned
    /// by the table, or null when no table has the key.
    pub unsafe fn GetLocalizedStringRaw(&self, hash: u32) -> *const u8 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                hash: u32,
            ) -> *const u8 = ::std::mem::transmute(Self::GetLocalizedStringRaw_ADDRESS);
            f(self as *const Self as _, hash)
        }
    }
}
impl std::convert::AsRef<Locator> for Locator {
    fn as_ref(&self) -> &Locator {
        self
    }
}
impl std::convert::AsMut<Locator> for Locator {
    fn as_mut(&mut self) -> &mut Locator {
        self
    }
}
