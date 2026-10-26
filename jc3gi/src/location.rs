#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The location streaming system's owned state. Its loaded list is read and changed on the game thread.
pub struct GameLocationSystemContext {
    _field_0: [u8; 4200],
    /// Locations whose loading has completed. Unloading removes an entry and clears its location pointer.
    pub m_LoadedLocations: crate::types::std_vector::Vector<
        *mut crate::location::LocationInfo,
    >,
}
fn _GameLocationSystemContext_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1088], GameLocationSystemContext>([0u8; 0x1088]);
    }
    unreachable!()
}
impl GameLocationSystemContext {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5418086704usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl GameLocationSystemContext {}
impl std::convert::AsRef<GameLocationSystemContext> for GameLocationSystemContext {
    fn as_ref(&self) -> &GameLocationSystemContext {
        self
    }
}
impl std::convert::AsMut<GameLocationSystemContext> for GameLocationSystemContext {
    fn as_mut(&mut self) -> &mut GameLocationSystemContext {
        self
    }
}
#[repr(C, align(8))]
/// A loaded location (`CLocation`), which owns the model collection populated from its model data.
/// Only some members are mapped.
pub struct Location {
    _field_0: [u8; 104],
    /// The model instances loaded with this location, or null when loading has not succeeded.
    pub m_ModelCollection: *mut crate::graphics_engine::model_instance::ModelCollection,
    _field_70: [u8; 48],
    /// The location's streaming detail level.
    pub m_LocationType: crate::location::LocationType,
    _field_a4: [u8; 36],
    /// The archive base name passed to the location loader.
    pub m_LocationFilename: *const u8,
}
fn _Location_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xD0], Location>([0u8; 0xD0]);
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
#[repr(C, align(8))]
/// A location's streaming metadata. The active location is detached before deferred destruction.
pub struct LocationInfo {
    _field_0: [u8; 24],
    /// The active location, or null while no location is owned by this info.
    pub m_Location: *mut crate::location::Location,
}
fn _LocationInfo_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x20], LocationInfo>([0u8; 0x20]);
    }
    unreachable!()
}
impl LocationInfo {}
impl std::convert::AsRef<LocationInfo> for LocationInfo {
    fn as_ref(&self) -> &LocationInfo {
        self
    }
}
impl std::convert::AsMut<LocationInfo> for LocationInfo {
    fn as_mut(&mut self) -> &mut LocationInfo {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The detail level loaded by a location.
pub enum LocationType {
    Invalid = -1isize as _,
    Global = 0isize as _,
    Near = 1isize as _,
    Far = 2isize as _,
    Close = 3isize as _,
    HighRes = 4isize as _,
    Normal = 5isize as _,
    MaxTypes = 6isize as _,
}
fn _LocationType_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], LocationType>([0u8; 0x4]);
    }
    unreachable!()
}
