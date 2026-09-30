#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The map screen UI (`CCommMapUI`). It handles map navigation, zooming, cursor movement, icon
/// selection, and context actions from the active steering input.
pub struct CommMapUI {
    _field_0: [u8; 481],
    /// Whether a mouse drag of the map is in progress. [`OnManageInput`](crate::ui::comm_map_ui::CommMapUI::OnManageInput) sets
    /// it when the held [`MOUSE1`](crate::input::input_action_map::Action::MOUSE1) moves the cursor or the
    /// mouse far enough from the press point, and pans the map from the cursor while it is set. A
    /// [`Released`](crate::input::input_action_map::EffectorState::Released) `MOUSE1` clears it and ends the
    /// drag instead of selecting; nothing else clears it.
    pub m_MouseDragging: bool,
    _field_1e2: [u8; 6],
}
fn _CommMapUI_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1E8], CommMapUI>([0u8; 0x1E8]);
    }
    unreachable!()
}
impl CommMapUI {
    pub const OnManageInput_ADDRESS: usize = 0x140E46970;
    /// Handles one frame of map input.
    /// [`UIManager::m_IsUsingGamepad`](crate::ui::ui_manager::UIManager::m_IsUsingGamepad) selects
    /// between two branches. With the flag clear it reads the mouse: the cursor from
    /// [`UIManager::GetMousePos`](crate::ui::ui_manager::UIManager::GetMousePos) through
    /// [`UIManager::GetMovieSpaceMouseCursor`](crate::ui::ui_manager::UIManager::GetMovieSpaceMouseCursor),
    /// the right-stick GUI actions and [`MOUSE1`](crate::input::input_action_map::Action::MOUSE1) /
    /// [`MOUSE2`](crate::input::input_action_map::Action::MOUSE2) from the effectors of the first mouse
    /// device (see [`m_MouseDragging`](crate::ui::comm_map_ui::CommMapUI::m_MouseDragging)), and the GUI zoom actions through the
    /// steering, each click stepping the snapped zoom level by one, with the mouse wheel taking
    /// precedence. A released `MOUSE1` outside a drag selects at the cursor, and a released `MOUSE2`
    /// selects and opens the context menu. With the flag set it pans from the steering's stick
    /// actions and zooms continuously around the map centre. Both branches then pick the icon or
    /// province under the selection point and delegate the common UI input handling.
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
    /// Advances the visible map UI for one frame. `UIManager::PreRender` calls it, not
    /// [`UIManager::PreUpdate`](crate::ui::ui_manager::UIManager::PreUpdate), so it runs after the frame's
    /// input dispatch. It updates the base UI, map initialization, navigation-path and zoom state,
    /// map elements, the cursor display, and the periodic UI garbage-collection timer while holding
    /// the map visibility lock. It reads no input actions. The map elements it updates read
    /// [`UIManager::m_IsUsingGamepad`](crate::ui::ui_manager::UIManager::m_IsUsingGamepad): with the flag
    /// set the gamepad reticle is shown and the cursor-relative elements follow the map centre, and
    /// with it clear the reticle is hidden and they follow the mouse cursor.
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
