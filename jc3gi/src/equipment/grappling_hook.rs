#![cfg_attr(any(), rustfmt::skip)]
#[derive(Copy, Clone)]
#[repr(C, align(8))]
/// The grappling hook's tuning (`SGrappleTuning`), a resource the hook reads through its tuning
/// handle. The reel-in fields below are the ones the reel's velocity update
/// ([`NStateTask_ReelIn::UpdateReelVelocity`](crate::equipment::grappling_hook::NStateTask_ReelIn::UpdateReelVelocity)) reads; the rest of the layout (the grapple range,
/// the hold time, the tether spring and break settings, the wire's look, and the auto-aim, push,
/// kick, and wire settings) is unmapped.
pub struct GrappleTuning {
    _field_0: [u8; 40],
    /// The reel's top speed along the line (metres per second): a reel moving faster is slowed by
    /// [`m_ReelInClampAccel`](crate::equipment::grappling_hook::GrappleTuning::m_ReelInClampAccel).
    pub m_ReelInSpeedMax: f32,
    /// The reel's top speed while the reel boost applies (metres per second): the retract
    /// accelerates the reel ([`GrapplingHook::GetRetractAcceleratesReelIn`](crate::equipment::grappling_hook::GrapplingHook::GetRetractAcceleratesReelIn)) and the retract has
    /// been held ([`GrapplingHook::m_TimeHoldingRetract`](crate::equipment::grappling_hook::GrapplingHook::m_TimeHoldingRetract)) for longer than a tenth of a second.
    pub m_RetractReelInSpeedMax: f32,
    /// The reel's least speed along the line (metres per second): a reel moving slower is sped up
    /// by [`m_ReelInClampAccel`](crate::equipment::grappling_hook::GrappleTuning::m_ReelInClampAccel).
    pub m_ReelInSpeedMin: f32,
    /// The acceleration (metres per second squared) that brings the reel's speed back between
    /// [`m_ReelInSpeedMin`](crate::equipment::grappling_hook::GrappleTuning::m_ReelInSpeedMin) and the top speed.
    pub m_ReelInClampAccel: f32,
    /// The reel's acceleration along the line (metres per second squared).
    pub m_ReelInAccel: f32,
    /// The reel's acceleration along the line while the character's control parameters hold the
    /// retract context flag (metres per second squared). It replaces
    /// [`m_ReelInAccel`](crate::equipment::grappling_hook::GrappleTuning::m_ReelInAccel) rather than adding to it, and applies only beyond
    /// [`m_ReelInDragMaxDistance`](crate::equipment::grappling_hook::GrappleTuning::m_ReelInDragMaxDistance) from the target.
    pub m_RetractReelInAccel: f32,
    /// The drag on the reel's velocity across the line, per second.
    pub m_ReelInPerpDrag: f32,
    /// The distance from the target (metres) within which the reel stops at the target rather
    /// than pass it in one step.
    pub m_ReelInDragMaxDistance: f32,
    /// The distance from the target (metres) at which the reel counts as arrived and finishes.
    pub m_ReelInArrivedDistance: f32,
    _field_4c: [u8; 4],
}
fn _GrappleTuning_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x50], GrappleTuning>([0u8; 0x50]);
    }
    unreachable!()
}
impl GrappleTuning {}
impl std::convert::AsRef<GrappleTuning> for GrappleTuning {
    fn as_ref(&self) -> &GrappleTuning {
        self
    }
}
impl std::convert::AsMut<GrappleTuning> for GrappleTuning {
    fn as_mut(&mut self) -> &mut GrappleTuning {
        self
    }
}
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
    _field_324: [u8; 104],
    /// Whether the dual tethers were squeezed on the last update: set by
    /// [`SqueezeDualTethers`](crate::equipment::grappling_hook::GrapplingHook::SqueezeDualTethers) when its squeeze value is above zero, and
    /// cleared when it is zero or no tether exists.
    pub m_IsSqueezingDualTethers: bool,
    _field_38d: [u8; 3],
    /// How long the retract input (`RETRACT_GRAPPLE`) has been released, seconds: counted up by
    /// [`UpdateDualTetherHandling`](crate::equipment::grappling_hook::GrapplingHook::UpdateDualTetherHandling) while the input's value is zero
    /// and reset to zero while it is above.
    pub m_TimeSinceRetractHeld: f32,
    /// How long the retract input (`RETRACT_GRAPPLE`) has been held, seconds: counted up by
    /// [`UpdateDualTetherHandling`](crate::equipment::grappling_hook::GrapplingHook::UpdateDualTetherHandling) while the input's value is above
    /// zero and reset to zero while it is zero. It ramps the tether squeeze in, and the reel's
    /// boosted top speed ([`GrappleTuning::m_RetractReelInSpeedMax`](crate::equipment::grappling_hook::GrappleTuning::m_RetractReelInSpeedMax)) applies once it passes a
    /// tenth of a second.
    pub m_TimeHoldingRetract: f32,
    _field_398: [u8; 48],
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
    pub const GetNumDualTethers_ADDRESS: usize = 0x1408E5030;
    /// The number of dual tethers: the wires of the hook's tether list that are neither invalid
    /// nor broken and have neither end retracting. The tether release reads it.
    pub unsafe fn GetNumDualTethers(&self) -> i32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> i32 = ::std::mem::transmute(
                Self::GetNumDualTethers_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const GetHighestWireTension_ADDRESS: usize = 0x1408E5090;
    /// The highest tension fraction among the dual tethers that
    /// [`GetNumDualTethers`](crate::equipment::grappling_hook::GrapplingHook::GetNumDualTethers) counts, or zero without one. A wire's tension
    /// fraction is updated with its constraint (`CGrapplingHookWire::UpdateWireTension`). The
    /// player's camera shake and rumble (`CPlayer::UpdateCameraShakeRumble`) read it while the
    /// tethers are squeezed, with a stronger effect above four fifths.
    pub unsafe fn GetHighestWireTension(&self) -> f32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> f32 = ::std::mem::transmute(
                Self::GetHighestWireTension_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const HasValidTethers_ADDRESS: usize = 0x1408E5100;
    /// Whether the hook's tether list holds a wire that is neither invalid nor broken and has both
    /// ends attached.
    pub unsafe fn HasValidTethers(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::HasValidTethers_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const GetRetractAcceleratesReelIn_ADDRESS: usize = 0x1408BD090;
    /// Whether the retract accelerates a reel: the hook's own retract-accelerates flag is set and
    /// the retract reel-in boost upgrade is purchased and enabled.
    pub unsafe fn GetRetractAcceleratesReelIn(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::GetRetractAcceleratesReelIn_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const UpdateDualTetherHandling_ADDRESS: usize = 0x14093A290;
    /// The per-tick tether input. It reads the retract input's value (`RETRACT_GRAPPLE`, `0..=1`)
    /// and the release input's click (`RELEASE_GRAPPLE`) from `action_map` (a `CActionMap`), and
    /// counts [`m_TimeHoldingRetract`](crate::equipment::grappling_hook::GrapplingHook::m_TimeHoldingRetract) and
    /// [`m_TimeSinceRetractHeld`](crate::equipment::grappling_hook::GrapplingHook::m_TimeSinceRetractHeld). A release click releases the
    /// tethers. Otherwise the squeeze value is the cube of the retract value, while the hook is
    /// neither initialising, reeling in, nor holding a custom wire, the character can retract the
    /// grapple, and the player has the tether retract ability, and zero otherwise. Unless a game
    /// setting selects otherwise, the squeeze is scaled by a ramp of the hold time, from a tenth of
    /// the squeeze to all of it over the three tenths of a second after the first three. The value
    /// goes to [`SqueezeDualTethers`](crate::equipment::grappling_hook::GrapplingHook::SqueezeDualTethers).
    pub unsafe fn UpdateDualTetherHandling(
        &mut self,
        holder: *const crate::types::shared_ptr::SharedPtr<
            crate::character::character::Character,
        >,
        player: *mut ::std::ffi::c_void,
        tuning: *const crate::equipment::grappling_hook::GrappleTuning,
        action_map: *mut ::std::ffi::c_void,
        dt: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                holder: *const crate::types::shared_ptr::SharedPtr<
                    crate::character::character::Character,
                >,
                player: *mut ::std::ffi::c_void,
                tuning: *const crate::equipment::grappling_hook::GrappleTuning,
                action_map: *mut ::std::ffi::c_void,
                dt: f32,
            ) = ::std::mem::transmute(Self::UpdateDualTetherHandling_ADDRESS);
            f(self as *mut Self as _, holder, player, tuning, action_map, dt)
        }
    }
    pub const SqueezeDualTethers_ADDRESS: usize = 0x140923E00;
    /// Squeezes every wire of the tether list by `squeeze_value` (`0..=1`): each wire rebuilds its
    /// constraint toward a shorter length and smooths its squeeze factor toward the value. It sets
    /// [`m_IsSqueezingDualTethers`](crate::equipment::grappling_hook::GrapplingHook::m_IsSqueezingDualTethers).
    pub unsafe fn SqueezeDualTethers(
        &mut self,
        player: *mut ::std::ffi::c_void,
        tuning: *const crate::equipment::grappling_hook::GrappleTuning,
        squeeze_value: f32,
        dt: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                player: *mut ::std::ffi::c_void,
                tuning: *const crate::equipment::grappling_hook::GrappleTuning,
                squeeze_value: f32,
                dt: f32,
            ) = ::std::mem::transmute(Self::SqueezeDualTethers_ADDRESS);
            f(self as *mut Self as _, player, tuning, squeeze_value, dt)
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
#[repr(C, align(8))]
/// The reel-in state task (`NStateTask_ReelIn`): the character state task that carries the
/// character along the grapple line to its target while the hook is
/// [`GHS_REELING_IN`](crate::equipment::grappling_hook::GrapplingHookState::GHS_REELING_IN).
pub struct NStateTask_ReelIn {}
impl NStateTask_ReelIn {
    pub const UpdateReelVelocity_ADDRESS: usize = 0x14083A0D0;
    /// Advances the reel's velocity by `dt`. It accelerates `velocity` (in and out: the
    /// character's world velocity less the velocity it inherited) along `wanted_reel_direction` by
    /// [`GrappleTuning::m_ReelInAccel`](crate::equipment::grappling_hook::GrappleTuning::m_ReelInAccel), or [`GrappleTuning::m_RetractReelInAccel`](crate::equipment::grappling_hook::GrappleTuning::m_RetractReelInAccel) while the
    /// character's control parameters hold the retract context flag, adds the wind-up,
    /// collision-avoidance, and
    /// vehicle-entry pushes, drags the velocity across the line, clamps its speed along the line
    /// between [`GrappleTuning::m_ReelInSpeedMin`](crate::equipment::grappling_hook::GrappleTuning::m_ReelInSpeedMin) and the top speed
    /// ([`GrappleTuning::m_ReelInSpeedMax`](crate::equipment::grappling_hook::GrappleTuning::m_ReelInSpeedMax), or [`GrappleTuning::m_RetractReelInSpeedMax`](crate::equipment::grappling_hook::GrappleTuning::m_RetractReelInSpeedMax) while the
    /// reel boost applies), stops at the target within [`GrappleTuning::m_ReelInDragMaxDistance`](crate::equipment::grappling_hook::GrappleTuning::m_ReelInDragMaxDistance),
    /// and adds back the blended inherited velocity. `current_position` is the character's
    /// position and `distance_to_target` its distance from the target. The reel task's update
    /// calls it once per tick, and the trajectory prediction (`SimulateImmediateTrajectory`) calls
    /// it per predicted step with `is_simulating` set, which only suppresses its debug drawing. The
    /// disassembler mislabels it `NStateTask_ReelIn::DoPreReelBlockCheck`.
    pub unsafe fn UpdateReelVelocity(
        character: *mut crate::character::character::Character,
        hook: *const crate::types::shared_ptr::SharedPtr<
            crate::equipment::grappling_hook::GrapplingHook,
        >,
        velocity: *mut crate::types::math::Vector3,
        instance_properties: *mut ::std::ffi::c_void,
        current_position: *const crate::types::math::Vector3,
        wanted_reel_direction: *const crate::types::math::Vector3,
        distance_to_target: f32,
        tuning: *const crate::equipment::grappling_hook::GrappleTuning,
        dt: f32,
        is_simulating: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                character: *mut crate::character::character::Character,
                hook: *const crate::types::shared_ptr::SharedPtr<
                    crate::equipment::grappling_hook::GrapplingHook,
                >,
                velocity: *mut crate::types::math::Vector3,
                instance_properties: *mut ::std::ffi::c_void,
                current_position: *const crate::types::math::Vector3,
                wanted_reel_direction: *const crate::types::math::Vector3,
                distance_to_target: f32,
                tuning: *const crate::equipment::grappling_hook::GrappleTuning,
                dt: f32,
                is_simulating: bool,
            ) = ::std::mem::transmute(Self::UpdateReelVelocity_ADDRESS);
            f(
                character,
                hook,
                velocity,
                instance_properties,
                current_position,
                wanted_reel_direction,
                distance_to_target,
                tuning,
                dt,
                is_simulating,
            )
        }
    }
}
impl std::convert::AsRef<NStateTask_ReelIn> for NStateTask_ReelIn {
    fn as_ref(&self) -> &NStateTask_ReelIn {
        self
    }
}
impl std::convert::AsMut<NStateTask_ReelIn> for NStateTask_ReelIn {
    fn as_mut(&mut self) -> &mut NStateTask_ReelIn {
        self
    }
}
#[repr(C, align(8))]
/// The reeled-in ledge climb's motion task (`NStateTask_ReeledInLedgeClimbMotionTask`): the
/// character state task that carries the character over a ledge it has reeled in to. The grapple's
/// climb (`ACT_GRPL_CLIMB` into `S_GRAPPLE_CLIMB_START` in `rico_base.afsmb`) is the only ledge
/// climb the player's state machine has: no state machine under `animations/jc_statemachine` uses
/// the vault or obstacle-climb acts.
pub struct NStateTask_ReeledInLedgeClimbMotionTask {}
impl NStateTask_ReeledInLedgeClimbMotionTask {
    pub const OnEnter_ADDRESS: usize = 0x140845520;
    /// Enters the task: records the character's upright orientation and its measured world
    /// velocity, and reads the climb's speed tuning from the character's blackboard.
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
    pub const Update_ADDRESS: usize = 0x140845630;
    /// The per-tick update. Runs the non-swimming movement update, then drives the character over
    /// the ledge with a code-controlled velocity: projected along the climb from the climb
    /// segment's local time (`NGrapplingMotionUtils::ProjectClimbOverVelocityAtTime`) while the
    /// segment plays, the animation's root velocity after it, plus the velocity the character
    /// entered with. It sets the motion state to
    /// [`MOTION_STATE_TASK_DEFINED`](crate::character::character::Character::MOTION_STATE_TASK_DEFINED)
    /// and writes the proxy's wanted velocity. It runs on every tick its state is active.
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
impl std::convert::AsRef<NStateTask_ReeledInLedgeClimbMotionTask>
for NStateTask_ReeledInLedgeClimbMotionTask {
    fn as_ref(&self) -> &NStateTask_ReeledInLedgeClimbMotionTask {
        self
    }
}
impl std::convert::AsMut<NStateTask_ReeledInLedgeClimbMotionTask>
for NStateTask_ReeledInLedgeClimbMotionTask {
    fn as_mut(&mut self) -> &mut NStateTask_ReeledInLedgeClimbMotionTask {
        self
    }
}
