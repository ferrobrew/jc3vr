#![cfg_attr(any(), rustfmt::skip)]
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// Which hook model a wire end shows (`CWireEnd::EHookModelState`).
pub enum HookModelState {
    GWEHMS_HIDDEN = 0isize as _,
    GWEHMS_VISIBLE_OPEN = 1isize as _,
    GWEHMS_VISIBLE_CLOSED = 2isize as _,
}
fn _HookModelState_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], HookModelState>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// A latched grapple target (`CWireEnd::SHookTargetInfo`): the target position, plus the target or
/// game object it is relative to and the position in that object's space. The hook fills it from
/// the aim state when it fires, and the wire end flies to and attaches at it.
pub struct HookTargetInfo {
    /// The target the hook homes on, or null for a target-less (surface) latch.
    pub m_HomingHookTarget: *mut crate::aim::target::Target,
    /// The world-space target position at the time it was latched.
    /// `CGrapplingHook::SetUpTarget` writes the aim slot's position here.
    pub m_TargetPosition: crate::types::math::Vector3,
    /// The target position in the space of [`m_TargetGameObject`](crate::equipment::wire_end::HookTargetInfo::m_TargetGameObject): in a
    /// character's, in the space of bone [`m_BoneIndex`](crate::equipment::wire_end::HookTargetInfo::m_BoneIndex); in any other
    /// object's, as [`SetupTargetFromGameObject`](crate::equipment::wire_end::HookTargetInfo::SetupTargetFromGameObject) was given it.
    /// [`GetTargetWorldPosition`](crate::equipment::wire_end::HookTargetInfo::GetTargetWorldPosition) carries it into the world, so the
    /// latched point follows the object.
    pub m_TargetLocalPosition: crate::types::math::Vector3,
    /// Whether the aim slot was in range when the target was latched.
    pub m_WasInRange: bool,
    _field_21: [u8; 3],
    /// The bone of a character target that
    /// [`m_TargetLocalPosition`](crate::equipment::wire_end::HookTargetInfo::m_TargetLocalPosition) is relative to (the bone nearest the
    /// latched point), or -1 for none.
    pub m_BoneIndex: i32,
    /// The target game object (its `CGameObject` subobject), or null.
    pub m_TargetGameObject: crate::types::shared_ptr::WeakPtrGameObject,
}
fn _HookTargetInfo_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x38], HookTargetInfo>([0u8; 0x38]);
    }
    unreachable!()
}
impl HookTargetInfo {
    pub const SetupTargetFromTarget_ADDRESS: usize = 0x140926C00;
    /// Latches `target` with `local_position` in the target's game-object space.
    pub unsafe fn SetupTargetFromTarget(
        &mut self,
        target: *mut crate::aim::target::Target,
        local_position: *const crate::types::math::Vector3,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                target: *mut crate::aim::target::Target,
                local_position: *const crate::types::math::Vector3,
            ) = ::std::mem::transmute(Self::SetupTargetFromTarget_ADDRESS);
            f(self as *mut Self as _, target, local_position)
        }
    }
    pub const SetupTargetFromGameObject_ADDRESS: usize = 0x140926E50;
    /// Latches the game object `game_object` (a `CGameObject` subobject): copies its weak self
    /// reference into [`m_TargetGameObject`](crate::equipment::wire_end::HookTargetInfo::m_TargetGameObject), and for a character the
    /// nearest bone to `local_position` (in the character's model space) and the position in that
    /// bone's space; for any other object `local_position` as it is. It writes neither
    /// [`m_TargetPosition`](crate::equipment::wire_end::HookTargetInfo::m_TargetPosition), [`m_WasInRange`](crate::equipment::wire_end::HookTargetInfo::m_WasInRange), nor
    /// [`m_HomingHookTarget`](crate::equipment::wire_end::HookTargetInfo::m_HomingHookTarget), and for an object other than a character
    /// not [`m_BoneIndex`](crate::equipment::wire_end::HookTargetInfo::m_BoneIndex).
    pub unsafe fn SetupTargetFromGameObject(
        &mut self,
        game_object: *mut ::std::ffi::c_void,
        local_position: *const crate::types::math::Vector3,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                game_object: *mut ::std::ffi::c_void,
                local_position: *const crate::types::math::Vector3,
            ) = ::std::mem::transmute(Self::SetupTargetFromGameObject_ADDRESS);
            f(self as *mut Self as _, game_object, local_position)
        }
    }
    pub const GetTargetWorldPosition_ADDRESS: usize = 0x140927040;
    /// Writes the latched target's current world position, following the target object when there
    /// is one.
    pub unsafe fn GetTargetWorldPosition(
        &self,
        result: *mut crate::types::math::Vector3,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                result: *mut crate::types::math::Vector3,
            ) = ::std::mem::transmute(Self::GetTargetWorldPosition_ADDRESS);
            f(self as *const Self as _, result)
        }
    }
}
impl std::convert::AsRef<HookTargetInfo> for HookTargetInfo {
    fn as_ref(&self) -> &HookTargetInfo {
        self
    }
}
impl std::convert::AsMut<HookTargetInfo> for HookTargetInfo {
    fn as_mut(&mut self) -> &mut HookTargetInfo {
        self
    }
}
#[repr(C, align(16))]
/// One end of a grapple wire (`CWireEnd`). A [`GrapplingHookWire`](crate::equipment::grappling_hook::GrapplingHookWire)
/// holds two: [`m_WireEnds[0]`](crate::equipment::grappling_hook::GrapplingHookWire::m_WireEnds) is the
/// hook (far) end, which flies to the target, and `m_WireEnds[1]` the device end, attached to the
/// holder's grapple device bone
/// ([`GrapplingHook::AttachLocalWireEndToDevice`](crate::equipment::grappling_hook::GrapplingHook::AttachLocalWireEndToDevice)).
/// An end extends toward a latched [`HookTargetInfo`](crate::equipment::wire_end::HookTargetInfo), then attaches to a proxy
/// (`CGrapplePointProxy`), not to a bare coordinate.
pub struct WireEnd {
    _field_0: [u8; 80],
    /// The end's state; see [`WireEndState`](crate::equipment::wire_end::WireEndState).
    pub m_WireEndState: crate::equipment::wire_end::WireEndState,
    _field_54: [u8; 4],
    /// The attachment proxy (`CGrapplePointProxy`) while attached, or null.
    pub m_PointProxy: *mut ::std::ffi::c_void,
    /// The wire's other end.
    pub m_OtherEnd: *mut crate::equipment::wire_end::WireEnd,
    /// The wire that holds the end.
    pub m_OwnerWire: *mut ::std::ffi::c_void,
    /// The material effect table the impact effect is played from
    /// ([`PlayImpactEffect`](crate::equipment::wire_end::WireEnd::PlayImpactEffect)).
    pub m_MaterialEffectTable: *mut ::std::ffi::c_void,
    _field_78: [u8; 4],
    /// The hook model shown; see [`HookModelState`](crate::equipment::wire_end::HookModelState).
    pub m_HookModelState: crate::equipment::wire_end::HookModelState,
    _field_80: [u8; 32],
    /// The end's world position this tick.
    pub m_WorldPositionT1: crate::types::math::Vector3,
    /// The end's world position the tick before.
    pub m_WorldPositionT0: crate::types::math::Vector3,
    /// The target the end flies to, latched by [`SetExtend`](crate::equipment::wire_end::WireEnd::SetExtend).
    pub m_TargetInfo: crate::equipment::wire_end::HookTargetInfo,
    /// The flight speed, metres per second.
    pub m_Speed: f32,
    /// Whether the end flies as a ghost wire: it makes no cast in flight, and at its goal it
    /// attaches to the latched object ([`ForceAttachToTarget`](crate::equipment::wire_end::WireEnd::ForceAttachToTarget)) when its
    /// last casts find nothing.
    pub m_GhostWire: bool,
    _field_f5: [u8; 3],
    /// The direction of flight, unit length.
    pub m_HookDirection: crate::types::math::Vector3,
    _field_104: [u8; 12],
    /// The end's cast filter.
    pub m_HookCastCollisionFilter: crate::equipment::wire_end::WireEndCastFilter,
}
fn _WireEnd_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1F0], WireEnd>([0u8; 0x1F0]);
    }
    unreachable!()
}
impl WireEnd {
    pub const SetExtend_ADDRESS: usize = 0x14093D700;
    /// Puts the end into its extending state: detaches it, sets its position to `start_pos`, its
    /// speed to `speed`, and its ghost flag to `ghost_wire`, latches `target_info`, shows the open
    /// hook, and points it at the target's world position. A scripted fire and the fire
    /// animation's release both start the far end through it, and a tether's second fire starts
    /// the device end through it.
    pub unsafe fn SetExtend(
        &mut self,
        start_pos: *const crate::types::math::Vector3,
        speed: f32,
        target_info: *mut crate::equipment::wire_end::HookTargetInfo,
        ghost_wire: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                start_pos: *const crate::types::math::Vector3,
                speed: f32,
                target_info: *mut crate::equipment::wire_end::HookTargetInfo,
                ghost_wire: bool,
            ) = ::std::mem::transmute(Self::SetExtend_ADDRESS);
            f(self as *mut Self as _, start_pos, speed, target_info, ghost_wire)
        }
    }
    pub const Attach_ADDRESS: usize = 0x14094AFB0;
    /// Attaches the end at `hit_pos`: builds the attachment proxy from the ray hit `cast_info`
    /// with [`SetupProxyForAttachment`](crate::equipment::wire_end::WireEnd::SetupProxyForAttachment) and moves the end to its
    /// attached state.
    pub unsafe fn Attach(
        &mut self,
        hit_pos: *const crate::types::math::Vector3,
        cast_info: *mut crate::physics::CastRayInfo,
        hook_owner: *const crate::types::shared_ptr::SharedPtr<
            crate::character::character::Character,
        >,
        reattaching: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                hit_pos: *const crate::types::math::Vector3,
                cast_info: *mut crate::physics::CastRayInfo,
                hook_owner: *const crate::types::shared_ptr::SharedPtr<
                    crate::character::character::Character,
                >,
                reattaching: bool,
            ) = ::std::mem::transmute(Self::Attach_ADDRESS);
            f(self as *mut Self as _, hit_pos, cast_info, hook_owner, reattaching)
        }
    }
    pub const AttachToProxy_ADDRESS: usize = 0x1408F4850;
    /// Attaches the end to an existing attachment proxy (a `CGrapplePointProxy`).
    pub unsafe fn AttachToProxy(&mut self, proxy: *mut ::std::ffi::c_void) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                proxy: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::AttachToProxy_ADDRESS);
            f(self as *mut Self as _, proxy)
        }
    }
    pub const AttachToCharacterBone_ADDRESS: usize = 0x1408F91F0;
    /// Attaches the end to bone `bone` of `character` at `local_offset`, through a new
    /// `CCharacterBoneAttachedPointProxy`.
    pub unsafe fn AttachToCharacterBone(
        &mut self,
        character: *const crate::types::shared_ptr::SharedPtr<
            crate::character::character::Character,
        >,
        bone: i32,
        local_offset: *const crate::types::math::Matrix4,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                character: *const crate::types::shared_ptr::SharedPtr<
                    crate::character::character::Character,
                >,
                bone: i32,
                local_offset: *const crate::types::math::Matrix4,
            ) = ::std::mem::transmute(Self::AttachToCharacterBone_ADDRESS);
            f(self as *mut Self as _, character, bone, local_offset)
        }
    }
    pub const ForceAttachToTarget_ADDRESS: usize = 0x14094B880;
    /// Attaches the end to its latched target object's body without a ray hit: requires a physics
    /// game object whose physics instance exists and is valid (a vehicle's own instance for a
    /// vehicle), builds a [`CastRayInfo`](crate::physics::CastRayInfo) with the normal against
    /// [`m_HookDirection`](crate::equipment::wire_end::WireEnd::m_HookDirection) and the instance's body, and calls
    /// [`Attach`](crate::equipment::wire_end::WireEnd::Attach) at the target's world position. It plays no impact effect.
    pub unsafe fn ForceAttachToTarget(
        &mut self,
        owner: *const crate::types::shared_ptr::SharedPtr<
            crate::character::character::Character,
        >,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                owner: *const crate::types::shared_ptr::SharedPtr<
                    crate::character::character::Character,
                >,
            ) = ::std::mem::transmute(Self::ForceAttachToTarget_ADDRESS);
            f(self as *mut Self as _, owner)
        }
    }
    pub const SetupProxyForAttachment_ADDRESS: usize = 0x140948F40;
    /// Builds the attachment proxy for the hit `cast_info` at `hit_pos`, by the hit object's kind,
    /// or returns null when the object cannot be attached: an object whose grappling mode is
    /// [`E_GM_UnGrappable`](crate::physics::GrapplingMode::E_GM_UnGrappable), one flagged dead or without
    /// a live physics instance, or a character with the grapple-immunity control flag. A weapon's
    /// dynamic body gets a `CGenericObjectPointProxy` on the weapon, as other loose objects do.
    /// Nothing here reads [`E_GM_ReelInNoAttach`](crate::physics::GrapplingMode::E_GM_ReelInNoAttach).
    pub unsafe fn SetupProxyForAttachment(
        &mut self,
        owner: *const crate::types::shared_ptr::SharedPtr<
            crate::character::character::Character,
        >,
        hit_pos: *const crate::types::math::Vector3,
        cast_info: *mut crate::physics::CastRayInfo,
        reattaching: bool,
    ) -> *mut ::std::ffi::c_void {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                owner: *const crate::types::shared_ptr::SharedPtr<
                    crate::character::character::Character,
                >,
                hit_pos: *const crate::types::math::Vector3,
                cast_info: *mut crate::physics::CastRayInfo,
                reattaching: bool,
            ) -> *mut ::std::ffi::c_void = ::std::mem::transmute(
                Self::SetupProxyForAttachment_ADDRESS,
            );
            f(self as *mut Self as _, owner, hit_pos, cast_info, reattaching)
        }
    }
    pub const TryCollide_ADDRESS: usize = 0x14094B5E0;
    /// Casts the end's segment from `from` to `to` on
    /// [`RAYCAST_EVERYTHING_PHYSICAL`](crate::physics::PfxLayer::RAYCAST_EVERYTHING_PHYSICAL) with its cast
    /// filter, and at a hit attaches there and plays the impact effect.
    pub unsafe fn TryCollide(
        &mut self,
        from: *const crate::types::math::Vector3,
        to: *const crate::types::math::Vector3,
        owner: *const crate::types::shared_ptr::SharedPtr<
            crate::character::character::Character,
        >,
        reattaching: bool,
        p6: bool,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                from: *const crate::types::math::Vector3,
                to: *const crate::types::math::Vector3,
                owner: *const crate::types::shared_ptr::SharedPtr<
                    crate::character::character::Character,
                >,
                reattaching: bool,
                p6: bool,
            ) -> bool = ::std::mem::transmute(Self::TryCollide_ADDRESS);
            f(self as *mut Self as _, from, to, owner, reattaching, p6)
        }
    }
    pub const Update_ADDRESS: usize = 0x14094CD00;
    /// Advances the end by its state. Extending, it flies toward the latched target's world
    /// position, and past the wire's [`m_MaxLength`](crate::equipment::grappling_hook::GrapplingHookWire::m_MaxLength)
    /// it retracts. A non-ghost end casts each step with [`TryCollide`](crate::equipment::wire_end::WireEnd::TryCollide), and near
    /// the target sweeps a small sphere ahead (the disassembler names that sphere cast
    /// `CDebugRenderer::DebugLineTTL`; it is a second `CPhysicsSystem::CastSphere`). A ghost end
    /// makes no cast in flight; within one step of its goal it casts twice with its ghost filter,
    /// and when both miss it clears [`m_GhostWire`](crate::equipment::wire_end::WireEnd::m_GhostWire) and attaches to the target
    /// ([`ForceAttachToTarget`](crate::equipment::wire_end::WireEnd::ForceAttachToTarget)). It returns at once while
    /// [`GWES_INVALID`](crate::equipment::wire_end::WireEndState::GWES_INVALID).
    pub unsafe fn Update(
        &mut self,
        owner: *const crate::types::shared_ptr::SharedPtr<
            crate::character::character::Character,
        >,
        dt: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                owner: *const crate::types::shared_ptr::SharedPtr<
                    crate::character::character::Character,
                >,
                dt: f32,
            ) = ::std::mem::transmute(Self::Update_ADDRESS);
            f(self as *mut Self as _, owner, dt)
        }
    }
    pub const PlayImpactEffect_ADDRESS: usize = 0x140926440;
    /// Plays the hook's impact effect and sound at `pos` for the hit material of `cast_info` (its
    /// body's material), through the attached object's effect container for a damageable one.
    /// Needs [`m_PointProxy`](crate::equipment::wire_end::WireEnd::m_PointProxy) and
    /// [`m_MaterialEffectTable`](crate::equipment::wire_end::WireEnd::m_MaterialEffectTable), and plays nothing beyond 200 metres
    /// of the camera.
    pub unsafe fn PlayImpactEffect(
        &mut self,
        pos: *const crate::types::math::Vector3,
        normal: *const crate::types::math::Vector3,
        cast_info: *mut crate::physics::CastRayInfo,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                pos: *const crate::types::math::Vector3,
                normal: *const crate::types::math::Vector3,
                cast_info: *mut crate::physics::CastRayInfo,
            ) = ::std::mem::transmute(Self::PlayImpactEffect_ADDRESS);
            f(self as *mut Self as _, pos, normal, cast_info)
        }
    }
}
impl std::convert::AsRef<WireEnd> for WireEnd {
    fn as_ref(&self) -> &WireEnd {
        self
    }
}
impl std::convert::AsMut<WireEnd> for WireEnd {
    fn as_mut(&mut self) -> &mut WireEnd {
        self
    }
}
#[repr(C, align(16))]
/// A wire end's cast filter (`CWireEnd::CGrapplingHookWireEndCastFilter`), an
/// [`IgnoreMaterialFlagFilter`](crate::physics::IgnoreMaterialFlagFilter). Its hit test ([`OnRayHit`](crate::equipment::wire_end::WireEndCastFilter::OnRayHit)) rejects the two
/// ignored objects and characters with a control flag, and accepts only characters, vehicles, rigid
/// bodies, heightfields, ragdoll proxy phantoms, and destruction instances. In ghost mode
/// ([`m_GhostModeTargetInfo`](crate::equipment::wire_end::WireEndCastFilter::m_GhostModeTargetInfo) set) it accepts only the latched target
/// object or its children.
pub struct WireEndCastFilter {
    pub base: crate::physics::IgnoreMaterialFlagFilter,
    /// The first ignored object: the object a scripted fire names
    /// ([`GrapplingHook::FireHookCustom`](crate::equipment::grappling_hook::GrapplingHook::FireHookCustom)).
    pub m_IgnoreObject1: *mut ::std::ffi::c_void,
    /// The second ignored object: the hook's holder.
    pub m_IgnoreObject2: *mut ::std::ffi::c_void,
    /// The end's own [`WireEnd::m_TargetInfo`](crate::equipment::wire_end::WireEnd::m_TargetInfo) while the end flies as a ghost wire, or null.
    pub m_GhostModeTargetInfo: *mut crate::equipment::wire_end::HookTargetInfo,
    _field_d8: [u8; 8],
}
fn _WireEndCastFilter_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xE0], WireEndCastFilter>([0u8; 0xE0]);
    }
    unreachable!()
}
impl WireEndCastFilter {
    /// `CRaycastFilter::CRaycastFilter`: constructs the filter with its two material masks, the
    /// instance to ignore (may be null), and the world (null selects [`PhysicsSystem::m_World`](crate::physics::PhysicsSystem::m_World)).
    pub unsafe fn Construct(
        &mut self,
        ignore_material_mask: crate::physics::MaterialPassFlags,
        force_material_mask: crate::physics::MaterialPassFlags,
        ignore_instance: *mut ::std::ffi::c_void,
        world: *const crate::physics::hknpWorld,
    ) -> *mut crate::physics::RaycastFilter {
        unsafe {
            self.base
                .Construct(
                    ignore_material_mask,
                    force_material_mask,
                    ignore_instance,
                    world,
                )
        }
    }
    /// Sets or clears [`BULLET_PASS`](crate::physics::MaterialPassFlags::BULLET_PASS) in the ignore mask.
    pub unsafe fn SetIgnoreBulletPass(&mut self, ignore: bool) {
        unsafe { self.base.SetIgnoreBulletPass(ignore) }
    }
    /// Sets or clears [`RAY_PASS`](crate::physics::MaterialPassFlags::RAY_PASS) in the ignore mask.
    pub unsafe fn SetIgnoreRayPass(&mut self, ignore: bool) {
        unsafe { self.base.SetIgnoreRayPass(ignore) }
    }
    /// Sets or clears [`CAMERA_PASS`](crate::physics::MaterialPassFlags::CAMERA_PASS) in the ignore mask.
    pub unsafe fn SetIgnoreCameraPass(&mut self, ignore: bool) {
        unsafe { self.base.SetIgnoreCameraPass(ignore) }
    }
    pub const OnRayHit_ADDRESS: usize = 0x1409269E0;
    /// The filter's hit test (slot 7 of its virtual table).
    pub unsafe fn OnRayHit(&mut self, hit: *mut ::std::ffi::c_void) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                hit: *mut ::std::ffi::c_void,
            ) -> bool = ::std::mem::transmute(Self::OnRayHit_ADDRESS);
            f(self as *mut Self as _, hit)
        }
    }
}
impl std::convert::AsRef<crate::physics::IgnoreMaterialFlagFilter>
for WireEndCastFilter {
    fn as_ref(&self) -> &crate::physics::IgnoreMaterialFlagFilter {
        &self.base
    }
}
impl std::convert::AsMut<crate::physics::IgnoreMaterialFlagFilter>
for WireEndCastFilter {
    fn as_mut(&mut self) -> &mut crate::physics::IgnoreMaterialFlagFilter {
        &mut self.base
    }
}
impl std::convert::AsRef<crate::physics::RaycastFilter> for WireEndCastFilter {
    fn as_ref(&self) -> &crate::physics::RaycastFilter {
        &self.base.base
    }
}
impl std::convert::AsMut<crate::physics::RaycastFilter> for WireEndCastFilter {
    fn as_mut(&mut self) -> &mut crate::physics::RaycastFilter {
        &mut self.base.base
    }
}
impl std::convert::AsRef<WireEndCastFilter> for WireEndCastFilter {
    fn as_ref(&self) -> &WireEndCastFilter {
        self
    }
}
impl std::convert::AsMut<WireEndCastFilter> for WireEndCastFilter {
    fn as_mut(&mut self) -> &mut WireEndCastFilter {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// A wire end's state (`CWireEnd::EEndState`).
pub enum WireEndState {
    GWES_EXTENDING = 0isize as _,
    GWES_ATTACHED = 1isize as _,
    GWES_RETRACTING = 2isize as _,
    GWES_DETACHED = 3isize as _,
    GWES_INVALID = 4isize as _,
}
fn _WireEndState_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], WireEndState>([0u8; 0x4]);
    }
    unreachable!()
}
