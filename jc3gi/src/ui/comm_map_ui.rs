#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The map screen UI (`CCommMapUI`). It handles map navigation, zooming, cursor movement, icon
/// selection, and context actions from the active steering input.
///
/// Positions on the map come in two spaces: UI positions, which are stage coordinates of the UI
/// movie, and UI-map positions, which are offsets from [`m_MapCenterUiPosition`](crate::ui::comm_map_ui::CommMapUI::m_MapCenterUiPosition)
/// divided by [`m_MapScale`](crate::ui::comm_map_ui::CommMapUI::m_MapScale). The gamepad selects at a fixed stage point, the
/// gamepad cursor at the centre of the stage, and pans the map under it; the mouse selects at the
/// stage point it clicks.
pub struct CommMapUI {
    _field_0: [u8; 481],
    /// Whether a mouse drag of the map is in progress. [`OnManageInput`](crate::ui::comm_map_ui::CommMapUI::OnManageInput) sets
    /// it when the held [`MOUSE1`](crate::input::input_action_map::Action::MOUSE1) moves the cursor or the
    /// mouse far enough from the press point, and pans the map from the cursor while it is set. A
    /// [`Released`](crate::input::input_action_map::EffectorState::Released) `MOUSE1` clears it and ends the
    /// drag instead of selecting; nothing else clears it. The cursor threshold is a few stage
    /// pixels, measured from [`m_MouseClickUiMapPosition`](crate::ui::comm_map_ui::CommMapUI::m_MouseClickUiMapPosition).
    pub m_MouseDragging: bool,
    /// Whether the mouse branch of [`OnManageInput`](crate::ui::comm_map_ui::CommMapUI::OnManageInput) made a selection this
    /// frame: a released [`MOUSE1`](crate::input::input_action_map::Action::MOUSE1) outside a drag, or a
    /// released [`MOUSE2`](crate::input::input_action_map::Action::MOUSE2). Either writes the cursor to
    /// [`m_MouseCursorUiPosition`](crate::ui::comm_map_ui::CommMapUI::m_MouseCursorUiPosition) and sets this flag, and while it
    /// is set the same call picks the icon or province at that point. `OnManageInput` clears it,
    /// together with [`m_ActivateCtxMenu`](crate::ui::comm_map_ui::CommMapUI::m_ActivateCtxMenu), at the start of every call.
    pub m_MouseCursorPositionUpdated: bool,
    /// Whether the context menu (the waypoint, fast travel, and icon actions) opens this frame.
    /// [`OnManageInput`](crate::ui::comm_map_ui::CommMapUI::OnManageInput) sets it in either branch for a clicked
    /// [`GUI_OK`](crate::input::input_action_map::Action::GUI_OK), which it then freezes, and in the mouse
    /// branch for a released [`MOUSE2`](crate::input::input_action_map::Action::MOUSE2). Only the
    /// `MOUSE2` path also selects at the cursor; `GUI_OK` opens the menu for whatever the last
    /// selection picked. The map's update calls [`ActivateCtxMenu`](crate::ui::comm_map_ui::CommMapUI::ActivateCtxMenu) while
    /// it is set.
    pub m_ActivateCtxMenu: bool,
    _field_1e4: [u8; 4],
    /// The UI position of the map's origin. Panning moves it; the drag sets it so the UI-map point
    /// pressed stays under the cursor.
    pub m_MapCenterUiPosition: crate::types::math::Vector2,
    /// The mouse selection point, a UI position. Only the selecting releases described on
    /// [`m_MouseCursorPositionUpdated`](crate::ui::comm_map_ui::CommMapUI::m_MouseCursorPositionUpdated) write it, so between
    /// selections it keeps the last one: moving the mouse does not change it. With
    /// [`UIManager::m_IsUsingGamepad`](crate::ui::ui_manager::UIManager::m_IsUsingGamepad) clear, the
    /// context menu opens here and the selection-relative map elements are placed from it.
    pub m_MouseCursorUiPosition: crate::types::math::Vector2,
    /// The map's current zoom: the UI-space size of one UI-map unit.
    pub m_MapScale: f32,
    _field_1fc: [u8; 4],
    /// The UI-map position under the cursor when [`MOUSE1`](crate::input::input_action_map::Action::MOUSE1)
    /// was clicked. A drag keeps this point under the cursor, and the cursor's distance from it
    /// decides when a held press becomes a drag (see [`m_MouseDragging`](crate::ui::comm_map_ui::CommMapUI::m_MouseDragging)).
    pub m_MouseClickUiMapPosition: crate::types::math::Vector2,
}
fn _CommMapUI_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x208], CommMapUI>([0u8; 0x208]);
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
    /// selects and opens the context menu (see
    /// [`m_MouseCursorPositionUpdated`](crate::ui::comm_map_ui::CommMapUI::m_MouseCursorPositionUpdated)). With the flag set it
    /// pans from the steering's left-stick GUI actions and zooms continuously around the gamepad
    /// cursor. In both branches a clicked [`GUI_OK`](crate::input::input_action_map::Action::GUI_OK)
    /// requests the context menu (see [`m_ActivateCtxMenu`](crate::ui::comm_map_ui::CommMapUI::m_ActivateCtxMenu)), and
    /// [`GUI_RECENTER`](crate::input::input_action_map::Action::GUI_RECENTER) and
    /// [`GUI_FOCUS`](crate::input::input_action_map::Action::GUI_FOCUS) auto-move the map, to the player
    /// (or from the player to the user waypoint) and to the next mission. The gamepad branch picks the icon or province under the gamepad cursor
    /// whenever the map moves or zooms; the mouse branch picks only on a selection. It then
    /// delegates the common UI input handling.
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
    pub const ActivateCtxMenu_ADDRESS: usize = 0x140E253A0;
    /// Opens the map's context menu for the selected icon or province. It places the menu at the
    /// gamepad cursor while
    /// [`UIManager::m_IsUsingGamepad`](crate::ui::ui_manager::UIManager::m_IsUsingGamepad) is set, and at
    /// [`m_MouseCursorUiPosition`](crate::ui::comm_map_ui::CommMapUI::m_MouseCursorUiPosition) while it is clear. The map's
    /// element update calls it from [`Update`](crate::ui::comm_map_ui::CommMapUI::Update) while
    /// [`m_ActivateCtxMenu`](crate::ui::comm_map_ui::CommMapUI::m_ActivateCtxMenu) is set; a running tutorial can refuse it.
    pub unsafe fn ActivateCtxMenu(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::ActivateCtxMenu_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const Update_ADDRESS: usize = 0x140EFBDA0;
    /// Advances the visible map UI for one frame. `UIManager::PreRender` calls it, not
    /// [`UIManager::PreUpdate`](crate::ui::ui_manager::UIManager::PreUpdate), so it runs after the frame's
    /// input dispatch. It updates the base UI, map initialization, navigation-path and zoom state,
    /// map elements, the cursor display, and the periodic UI garbage-collection timer while holding
    /// the map visibility lock. It reads no input actions. The map elements it updates read
    /// [`UIManager::m_IsUsingGamepad`](crate::ui::ui_manager::UIManager::m_IsUsingGamepad): with the flag
    /// set the gamepad reticle is shown and the cursor-relative elements follow the gamepad cursor,
    /// and with it clear the reticle is hidden and they follow
    /// [`m_MouseCursorUiPosition`](crate::ui::comm_map_ui::CommMapUI::m_MouseCursorUiPosition). The element update also opens
    /// the context menu (see [`ActivateCtxMenu`](crate::ui::comm_map_ui::CommMapUI::ActivateCtxMenu)).
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
