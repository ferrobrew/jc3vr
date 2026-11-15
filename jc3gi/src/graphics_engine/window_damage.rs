#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A block of per-instance render metadata (`CMetaData`): a model instance's render info points at
/// one through its tagged metadata word ([`ModelInstance::m_MetaFlags`](crate::graphics_engine::model_instance::ModelInstance::m_MetaFlags)),
/// and the render blocks that understand a kind read it while drawing the instance. Blocks are
/// reference counted, and one released while referenced is destroyed only on a later frame, from a
/// deletion list, so a block found through a live instance stays valid for the frame it was found
/// in.
pub struct MetaData {
    /// The block's kind (`CMetaData::GetMetaDataType` returns it).
    pub m_Type: crate::graphics_engine::window_damage::MetaDataType,
    _field_2: [u8; 6],
}
fn _MetaData_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], MetaData>([0u8; 0x8]);
    }
    unreachable!()
}
impl MetaData {}
impl std::convert::AsRef<MetaData> for MetaData {
    fn as_ref(&self) -> &MetaData {
        self
    }
}
impl std::convert::AsMut<MetaData> for MetaData {
    fn as_mut(&mut self) -> &mut MetaData {
        self
    }
}
#[repr(u16)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The kind of a [`MetaData`](crate::graphics_engine::window_damage::MetaData) block (`CMetaData`'s type word). Only the kinds read here are bound.
pub enum MetaDataType {
    Window = 512isize as _,
}
fn _MetaDataType_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x2], MetaDataType>([0u8; 0x2]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// A window's metadata (`CMetaWindow`): the damage drawn on a window model instance's glass. The
/// window render block ([`RenderBlockWindow`](crate::graphics_engine::render_block::RenderBlockWindow))
/// passes its [`m_WindowDamage`](crate::graphics_engine::window_damage::MetaWindow::m_WindowDamage) point list to the window shader each time it
/// draws the instance. Damage comes from two paths: a bullet that hits a shape of the glass material
/// ([`EGameMaterialId_Glass`](crate::physics::GameMaterialId::EGameMaterialId_Glass)) adds a point where it
/// hit, through [`AddWindowDamagePoint`](crate::graphics_engine::window_damage::MetaWindow::AddWindowDamagePoint), deferred to the start of the
/// ammunition manager's render update because bullets update on worker threads; and the vehicle
/// part code adds points to a part's window metadata through the same function.
pub struct MetaWindow {
    pub base: crate::graphics_engine::window_damage::MetaData,
    /// The window's damage points.
    pub m_WindowDamage: crate::graphics_engine::window_damage::WindowContactPointList,
}
fn _MetaWindow_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x418], MetaWindow>([0u8; 0x418]);
    }
    unreachable!()
}
impl MetaWindow {
    pub const AddWindowDamagePoint_ADDRESS: usize = 0x1400A59E0;
    /// `CMetaWindow::AddWindowDamagePoint`: adds a damage point to
    /// [`m_WindowDamage`](crate::graphics_engine::window_damage::MetaWindow::m_WindowDamage) through
    /// [`WindowContactPointList::AddDamagePoint`](crate::graphics_engine::window_damage::WindowContactPointList::AddDamagePoint). `local_position` is in the window model
    /// instance's own space (the bullet path maps its world hit through the instance's matrix,
    /// [`ModelInstance::GetMatrix`](crate::graphics_engine::model_instance::ModelInstance::GetMatrix)).
    /// A bullet adds a fixed strength of one fifth.
    pub unsafe fn AddWindowDamagePoint(
        &mut self,
        local_position: *const crate::types::math::Vector3,
        damage: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                local_position: *const crate::types::math::Vector3,
                damage: f32,
            ) = ::std::mem::transmute(Self::AddWindowDamagePoint_ADDRESS);
            f(self as *mut Self as _, local_position, damage)
        }
    }
}
impl std::convert::AsRef<crate::graphics_engine::window_damage::MetaData>
for MetaWindow {
    fn as_ref(&self) -> &crate::graphics_engine::window_damage::MetaData {
        &self.base
    }
}
impl std::convert::AsMut<crate::graphics_engine::window_damage::MetaData>
for MetaWindow {
    fn as_mut(&mut self) -> &mut crate::graphics_engine::window_damage::MetaData {
        &mut self.base
    }
}
impl std::convert::AsRef<MetaWindow> for MetaWindow {
    fn as_ref(&self) -> &MetaWindow {
        self
    }
}
impl std::convert::AsMut<MetaWindow> for MetaWindow {
    fn as_mut(&mut self) -> &mut MetaWindow {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// One damage contact on a window (`CWindowContactPointList::SContactDamage`).
pub struct WindowContactDamage {
    /// The contact, in the window model instance's own space.
    pub m_LocalPosition: crate::types::math::Vector3,
    /// The contact's strength, which the window shader clamps to the unit range. It also sets the
    /// radius within which a later contact merges into this one
    /// ([`AddDamage`](crate::graphics_engine::window_damage::WindowContactPointList::AddDamage)).
    pub m_DamageMeters: f32,
}
fn _WindowContactDamage_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], WindowContactDamage>([0u8; 0x10]);
    }
    unreachable!()
}
impl WindowContactDamage {}
impl std::convert::AsRef<WindowContactDamage> for WindowContactDamage {
    fn as_ref(&self) -> &WindowContactDamage {
        self
    }
}
impl std::convert::AsMut<WindowContactDamage> for WindowContactDamage {
    fn as_mut(&mut self) -> &mut WindowContactDamage {
        self
    }
}
#[repr(C, align(8))]
/// A window's damage contacts (`NGraphicsEngine::CWindowContactPointList`): two fixed lists of up to
/// [`MAX_CONTACTS`](crate::graphics_engine::window_damage::WindowContactPointList::MAX_CONTACTS) contacts, one per [`WindowDamageKind`](crate::graphics_engine::window_damage::WindowDamageKind), and a buffered
/// shader constant block the draw fills from them. The main thread adds contacts while the render
/// thread draws: a new contact is written before its list's count is raised atomically, so the
/// draw never reads a slot that is still being written.
///
/// The window render block draws the [`Point`](crate::graphics_engine::window_damage::WindowDamageKind::Point) list: each frame it moves
/// the contacts into world space through the instance's matrix and hands the window shader the
/// count and each contact's position and strength. The shader projects the pixel's offset from
/// each contact onto the glass's tangent frame to sample a crack texture over about a metre around
/// the contact; a pixel whose crack mask falls under the contact's strength is drawn cracked, with
/// the crack texture's normal and a brightened colour, and at least 60 percent opaque. Damage never
/// makes the glass more transparent and never discards it: a window that takes enough damage is
/// drawn cracked over, not open.
pub struct WindowContactPointList {
    /// The contacts of each list, by [`WindowDamageKind`](crate::graphics_engine::window_damage::WindowDamageKind).
    pub m_Contacts: [[crate::graphics_engine::window_damage::WindowContactDamage; 32]; 2],
    /// The live contacts in each list, by [`WindowDamageKind`](crate::graphics_engine::window_damage::WindowDamageKind).
    pub m_NumContacts: [u32; 2],
    _field_408: [u8; 8],
}
fn _WindowContactPointList_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x410], WindowContactPointList>([0u8; 0x410]);
    }
    unreachable!()
}
impl WindowContactPointList {
    pub const AddDamagePoint_ADDRESS: usize = 0x1401A1F60;
    /// `CWindowContactPointList::AddDamagePoint`: [`AddDamage`](crate::graphics_engine::window_damage::WindowContactPointList::AddDamage) into the
    /// [`Point`](crate::graphics_engine::window_damage::WindowDamageKind::Point) list.
    pub unsafe fn AddDamagePoint(
        &mut self,
        local_position: *const crate::types::math::Vector3,
        damage: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                local_position: *const crate::types::math::Vector3,
                damage: f32,
            ) = ::std::mem::transmute(Self::AddDamagePoint_ADDRESS);
            f(self as *mut Self as _, local_position, damage)
        }
    }
    pub const AddDamage_ADDRESS: usize = 0x14019F830;
    /// `CWindowContactPointList::AddDamage`: adds a contact of strength `damage` at
    /// `local_position` to the `kind` list. When the nearest existing contact lies within
    /// `sqrt(0.5)` of its own strength (read as a radius) of the new one, the new damage is added to
    /// that contact's strength, clamped to the unit range, instead. Otherwise the contact is
    /// appended, its strength not clamped. A full list adds the distance to the nearest contact to
    /// that contact's strength instead of the damage.
    pub unsafe fn AddDamage(
        &mut self,
        kind: crate::graphics_engine::window_damage::WindowDamageKind,
        local_position: *const crate::types::math::Vector3,
        damage: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                kind: crate::graphics_engine::window_damage::WindowDamageKind,
                local_position: *const crate::types::math::Vector3,
                damage: f32,
            ) = ::std::mem::transmute(Self::AddDamage_ADDRESS);
            f(self as *mut Self as _, kind, local_position, damage)
        }
    }
}
impl WindowContactPointList {
    /// The capacity of each list.
    pub const MAX_CONTACTS: u32 = 32;
}
impl std::convert::AsRef<WindowContactPointList> for WindowContactPointList {
    fn as_ref(&self) -> &WindowContactPointList {
        self
    }
}
impl std::convert::AsMut<WindowContactPointList> for WindowContactPointList {
    fn as_mut(&mut self) -> &mut WindowContactPointList {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The two lists of a [`WindowContactPointList`](crate::graphics_engine::window_damage::WindowContactPointList) (`CWindowContactPointList::EDamageType`).
pub enum WindowDamageKind {
    Point = 0isize as _,
    Tile = 1isize as _,
}
fn _WindowDamageKind_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], WindowDamageKind>([0u8; 0x4]);
    }
    unreachable!()
}
