#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The physics proxy a character hangs from under a grapple line (`CGrapplingHangPhysicsProxy`, a
/// `CGrapplePointProxy`): a rigid body the hang task swings, with the character attached to it.
/// Only some members are bound.
pub struct GrapplingHangPhysicsProxy {
    vftable: *const crate::equipment::grappling_hook::GrapplingHangPhysicsProxyVftable,
    _field_8: [u8; 24],
    /// The proxy's rigid body, which the character hangs from. The hang task sets its angular
    /// velocity and applies its impulses to it.
    pub m_PfxProxyBody: crate::types::shared_ptr::SharedPtr<
        crate::physics::PfxRigidBody,
    >,
}
fn _GrapplingHangPhysicsProxy_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x30], GrapplingHangPhysicsProxy>([0u8; 0x30]);
    }
    unreachable!()
}
impl GrapplingHangPhysicsProxy {
    pub fn vftable(
        &self,
    ) -> *const crate::equipment::grappling_hook::GrapplingHangPhysicsProxyVftable {
        self.vftable
            as *const crate::equipment::grappling_hook::GrapplingHangPhysicsProxyVftable
    }
    /// Whether the proxy is set up and its body live. The hang task
    /// ([`NStateTask_GrapplingHangTask::Update`](crate::equipment::grappling_hook::NStateTask_GrapplingHangTask::Update)) acts on the proxy only while it holds.
    pub unsafe fn IsOk(&self) -> bool {
        unsafe {
            let f = (&raw const (*self.vftable()).IsOk).read();
            f(self as *const Self as _)
        }
    }
}
impl std::convert::AsRef<GrapplingHangPhysicsProxy> for GrapplingHangPhysicsProxy {
    fn as_ref(&self) -> &GrapplingHangPhysicsProxy {
        self
    }
}
impl std::convert::AsMut<GrapplingHangPhysicsProxy> for GrapplingHangPhysicsProxy {
    fn as_mut(&mut self) -> &mut GrapplingHangPhysicsProxy {
        self
    }
}
#[repr(C, align(8))]
pub struct GrapplingHangPhysicsProxyVftable {
    _vfunc_0: unsafe extern "system" fn(
        this: *mut crate::equipment::grappling_hook::GrapplingHangPhysicsProxy,
    ),
    _vfunc_1: unsafe extern "system" fn(
        this: *mut crate::equipment::grappling_hook::GrapplingHangPhysicsProxy,
    ),
    _vfunc_2: unsafe extern "system" fn(
        this: *mut crate::equipment::grappling_hook::GrapplingHangPhysicsProxy,
    ),
    /// Whether the proxy is set up and its body live. The hang task
    /// ([`NStateTask_GrapplingHangTask::Update`](crate::equipment::grappling_hook::NStateTask_GrapplingHangTask::Update)) acts on the proxy only while it holds.
    pub IsOk: unsafe extern "system" fn(
        this: *const crate::equipment::grappling_hook::GrapplingHangPhysicsProxy,
    ) -> bool,
}
fn _GrapplingHangPhysicsProxyVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<
            [u8; 0x20],
            GrapplingHangPhysicsProxyVftable,
        >([0u8; 0x20]);
    }
    unreachable!()
}
impl GrapplingHangPhysicsProxyVftable {}
impl std::convert::AsRef<GrapplingHangPhysicsProxyVftable>
for GrapplingHangPhysicsProxyVftable {
    fn as_ref(&self) -> &GrapplingHangPhysicsProxyVftable {
        self
    }
}
impl std::convert::AsMut<GrapplingHangPhysicsProxyVftable>
for GrapplingHangPhysicsProxyVftable {
    fn as_mut(&mut self) -> &mut GrapplingHangPhysicsProxyVftable {
        self
    }
}
/// Enables the hang task's discrete stick swing (`NStateTask_GrapplingHangTask::s_enable_discrete_swing`).
/// Zero-initialised and written by no code in this build.
pub unsafe fn get_GrapplingHangTask_EnableDiscreteSwing() -> &'static mut bool {
    unsafe { &mut *(0x142F30E90 as *mut bool) }
}
/// Enables the hang task's continuous stick swing
/// (`NStateTask_GrapplingHangTask::s_enable_small_impulse_swing`). Zero-initialised and written by
/// no code in this build.
pub unsafe fn get_GrapplingHangTask_EnableSmallImpulseSwing() -> &'static mut bool {
    unsafe { &mut *(0x142F30E84 as *mut bool) }
}
/// The continuous stick swing's impulse at full stick deflection
/// ([`GrapplingHangTask_EnableSmallImpulseSwing`](get_GrapplingHangTask_EnableSmallImpulseSwing)), applied once per hang-task tick and scaled by
/// the stick's magnitude.
pub unsafe fn get_GrapplingHangTask_SmallImpulseSwingMagnitude() -> &'static mut f32 {
    unsafe { &mut *(0x142D65A80 as *mut f32) }
}
#[repr(C, align(8))]
/// The grappling-hook equipment item (`CGrapplingHook`, a `CGameObject`): owns the wires, the
/// reel/tether state machine, and the cached fire target. A character's hook is held by its
/// inventory ([`Inventory::m_GrapplingHook`](crate::character::inventory::Inventory::m_GrapplingHook)).
pub struct GrapplingHook {
    _field_0: [u8; 564],
    /// The current state-machine state; see [`GrapplingHookState`](crate::equipment::grappling_hook::GrapplingHookState). The state records the *last*
    /// reel outcome as much as the current one: leaving a `GHS_REELED_*` attachment does not
    /// reliably return it to [`GHS_INACTIVE`](crate::equipment::grappling_hook::GrapplingHookState::GHS_INACTIVE) — readers such as
    /// `IsHookAimingState` and `ShouldShowAttachedUI` therefore combine it with
    /// [`m_ActiveWire`](crate::equipment::grappling_hook::GrapplingHook::m_ActiveWire) rather than trusting it alone.
    pub m_State: crate::equipment::grappling_hook::GrapplingHookState,
    /// The latched fire target, filled by [`SetUpTarget`](crate::equipment::grappling_hook::GrapplingHook::SetUpTarget) when the hook fires.
    /// [`GetLastGrappleTargetPos`](crate::equipment::grappling_hook::GrapplingHook::GetLastGrappleTargetPos) reads its current world position,
    /// so the fire and its arm IK follow the latched target rather than the live aim slot.
    pub m_HookFireInfo: crate::equipment::wire_end::HookTargetInfo,
    /// The countdown (seconds) armed when a hook fire is committed, within which the fire
    /// animation must emit its release track message before the fire is abandoned. Set to `2.0`
    /// alongside [`m_WaitingForGrappleFire`](crate::equipment::grappling_hook::GrapplingHook::m_WaitingForGrappleFire) by the fire
    /// dispatchers (`FireHookTowardsPos`, `TryFireHook`).
    pub m_WaitForFireSignalTimeout: f32,
    /// Whether a grapple-hook fire is committed and awaiting the fire animation's release track
    /// message: set when the fire dispatches (before the fire act plays and the wire spawns),
    /// cleared when the hook releases or the fire times out. The fire act's directional alignment
    /// of the character runs within this window.
    pub m_WaitingForGrappleFire: bool,
    /// The dual-tether counterpart of
    /// [`m_WaitingForGrappleFire`](crate::equipment::grappling_hook::GrapplingHook::m_WaitingForGrappleFire); the two are cleared
    /// together as one word.
    pub m_WaitingForTetherFire: bool,
    _field_276: [u8; 162],
    /// The cached reel-in target position in world space. The reel-in aim IK turns the chest
    /// toward it while a wire is active. The offset is confirmed; the name is
    /// unconfirmed.
    pub m_CachedReelTargetPosition: crate::types::math::Vector3,
    _field_324: [u8; 164],
    /// The wire currently driving the reel/attach flow, or null when none is live. The state
    /// readers null-check this before interpreting [`m_State`](crate::equipment::grappling_hook::GrapplingHook::m_State).
    pub m_ActiveWire: crate::types::shared_ptr::SharedPtr<
        crate::equipment::grappling_hook::GrapplingHookWire,
    >,
    _field_3d8: [u8; 24],
    /// The physics proxy the character hangs from while reeled into a hang
    /// ([`GHS_REELED_HANG`](crate::equipment::grappling_hook::GrapplingHookState::GHS_REELED_HANG)).
    pub m_GrapplingHangProxy: crate::equipment::grappling_hook::GrapplingHangPhysicsProxy,
}
fn _GrapplingHook_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x420], GrapplingHook>([0u8; 0x420]);
    }
    unreachable!()
}
impl GrapplingHook {
    pub const SetUpTarget_ADDRESS: usize = 0x140939CF0;
    /// Latches the `player`'s (a `CPlayer`) aim slot `target_type` into `target_info`: reads the
    /// slot's [`m_AimPos`](crate::aim::aim::PlayerAimControl::m_AimPos) and
    /// [`m_Target`](crate::aim::aim::PlayerAimControl::m_Target) from the player's aim control, stores the
    /// position in the target object's space, and falls back to the aim control's current grapple
    /// game object only for [`TARGET_GRAPPLE`](crate::aim::target::TargetType::TARGET_GRAPPLE). Later changes to the
    /// aim slot do not move the latched target.
    pub unsafe fn SetUpTarget(
        player: *mut ::std::ffi::c_void,
        target_info: *mut crate::equipment::wire_end::HookTargetInfo,
        target_type: crate::aim::target::TargetType,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                player: *mut ::std::ffi::c_void,
                target_info: *mut crate::equipment::wire_end::HookTargetInfo,
                target_type: crate::aim::target::TargetType,
            ) = ::std::mem::transmute(Self::SetUpTarget_ADDRESS);
            f(player, target_info, target_type)
        }
    }
    pub const GetLastGrappleTargetPos_ADDRESS: usize = 0x14093A4B0;
    /// Writes the latched fire target's current world position
    /// ([`m_HookFireInfo`](crate::equipment::grappling_hook::GrapplingHook::m_HookFireInfo)) and returns true.
    pub unsafe fn GetLastGrappleTargetPos(
        &self,
        position: *mut crate::types::math::Vector3,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                position: *mut crate::types::math::Vector3,
            ) -> bool = ::std::mem::transmute(Self::GetLastGrappleTargetPos_ADDRESS);
            f(self as *const Self as _, position)
        }
    }
    pub const TryFireHook_ADDRESS: usize = 0x1409432E0;
    /// Computes the fire direction toward the latched target and hands it to
    /// [`DoActFire`](crate::equipment::grappling_hook::GrapplingHook::DoActFire).
    pub unsafe fn TryFireHook(
        &mut self,
        holder: *const crate::types::shared_ptr::SharedPtr<
            crate::character::character::Character,
        >,
        dual_tether_fire: bool,
        grapple_reel_fire: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                holder: *const crate::types::shared_ptr::SharedPtr<
                    crate::character::character::Character,
                >,
                dual_tether_fire: bool,
                grapple_reel_fire: bool,
            ) = ::std::mem::transmute(Self::TryFireHook_ADDRESS);
            f(self as *mut Self as _, holder, dual_tether_fire, grapple_reel_fire)
        }
    }
    pub const DoActFire_ADDRESS: usize = 0x1408FED80;
    /// Queues the directional fire act for `aim_direction` on `holder`: picks the
    /// `ACT_GRAPPLE_FIRE_*` sector (or `ACT_GRAPPLE_TETHER_FIRE_*` for a dual tether) from the
    /// angle between the direction and the character. No wire is created here; the hook spawns on
    /// the fire animation's release track message.
    pub unsafe fn DoActFire(
        aim_direction: *const crate::types::math::Vector3,
        holder: *const crate::types::shared_ptr::SharedPtr<
            crate::character::character::Character,
        >,
        dual_tether_fire: bool,
        grapple_reel_fire: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                aim_direction: *const crate::types::math::Vector3,
                holder: *const crate::types::shared_ptr::SharedPtr<
                    crate::character::character::Character,
                >,
                dual_tether_fire: bool,
                grapple_reel_fire: bool,
            ) = ::std::mem::transmute(Self::DoActFire_ADDRESS);
            f(aim_direction, holder, dual_tether_fire, grapple_reel_fire)
        }
    }
    pub const OnPreSimUpdate_ADDRESS: usize = 0x140949F70;
    /// The pre-simulation hook update: dispatches the grapple input (fire, reel, dual tether,
    /// retract, release) and advances the reel and tether state.
    pub unsafe fn OnPreSimUpdate(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::OnPreSimUpdate_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const OnPostCamUpdate_ADDRESS: usize = 0x14094ED60;
    /// The post-camera hook update. On the fire animation's release frame it creates the wire
    /// ([`CreateNewWire`](crate::equipment::grappling_hook::GrapplingHook::CreateNewWire)), pins the near end to the device, and starts the
    /// far end extending toward the latched target
    /// ([`WireEnd::SetExtend`](crate::equipment::wire_end::WireEnd::SetExtend)).
    pub unsafe fn OnPostCamUpdate(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::OnPostCamUpdate_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const CreateNewWire_ADDRESS: usize = 0x140923BC0;
    /// Spawns a new wire at `spawn_pos` and returns it through `result`.
    pub unsafe fn CreateNewWire(
        &mut self,
        result: *mut crate::types::shared_ptr::SharedPtr<
            crate::equipment::grappling_hook::GrapplingHookWire,
        >,
        spawn_pos: *const crate::types::math::Vector3,
    ) -> *mut crate::types::shared_ptr::SharedPtr<
        crate::equipment::grappling_hook::GrapplingHookWire,
    > {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                result: *mut crate::types::shared_ptr::SharedPtr<
                    crate::equipment::grappling_hook::GrapplingHookWire,
                >,
                spawn_pos: *const crate::types::math::Vector3,
            ) -> *mut crate::types::shared_ptr::SharedPtr<
                crate::equipment::grappling_hook::GrapplingHookWire,
            > = ::std::mem::transmute(Self::CreateNewWire_ADDRESS);
            f(self as *mut Self as _, result, spawn_pos)
        }
    }
    pub const RetractActiveWire_ADDRESS: usize = 0x1408F7A10;
    /// Retracts the active wire: destroys its physics constraint, detaches the near end, and
    /// notifies the HUD.
    pub unsafe fn RetractActiveWire(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::RetractActiveWire_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const IsHookAimingState_ADDRESS: usize = 0x1408C8EA0;
    /// Whether the hook is in its "aiming" state: an active wire that is not reeling in and not a
    /// custom wire; with the re-reel upgrade, reeling in also counts.
    pub unsafe fn IsHookAimingState(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsHookAimingState_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const ShouldShowAttachedUI_ADDRESS: usize = 0x1408C94C0;
    /// Whether the HUD shows the attached-wire part of the grapple reticle: an active, attached
    /// wire while the hook is not in [`GHS_REELING_IN`](crate::equipment::grappling_hook::GrapplingHookState::GHS_REELING_IN),
    /// [`GHS_REELED_ATTACHED`](crate::equipment::grappling_hook::GrapplingHookState::GHS_REELED_ATTACHED),
    /// [`GHS_REELED_HANG`](crate::equipment::grappling_hook::GrapplingHookState::GHS_REELED_HANG), or
    /// [`GHS_REELED_UPSIDEDOWN`](crate::equipment::grappling_hook::GrapplingHookState::GHS_REELED_UPSIDEDOWN).
    pub unsafe fn ShouldShowAttachedUI(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::ShouldShowAttachedUI_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const GetActiveWireAttachmentGraphicsWorldPosition_ADDRESS: usize = 0x1408D7B30;
    /// Writes the interpolated graphics world position of the active wire's attached far end for
    /// the render interpolation factor `dtf`. Returns false without an attached active wire.
    pub unsafe fn GetActiveWireAttachmentGraphicsWorldPosition(
        &self,
        dtf: f32,
        gfx_pos_out: *mut crate::types::math::Vector3,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                dtf: f32,
                gfx_pos_out: *mut crate::types::math::Vector3,
            ) -> bool = ::std::mem::transmute(
                Self::GetActiveWireAttachmentGraphicsWorldPosition_ADDRESS,
            );
            f(self as *const Self as _, dtf, gfx_pos_out)
        }
    }
    pub const PushWireDataToUI_ADDRESS: usize = 0x140902DC0;
    /// Pushes the wires' tension and attach-type data to the HUD (`CHUDUI::UpdateWireData`).
    pub unsafe fn PushWireDataToUI(&self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) = ::std::mem::transmute(
                Self::PushWireDataToUI_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
}
impl std::convert::AsRef<GrapplingHook> for GrapplingHook {
    fn as_ref(&self) -> &GrapplingHook {
        self
    }
}
impl std::convert::AsMut<GrapplingHook> for GrapplingHook {
    fn as_mut(&mut self) -> &mut GrapplingHook {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The grappling hook's high-level state machine (`CGrapplingHook::m_State`). `SetState` is the
/// mutator; the reel tasks (`NStateTask_ReelIn`) and the reeled-in controller
/// (`NReeledInController`) drive the transitions. A reel-to-target starts in
/// [`GHS_REELING_IN`](crate::equipment::grappling_hook::GrapplingHookState::GHS_REELING_IN) -- during which
/// `NReeledInController::RotateToGrappleTarget` rotates the character's root toward the target as
/// the reel pulls them in -- and completes into one of the `GHS_REELED_*` attachment states
/// depending on the landing surface and pose. `GHS_CUSTOM_ACTIVE_WIRE` is a wire held open outside
/// the reel flow (tethers).
pub enum GrapplingHookState {
    GHS_INITIALIZING = 0isize as _,
    GHS_INACTIVE = 1isize as _,
    GHS_REELING_IN = 2isize as _,
    GHS_REELED_ATTACHED = 3isize as _,
    GHS_REELED_HANG = 4isize as _,
    GHS_REELED_UPSIDEDOWN = 5isize as _,
    GHS_REELED_STUNT = 6isize as _,
    GHS_CUSTOM_ACTIVE_WIRE = 7isize as _,
}
fn _GrapplingHookState_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], GrapplingHookState>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// A single grapple wire (`CGrapplingHookWire`, a `CGameObject`): two `CWireEnd`s (near/device and
/// far/hook), the physics constraint, and the wire rendering. Only referenced through
/// [`GrapplingHook::m_ActiveWire`](crate::equipment::grappling_hook::GrapplingHook::m_ActiveWire) here; the layout is unmapped.
pub struct GrapplingHookWire {}
impl GrapplingHookWire {}
impl std::convert::AsRef<GrapplingHookWire> for GrapplingHookWire {
    fn as_ref(&self) -> &GrapplingHookWire {
        self
    }
}
impl std::convert::AsMut<GrapplingHookWire> for GrapplingHookWire {
    fn as_mut(&mut self) -> &mut GrapplingHookWire {
        self
    }
}
#[repr(C, align(8))]
/// The hang task (`NStateTask_GrapplingHangTask`): the character state task that runs while the
/// character hangs under a grapple line.
pub struct NStateTask_GrapplingHangTask {}
impl NStateTask_GrapplingHangTask {
    pub const Update_ADDRESS: usize = 0x1408151D0;
    /// The per-tick update. While the hook's
    /// [`m_GrapplingHangProxy`](crate::equipment::grappling_hook::GrapplingHook::m_GrapplingHangProxy) is
    /// [`IsOk`](crate::equipment::grappling_hook::GrapplingHangPhysicsProxy::IsOk), it post-updates the proxy for the context's
    /// time step, reads the move stick (squared to a circle and turned into the world through the
    /// camera's input matrix), and blends the proxy body's angular velocity toward a yaw rate: toward
    /// the weapon aim point while the character aims a weapon or the grapple or throws a grenade,
    /// and toward zero otherwise. A gun held in a hand that fired within the last tenth of a second
    /// pushes the body back along the weapon's forward axis at that hand. Two stick-driven swings
    /// follow, each behind its own flag: a discrete swing impulse
    /// ([`GrapplingHangTask_EnableDiscreteSwing`](get_GrapplingHangTask_EnableDiscreteSwing)) and a continuous one scaled by the stick's
    /// magnitude ([`GrapplingHangTask_EnableSmallImpulseSwing`](get_GrapplingHangTask_EnableSmallImpulseSwing),
    /// [`GrapplingHangTask_SmallImpulseSwingMagnitude`](get_GrapplingHangTask_SmallImpulseSwingMagnitude)), both applied at the hips
    /// ([`Character::GetHipsPosition`](crate::character::character::Character::GetHipsPosition)) with
    /// [`PfxRigidBody::ApplyImpulse`](crate::physics::PfxRigidBody::ApplyImpulse). Nothing in this build sets either flag, so the stick does not
    /// swing a hanging character. Without a valid proxy it runs the drop-out alignment instead, or
    /// cancels the hang.
    pub unsafe fn Update(
        ctx: *mut crate::state::StateContext,
        p1: *mut ::std::ffi::c_void,
        p2: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                ctx: *mut crate::state::StateContext,
                p1: *mut ::std::ffi::c_void,
                p2: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::Update_ADDRESS);
            f(ctx, p1, p2)
        }
    }
}
impl std::convert::AsRef<NStateTask_GrapplingHangTask> for NStateTask_GrapplingHangTask {
    fn as_ref(&self) -> &NStateTask_GrapplingHangTask {
        self
    }
}
impl std::convert::AsMut<NStateTask_GrapplingHangTask> for NStateTask_GrapplingHangTask {
    fn as_mut(&mut self) -> &mut NStateTask_GrapplingHangTask {
        self
    }
}
#[repr(C, align(8))]
/// The in-air grapple fire motion state task (`NStateTask_InAirGrappleFireMotionTask`). Like the
/// other `NStateTask` functions, it runs against the character's state context.
pub struct NStateTask_InAirGrappleFireMotionTask {}
impl NStateTask_InAirGrappleFireMotionTask {
    pub const OnEnter_ADDRESS: usize = 0x140810B60;
    /// Enters the task, orienting the fire motion from the camera matrix
    /// (`CGameCameraManager::GetCameraMatrix`, read directly rather than through the aim slot) and
    /// the latched target ([`GrapplingHook::GetLastGrappleTargetPos`](crate::equipment::grappling_hook::GrapplingHook::GetLastGrappleTargetPos)).
    pub unsafe fn OnEnter(
        ctx: *mut crate::state::StateContext,
        p1: *mut ::std::ffi::c_void,
        p2: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                ctx: *mut crate::state::StateContext,
                p1: *mut ::std::ffi::c_void,
                p2: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::OnEnter_ADDRESS);
            f(ctx, p1, p2)
        }
    }
}
impl std::convert::AsRef<NStateTask_InAirGrappleFireMotionTask>
for NStateTask_InAirGrappleFireMotionTask {
    fn as_ref(&self) -> &NStateTask_InAirGrappleFireMotionTask {
        self
    }
}
impl std::convert::AsMut<NStateTask_InAirGrappleFireMotionTask>
for NStateTask_InAirGrappleFireMotionTask {
    fn as_mut(&mut self) -> &mut NStateTask_InAirGrappleFireMotionTask {
        self
    }
}
