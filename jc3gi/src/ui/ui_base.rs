#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The base of every Scaleform screen (`CUIBase`): one per screen class (`CPauseUI`, `CHUDUI`,
/// ...), each driving its own clip inside the shared root movie. The root movie's AS3 loads the
/// screen's movie when the screen activates, then reports the loaded clip back through the
/// ExternalInterface command [`COMMAND_ACTIVE`](crate::ui::ui_base::UIBase::COMMAND_ACTIVE), which
/// [`HandleCommand`](crate::ui::ui_base::UIBase::HandleCommand) stores in [`m_Clip`](crate::ui::ui_base::UIBase::m_Clip) before calling the
/// [`OnActive`](crate::ui::ui_base::UIBase::OnActive) virtual. From then on the engine calls AS3 methods on the clip
/// through [`Invoke`](crate::ui::ui_base::UIBase::Invoke), and the clip calls back into the screen with ExternalInterface
/// calls named `"<class name>:<command>"`, which
/// [`ScaleformEIHandler::Callback`](crate::ui::scaleform::ScaleformEIHandler::Callback) routes to the
/// screen's [`HandleCommand`](crate::ui::ui_base::UIBase::HandleCommand) virtual as the command's `hashlittle` hash.
pub struct UIBase {
    vftable: *const crate::ui::ui_base::UIBaseVftable,
    _field_8: [u8; 12],
    /// The screen's activation state.
    pub m_Current: crate::ui::overlay_ui::UIBaseState,
    /// The state the screen is moving towards; activation and deactivation requests set it, and
    /// the root movie's load and unload reports move [`m_Current`](crate::ui::ui_base::UIBase::m_Current) to it.
    pub m_Desired: crate::ui::overlay_ui::UIBaseState,
    _field_1c: [u8; 100],
    /// The screen's own AS3 clip (the document class instance of its movie, for example `pause`
    /// for `CPauseUI`), a managed display-object value. Set by [`COMMAND_ACTIVE`](crate::ui::ui_base::UIBase::COMMAND_ACTIVE)
    /// and released by [`COMMAND_INACTIVE`](crate::ui::ui_base::UIBase::COMMAND_INACTIVE); null while the screen is not
    /// loaded.
    pub m_Clip: crate::ui::scaleform::Value,
    /// The root movie the clip lives in.
    pub m_Movie: *mut crate::ui::scaleform::MovieImpl,
    _field_b8: [u8; 56],
}
fn _UIBase_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xF0], UIBase>([0u8; 0xF0]);
    }
    unreachable!()
}
impl UIBase {
    pub fn vftable(&self) -> *const crate::ui::ui_base::UIBaseVftable {
        self.vftable as *const crate::ui::ui_base::UIBaseVftable
    }
    pub const BaseHandleCommand_ADDRESS: usize = 0x140FF8E10;
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
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                command: u32,
                args: *const crate::ui::scaleform::Value,
                num_args: u32,
                movie: *mut crate::ui::scaleform::MovieImpl,
            ) = ::std::mem::transmute(Self::BaseHandleCommand_ADDRESS);
            f(self as *mut Self as _, command, args, num_args, movie)
        }
    }
    pub const Deactivate_ADDRESS: usize = 0x140FF80D0;
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
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, unload: bool) = ::std::mem::transmute(
                Self::Deactivate_ADDRESS,
            );
            f(self as *mut Self as _, unload)
        }
    }
    pub const InvokeImpl_ADDRESS: usize = 0x140FF8300;
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
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                method: *const u8,
                args: *const crate::ui::scaleform::Value,
                num_args: u32,
                result: *mut crate::ui::scaleform::Value,
                delay: f32,
            ) -> bool = ::std::mem::transmute(Self::InvokeImpl_ADDRESS);
            f(self as *mut Self as _, method, args, num_args, result, delay)
        }
    }
    pub const BaseOnManageInput_ADDRESS: usize = 0x140F19CB0;
    /// The base [`OnManageInput`](crate::ui::ui_base::UIBase::OnManageInput): maps the steering's UI actions to the
    /// clip's AS3 input handlers.
    pub unsafe fn BaseOnManageInput(
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
            ) = ::std::mem::transmute(Self::BaseOnManageInput_ADDRESS);
            f(self as *mut Self as _, steering, dt, check_for_freeze)
        }
    }
    pub const CreateObject_ADDRESS: usize = 0x140F1A6E0;
    /// Creates an empty AS3 `Object` in [`m_Movie`](crate::ui::ui_base::UIBase::m_Movie) into `value`.
    pub unsafe fn CreateObject(&mut self, value: *mut crate::ui::scaleform::Value) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                value: *mut crate::ui::scaleform::Value,
            ) = ::std::mem::transmute(Self::CreateObject_ADDRESS);
            f(self as *mut Self as _, value)
        }
    }
    pub const CreateArray_ADDRESS: usize = 0x140F1A6D0;
    /// Creates an empty AS3 `Array` in [`m_Movie`](crate::ui::ui_base::UIBase::m_Movie) into `value`.
    pub unsafe fn CreateArray(&mut self, value: *mut crate::ui::scaleform::Value) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                value: *mut crate::ui::scaleform::Value,
            ) = ::std::mem::transmute(Self::CreateArray_ADDRESS);
            f(self as *mut Self as _, value)
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
impl UIBase {
    /// The ExternalInterface command hash for `active`: the root movie reports the screen's loaded
    /// clip (the single argument).
    pub const COMMAND_ACTIVE: u32 = 3131892121;
    /// The command hash for `event`: the clip posts the string argument as an engine event
    /// (`NEvent::CPostEvent::PostMsg`), which is how AS3 `SendEvent` and `PlaySound` reach native
    /// code.
    pub const COMMAND_EVENT: u32 = 3406611444;
    /// The command hash for `inactive`: the root movie has unloaded the screen's clip.
    pub const COMMAND_INACTIVE: u32 = 3815554577;
}
impl std::convert::AsRef<UIBase> for UIBase {
    fn as_ref(&self) -> &UIBase {
        self
    }
}
impl std::convert::AsMut<UIBase> for UIBase {
    fn as_mut(&mut self) -> &mut UIBase {
        self
    }
}
#[repr(C, align(8))]
pub struct UIBaseVftable {
    _vfunc_0: unsafe extern "system" fn(this: *mut crate::ui::ui_base::UIBase),
    _vfunc_1: unsafe extern "system" fn(this: *mut crate::ui::ui_base::UIBase),
    _vfunc_2: unsafe extern "system" fn(this: *mut crate::ui::ui_base::UIBase),
    /// Runs the screen's per-frame input: reads the steering's UI actions and calls the
    /// matching AS3 handler on the clip through [`Invoke`](crate::ui::ui_base::UIBase::Invoke) (`OnUp`, `OnDown`,
    /// `OnLeft`, `OnRight`, the `OnDpad*` family, `OnSelect`, `OnCancel`, `OnSpecialButton`,
    /// `OnYButton`, `OnNextTabButton`, `OnPrevTabButton`, `OnNextPageButton`,
    /// `OnPrevPageButton`, and the rest of the `uibase` input methods). Only the screen with
    /// the highest input priority is called each frame.
    pub OnManageInput: unsafe extern "system" fn(
        this: *mut crate::ui::ui_base::UIBase,
        steering: *mut crate::input::steering::Steering,
        dt: f32,
        check_for_freeze: bool,
    ),
    _vfunc_4: unsafe extern "system" fn(this: *mut crate::ui::ui_base::UIBase),
    _vfunc_5: unsafe extern "system" fn(this: *mut crate::ui::ui_base::UIBase),
    _vfunc_6: unsafe extern "system" fn(this: *mut crate::ui::ui_base::UIBase),
    /// Called when the screen's clip has loaded and been stored in [`m_Clip`](crate::ui::ui_base::UIBase::m_Clip),
    /// with [`m_Current`](crate::ui::ui_base::UIBase::m_Current) already
    /// [`ACTIVE`](crate::ui::overlay_ui::UIBaseState::ACTIVE). Screens push their initial data to
    /// the clip here.
    pub OnActive: unsafe extern "system" fn(this: *mut crate::ui::ui_base::UIBase),
    /// Called by [`Deactivate`](crate::ui::ui_base::UIBase::Deactivate) when a deactivation is requested while the
    /// screen is not [`INACTIVE`](crate::ui::overlay_ui::UIBaseState::INACTIVE), before
    /// [`m_Current`](crate::ui::ui_base::UIBase::m_Current) moves to
    /// [`DEACTIVATING`](crate::ui::overlay_ui::UIBaseState::DEACTIVATING) and the root movie is asked
    /// to unload the clip. The clip is still loaded. Runs on the thread that requested the
    /// deactivation, which is not necessarily the capture thread.
    pub OnDeactivate: unsafe extern "system" fn(this: *mut crate::ui::ui_base::UIBase),
    /// Called by the [`COMMAND_INACTIVE`](crate::ui::ui_base::UIBase::COMMAND_INACTIVE) handling once the root
    /// movie has unloaded the clip, before [`m_Clip`](crate::ui::ui_base::UIBase::m_Clip) is released.
    pub OnInactive: unsafe extern "system" fn(this: *mut crate::ui::ui_base::UIBase),
    _vfunc_10: unsafe extern "system" fn(this: *mut crate::ui::ui_base::UIBase),
    _vfunc_11: unsafe extern "system" fn(this: *mut crate::ui::ui_base::UIBase),
    _vfunc_12: unsafe extern "system" fn(this: *mut crate::ui::ui_base::UIBase),
    /// Handles an ExternalInterface command from the clip. `command` is the `hashlittle` hash
    /// of the command name (the part after the colon), `args` the call's arguments, and
    /// `movie` the calling movie, through which a command that returns a value sets it with
    /// [`SetExternalInterfaceRetVal`](crate::ui::scaleform::MovieImpl::SetExternalInterfaceRetVal).
    /// The base implementation is [`BaseHandleCommand`](crate::ui::ui_base::UIBase::BaseHandleCommand); screens
    /// override it and fall back to it for commands they do not handle.
    pub HandleCommand: unsafe extern "system" fn(
        this: *mut crate::ui::ui_base::UIBase,
        command: u32,
        args: *const crate::ui::scaleform::Value,
        num_args: u32,
        movie: *mut crate::ui::scaleform::MovieImpl,
    ),
    /// Calls the AS3 method `method` on the clip. The implementation is
    /// [`InvokeImpl`](crate::ui::ui_base::UIBase::InvokeImpl), shared by every screen.
    pub Invoke: unsafe extern "system" fn(
        this: *mut crate::ui::ui_base::UIBase,
        method: *const u8,
        args: *const crate::ui::scaleform::Value,
        num_args: u32,
        result: *mut crate::ui::scaleform::Value,
        delay: f32,
    ) -> bool,
}
fn _UIBaseVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x78], UIBaseVftable>([0u8; 0x78]);
    }
    unreachable!()
}
impl UIBaseVftable {}
impl std::convert::AsRef<UIBaseVftable> for UIBaseVftable {
    fn as_ref(&self) -> &UIBaseVftable {
        self
    }
}
impl std::convert::AsMut<UIBaseVftable> for UIBaseVftable {
    fn as_mut(&mut self) -> &mut UIBaseVftable {
        self
    }
}
pub const SetIntField_ADDRESS: usize = 0x140F68ED0;
/// `CUIBase::SetIntField` (static): sets the member `name` of the AS3 object `object` to `value`,
/// passing an unmanaged int value to
/// [`ValueObjectInterface::SetMember`](crate::ui::scaleform::ValueObjectInterface::SetMember). The
/// settings-page builders fill their row objects with it.
pub unsafe fn SetIntField(
    object: *mut crate::ui::scaleform::Value,
    name: *const u8,
    value: i32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            object: *mut crate::ui::scaleform::Value,
            name: *const u8,
            value: i32,
        ) = ::std::mem::transmute(SetIntField_ADDRESS);
        f(object, name, value)
    }
}
pub const SetStringField_ADDRESS: usize = 0x140F690F0;
/// `CUIBase::SetStringField` (static): sets the member `name` of the AS3 object `object` to the
/// C string `value`, passed as an unmanaged string value (the pointer itself, which the movie
/// copies into an AS3 string) to
/// [`ValueObjectInterface::SetMember`](crate::ui::scaleform::ValueObjectInterface::SetMember).
pub unsafe fn SetStringField(
    object: *mut crate::ui::scaleform::Value,
    name: *const u8,
    value: *const u8,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            object: *mut crate::ui::scaleform::Value,
            name: *const u8,
            value: *const u8,
        ) = ::std::mem::transmute(SetStringField_ADDRESS);
        f(object, name, value)
    }
}
