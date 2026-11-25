#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A point of interest (`CPOI`, a `CGameObject`): a world-anchored HUD marker. Mission objectives,
/// waypoints, enemy and vehicle health markers, weapon pickup hints, active explosives, and the
/// detector target are all POIs. Each registered POI has a record in
/// [`HUDUI::m_POIListHead`](crate::ui::hud_ui::HUDUI::m_POIListHead), which
/// [`HUDUI::UpdatePOIs`](crate::ui::hud_ui::HUDUI::UpdatePOIs) projects to the screen every frame. Only the
/// fields that update reads are mapped.
pub struct POI {
    vftable: *const crate::ui::poi::POIVftable,
    _field_8: [u8; 256],
    /// The POI's id. The HUD keeps the id of the POI whose label it shows in
    /// [`HUDUI::m_ShowingLabelPOIId`](crate::ui::hud_ui::HUDUI::m_ShowingLabelPOIId).
    pub m_ID: u32,
    _field_10c: [u8; 44],
    /// A screen-space offset, in stage pixels, added to the projected position
    /// (the `offset` argument of [`UIManager::Get2DInfo`](crate::ui::ui_manager::UIManager::Get2DInfo)).
    pub m_Attach2DOffset: crate::types::math::Vector2,
    /// `CPOI::EDisplayType`: which marker clip variant the HUD shows, and some type-specific
    /// behaviour (`DISPLAY_TYPE_*`). Left as a raw integer because only some values are confirmed.
    pub m_DisplayType: u32,
    _field_144: [u8; 45],
    /// When set, [`HUDUI::UpdatePOIs`](crate::ui::hud_ui::HUDUI::UpdatePOIs) skips the POI entirely: its
    /// marker is neither created, moved, shown, nor hidden.
    pub m_IsInvisible: bool,
    _field_172: [u8; 6],
    /// The marker's progress bar value (the health bar of enemy and vehicle markers).
    pub m_ProgressBar: f32,
    /// The progress bar's maximum.
    pub m_ProgressBarMax: f32,
    _field_180: [u8; 12],
    /// The nearest distance from the player character, in metres, at which the marker shows.
    pub m_MinDistance: f32,
    /// The farthest distance from the player character, in metres, at which the marker shows.
    pub m_MaxDistance: f32,
    _field_194: [u8; 10],
    /// Whether the marker is clamped to the screen edge (as a directional arrow) when its point is
    /// off-screen, rather than hidden (the `clamp_to_screen` argument of
    /// [`UIManager::Get2DInfo`](crate::ui::ui_manager::UIManager::Get2DInfo)).
    pub m_ClampToScreen: bool,
    /// Whether the marker's icon scale shrinks with distance.
    pub m_ScaleBasedOnDistance: bool,
    _field_1a0: [u8; 24],
    /// The world transform of the marker's anchor. `CPOI::UpdateTransform` rebuilds it from the
    /// attached object (a fixed offset, the attached object's bounding-box centre or top, or a local
    /// transform) on every [`UpdatePOI`](crate::ui::poi::POI::UpdatePOI); its translation row is the world point
    /// the HUD projects.
    pub m_Transform: crate::types::math::Matrix4,
    _field_1f8: [u8; 8],
}
fn _POI_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x200], POI>([0u8; 0x200]);
    }
    unreachable!()
}
impl POI {
    pub fn vftable(&self) -> *const crate::ui::poi::POIVftable {
        self.vftable as *const crate::ui::poi::POIVftable
    }
    /// `CPOI::UpdatePOI`: counts down the lifetime, rebuilds
    /// [`m_Transform`](crate::ui::poi::POI::m_Transform) from the attached object (`CPOI::UpdateTransform`),
    /// and refreshes the health or destruction-disable state. `dt` is the frame time, `dtf`
    /// the sub-frame interpolation fraction. [`HUDUI::UpdatePOIs`](crate::ui::hud_ui::HUDUI::UpdatePOIs)
    /// calls it on each live POI just before it projects the POI.
    pub unsafe fn UpdatePOI(&mut self, dt: f32, dtf: f32) {
        unsafe {
            let f = (&raw const (*self.vftable()).UpdatePOI).read();
            f(self as *mut Self as _, dt, dtf)
        }
    }
}
impl POI {
    /// The [`m_DisplayType`](crate::ui::poi::POI::m_DisplayType) of the user-placed map waypoint: removing a POI
    /// of this type clears the comm-link map's user waypoint.
    pub const DISPLAY_TYPE_USER_WAYPOINT: u32 = 10;
    /// The [`m_DisplayType`](crate::ui::poi::POI::m_DisplayType) of a weapon pickup hint: the HUD sets the
    /// marker's weapon icon from the attached weapon, and fades and scales it over its own range
    /// instead of the generic distance falloff.
    pub const DISPLAY_TYPE_WEAPON_PICKUP: u32 = 22;
}
impl std::convert::AsRef<POI> for POI {
    fn as_ref(&self) -> &POI {
        self
    }
}
impl std::convert::AsMut<POI> for POI {
    fn as_mut(&mut self) -> &mut POI {
        self
    }
}
#[repr(C, align(8))]
pub struct POIVftable {
    _vfunc_0: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_1: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_2: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_3: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_4: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_5: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_6: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_7: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_8: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_9: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_10: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_11: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_12: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_13: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_14: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_15: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_16: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_17: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_18: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_19: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_20: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_21: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_22: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_23: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_24: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_25: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_26: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_27: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_28: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_29: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_30: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_31: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_32: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_33: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_34: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_35: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_36: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_37: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_38: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_39: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_40: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_41: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_42: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_43: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_44: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    _vfunc_45: unsafe extern "system" fn(this: *mut crate::ui::poi::POI),
    /// `CPOI::UpdatePOI`: counts down the lifetime, rebuilds
    /// [`m_Transform`](crate::ui::poi::POI::m_Transform) from the attached object (`CPOI::UpdateTransform`),
    /// and refreshes the health or destruction-disable state. `dt` is the frame time, `dtf`
    /// the sub-frame interpolation fraction. [`HUDUI::UpdatePOIs`](crate::ui::hud_ui::HUDUI::UpdatePOIs)
    /// calls it on each live POI just before it projects the POI.
    pub UpdatePOI: unsafe extern "system" fn(
        this: *mut crate::ui::poi::POI,
        dt: f32,
        dtf: f32,
    ),
}
fn _POIVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x178], POIVftable>([0u8; 0x178]);
    }
    unreachable!()
}
impl POIVftable {}
impl std::convert::AsRef<POIVftable> for POIVftable {
    fn as_ref(&self) -> &POIVftable {
        self
    }
}
impl std::convert::AsMut<POIVftable> for POIVftable {
    fn as_mut(&mut self) -> &mut POIVftable {
        self
    }
}
#[repr(C, align(8))]
/// A `boost::weak_ptr<CPOI>`: the object pointer (`px`) and the shared control block (`pn`). `px`
/// may dangle once the POI is destroyed; take a reference through [`lock`](crate::ui::poi::WeakPtrPOI::lock) before
/// reading it.
pub struct WeakPtrPOI {
    /// The POI (`px`); only meaningful while the control block's use count is non-zero.
    pub px: *mut crate::ui::poi::POI,
    /// The shared reference-count control block (`pn`), or null.
    pub pn: *mut ::std::ffi::c_void,
}
fn _WeakPtrPOI_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], WeakPtrPOI>([0u8; 0x10]);
    }
    unreachable!()
}
impl WeakPtrPOI {
    pub const lock_ADDRESS: usize = 0x140E34A70;
    /// `boost::weak_ptr<CPOI>::lock`: writes into `result` a strong reference to the POI (taking
    /// one use count with a compare-and-swap that fails once the count is zero), or a null pointer
    /// when the POI is already destroyed. Returns `result`. Release the reference with
    /// [`ResetSharedPtrPOI`](crate::ui::poi::ResetSharedPtrPOI).
    pub unsafe fn lock(
        &self,
        result: *mut crate::types::shared_ptr::SharedPtr<crate::ui::poi::POI>,
    ) -> *mut crate::types::shared_ptr::SharedPtr<crate::ui::poi::POI> {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                result: *mut crate::types::shared_ptr::SharedPtr<crate::ui::poi::POI>,
            ) -> *mut crate::types::shared_ptr::SharedPtr<crate::ui::poi::POI> = ::std::mem::transmute(
                Self::lock_ADDRESS,
            );
            f(self as *const Self as _, result)
        }
    }
}
impl std::convert::AsRef<WeakPtrPOI> for WeakPtrPOI {
    fn as_ref(&self) -> &WeakPtrPOI {
        self
    }
}
impl std::convert::AsMut<WeakPtrPOI> for WeakPtrPOI {
    fn as_mut(&mut self) -> &mut WeakPtrPOI {
        self
    }
}
pub const ResetSharedPtrPOI_ADDRESS: usize = 0x1408EA4A0;
/// `boost::shared_ptr<CPOI>::reset`: nulls `ptr` and releases its use count, disposing of the POI
/// when it was the last strong reference and destroying the control block when no weak references
/// remain.
pub unsafe fn ResetSharedPtrPOI(
    ptr: *mut crate::types::shared_ptr::SharedPtr<crate::ui::poi::POI>,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ptr: *mut crate::types::shared_ptr::SharedPtr<crate::ui::poi::POI>,
        ) = ::std::mem::transmute(ResetSharedPtrPOI_ADDRESS);
        f(ptr)
    }
}
