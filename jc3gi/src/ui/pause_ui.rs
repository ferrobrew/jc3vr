#![cfg_attr(any(), rustfmt::skip)]
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// `CPauseUI::EPauseMenuType`: which of the pause movie's menus is shown. The movie keeps one
/// list of main-menu entries per type and switches between them with its `SwitchToMenu` method.
pub enum PauseMenuType {
    PAUSE_SHIP = 0isize as _,
    PAUSE_DEV = 1isize as _,
    PAUSE_PROCTOR = 2isize as _,
    PAUSE_OPTIONS = 3isize as _,
    PAUSE_FRONTEND = 4isize as _,
    PAUSE_MAIN_MENU = 5isize as _,
    PAUSE_CONTROLS = 6isize as _,
}
fn _PauseMenuType_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], PauseMenuType>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// The pause and options menu (`CPauseUI`), whose movie is `ui/pause.gfx`. The movie's document
/// class (`pause`) holds the menus as lists of entries shown on a column of main buttons, and a
/// settings screen that lists one page of settings rows at a time. The engine fills the movie at
/// [`OnActiveImpl`](crate::ui::pause_ui::PauseUI::OnActiveImpl) (the `InitMenu` and `SwitchToMenu` calls), fills the
/// settings pages on request through [`PopulateSettings`](crate::ui::pause_ui::PauseUI::PopulateSettings), and receives
/// the movie's changes through [`HandleCommandImpl`](crate::ui::pause_ui::PauseUI::HandleCommandImpl). The instance is
/// created once and survives across pauses; the movie itself is loaded on every activation and
/// unloaded on deactivation.
pub struct PauseUI {
    pub base: crate::ui::ui_base::UIBase,
    _field_f0: [u8; 48],
    /// The menu the movie was last switched to on activation.
    pub m_CurrentMenuType: crate::ui::pause_ui::PauseMenuType,
    /// Which menus are available this activation, indexed by [`PauseMenuType`](crate::ui::pause_ui::PauseMenuType); passed to the
    /// movie's `InitMenu`.
    pub m_IsValidMenu: [bool; 7],
    /// Set when the settings changed outside the menu; the next [`UpdateImpl`](crate::ui::pause_ui::PauseUI::UpdateImpl)
    /// runs [`PopulateSettings`](crate::ui::pause_ui::PauseUI::PopulateSettings) and has the movie redraw the shown page
    /// (its `RefreshCurrentSettings` method), then clears it.
    pub m_RefreshSettings: bool,
    /// Set by every [`COMMAND_SET_SETTING_VALUE`](crate::ui::pause_ui::PauseUI::COMMAND_SET_SETTING_VALUE); tells
    /// [`PersistChanges`](crate::ui::pause_ui::PauseUI::PersistChanges) and [`OnDeactivateImpl`](crate::ui::pause_ui::PauseUI::OnDeactivateImpl)
    /// there is something to save.
    pub m_AnySettingsChanged: bool,
    /// Set while the settings manager captures a key for
    /// [`COMMAND_GET_KEY_FOR_ACTION`](crate::ui::pause_ui::PauseUI::COMMAND_GET_KEY_FOR_ACTION).
    pub m_Waitingforkey: bool,
    _field_12e: [u8; 5],
    /// Set between [`COMMAND_SHOW_FULLSCREEN_VIDEO`](crate::ui::pause_ui::PauseUI::COMMAND_SHOW_FULLSCREEN_VIDEO) and
    /// [`COMMAND_HIDE_FULLSCREEN_VIDEO`](crate::ui::pause_ui::PauseUI::COMMAND_HIDE_FULLSCREEN_VIDEO).
    pub m_ShowingFullScreenVideo: bool,
    /// The controller-mapping category the controls screen shows.
    pub m_CurrentScreen: i32,
    _field_138: [u8; 64],
    /// The helper that builds the settings pages.
    pub m_SettingsUI: crate::ui::pause_ui::SettingsUI,
    _field_180: [u8; 280],
}
fn _PauseUI_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x298], PauseUI>([0u8; 0x298]);
    }
    unreachable!()
}
impl PauseUI {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5418223712usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl PauseUI {
    pub fn vftable(&self) -> *const crate::ui::ui_base::UIBaseVftable {
        self.base.vftable() as *const crate::ui::ui_base::UIBaseVftable
    }
    /// The base [`HandleCommand`](crate::ui::ui_base::UIBase::HandleCommand): the commands shared by every screen.
    /// [`COMMAND_ACTIVE`](crate::ui::ui_base::UIBase::COMMAND_ACTIVE) stores the clip and calls
    /// [`OnActive`](crate::ui::ui_base::UIBase::OnActive); [`COMMAND_INACTIVE`](crate::ui::ui_base::UIBase::COMMAND_INACTIVE) calls
    /// [`OnInactive`](crate::ui::ui_base::UIBase::OnInactive) and releases it; [`COMMAND_EVENT`](crate::ui::ui_base::UIBase::COMMAND_EVENT) posts an engine event;
    /// the rest cover activation of other screens, tweens, localized-string lookups, and input
    /// priority.
    pub unsafe fn BaseHandleCommand(
        &mut self,
        command: u32,
        args: *const crate::ui::scaleform::Value,
        num_args: u32,
        movie: *mut crate::ui::scaleform::MovieImpl,
    ) {
        unsafe { self.base.BaseHandleCommand(command, args, num_args, movie) }
    }
    /// Requests the screen's deactivation: sets [`m_Desired`](crate::ui::ui_base::UIBase::m_Desired) to
    /// [`INACTIVE`](crate::ui::overlay_ui::UIBaseState::INACTIVE) and, unless the screen is already
    /// inactive, calls [`OnDeactivate`](crate::ui::ui_base::UIBase::OnDeactivate), moves
    /// [`m_Current`](crate::ui::ui_base::UIBase::m_Current) to
    /// [`DEACTIVATING`](crate::ui::overlay_ui::UIBaseState::DEACTIVATING), clears the command queue and
    /// the tweens, and, when `unload` is set, has the root movie unload the clip (its
    /// `root.Deactivate` or `root.DeactivateDyn`), which ends in
    /// [`COMMAND_INACTIVE`](crate::ui::ui_base::UIBase::COMMAND_INACTIVE). The engine's deactivation requests go
    /// through it: the movies' deactivate commands, `CUIManager::ForceDeactivatePause`, the UI manager's
    /// event handling, and the game-state transitions.
    pub unsafe fn Deactivate(&mut self, unload: bool) {
        unsafe { self.base.Deactivate(unload) }
    }
    /// The [`Invoke`](crate::ui::ui_base::UIBase::Invoke) implementation. When the screen is
    /// [`ACTIVE`](crate::ui::overlay_ui::UIBaseState::ACTIVE), `delay` is zero, and the caller is the
    /// UI manager's capture thread (or asks for a `result`), it calls the method on
    /// [`m_Clip`](crate::ui::ui_base::UIBase::m_Clip) at once through
    /// [`ValueObjectInterface::Invoke`](crate::ui::scaleform::ValueObjectInterface::Invoke). Otherwise,
    /// and only when no `result` is wanted, it copies the call into the screen's command queue,
    /// which the screen's update drains on the capture thread; a queued call from another thread
    /// gets a minimal delay. Returns false only for a deferred call that wants a result.
    pub unsafe fn InvokeImpl(
        &mut self,
        method: *const u8,
        args: *const crate::ui::scaleform::Value,
        num_args: u32,
        result: *mut crate::ui::scaleform::Value,
        delay: f32,
    ) -> bool {
        unsafe { self.base.InvokeImpl(method, args, num_args, result, delay) }
    }
    /// The base [`OnManageInput`](crate::ui::ui_base::UIBase::OnManageInput): maps the steering's UI actions to the
    /// clip's AS3 input handlers.
    pub unsafe fn BaseOnManageInput(
        &mut self,
        steering: *mut crate::input::steering::Steering,
        dt: f32,
        check_for_freeze: bool,
    ) {
        unsafe { self.base.BaseOnManageInput(steering, dt, check_for_freeze) }
    }
    /// Creates an empty AS3 `Object` in [`m_Movie`](crate::ui::ui_base::UIBase::m_Movie) into `value`.
    pub unsafe fn CreateObject(&mut self, value: *mut crate::ui::scaleform::Value) {
        unsafe { self.base.CreateObject(value) }
    }
    /// Creates an empty AS3 `Array` in [`m_Movie`](crate::ui::ui_base::UIBase::m_Movie) into `value`.
    pub unsafe fn CreateArray(&mut self, value: *mut crate::ui::scaleform::Value) {
        unsafe { self.base.CreateArray(value) }
    }
    pub const OnActiveImpl_ADDRESS: usize = 0x140F15600;
    /// The [`OnActive`](crate::ui::ui_base::UIBase::OnActive) override. Calls the base, then fills the freshly loaded
    /// movie: `InitMenu` with [`m_IsValidMenu`](crate::ui::pause_ui::PauseUI::m_IsValidMenu) and the installed-DLC flags
    /// (which builds each valid menu's fixed entries, the options menu among them), the
    /// engine-owned entries of the in-game, front-end, and main menus (through the movie's
    /// `AddEventOption`), the player info and infocast, and finally `SwitchToMenu` with
    /// [`m_CurrentMenuType`](crate::ui::pause_ui::PauseUI::m_CurrentMenuType). Runs on the capture thread, inside the
    /// root movie's advance.
    pub unsafe fn OnActiveImpl(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::OnActiveImpl_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const OnDeactivateImpl_ADDRESS: usize = 0x140E508A0;
    /// The [`OnDeactivate`](crate::ui::ui_base::UIBase::OnDeactivate) override: when
    /// [`m_AnySettingsChanged`](crate::ui::pause_ui::PauseUI::m_AnySettingsChanged) is set, has the settings manager
    /// persist its settings (`CSettingsManager::PersistChanges`) and clears the flag, then stops
    /// observing the login manager. Every deactivation of the loaded pause menu runs it, whether
    /// or not the movie's settings screen sent [`COMMAND_PERSIST_CHANGES`](crate::ui::pause_ui::PauseUI::COMMAND_PERSIST_CHANGES)
    /// first.
    pub unsafe fn OnDeactivateImpl(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::OnDeactivateImpl_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const OnInactiveImpl_ADDRESS: usize = 0x140E31B20;
    /// The [`OnInactive`](crate::ui::ui_base::UIBase::OnInactive) override: empties and releases two cached AS3
    /// arrays, calls the base, freezes a set of input actions' device effectors, and has the
    /// overlay screen show its game messages again, and clears
    /// [`m_ShowingFullScreenVideo`](crate::ui::pause_ui::PauseUI::m_ShowingFullScreenVideo).
    pub unsafe fn OnInactiveImpl(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::OnInactiveImpl_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const HandleCommandImpl_ADDRESS: usize = 0x140F06EE0;
    /// The [`HandleCommand`](crate::ui::ui_base::UIBase::HandleCommand) override: dispatches the pause movie's
    /// commands (the `COMMAND_*` constants on this type) and falls back to
    /// [`UIBase::BaseHandleCommand`](crate::ui::ui_base::UIBase::BaseHandleCommand) for the rest. The movie's calls arrive synchronously from
    /// inside AS3, so the movie resumes with any change this makes already applied.
    pub unsafe fn HandleCommandImpl(
        &mut self,
        command: u32,
        args: *const crate::ui::scaleform::Value,
        num_args: u32,
        movie: *mut crate::ui::scaleform::MovieImpl,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                command: u32,
                args: *const crate::ui::scaleform::Value,
                num_args: u32,
                movie: *mut crate::ui::scaleform::MovieImpl,
            ) = ::std::mem::transmute(Self::HandleCommandImpl_ADDRESS);
            f(self as *mut Self as _, command, args, num_args, movie)
        }
    }
    pub const UpdateImpl_ADDRESS: usize = 0x140EBB1D0;
    /// The per-frame update: tracks install, login, and store state for the menu entries it
    /// enables, finishes a key capture, and services
    /// [`m_RefreshSettings`](crate::ui::pause_ui::PauseUI::m_RefreshSettings).
    pub unsafe fn UpdateImpl(&mut self, dt: f32, dtf: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32, dtf: f32) = ::std::mem::transmute(
                Self::UpdateImpl_ADDRESS,
            );
            f(self as *mut Self as _, dt, dtf)
        }
    }
    pub const OnManageInputImpl_ADDRESS: usize = 0x140EBB5A0;
    /// The [`OnManageInput`](crate::ui::ui_base::UIBase::OnManageInput) override: the base input mapping plus the
    /// gamepad-only select and cancel handlers of the membership page.
    pub unsafe fn OnManageInputImpl(
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
            ) = ::std::mem::transmute(Self::OnManageInputImpl_ADDRESS);
            f(self as *mut Self as _, steering, dt, check_for_freeze)
        }
    }
    pub const PopulateSettings_ADDRESS: usize = 0x140E8BD10;
    /// When the screen is [`ACTIVE`](crate::ui::overlay_ui::UIBaseState::ACTIVE), empties the clip's
    /// `m_SettingsArray` and refills it through
    /// [`SettingsUI::PopulateSettingsMenu`](crate::ui::pause_ui::SettingsUI::PopulateSettingsMenu). Called by
    /// [`COMMAND_POPULATE_SETTINGS`](crate::ui::pause_ui::PauseUI::COMMAND_POPULATE_SETTINGS) and by
    /// [`UpdateImpl`](crate::ui::pause_ui::PauseUI::UpdateImpl) for [`m_RefreshSettings`](crate::ui::pause_ui::PauseUI::m_RefreshSettings).
    pub unsafe fn PopulateSettings(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::PopulateSettings_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const PersistChanges_ADDRESS: usize = 0x140DFBA30;
    /// When [`m_AnySettingsChanged`](crate::ui::pause_ui::PauseUI::m_AnySettingsChanged) is set, has the settings manager
    /// persist its settings (`CSettingsManager::PersistChanges`) and clears the flag.
    pub unsafe fn PersistChanges(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::PersistChanges_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const RestoreDefaultMapping_ADDRESS: usize = 0x140EBB920;
    /// Restores the default key bindings.
    pub unsafe fn RestoreDefaultMapping(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::RestoreDefaultMapping_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    /// Runs the screen's per-frame input: reads the steering's UI actions and calls the
    /// matching AS3 handler on the clip through [`Invoke`](crate::ui::ui_base::UIBase::Invoke) (`OnUp`, `OnDown`,
    /// `OnLeft`, `OnRight`, the `OnDpad*` family, `OnSelect`, `OnCancel`, `OnSpecialButton`,
    /// `OnYButton`, `OnNextTabButton`, `OnPrevTabButton`, `OnNextPageButton`,
    /// `OnPrevPageButton`, and the rest of the `uibase` input methods). Only the screen with
    /// the highest input priority is called each frame.
    pub unsafe fn OnManageInput(
        &mut self,
        steering: *mut crate::input::steering::Steering,
        dt: f32,
        check_for_freeze: bool,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).OnManageInput).read();
            f(self as *mut Self as _, steering, dt, check_for_freeze)
        }
    }
    /// Called when the screen's clip has loaded and been stored in [`m_Clip`](crate::ui::ui_base::UIBase::m_Clip),
    /// with [`m_Current`](crate::ui::ui_base::UIBase::m_Current) already
    /// [`ACTIVE`](crate::ui::overlay_ui::UIBaseState::ACTIVE). Screens push their initial data to
    /// the clip here.
    pub unsafe fn OnActive(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).OnActive).read();
            f(self as *mut Self as _)
        }
    }
    /// Called by [`Deactivate`](crate::ui::ui_base::UIBase::Deactivate) when a deactivation is requested while the
    /// screen is not [`INACTIVE`](crate::ui::overlay_ui::UIBaseState::INACTIVE), before
    /// [`m_Current`](crate::ui::ui_base::UIBase::m_Current) moves to
    /// [`DEACTIVATING`](crate::ui::overlay_ui::UIBaseState::DEACTIVATING) and the root movie is asked
    /// to unload the clip. The clip is still loaded. Runs on the thread that requested the
    /// deactivation, which is not necessarily the capture thread.
    pub unsafe fn OnDeactivate(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).OnDeactivate).read();
            f(self as *mut Self as _)
        }
    }
    /// Called by the [`COMMAND_INACTIVE`](crate::ui::ui_base::UIBase::COMMAND_INACTIVE) handling once the root
    /// movie has unloaded the clip, before [`m_Clip`](crate::ui::ui_base::UIBase::m_Clip) is released.
    pub unsafe fn OnInactive(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).OnInactive).read();
            f(self as *mut Self as _)
        }
    }
    /// Handles an ExternalInterface command from the clip. `command` is the `hashlittle` hash
    /// of the command name (the part after the colon), `args` the call's arguments, and
    /// `movie` the calling movie, through which a command that returns a value sets it with
    /// [`SetExternalInterfaceRetVal`](crate::ui::scaleform::MovieImpl::SetExternalInterfaceRetVal).
    /// The base implementation is [`BaseHandleCommand`](crate::ui::ui_base::UIBase::BaseHandleCommand); screens
    /// override it and fall back to it for commands they do not handle.
    pub unsafe fn HandleCommand(
        &mut self,
        command: u32,
        args: *const crate::ui::scaleform::Value,
        num_args: u32,
        movie: *mut crate::ui::scaleform::MovieImpl,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).HandleCommand).read();
            f(self as *mut Self as _, command, args, num_args, movie)
        }
    }
    /// Calls the AS3 method `method` on the clip. The implementation is
    /// [`InvokeImpl`](crate::ui::ui_base::UIBase::InvokeImpl), shared by every screen.
    pub unsafe fn Invoke(
        &mut self,
        method: *const u8,
        args: *const crate::ui::scaleform::Value,
        num_args: u32,
        result: *mut crate::ui::scaleform::Value,
        delay: f32,
    ) -> bool {
        unsafe {
            let f = (&raw const (*self.vftable()).Invoke).read();
            f(self as *mut Self as _, method, args, num_args, result, delay)
        }
    }
}
impl PauseUI {
    /// The command hash for `check_dlc_sell_messages`.
    pub const COMMAND_CHECK_DLC_SELL_MESSAGES: u32 = 1335715710;
    /// The command hash for `get_key_for_action`: `(action)`, a key-binding row was selected. The
    /// settings manager starts capturing the next key for the action, and
    /// [`m_Waitingforkey`](crate::ui::pause_ui::PauseUI::m_Waitingforkey) is set until it arrives.
    pub const COMMAND_GET_KEY_FOR_ACTION: u32 = 1680140325;
    /// The command hash for `get_setting_value`: `(type)`, returning the setting's value.
    pub const COMMAND_GET_SETTING_VALUE: u32 = 2444176115;
    /// The command hash for `hide_fullscreen_video`.
    pub const COMMAND_HIDE_FULLSCREEN_VIDEO: u32 = 1686917934;
    /// The command hash for `load_controller_mapping`: `(screen)`, the controls screen changed
    /// category; stored in [`m_CurrentScreen`](crate::ui::pause_ui::PauseUI::m_CurrentScreen).
    pub const COMMAND_LOAD_CONTROLLER_MAPPING: u32 = 898583342;
    /// The command hash for `persist_changes`: the settings or controls screen is closing
    /// ([`PersistChanges`](crate::ui::pause_ui::PauseUI::PersistChanges)).
    pub const COMMAND_PERSIST_CHANGES: u32 = 952971356;
    /// The command hash for `populate_settings`: the movie is about to show the settings screen
    /// and asks for its pages ([`PopulateSettings`](crate::ui::pause_ui::PauseUI::PopulateSettings)).
    pub const COMMAND_POPULATE_SETTINGS: u32 = 1938993564;
    /// The command hash for `request_new_infocast`.
    pub const COMMAND_REQUEST_NEW_INFOCAST: u32 = 3419875631;
    /// The command hash for `restore_default_keymap`: the key-binding rows' "reset all" was
    /// confirmed ([`RestoreDefaultMapping`](crate::ui::pause_ui::PauseUI::RestoreDefaultMapping)).
    pub const COMMAND_RESTORE_DEFAULT_KEYMAP: u32 = 2809691390;
    /// The command hash for `set_setting_value`: `(type, value)`, both ints, a settings row's new
    /// value. Sets [`m_AnySettingsChanged`](crate::ui::pause_ui::PauseUI::m_AnySettingsChanged) and calls
    /// [`SettingsManager::SetValue`](crate::settings_manager::SettingsManager::SetValue).
    pub const COMMAND_SET_SETTING_VALUE: u32 = 1902920154;
    /// The command hash for `set_upgrade_value`: `(type, value)` from a row on the DLC page; loads
    /// the player's gear upgrade for the chosen DLC.
    pub const COMMAND_SET_UPGRADE_VALUE: u32 = 1643712435;
    /// The command hash for `show_fullscreen_video`.
    pub const COMMAND_SHOW_FULLSCREEN_VIDEO: u32 = 1075637403;
    /// The number of main buttons the movie lays a menu's entries out on
    /// (`MCI_button_container.MCI_root.MCI_button_00` onwards, each holding a
    /// `stat_category_button` as `MCI_button`). The movie's `SetMainMenuOption` shows the first
    /// entries of a list on them and silently drops the rest.
    pub const MAIN_BUTTON_COUNT: i32 = 11;
    /// The movie's `OptionClass.m_Type` for the options menu's graphics entry: selecting an entry
    /// of this type opens the settings screen on the
    /// [`PAGE_GRAPHICS`](crate::ui::pause_ui::SettingsUI::PAGE_GRAPHICS) page. The value is a constant of the movie's
    /// `pause` class.
    pub const OPTION_GRAPHICS: i32 = 11;
    /// The `CPauseUI` vtable (the [`UIBase`](crate::ui::ui_base::UIBase) virtuals).
    pub const VFTABLE: u64 = 5407236856;
}
impl std::convert::AsRef<crate::ui::ui_base::UIBase> for PauseUI {
    fn as_ref(&self) -> &crate::ui::ui_base::UIBase {
        &self.base
    }
}
impl std::convert::AsMut<crate::ui::ui_base::UIBase> for PauseUI {
    fn as_mut(&mut self) -> &mut crate::ui::ui_base::UIBase {
        &mut self.base
    }
}
impl std::convert::AsRef<PauseUI> for PauseUI {
    fn as_ref(&self) -> &PauseUI {
        self
    }
}
impl std::convert::AsMut<PauseUI> for PauseUI {
    fn as_mut(&mut self) -> &mut PauseUI {
        self
    }
}
#[repr(C, align(8))]
/// The settings-page builder (`CSettingsUI`), embedded in [`PauseUI`](crate::ui::pause_ui::PauseUI) and holding only its owner.
/// [`PopulateSettingsMenu`](crate::ui::pause_ui::SettingsUI::PopulateSettingsMenu) fills the pause movie's
/// `m_SettingsArray` with one AS3 object per page, `{m_Name, m_Settings}`, where `m_Settings` is
/// an array of row objects. A row carries `m_Name` (the label), `m_Type` (the
/// [`SettingType`](crate::settings_manager::SettingType) the movie sends back with its new value),
/// `m_Format` (one of the `WIDGET_*` constants), `m_Value`, `m_DefaultValue`, and
/// `m_NeedConfirm`; a toggle adds the parallel `m_Labels` and `m_Values` arrays, and a slider adds
/// `m_MinValue` and `m_MaxValue`.
pub struct SettingsUI {
    /// The owning screen, whose movie creates the page objects.
    pub m_Owner: *mut crate::ui::ui_base::UIBase,
}
fn _SettingsUI_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], SettingsUI>([0u8; 0x8]);
    }
    unreachable!()
}
impl SettingsUI {
    pub const PopulateSettingsMenu_ADDRESS: usize = 0x140E7B1C0;
    /// Appends the four pages, in `PAGE_*` order, to `settings` (the movie's `m_SettingsArray`),
    /// each named by a localized string and filled by its category's builder.
    pub unsafe fn PopulateSettingsMenu(
        &mut self,
        settings: *mut crate::ui::scaleform::Value,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                settings: *mut crate::ui::scaleform::Value,
            ) = ::std::mem::transmute(Self::PopulateSettingsMenu_ADDRESS);
            f(self as *mut Self as _, settings)
        }
    }
    pub const AppendSettingToggle_ADDRESS: usize = 0x140E13E30;
    /// Appends a [`WIDGET_TOGGLE`](crate::ui::pause_ui::SettingsUI::WIDGET_TOGGLE) row for `setting` to `page_rows`, with the
    /// current and default values read from the settings manager.
    pub unsafe fn AppendSettingToggle(
        &mut self,
        page_rows: *mut crate::ui::scaleform::Value,
        setting: i32,
        name: *const u8,
        labels: *const crate::ui::scaleform::Value,
        values: *const crate::ui::scaleform::Value,
        need_confirm: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                page_rows: *mut crate::ui::scaleform::Value,
                setting: i32,
                name: *const u8,
                labels: *const crate::ui::scaleform::Value,
                values: *const crate::ui::scaleform::Value,
                need_confirm: bool,
            ) = ::std::mem::transmute(Self::AppendSettingToggle_ADDRESS);
            f(
                self as *mut Self as _,
                page_rows,
                setting,
                name,
                labels,
                values,
                need_confirm,
            )
        }
    }
}
impl SettingsUI {
    /// The page index of the DLC settings, whose rows the movie sends back as
    /// [`COMMAND_SET_UPGRADE_VALUE`](crate::ui::pause_ui::PauseUI::COMMAND_SET_UPGRADE_VALUE) rather than
    /// [`COMMAND_SET_SETTING_VALUE`](crate::ui::pause_ui::PauseUI::COMMAND_SET_SETTING_VALUE).
    pub const PAGE_DLC: i32 = 3;
    /// The page index of the gameplay settings.
    pub const PAGE_GAMEPLAY: i32 = 2;
    /// The page index of the graphics settings in `m_SettingsArray`.
    pub const PAGE_GRAPHICS: i32 = 0;
    /// The page index of the sound settings.
    pub const PAGE_SOUND: i32 = 1;
    /// The number of `pause_code.controls_row` row clips the settings screen's list manager
    /// (`MCI_settings_screen.MCI_panel.MCI_container.MCI_root.MCI_manager`) draws a page's rows on,
    /// named `MCI_controls_row_00` onwards. The clips show a scrolling window of the page, and each
    /// clip's `GetData()` returns the row object it currently shows (null for an empty clip).
    pub const ROW_CLIP_COUNT: i32 = 13;
    /// The number of tab selectors on the settings screen's tab bar (`MCI_tabs.MCI_navbar` on the
    /// pause movie's document, a `shared.main_nav` from `ui/shared_lib.gfx`), named
    /// `MCI_button_00` onwards and listed in its public `m_Selectors`. A selector's position in the
    /// bar is its page index: selecting it calls the bar's select callback with that index. The
    /// settings screen names one selector per page and hides the bar.
    pub const TAB_COUNT: i32 = 9;
    /// The `m_Format` of a key-binding row (used by the controls screen's pages).
    pub const WIDGET_KEY: i32 = 2;
    /// The `m_Format` of a slider row: an integer between `m_MinValue` and `m_MaxValue`.
    pub const WIDGET_SLIDER: i32 = 1;
    /// The `m_Format` of a toggle row: the value is one of `m_Values`, shown as the matching entry
    /// of `m_Labels` and chosen from a pop-up list.
    pub const WIDGET_TOGGLE: i32 = 0;
}
impl std::convert::AsRef<SettingsUI> for SettingsUI {
    fn as_ref(&self) -> &SettingsUI {
        self
    }
}
impl std::convert::AsMut<SettingsUI> for SettingsUI {
    fn as_mut(&mut self) -> &mut SettingsUI {
        self
    }
}
