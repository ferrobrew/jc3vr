#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A loaded location (`CLocation`), which owns the model collection populated from its model data.
/// Only the fields used by the location model loader are mapped.
pub struct Location {
    _field_0: [u8; 104],
    /// The model instances loaded with this location, or null when loading has not succeeded.
    pub m_ModelCollection: *mut crate::graphics_engine::model_instance::ModelCollection,
}
fn _Location_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x70], Location>([0u8; 0x70]);
    }
    unreachable!()
}
impl Location {
    pub const LoadModelCollection_ADDRESS: usize = 0x140508670;
    /// Loads the location's model collection from a bounded buffer and resource name. A successful
    /// load assigns [`m_ModelCollection`](crate::location::Location::m_ModelCollection); a failed load clears it.
    pub unsafe fn LoadModelCollection(
        &mut self,
        buffer: *const u8,
        size: u32,
        name: *const u8,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                buffer: *const u8,
                size: u32,
                name: *const u8,
            ) = ::std::mem::transmute(Self::LoadModelCollection_ADDRESS);
            f(self as *mut Self as _, buffer, size, name)
        }
    }
}
impl std::convert::AsRef<Location> for Location {
    fn as_ref(&self) -> &Location {
        self
    }
}
impl std::convert::AsMut<Location> for Location {
    fn as_mut(&mut self) -> &mut Location {
        self
    }
}
