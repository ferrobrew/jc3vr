#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(1))]
/// The map screen UI (`CCommMapUI`). It handles map navigation, zooming, cursor movement, icon
/// selection, and context actions from the active steering input.
pub struct CommMapUI {
    _field_0: [u8; 16],
}
fn _CommMapUI_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], CommMapUI>([0u8; 0x10]);
    }
    unreachable!()
}
impl CommMapUI {
    pub const OnManageInput_ADDRESS: usize = 0x140E46970;
    /// Handles one frame of map input. Mouse input updates the map cursor and map actions, while
    /// steering input handles navigation, zoom, selection, and context actions. The map delegates
    /// common UI input handling after its map-specific processing.
    pub unsafe fn OnManageInput(
        &mut self,
        steering: *mut crate::input::steering::Steering,
        dt: f32,
        check_for_freeze: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                steering: *mut crate::input::steering::Steering,
                dt: f32,
                check_for_freeze: bool,
            ) = ::std::mem::transmute(Self::OnManageInput_ADDRESS);
            f(self as *mut Self as _, steering, dt, check_for_freeze)
        }
    }
    pub const Update_ADDRESS: usize = 0x140EFBDA0;
    /// Advances the visible map UI for one frame. It updates the base UI, map initialization,
    /// navigation-path and zoom state, map elements, the cursor display, and the periodic UI
    /// garbage-collection timer while holding the map visibility lock.
    pub unsafe fn Update(&mut self, dt: f32, dtf: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32, dtf: f32) = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _, dt, dtf)
        }
    }
}
impl std::convert::AsRef<CommMapUI> for CommMapUI {
    fn as_ref(&self) -> &CommMapUI {
        self
    }
}
impl std::convert::AsMut<CommMapUI> for CommMapUI {
    fn as_mut(&mut self) -> &mut CommMapUI {
        self
    }
}
