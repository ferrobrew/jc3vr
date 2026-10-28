#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A [`CastRayInfoBase`](crate::physics::CastRayInfoBase) that records the hit object and resolves the hit material on demand
/// (`CCastRayInfo`). Engine callers construct the base first and then switch the virtual table to
/// [`VFTABLE`](crate::physics::CastRayInfo::VFTABLE) and clear [`m_GameObject`](crate::physics::CastRayInfo::m_GameObject).
pub struct CastRayInfo {
    pub base: crate::physics::CastRayInfoBase,
    /// The hit object: the [`PhysicsGameObject`](crate::physics::PhysicsGameObject) that owns the hit physics instance (for a
    /// character, the `CCharacter`), written by the virtual hit-object setter [`PhysicsSystem::CastRay`](crate::physics::PhysicsSystem::CastRay) calls. Null
    /// for world geometry and on a miss.
    pub m_GameObject: *mut ::std::ffi::c_void,
    _field_40: [u8; 8],
}
fn _CastRayInfo_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x48], CastRayInfo>([0u8; 0x48]);
    }
    unreachable!()
}
impl CastRayInfo {
    /// Clears the result for reuse: a unit normal and fraction, a null object, an invalid body,
    /// then the virtual hit-object setter with null. It calls through the virtual table, which must
    /// already be set.
    pub unsafe fn Reset(&mut self) {
        unsafe { self.base.Reset() }
    }
    /// Whether the hit material lets bullets pass
    /// ([`BULLET_PASS`](crate::physics::MaterialPassFlags::BULLET_PASS)); false for two special material ids.
    pub unsafe fn DidHitBulletPassMaterial(&self) -> bool {
        unsafe { self.base.DidHitBulletPassMaterial() }
    }
    pub const GetHitMaterial_ADDRESS: usize = 0x1402B3BA0;
    /// `CCastRayInfo::GetHitMaterial` (a virtual): the hit shape's game material
    /// (`EGameMaterialId`), looked up through the physics system's world.
    pub unsafe fn GetHitMaterial(&self) -> i32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> i32 = ::std::mem::transmute(
                Self::GetHitMaterial_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
}
impl CastRayInfo {
    /// The address of the `CCastRayInfo` virtual table.
    pub const VFTABLE: u64 = 5405692616;
}
impl std::convert::AsRef<crate::physics::CastRayInfoBase> for CastRayInfo {
    fn as_ref(&self) -> &crate::physics::CastRayInfoBase {
        &self.base
    }
}
impl std::convert::AsMut<crate::physics::CastRayInfoBase> for CastRayInfo {
    fn as_mut(&mut self) -> &mut crate::physics::CastRayInfoBase {
        &mut self.base
    }
}
impl std::convert::AsRef<CastRayInfo> for CastRayInfo {
    fn as_ref(&self) -> &CastRayInfo {
        self
    }
}
impl std::convert::AsMut<CastRayInfo> for CastRayInfo {
    fn as_mut(&mut self) -> &mut CastRayInfo {
        self
    }
}
#[repr(C, align(8))]
/// The result of a ray query (`CCastRayInfoBase`): the hit normal and fraction, and the hit object
/// and body. [`PhysicsSystem::CastRay`](crate::physics::PhysicsSystem::CastRay) fills it from the query's closest hit, and on a miss sets the
/// fraction to one and clears the object. Engine callers construct it on the stack: the
/// [`VFTABLE`](crate::physics::CastRayInfoBase::VFTABLE) pointer, a null [`m_HitObjectId`](crate::physics::CastRayInfoBase::m_HitObjectId), then
/// [`Reset`](crate::physics::CastRayInfoBase::Reset).
pub struct CastRayInfoBase {
    pub __vftable: u64,
    /// The surface normal at the hit.
    pub m_Normal: crate::types::math::Vector3,
    /// The hit's position along the queried segment, as a fraction of the segment from the query's
    /// start distance to its end distance; one on a miss.
    pub m_HitDistance: f32,
    /// The object id (`SObjectID`) of the hit object, or null for world geometry.
    pub m_HitObjectId: u64,
    _field_20: [u8; 16],
    /// The Havok body id of the hit body.
    pub m_BodyId: u32,
    _field_34: [u8; 4],
}
fn _CastRayInfoBase_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x38], CastRayInfoBase>([0u8; 0x38]);
    }
    unreachable!()
}
impl CastRayInfoBase {
    pub const Reset_ADDRESS: usize = 0x1402558C0;
    /// Clears the result for reuse: a unit normal and fraction, a null object, an invalid body,
    /// then the virtual hit-object setter with null. It calls through the virtual table, which must
    /// already be set.
    pub unsafe fn Reset(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Reset_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const DidHitBulletPassMaterial_ADDRESS: usize = 0x14023BDF0;
    /// Whether the hit material lets bullets pass
    /// ([`BULLET_PASS`](crate::physics::MaterialPassFlags::BULLET_PASS)); false for two special material ids.
    pub unsafe fn DidHitBulletPassMaterial(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::DidHitBulletPassMaterial_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
}
impl CastRayInfoBase {
    /// The address of the `CCastRayInfoBase` virtual table.
    pub const VFTABLE: u64 = 5405687560;
}
impl std::convert::AsRef<CastRayInfoBase> for CastRayInfoBase {
    fn as_ref(&self) -> &CastRayInfoBase {
        self
    }
}
impl std::convert::AsMut<CastRayInfoBase> for CastRayInfoBase {
    fn as_mut(&mut self) -> &mut CastRayInfoBase {
        self
    }
}
#[repr(C, align(4))]
/// The Havok-facing input of a character proxy (`CPfxCharacterInstance::CCharacterInput`), read by
/// the physics step to drive the proxy.
pub struct CharacterInput {
    pub jump: bool,
    /// Whether [`wantedVelocity`](crate::physics::CharacterInput::wantedVelocity) is game-controlled. The on-foot locomotion
    /// task sets it while the proxy is supported on stairs
    /// ([`PfxCharacterInstance::m_IsSupportedOnStairs`](crate::physics::PfxCharacterInstance::m_IsSupportedOnStairs)) and the character's blackboard carries a
    /// slope direction, and clears it otherwise. The proxy update rotates the wanted velocity by
    /// the proxy's rotation only while it is set; otherwise it hands the vector to the character
    /// state as it stands.
    pub gameControledVelocity: bool,
    _field_2: [u8; 2],
    /// The velocity the proxy should move at, metres per second, in the character's local frame
    /// (not world space). The on-foot locomotion task
    /// (`NStateTask_MovementLocomotionTask::Update`) writes its solved velocity here every tick,
    /// including standing idle, and returns without writing while the character is attached to a
    /// vehicle. [`Character::SetWantedWorldVelocity`](crate::character::character::Character::SetWantedWorldVelocity)
    /// and [`Character::GetWantedWorldVelocity`](crate::character::character::Character::GetWantedWorldVelocity)
    /// convert it from and to world space.
    pub wantedVelocity: crate::types::math::Vector3,
}
fn _CharacterInput_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], CharacterInput>([0u8; 0x10]);
    }
    unreachable!()
}
impl CharacterInput {}
impl std::convert::AsRef<CharacterInput> for CharacterInput {
    fn as_ref(&self) -> &CharacterInput {
        self
    }
}
impl std::convert::AsMut<CharacterInput> for CharacterInput {
    fn as_mut(&mut self) -> &mut CharacterInput {
        self
    }
}
#[repr(C, align(8))]
/// A physics constraint placed in the world (`CConstraint`): a game object that joins two objects'
/// bodies with a Havok constraint, made from its data's kind (fixed, deformable fixed, ball and
/// socket, stiff spring, hinge, limited hinge, ragdoll, wheel, or prismatic), optionally motored
/// (position, spring-damper, or velocity motors) and breakable. It names the two objects by object
/// id and connects them once both exist; the physics step then resolves the constraint with the
/// bodies' other contacts. Only its type id is bound.
pub struct Constraint {}
impl Constraint {
    pub const TYPE_ID_ADDRESS: usize = 0x140977BC0;
    /// `CConstraint::TYPE_ID`: the run-time type id of `CConstraint`, for
    /// [`GameObject::IsType`](crate::game::GameObject::IsType). Registers it on first use.
    pub unsafe fn TYPE_ID() -> *const u32 {
        unsafe {
            let f: unsafe extern "system" fn() -> *const u32 = ::std::mem::transmute(
                Self::TYPE_ID_ADDRESS,
            );
            f()
        }
    }
}
impl std::convert::AsRef<Constraint> for Constraint {
    fn as_ref(&self) -> &Constraint {
        self
    }
}
impl std::convert::AsMut<Constraint> for Constraint {
    fn as_mut(&mut self) -> &mut Constraint {
        self
    }
}
#[repr(C, align(8))]
/// The world's collision filter (`CCustomGroupCollisionFilter`), behind the constraint collision
/// filter that also keeps constrained bodies apart. It reads each body's collision-filter word
/// ([`hknpBody::m_collisionFilterInfo`](crate::physics::hknpBody::m_collisionFilterInfo)), whose fields are the layer
/// ([`LAYER_MASK`](crate::physics::CustomGroupCollisionFilter::LAYER_MASK), a [`PfxLayer`](crate::physics::PfxLayer)), the subsystem id and the no-collide id (each
/// [`FIELD_MASK`](crate::physics::CustomGroupCollisionFilter::FIELD_MASK) wide, from [`SUBSYSTEM_SHIFT`](crate::physics::CustomGroupCollisionFilter::SUBSYSTEM_SHIFT) and
/// [`NO_COLLIDE_SHIFT`](crate::physics::CustomGroupCollisionFilter::NO_COLLIDE_SHIFT)), and the system group (from
/// [`SYSTEM_GROUP_SHIFT`](crate::physics::CustomGroupCollisionFilter::SYSTEM_GROUP_SHIFT)). Two bodies in different system groups, or in
/// system group zero, collide when the layer table says their layers do. Two bodies in the same
/// non-zero system group collide only when their layers are equal, at least one has a non-zero
/// subsystem id, and neither's subsystem id is the other's no-collide id. Only the word's layout is
/// bound.
pub struct CustomGroupCollisionFilter {}
impl CustomGroupCollisionFilter {}
impl CustomGroupCollisionFilter {
    /// The width of the subsystem-id and no-collide-id fields.
    pub const FIELD_MASK: u32 = 31;
    /// The layer field of a collision-filter word.
    pub const LAYER_MASK: u32 = 31;
    /// Where the no-collide-id field begins.
    pub const NO_COLLIDE_SHIFT: u32 = 10;
    /// Where the subsystem-id field begins.
    pub const SUBSYSTEM_SHIFT: u32 = 5;
    /// Where the system-group field, the word's high half, begins.
    pub const SYSTEM_GROUP_SHIFT: u32 = 16;
}
impl std::convert::AsRef<CustomGroupCollisionFilter> for CustomGroupCollisionFilter {
    fn as_ref(&self) -> &CustomGroupCollisionFilter {
        self
    }
}
impl std::convert::AsMut<CustomGroupCollisionFilter> for CustomGroupCollisionFilter {
    fn as_mut(&mut self) -> &mut CustomGroupCollisionFilter {
        self
    }
}
#[repr(u32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// A game material (`EGameMaterialId`): the surface kind a physics shape is tagged with, which
/// picks the impact effects and sounds played against it. Only the values the melee hit's impact
/// lookup uses are bound.
pub enum GameMaterialId {
    EGameMaterialId_Flesh = 25isize as _,
    EGameMaterialId_Default = 31isize as _,
}
fn _GameMaterialId_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], GameMaterialId>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// How the grapple treats a physics game object (`CPhysicsGameObject::EGrapplingMode`), from
/// [`PhysicsGameObject::GetGrapplingMode`](crate::physics::PhysicsGameObject::GetGrapplingMode).
pub enum GrapplingMode {
    E_GM_Normal = 0isize as _,
    E_GM_ReelInNoAttach = 1isize as _,
    E_GM_UnGrappable = 2isize as _,
}
fn _GrapplingMode_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], GrapplingMode>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// The physics system's global collision listener. Contact impulse dispatch resolves both body
/// instances and calls each instance's event virtual when its subscription flag is active.
pub struct HavokCollisionListener {}
impl HavokCollisionListener {
    pub const onContactImpulseEvent_ADDRESS: usize = 0x1401CE500;
    /// Dispatches a contact impulse event without modifying the contact's solver impulses.
    pub unsafe fn onContactImpulseEvent(
        &mut self,
        input: *const ::std::ffi::c_void,
        event: *const crate::havok::hknpContactImpulseEvent,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                input: *const ::std::ffi::c_void,
                event: *const crate::havok::hknpContactImpulseEvent,
            ) = ::std::mem::transmute(Self::onContactImpulseEvent_ADDRESS);
            f(self as *mut Self as _, input, event)
        }
    }
}
impl std::convert::AsRef<HavokCollisionListener> for HavokCollisionListener {
    fn as_ref(&self) -> &HavokCollisionListener {
        self
    }
}
impl std::convert::AsMut<HavokCollisionListener> for HavokCollisionListener {
    fn as_mut(&mut self) -> &mut HavokCollisionListener {
        self
    }
}
#[repr(C, align(16))]
/// A [`RaycastFilter`](crate::physics::RaycastFilter) whose ignore mask is set pass by pass (`CIgnoreMaterialFlagFilter`).
pub struct IgnoreMaterialFlagFilter {
    pub base: crate::physics::RaycastFilter,
}
fn _IgnoreMaterialFlagFilter_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xC0], IgnoreMaterialFlagFilter>([0u8; 0xC0]);
    }
    unreachable!()
}
impl IgnoreMaterialFlagFilter {
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
    pub const SetIgnoreBulletPass_ADDRESS: usize = 0x140B86840;
    /// Sets or clears [`BULLET_PASS`](crate::physics::MaterialPassFlags::BULLET_PASS) in the ignore mask.
    pub unsafe fn SetIgnoreBulletPass(&mut self, ignore: bool) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, ignore: bool) = ::std::mem::transmute(
                Self::SetIgnoreBulletPass_ADDRESS,
            );
            f(self as *mut Self as _, ignore)
        }
    }
    pub const SetIgnoreRayPass_ADDRESS: usize = 0x140B86860;
    /// Sets or clears [`RAY_PASS`](crate::physics::MaterialPassFlags::RAY_PASS) in the ignore mask.
    pub unsafe fn SetIgnoreRayPass(&mut self, ignore: bool) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, ignore: bool) = ::std::mem::transmute(
                Self::SetIgnoreRayPass_ADDRESS,
            );
            f(self as *mut Self as _, ignore)
        }
    }
    pub const SetIgnoreCameraPass_ADDRESS: usize = 0x140B86880;
    /// Sets or clears [`CAMERA_PASS`](crate::physics::MaterialPassFlags::CAMERA_PASS) in the ignore mask.
    pub unsafe fn SetIgnoreCameraPass(&mut self, ignore: bool) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, ignore: bool) = ::std::mem::transmute(
                Self::SetIgnoreCameraPass_ADDRESS,
            );
            f(self as *mut Self as _, ignore)
        }
    }
}
impl std::convert::AsRef<crate::physics::RaycastFilter> for IgnoreMaterialFlagFilter {
    fn as_ref(&self) -> &crate::physics::RaycastFilter {
        &self.base
    }
}
impl std::convert::AsMut<crate::physics::RaycastFilter> for IgnoreMaterialFlagFilter {
    fn as_mut(&mut self) -> &mut crate::physics::RaycastFilter {
        &mut self.base
    }
}
impl std::convert::AsRef<IgnoreMaterialFlagFilter> for IgnoreMaterialFlagFilter {
    fn as_ref(&self) -> &IgnoreMaterialFlagFilter {
        self
    }
}
impl std::convert::AsMut<IgnoreMaterialFlagFilter> for IgnoreMaterialFlagFilter {
    fn as_mut(&mut self) -> &mut IgnoreMaterialFlagFilter {
        self
    }
}
crate::__bitflags! {
    #[doc =
    " Material pass bits of a [`RaycastFilter`](crate::physics::RaycastFilter)'s material masks. Game materials carry per-pass"]
    #[doc =
    " \"holey\" flags (a bullet passes foliage, a ray passes glass, and similar); a filter that sets a"]
    #[doc = " bit in its ignore mask does not stop at materials flagged for that pass."]
    pub struct MaterialPassFlags : u32 { const BULLET_PASS = 32usize as _; const
    CAMERA_PASS = 64usize as _; const RAY_PASS = 128usize as _; }
}
fn _MaterialPassFlags_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], MaterialPassFlags>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(16))]
/// `CPfxCharacterInstance`: the character's physics-side body instance (the Havok character proxy),
/// owned by `CCharacter` through a `boost::shared_ptr`. Only some members are bound.
pub struct PfxCharacterInstance {
    _field_0: [u8; 56],
    /// The proxy input the physics step reads; see [`CharacterInput`](crate::physics::CharacterInput).
    pub m_Input: crate::physics::CharacterInput,
    _field_48: [u8; 40],
    /// What the proxy stands on, as [`CheckSupportCustom`](crate::physics::PfxCharacterInstance::CheckSupportCustom) last found
    /// it: whether it is supported, sliding, or unsupported, and the surface's normal and
    /// velocity. Written at the start of each proxy update ([`UpdateContext`](crate::physics::PfxCharacterInstance::UpdateContext)),
    /// before the physics step, and read until the next: the state machines' ground-contact
    /// condition and [`IsSupported`](crate::physics::PfxCharacterInstance::IsSupported) read its supported state.
    pub m_GroundInformation: crate::physics::hknpCharacterSurfaceInfo,
    /// The game material of the surface the proxy stands on; cleared with the ground body when
    /// the proxy has no support.
    pub m_ContactMaterialId: i32,
    /// The body the proxy stands on, or [`INVALID_BODY_ID`](crate::physics::PfxCharacterInstance::INVALID_BODY_ID) without
    /// support. [`IsGroundBodyValid`](crate::physics::PfxCharacterInstance::IsGroundBodyValid) checks it names a body, and the
    /// ground raycasts (`CCharacter::DoGroundInfoRaycasts`) look up its owner to tell a vehicle
    /// underfoot.
    pub m_GroundBodyId: u32,
    /// The last valid [`m_GroundBodyId`](crate::physics::PfxCharacterInstance::m_GroundBodyId), kept across ticks without support.
    pub m_LastValidGroundBodyId: u32,
    _field_ac: [u8; 24],
    /// Whether the proxy is standing on stairs. The on-foot locomotion task takes its velocity
    /// along the blackboard's slope direction while it is set.
    pub m_IsSupportedOnStairs: bool,
    _field_c5: [u8; 3],
    /// The current proxy state, a [`ProxyState`](crate::physics::ProxyState) value, which selects the proxy's collision shape.
    /// Written by [`SetProxyState`](crate::physics::PfxCharacterInstance::SetProxyState).
    pub m_ProxyState: i32,
    _field_cc: [u8; 4],
    /// The Havok character rigid body, which holds the body id that
    /// [`GetCharacterRigidBodyId`](crate::physics::PfxCharacterInstance::GetCharacterRigidBodyId) reads.
    pub m_CharacterRigidBody: *mut crate::physics::hknpCharacterRigidBody,
    _field_d8: [u8; 16],
    /// The character rigid-body listener (`CCustomCharacterRBListener*`).
    pub m_CharacterRBListener: *mut ::std::ffi::c_void,
    /// The owning character, whose [`GetSharedPfxCharacterSettings`](crate::physics::PfxCharacterOwner::GetSharedPfxCharacterSettings)
    /// virtual returns the shared settings that hold the per-state shape table.
    pub m_Owner: *mut crate::physics::PfxCharacterOwner,
    _field_f8: [u8; 8],
}
fn _PfxCharacterInstance_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x100], PfxCharacterInstance>([0u8; 0x100]);
    }
    unreachable!()
}
impl PfxCharacterInstance {
    pub const SetOrientation_ADDRESS: usize = 0x140239100;
    /// Sets the physics body's rotation from the rotation part of `m` (via a quaternion
    /// conversion) and zeroes the body's angular velocity; the translation part is ignored. The
    /// swim movement cores ([`UpdateSurfaceMovement`](crate::input::swim::UpdateSurfaceMovement) and
    /// [`UpdateUnderwaterMovement`](crate::input::swim::UpdateUnderwaterMovement)) apply their computed
    /// body orientation through this each update.
    pub unsafe fn SetOrientation(&mut self, m: *const crate::types::math::Matrix4) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                m: *const crate::types::math::Matrix4,
            ) = ::std::mem::transmute(Self::SetOrientation_ADDRESS);
            f(self as *mut Self as _, m)
        }
    }
    pub const GetVelocity_ADDRESS: usize = 0x14024D820;
    /// Returns the physics body's current linear velocity (world space, 3 floats) via the body's
    /// Havok transform/velocity chain.
    pub unsafe fn GetVelocity(&self, out: *mut crate::types::math::Vector3) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                out: *mut crate::types::math::Vector3,
            ) = ::std::mem::transmute(Self::GetVelocity_ADDRESS);
            f(self as *const Self as _, out)
        }
    }
    pub const SetProxyState_ADDRESS: usize = 0x140239580;
    /// Switches the proxy to `state` when it differs from [`m_ProxyState`](crate::physics::PfxCharacterInstance::m_ProxyState):
    /// stores it, fetches the shared settings through [`m_Owner`](crate::physics::PfxCharacterInstance::m_Owner), swaps the live
    /// body's shape to the settings' shape for that state ([`hknpWorld::setBodyShape`](crate::physics::hknpWorld::setBodyShape)), and
    /// copies the state's per-shape byte into the rigid-body listener
    /// ([`m_CharacterRBListener`](crate::physics::PfxCharacterInstance::m_CharacterRBListener)). The capsule
    /// dimensions live in those pre-built shapes.
    pub unsafe fn SetProxyState(&mut self, state: i32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, state: i32) = ::std::mem::transmute(
                Self::SetProxyState_ADDRESS,
            );
            f(self as *mut Self as _, state)
        }
    }
    pub const GetCharacterRigidBodyId_ADDRESS: usize = 0x14024DA40;
    /// Writes the Havok body id of [`m_CharacterRigidBody`](crate::physics::PfxCharacterInstance::m_CharacterRigidBody) to `out`
    /// ([`INVALID_BODY_ID`](crate::physics::PfxCharacterInstance::INVALID_BODY_ID) when there is no body) and returns `out`.
    pub unsafe fn GetCharacterRigidBodyId(&self, out: *mut u32) -> *mut u32 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                out: *mut u32,
            ) -> *mut u32 = ::std::mem::transmute(Self::GetCharacterRigidBodyId_ADDRESS);
            f(self as *const Self as _, out)
        }
    }
    pub const BuildDefaultCharacterContext_ADDRESS: usize = 0x14024D370;
    /// Builds the Havok character context for the proxy: a character-state manager with the
    /// on-ground, in-air, and jumping states registered. It does not set the capsule dimensions.
    pub unsafe fn BuildDefaultCharacterContext(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::BuildDefaultCharacterContext_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const IsSupported_ADDRESS: usize = 0x14024D770;
    /// Whether the proxy is supported: the supported state of
    /// [`m_GroundInformation`](crate::physics::PfxCharacterInstance::m_GroundInformation) is
    /// [`SUPPORTED`](crate::physics::hknpCharacterSurfaceInfo::SUPPORTED). The state machines' ground-contact
    /// condition (`CSuperGroundContactCondition`) is this call, and `CCharacter::UpdateFalling`,
    /// the ground raycasts, and [`Character::IsInAir`](crate::character::character::Character::IsInAir)
    /// read it.
    pub unsafe fn IsSupported(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsSupported_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsGroundBodyValid_ADDRESS: usize = 0x14024D930;
    /// Whether [`m_GroundBodyId`](crate::physics::PfxCharacterInstance::m_GroundBodyId) names a body of the world: an id below the
    /// body array's capacity whose slot holds a static or dynamic body. The disassembler mislabels
    /// it `CPfxCharacterInstance::GetSurfaceNormal`.
    pub unsafe fn IsGroundBodyValid(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsGroundBodyValid_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const CheckSupportCustom_ADDRESS: usize = 0x1402B1150;
    /// Finds the proxy's support from the character rigid body's contacts and writes it to
    /// [`m_GroundInformation`](crate::physics::PfxCharacterInstance::m_GroundInformation), with the ground body; without support it
    /// sets [`m_GroundBodyId`](crate::physics::PfxCharacterInstance::m_GroundBodyId) to
    /// [`INVALID_BODY_ID`](crate::physics::PfxCharacterInstance::INVALID_BODY_ID) and clears the contact material. A valid ground
    /// body is also kept in [`m_LastValidGroundBodyId`](crate::physics::PfxCharacterInstance::m_LastValidGroundBodyId).
    pub unsafe fn CheckSupportCustom(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::CheckSupportCustom_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const UpdateContext_ADDRESS: usize = 0x1402BBDA0;
    /// The proxy's update before the physics step, from the character's integrating-physics
    /// update in its pre-physics pass: it finds the support
    /// ([`CheckSupportCustom`](crate::physics::PfxCharacterInstance::CheckSupportCustom)), runs the Havok character context on
    /// the support and [`m_Input`](crate::physics::PfxCharacterInstance::m_Input), and sets the character rigid body's velocity to
    /// the context's output ([`hknpCharacterRigidBody::setLinearVelocity`](crate::physics::hknpCharacterRigidBody::setLinearVelocity)). A proxy whose body is
    /// not active is left unsupported.
    pub unsafe fn UpdateContext(
        &mut self,
        dt: f32,
        input: *mut crate::physics::CharacterInput,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dt: f32,
                input: *mut crate::physics::CharacterInput,
            ) = ::std::mem::transmute(Self::UpdateContext_ADDRESS);
            f(self as *mut Self as _, dt, input)
        }
    }
}
impl PfxCharacterInstance {
    /// Havok's invalid body id, which [`GetCharacterRigidBodyId`](crate::physics::PfxCharacterInstance::GetCharacterRigidBodyId)
    /// reports when there is no body.
    pub const INVALID_BODY_ID: u32 = 2147483647;
}
impl std::convert::AsRef<PfxCharacterInstance> for PfxCharacterInstance {
    fn as_ref(&self) -> &PfxCharacterInstance {
        self
    }
}
impl std::convert::AsMut<PfxCharacterInstance> for PfxCharacterInstance {
    fn as_mut(&mut self) -> &mut PfxCharacterInstance {
        self
    }
}
#[repr(C, align(8))]
/// The character-side interface that exposes shared character settings (`IPfxCharacter`).
pub struct PfxCharacterOwner {
    vftable: *const crate::physics::PfxCharacterOwnerVftable,
}
fn _PfxCharacterOwner_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], PfxCharacterOwner>([0u8; 0x8]);
    }
    unreachable!()
}
impl PfxCharacterOwner {
    pub fn vftable(&self) -> *const crate::physics::PfxCharacterOwnerVftable {
        self.vftable as *const crate::physics::PfxCharacterOwnerVftable
    }
    /// Returns the character's shared proxy settings.
    pub unsafe fn GetSharedPfxCharacterSettings(
        &mut self,
    ) -> *mut crate::physics::SharedPfxCharacterSettings {
        unsafe {
            let f = (&raw const (*self.vftable()).GetSharedPfxCharacterSettings).read();
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<PfxCharacterOwner> for PfxCharacterOwner {
    fn as_ref(&self) -> &PfxCharacterOwner {
        self
    }
}
impl std::convert::AsMut<PfxCharacterOwner> for PfxCharacterOwner {
    fn as_mut(&mut self) -> &mut PfxCharacterOwner {
        self
    }
}
#[repr(C, align(8))]
pub struct PfxCharacterOwnerVftable {
    _vfunc_0: unsafe extern "system" fn(this: *mut crate::physics::PfxCharacterOwner),
    _vfunc_1: unsafe extern "system" fn(this: *mut crate::physics::PfxCharacterOwner),
    /// Returns the character's shared proxy settings.
    pub GetSharedPfxCharacterSettings: unsafe extern "system" fn(
        this: *mut crate::physics::PfxCharacterOwner,
    ) -> *mut crate::physics::SharedPfxCharacterSettings,
}
fn _PfxCharacterOwnerVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], PfxCharacterOwnerVftable>([0u8; 0x18]);
    }
    unreachable!()
}
impl PfxCharacterOwnerVftable {}
impl std::convert::AsRef<PfxCharacterOwnerVftable> for PfxCharacterOwnerVftable {
    fn as_ref(&self) -> &PfxCharacterOwnerVftable {
        self
    }
}
impl std::convert::AsMut<PfxCharacterOwnerVftable> for PfxCharacterOwnerVftable {
    fn as_mut(&mut self) -> &mut PfxCharacterOwnerVftable {
        self
    }
}
#[repr(C, align(8))]
/// An engine physics instance (`IPfxInstance`), the object behind a body's
/// [`BODY_PROPERTY_PFX_INSTANCE`](crate::physics::hknpWorld::BODY_PROPERTY_PFX_INSTANCE) property. The physics
/// system's collision listener (`CHavokCollisionListener`) subscribes to the world's
/// manifold-processed and contact-impulse events; for each event it looks up both bodies'
/// instances and calls the matching event virtual on each instance whose switch for that event is
/// set, with the other body's instance (possibly null) and whether the instance is the event's
/// first body. Only some members are bound.
pub struct PfxInstance {
    vftable: *const crate::physics::PfxInstanceVftable,
    _field_8: [u8; 32],
    /// The instance's owner (`IPfxGameObject*`): the [`PhysicsGameObject`](crate::physics::PhysicsGameObject) a physics query reports
    /// for a hit on the instance's bodies ([`CastRayInfo::m_GameObject`](crate::physics::CastRayInfo::m_GameObject)).
    pub m_GameObject: *mut ::std::ffi::c_void,
    /// Whether the collision listener calls
    /// [`OnManifoldProcessedEvent`](crate::physics::PfxInstance::OnManifoldProcessedEvent). Clear on a new instance.
    pub m_IsManifoldProcessedEventActive: bool,
    /// Whether the collision listener calls
    /// [`OnContactImpulseEvent`](crate::physics::PfxInstance::OnContactImpulseEvent). Clear on a new instance.
    pub m_IsContactImpulseEventActive: bool,
    _field_32: [u8; 6],
}
fn _PfxInstance_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x38], PfxInstance>([0u8; 0x38]);
    }
    unreachable!()
}
impl PfxInstance {
    pub fn vftable(&self) -> *const crate::physics::PfxInstanceVftable {
        self.vftable as *const crate::physics::PfxInstanceVftable
    }
    /// Called for a manifold-processed event between one of the instance's bodies and
    /// another while [`m_IsManifoldProcessedEventActive`](crate::physics::PfxInstance::m_IsManifoldProcessedEventActive)
    /// is set. `input` is the event handler input (`hknpEventHandlerInput`) and `event` the
    /// event (`hknpManifoldProcessedEvent`).
    pub unsafe fn OnManifoldProcessedEvent(
        &mut self,
        input: *const ::std::ffi::c_void,
        event: *const ::std::ffi::c_void,
        other: *mut crate::physics::PfxInstance,
        is_body_a: bool,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).OnManifoldProcessedEvent).read();
            f(self as *mut Self as _, input, event, other, is_body_a)
        }
    }
    /// Called for a contact-impulse event between one of the instance's bodies and another
    /// while [`m_IsContactImpulseEventActive`](crate::physics::PfxInstance::m_IsContactImpulseEventActive) is set.
    /// `input` is the event handler input (`hknpEventHandlerInput`). The listener calls it from
    /// the world's event dispatch, inside [`hknpWorld::stepSolve`](crate::physics::hknpWorld::stepSolve).
    pub unsafe fn OnContactImpulseEvent(
        &mut self,
        input: *const ::std::ffi::c_void,
        event: *const crate::havok::hknpContactImpulseEvent,
        other: *mut crate::physics::PfxInstance,
        is_body_a: bool,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).OnContactImpulseEvent).read();
            f(self as *mut Self as _, input, event, other, is_body_a)
        }
    }
}
impl std::convert::AsRef<PfxInstance> for PfxInstance {
    fn as_ref(&self) -> &PfxInstance {
        self
    }
}
impl std::convert::AsMut<PfxInstance> for PfxInstance {
    fn as_mut(&mut self) -> &mut PfxInstance {
        self
    }
}
#[repr(C, align(8))]
pub struct PfxInstanceVftable {
    _vfunc_0: unsafe extern "system" fn(this: *mut crate::physics::PfxInstance),
    _vfunc_1: unsafe extern "system" fn(this: *mut crate::physics::PfxInstance),
    _vfunc_2: unsafe extern "system" fn(this: *mut crate::physics::PfxInstance),
    _vfunc_3: unsafe extern "system" fn(this: *mut crate::physics::PfxInstance),
    _vfunc_4: unsafe extern "system" fn(this: *mut crate::physics::PfxInstance),
    _vfunc_5: unsafe extern "system" fn(this: *mut crate::physics::PfxInstance),
    _vfunc_6: unsafe extern "system" fn(this: *mut crate::physics::PfxInstance),
    _vfunc_7: unsafe extern "system" fn(this: *mut crate::physics::PfxInstance),
    _vfunc_8: unsafe extern "system" fn(this: *mut crate::physics::PfxInstance),
    _vfunc_9: unsafe extern "system" fn(this: *mut crate::physics::PfxInstance),
    _vfunc_10: unsafe extern "system" fn(this: *mut crate::physics::PfxInstance),
    _vfunc_11: unsafe extern "system" fn(this: *mut crate::physics::PfxInstance),
    _vfunc_12: unsafe extern "system" fn(this: *mut crate::physics::PfxInstance),
    _vfunc_13: unsafe extern "system" fn(this: *mut crate::physics::PfxInstance),
    _vfunc_14: unsafe extern "system" fn(this: *mut crate::physics::PfxInstance),
    _vfunc_15: unsafe extern "system" fn(this: *mut crate::physics::PfxInstance),
    _vfunc_16: unsafe extern "system" fn(this: *mut crate::physics::PfxInstance),
    _vfunc_17: unsafe extern "system" fn(this: *mut crate::physics::PfxInstance),
    _vfunc_18: unsafe extern "system" fn(this: *mut crate::physics::PfxInstance),
    _vfunc_19: unsafe extern "system" fn(this: *mut crate::physics::PfxInstance),
    /// Called for a manifold-processed event between one of the instance's bodies and
    /// another while [`m_IsManifoldProcessedEventActive`](crate::physics::PfxInstance::m_IsManifoldProcessedEventActive)
    /// is set. `input` is the event handler input (`hknpEventHandlerInput`) and `event` the
    /// event (`hknpManifoldProcessedEvent`).
    pub OnManifoldProcessedEvent: unsafe extern "system" fn(
        this: *mut crate::physics::PfxInstance,
        input: *const ::std::ffi::c_void,
        event: *const ::std::ffi::c_void,
        other: *mut crate::physics::PfxInstance,
        is_body_a: bool,
    ),
    /// Called for a contact-impulse event between one of the instance's bodies and another
    /// while [`m_IsContactImpulseEventActive`](crate::physics::PfxInstance::m_IsContactImpulseEventActive) is set.
    /// `input` is the event handler input (`hknpEventHandlerInput`). The listener calls it from
    /// the world's event dispatch, inside [`hknpWorld::stepSolve`](crate::physics::hknpWorld::stepSolve).
    pub OnContactImpulseEvent: unsafe extern "system" fn(
        this: *mut crate::physics::PfxInstance,
        input: *const ::std::ffi::c_void,
        event: *const crate::havok::hknpContactImpulseEvent,
        other: *mut crate::physics::PfxInstance,
        is_body_a: bool,
    ),
}
fn _PfxInstanceVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xB0], PfxInstanceVftable>([0u8; 0xB0]);
    }
    unreachable!()
}
impl PfxInstanceVftable {}
impl std::convert::AsRef<PfxInstanceVftable> for PfxInstanceVftable {
    fn as_ref(&self) -> &PfxInstanceVftable {
        self
    }
}
impl std::convert::AsMut<PfxInstanceVftable> for PfxInstanceVftable {
    fn as_mut(&mut self) -> &mut PfxInstanceVftable {
        self
    }
}
#[repr(u32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
/// The engine's collision layers (`EPfxLayer`), the low field of a body's collision-filter word
/// ([`CustomGroupCollisionFilter`](crate::physics::CustomGroupCollisionFilter)). The physics system's constructor fills the filter's layer table
/// with the pairs of layers that collide; only the layers in it collide at all. The query layers
/// (the raycast and shape-query layers) are what a query passes as its own layer.
pub enum PfxLayer {
    FULL_COLLISION = 0isize as _,
    NO_COLLISION = 1isize as _,
    STATIC = 2isize as _,
    TERRAIN = 3isize as _,
    DYNAMIC = 4isize as _,
    DESTRUCTION = 5isize as _,
    CHARACTER_COLLISION = 6isize as _,
    CHARACTER_GHOST_MODE = 7isize as _,
    CHARACTER_RAGDOLL = 8isize as _,
    CHARACTER_RAGDOLL_KEYFRAMED = 9isize as _,
    CHARACTER_ATTACHED_DYNAMIC_RB = 10isize as _,
    CHARACTER_RAGDOLL_PROXIES = 11isize as _,
    PARTICLE = 12isize as _,
    EXPLOSIVE = 13isize as _,
    PROJECTILE = 14isize as _,
    WEAPONS = 15isize as _,
    VEHICLE_COLLISION_WHEEL = 16isize as _,
    VEHICLE_COLLISION_DYNAMIC_ONLY = 17isize as _,
    VEHICLE_COLLISION = 18isize as _,
    VEHICLE_RAYCAST = 19isize as _,
    RAYCAST_DEFAULT = 20isize as _,
    RAYCAST_EVERYTHING_PHYSICAL = 21isize as _,
    RAYCAST_WHEELS = 22isize as _,
    SHAPEQUERY_DEFAULT = 23isize as _,
    SHAPEQUERY_TRIGGER = 24isize as _,
    SHAPEQUERY_EXPLOSION = 25isize as _,
    SHAPEQUERY_VEHICLES = 26isize as _,
    STATIC_AND_DESTRUCTION = 27isize as _,
    ALL_DYNAMIC_OBJECTS = 28isize as _,
    LISTENER_EFFECTS = 29isize as _,
}
fn _PfxLayer_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], PfxLayer>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// A pre-simulation callback (`CPfxPreSimulationCallback`), an object the physics system calls
/// before each step ([`PhysicsSystem::AddPreSimulationCallback`](crate::physics::PhysicsSystem::AddPreSimulationCallback)). The engine never deletes one;
/// its owner removes it ([`PhysicsSystem::RemovePreSimulationCallback`](crate::physics::PhysicsSystem::RemovePreSimulationCallback)) and frees it.
pub struct PfxPreSimulationCallback {
    vftable: *const crate::physics::PfxPreSimulationCallbackVftable,
}
fn _PfxPreSimulationCallback_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], PfxPreSimulationCallback>([0u8; 0x8]);
    }
    unreachable!()
}
impl PfxPreSimulationCallback {
    pub fn vftable(&self) -> *const crate::physics::PfxPreSimulationCallbackVftable {
        self.vftable as *const crate::physics::PfxPreSimulationCallbackVftable
    }
    /// The destructor.
    pub unsafe fn Destructor(&mut self, flags: u8) {
        unsafe {
            let f = (&raw const (*self.vftable()).Destructor).read();
            f(self as *mut Self as _, flags)
        }
    }
    /// Called by [`PhysicsSystem::Update`](crate::physics::PhysicsSystem::Update) before each step, on the thread that runs it, with
    /// the world and the time the update was given (not the step it takes, which is pinned to
    /// [`PhysicsSystem::STEP_SECONDS`](crate::physics::PhysicsSystem::STEP_SECONDS) after the callbacks), outside the step: the world may be changed
    /// here, and the step that follows integrates the velocities set here. The callbacks fire
    /// in the order they were added.
    pub unsafe fn preSimulationCallback(
        &mut self,
        world: *mut crate::physics::hknpWorld,
        dt: f32,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).preSimulationCallback).read();
            f(self as *mut Self as _, world, dt)
        }
    }
    /// Called when the world's origin moves (`CPhysicsSystem::RecenterWorld`), after every
    /// body has been moved, with the shift added to every Havok-space position. A callback
    /// that keeps Havok-space positions of its own moves them here.
    pub unsafe fn onWorldShiftCallback(
        &mut self,
        shift: *const crate::types::vector_math::AlignedVector3,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).onWorldShiftCallback).read();
            f(self as *mut Self as _, shift)
        }
    }
}
impl std::convert::AsRef<PfxPreSimulationCallback> for PfxPreSimulationCallback {
    fn as_ref(&self) -> &PfxPreSimulationCallback {
        self
    }
}
impl std::convert::AsMut<PfxPreSimulationCallback> for PfxPreSimulationCallback {
    fn as_mut(&mut self) -> &mut PfxPreSimulationCallback {
        self
    }
}
#[repr(C, align(8))]
pub struct PfxPreSimulationCallbackVftable {
    /// The destructor.
    pub Destructor: unsafe extern "system" fn(
        this: *mut crate::physics::PfxPreSimulationCallback,
        flags: u8,
    ),
    /// Called by [`PhysicsSystem::Update`](crate::physics::PhysicsSystem::Update) before each step, on the thread that runs it, with
    /// the world and the time the update was given (not the step it takes, which is pinned to
    /// [`PhysicsSystem::STEP_SECONDS`](crate::physics::PhysicsSystem::STEP_SECONDS) after the callbacks), outside the step: the world may be changed
    /// here, and the step that follows integrates the velocities set here. The callbacks fire
    /// in the order they were added.
    pub preSimulationCallback: unsafe extern "system" fn(
        this: *mut crate::physics::PfxPreSimulationCallback,
        world: *mut crate::physics::hknpWorld,
        dt: f32,
    ),
    /// Called when the world's origin moves (`CPhysicsSystem::RecenterWorld`), after every
    /// body has been moved, with the shift added to every Havok-space position. A callback
    /// that keeps Havok-space positions of its own moves them here.
    pub onWorldShiftCallback: unsafe extern "system" fn(
        this: *mut crate::physics::PfxPreSimulationCallback,
        shift: *const crate::types::vector_math::AlignedVector3,
    ),
}
fn _PfxPreSimulationCallbackVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<
            [u8; 0x18],
            PfxPreSimulationCallbackVftable,
        >([0u8; 0x18]);
    }
    unreachable!()
}
impl PfxPreSimulationCallbackVftable {}
impl std::convert::AsRef<PfxPreSimulationCallbackVftable>
for PfxPreSimulationCallbackVftable {
    fn as_ref(&self) -> &PfxPreSimulationCallbackVftable {
        self
    }
}
impl std::convert::AsMut<PfxPreSimulationCallbackVftable>
for PfxPreSimulationCallbackVftable {
    fn as_mut(&mut self) -> &mut PfxPreSimulationCallbackVftable {
        self
    }
}
#[repr(C, align(16))]
/// `CPfxRigidBody`: the engine's wrapper around a Havok rigid body in the physics system's world,
/// an engine physics instance ([`PfxInstance`](crate::physics::PfxInstance)) for one body. It is allocated with the engine's
/// [`OperatorNew`](crate::memory::OperatorNew) and destroyed through its deleting destructor
/// ([`Destruct`](crate::physics::PfxRigidBody::Destruct)), which removes the body from the world and destroys it. Only some
/// members are bound.
pub struct PfxRigidBody {
    vftable: *const crate::physics::PfxRigidBodyVftable,
    _field_8: [u8; 56],
    /// The body's id in the physics system's world.
    pub m_BodyId: u32,
    _field_44: [u8; 124],
}
fn _PfxRigidBody_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xC0], PfxRigidBody>([0u8; 0xC0]);
    }
    unreachable!()
}
impl PfxRigidBody {
    pub fn vftable(&self) -> *const crate::physics::PfxRigidBodyVftable {
        self.vftable as *const crate::physics::PfxRigidBodyVftable
    }
    pub const Construct_ADDRESS: usize = 0x1402940D0;
    /// `CPfxRigidBody::CPfxRigidBody`: wraps the existing body `*body_id` of the physics system's
    /// world. It sets the body's instance property
    /// ([`BODY_PROPERTY_PFX_INSTANCE`](crate::physics::hknpWorld::BODY_PROPERTY_PFX_INSTANCE)) to the wrapper, sets
    /// a body flag, and writes the body's collision-filter word to
    /// [`NO_COLLISION`](crate::physics::PfxLayer::NO_COLLISION). `data` is a `boost::shared_ptr<CDataCacheWrapper>`
    /// the wrapper keeps; the grappling hook's hang proxy passes an empty one (two null words) and
    /// `false` for `add_flag`, which the wrapper passes as the flags of its later body additions.
    /// The instance's owner ([`PfxInstance::m_GameObject`](crate::physics::PfxInstance::m_GameObject)) starts null.
    pub unsafe fn Construct(
        &mut self,
        body_id: *const u32,
        data: *const [u64; 2],
        add_flag: bool,
    ) -> *mut crate::physics::PfxRigidBody {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                body_id: *const u32,
                data: *const [u64; 2],
                add_flag: bool,
            ) -> *mut crate::physics::PfxRigidBody = ::std::mem::transmute(
                Self::Construct_ADDRESS,
            );
            f(self as *mut Self as _, body_id, data, add_flag)
        }
    }
    pub const SetCollisionLayer_ADDRESS: usize = 0x14024F4A0;
    /// Replaces the collision layer of the body's collision-filter word with `layer` (a
    /// [`PfxLayer`](crate::physics::PfxLayer)), keeping its other fields, and refreshes the body's collision state in the
    /// world. The disassembler mislabels it `boost::shared_ptr<CAnimationRuleSystem>::operator->`.
    pub unsafe fn SetCollisionLayer(&mut self, layer: u32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, layer: u32) = ::std::mem::transmute(
                Self::SetCollisionLayer_ADDRESS,
            );
            f(self as *mut Self as _, layer)
        }
    }
    pub const SetCollisionSystemGroup_ADDRESS: usize = 0x14024F410;
    /// Replaces the system group of the body's collision-filter word with `group`, keeping its
    /// other fields; see [`CustomGroupCollisionFilter`](crate::physics::CustomGroupCollisionFilter).
    pub unsafe fn SetCollisionSystemGroup(&mut self, group: u32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, group: u32) = ::std::mem::transmute(
                Self::SetCollisionSystemGroup_ADDRESS,
            );
            f(self as *mut Self as _, group)
        }
    }
    pub const SetGravityFactor_ADDRESS: usize = 0x14027FE60;
    /// Sets the factor of gravity on the body's motion: copies the motion's current motion
    /// properties into the wrapper, sets their gravity factor, and gives the motion the properties
    /// so changed. It acts only on a body that is neither static nor keyframed.
    pub unsafe fn SetGravityFactor(&mut self, factor: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, factor: f32) = ::std::mem::transmute(
                Self::SetGravityFactor_ADDRESS,
            );
            f(self as *mut Self as _, factor)
        }
    }
    pub const ApplyImpulse_ADDRESS: usize = 0x14024ED80;
    /// Applies the linear impulse `impulse` (world space) to the body at the world point
    /// `position`, through `hknpWorld::applyBodyImpulseAt`, and then notifies the body's own
    /// impulse virtual. The last argument only scales the debug display of the impulse.
    pub unsafe fn ApplyImpulse(
        &mut self,
        impulse: *const crate::types::math::Vector3,
        position: *const crate::types::math::Vector3,
        debug_scale: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                impulse: *const crate::types::math::Vector3,
                position: *const crate::types::math::Vector3,
                debug_scale: f32,
            ) = ::std::mem::transmute(Self::ApplyImpulse_ADDRESS);
            f(self as *mut Self as _, impulse, position, debug_scale)
        }
    }
    /// The deleting destructor: removes the body from the world when it is in it, clears the
    /// body's instance property, destroys the body, releases the wrapper's own references, and
    /// with [`DESTRUCT_FREE`](crate::physics::PfxRigidBody::DESTRUCT_FREE) in `flags` frees the wrapper's memory to the
    /// engine allocator. The disassembler mislabels it `CPfxRagdollProxyPhantom::Destruct`.
    pub unsafe fn Destruct(&mut self, flags: u8) -> *mut crate::physics::PfxRigidBody {
        unsafe {
            let f = (&raw const (*self.vftable()).Destruct).read();
            f(self as *mut Self as _, flags)
        }
    }
    /// Whether the body is in the simulation.
    pub unsafe fn IsAddedToWorld(&self) -> bool {
        unsafe {
            let f = (&raw const (*self.vftable()).IsAddedToWorld).read();
            f(self as *const Self as _)
        }
    }
    /// Adds the body, and any body attached to it, to the simulation at once
    /// (`hknpWorld::addBodies`) when it is not already in it. The disassembler mislabels it
    /// `CRagdollProxyPhantom::AddToWorld`.
    pub unsafe fn AddToWorld(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).AddToWorld).read();
            f(self as *mut Self as _)
        }
    }
    /// Removes the body, and any body attached to it, from the simulation
    /// (`hknpWorld::removeBodies`); the body still exists and can be added again. Bit 0 of
    /// `flags` selects the activation mode of the removal.
    pub unsafe fn RemoveFromWorld(&mut self, flags: u8) {
        unsafe {
            let f = (&raw const (*self.vftable()).RemoveFromWorld).read();
            f(self as *mut Self as _, flags)
        }
    }
    /// Writes the body's transform to `out`, game space: the body's rotation in the rows and
    /// its Havok-space position plus [`PhysicsSystem::m_AccumulatedShift`](crate::physics::PhysicsSystem::m_AccumulatedShift) in the translation.
    pub unsafe fn GetTransform(&self, out: *mut crate::types::math::Matrix4) {
        unsafe {
            let f = (&raw const (*self.vftable()).GetTransform).read();
            f(self as *const Self as _, out)
        }
    }
    /// Places the body at `transform`, game space, through `hknpWorld::setBodyTransform`. The
    /// velocities are kept. The disassembler mislabels it `CPartInstance::ClearWindowDamage`.
    pub unsafe fn SetTransform(
        &mut self,
        transform: *const crate::types::math::Matrix4,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).SetTransform).read();
            f(self as *mut Self as _, transform)
        }
    }
    /// [`PfxInstance::OnContactImpulseEvent`](crate::physics::PfxInstance::OnContactImpulseEvent); the wrapper's own does nothing.
    pub unsafe fn OnContactImpulseEvent(
        &mut self,
        input: *const ::std::ffi::c_void,
        event: *const crate::havok::hknpContactImpulseEvent,
        other: *mut crate::physics::PfxInstance,
        is_body_a: bool,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).OnContactImpulseEvent).read();
            f(self as *mut Self as _, input, event, other, is_body_a)
        }
    }
    /// Sets [`PfxInstance::m_IsContactImpulseEventActive`](crate::physics::PfxInstance::m_IsContactImpulseEventActive) to `active`, and sets or clears the
    /// body's [`RAISE_CONTACT_IMPULSE_EVENTS`](crate::physics::hknpBody::RAISE_CONTACT_IMPULSE_EVENTS) flag
    /// through the world's body flag setters (`hknpWorld::enableBodyFlags` and
    /// `disableBodyFlags`, which the disassembler mislabels `CPfxRigidBody::CPfxRigidBody` and
    /// a `hkSignal2::subscribe`), which rebuild the body's collision caches.
    pub unsafe fn SetContactImpulseEventActive(&mut self, active: bool) {
        unsafe {
            let f = (&raw const (*self.vftable()).SetContactImpulseEventActive).read();
            f(self as *mut Self as _, active)
        }
    }
}
impl PfxRigidBody {
    /// The [`Destruct`](crate::physics::PfxRigidBody::Destruct) flag that frees the wrapper's memory.
    pub const DESTRUCT_FREE: u8 = 1;
    /// The address of the `CPfxRigidBody` virtual table, which
    /// [`Construct`](crate::physics::PfxRigidBody::Construct) installs. It has as many slots as the vftable below.
    pub const VFTABLE: u64 = 5405740600;
}
impl std::convert::AsRef<PfxRigidBody> for PfxRigidBody {
    fn as_ref(&self) -> &PfxRigidBody {
        self
    }
}
impl std::convert::AsMut<PfxRigidBody> for PfxRigidBody {
    fn as_mut(&mut self) -> &mut PfxRigidBody {
        self
    }
}
#[repr(C, align(8))]
pub struct PfxRigidBodyVftable {
    /// The deleting destructor: removes the body from the world when it is in it, clears the
    /// body's instance property, destroys the body, releases the wrapper's own references, and
    /// with [`DESTRUCT_FREE`](crate::physics::PfxRigidBody::DESTRUCT_FREE) in `flags` frees the wrapper's memory to the
    /// engine allocator. The disassembler mislabels it `CPfxRagdollProxyPhantom::Destruct`.
    pub Destruct: unsafe extern "system" fn(
        this: *mut crate::physics::PfxRigidBody,
        flags: u8,
    ) -> *mut crate::physics::PfxRigidBody,
    _vfunc_1: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_2: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_3: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_4: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_5: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_6: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_7: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_8: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    /// Whether the body is in the simulation.
    pub IsAddedToWorld: unsafe extern "system" fn(
        this: *const crate::physics::PfxRigidBody,
    ) -> bool,
    /// Adds the body, and any body attached to it, to the simulation at once
    /// (`hknpWorld::addBodies`) when it is not already in it. The disassembler mislabels it
    /// `CRagdollProxyPhantom::AddToWorld`.
    pub AddToWorld: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_11: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    /// Removes the body, and any body attached to it, from the simulation
    /// (`hknpWorld::removeBodies`); the body still exists and can be added again. Bit 0 of
    /// `flags` selects the activation mode of the removal.
    pub RemoveFromWorld: unsafe extern "system" fn(
        this: *mut crate::physics::PfxRigidBody,
        flags: u8,
    ),
    _vfunc_13: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    /// Writes the body's transform to `out`, game space: the body's rotation in the rows and
    /// its Havok-space position plus [`PhysicsSystem::m_AccumulatedShift`](crate::physics::PhysicsSystem::m_AccumulatedShift) in the translation.
    pub GetTransform: unsafe extern "system" fn(
        this: *const crate::physics::PfxRigidBody,
        out: *mut crate::types::math::Matrix4,
    ),
    /// Places the body at `transform`, game space, through `hknpWorld::setBodyTransform`. The
    /// velocities are kept. The disassembler mislabels it `CPartInstance::ClearWindowDamage`.
    pub SetTransform: unsafe extern "system" fn(
        this: *mut crate::physics::PfxRigidBody,
        transform: *const crate::types::math::Matrix4,
    ),
    _vfunc_16: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_17: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_18: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_19: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_20: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    /// [`PfxInstance::OnContactImpulseEvent`](crate::physics::PfxInstance::OnContactImpulseEvent); the wrapper's own does nothing.
    pub OnContactImpulseEvent: unsafe extern "system" fn(
        this: *mut crate::physics::PfxRigidBody,
        input: *const ::std::ffi::c_void,
        event: *const crate::havok::hknpContactImpulseEvent,
        other: *mut crate::physics::PfxInstance,
        is_body_a: bool,
    ),
    _vfunc_22: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_23: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    /// Sets [`PfxInstance::m_IsContactImpulseEventActive`](crate::physics::PfxInstance::m_IsContactImpulseEventActive) to `active`, and sets or clears the
    /// body's [`RAISE_CONTACT_IMPULSE_EVENTS`](crate::physics::hknpBody::RAISE_CONTACT_IMPULSE_EVENTS) flag
    /// through the world's body flag setters (`hknpWorld::enableBodyFlags` and
    /// `disableBodyFlags`, which the disassembler mislabels `CPfxRigidBody::CPfxRigidBody` and
    /// a `hkSignal2::subscribe`), which rebuild the body's collision caches.
    pub SetContactImpulseEventActive: unsafe extern "system" fn(
        this: *mut crate::physics::PfxRigidBody,
        active: bool,
    ),
    _vfunc_25: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_26: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_27: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_28: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_29: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_30: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_31: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_32: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_33: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_34: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_35: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_36: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_37: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_38: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_39: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_40: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_41: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_42: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
    _vfunc_43: unsafe extern "system" fn(this: *mut crate::physics::PfxRigidBody),
}
fn _PfxRigidBodyVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x160], PfxRigidBodyVftable>([0u8; 0x160]);
    }
    unreachable!()
}
impl PfxRigidBodyVftable {}
impl std::convert::AsRef<PfxRigidBodyVftable> for PfxRigidBodyVftable {
    fn as_ref(&self) -> &PfxRigidBodyVftable {
        self
    }
}
impl std::convert::AsMut<PfxRigidBodyVftable> for PfxRigidBodyVftable {
    fn as_mut(&mut self) -> &mut PfxRigidBodyVftable {
        self
    }
}
#[repr(C, align(8))]
/// A physics game object (`CPhysicsGameObject`): a game object with a physics instance, the owner a
/// physics query reports for a hit body ([`CastRayInfo::m_GameObject`](crate::physics::CastRayInfo::m_GameObject)). Characters, vehicles,
/// and rigid props are physics game objects. Its `CGameObject` subobject sits at
/// [`GAME_OBJECT_BASE_OFFSET`](crate::physics::PhysicsGameObject::GAME_OBJECT_BASE_OFFSET), and its run-time type test goes
/// through that subobject ([`GameObject::IsType`](crate::game::GameObject::IsType)). Only one virtual is
/// bound.
pub struct PhysicsGameObject {
    vftable: *const crate::physics::PhysicsGameObjectVftable,
}
fn _PhysicsGameObject_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], PhysicsGameObject>([0u8; 0x8]);
    }
    unreachable!()
}
impl PhysicsGameObject {
    pub fn vftable(&self) -> *const crate::physics::PhysicsGameObjectVftable {
        self.vftable as *const crate::physics::PhysicsGameObjectVftable
    }
    /// The object's engine physics instance, or null. A rigid object
    /// ([`RigidObject`](crate::physics::RigidObject)) returns its one body's wrapper, a [`PfxRigidBody`](crate::physics::PfxRigidBody), whose
    /// [`PfxInstance::m_GameObject`](crate::physics::PfxInstance::m_GameObject) is the object.
    pub unsafe fn GetIPfxInstance(&mut self) -> *mut crate::physics::PfxInstance {
        unsafe {
            let f = (&raw const (*self.vftable()).GetIPfxInstance).read();
            f(self as *mut Self as _)
        }
    }
    /// How the grapple treats the object; see [`GrapplingMode`](crate::physics::GrapplingMode). The player aim reads it for
    /// the grapple slot's direct hit
    /// ([`PlayerAimControl::m_DirectGrapplePosIsGrapplable`](crate::aim::aim::PlayerAimControl::m_DirectGrapplePosIsGrapplable)),
    /// and a wire end's attach refuses an ungrapplable object
    /// ([`WireEnd::SetupProxyForAttachment`](crate::equipment::wire_end::WireEnd::SetupProxyForAttachment)).
    pub unsafe fn GetGrapplingMode(&self) -> crate::physics::GrapplingMode {
        unsafe {
            let f = (&raw const (*self.vftable()).GetGrapplingMode).read();
            f(self as *const Self as _)
        }
    }
    /// Takes the damage in `msg` and returns the damage dealt. Each kind of object overrides
    /// it: a character (`CCharacter::OnDamage`) takes the health, sends the hit effects, and
    /// runs its hit reaction from the message's impulse, position, and direction; a rigid
    /// object (`CRigidObject::OnDamage`) takes the health of its damage controller.
    pub unsafe fn OnDamage(&mut self, msg: *mut crate::damage::DamageMsg) -> i16 {
        unsafe {
            let f = (&raw const (*self.vftable()).OnDamage).read();
            f(self as *mut Self as _, msg)
        }
    }
}
impl PhysicsGameObject {
    /// The offset of the `CGameObject` subobject.
    pub const GAME_OBJECT_BASE_OFFSET: u64 = 8;
}
impl std::convert::AsRef<PhysicsGameObject> for PhysicsGameObject {
    fn as_ref(&self) -> &PhysicsGameObject {
        self
    }
}
impl std::convert::AsMut<PhysicsGameObject> for PhysicsGameObject {
    fn as_mut(&mut self) -> &mut PhysicsGameObject {
        self
    }
}
#[repr(C, align(8))]
pub struct PhysicsGameObjectVftable {
    _vfunc_0: unsafe extern "system" fn(this: *mut crate::physics::PhysicsGameObject),
    /// The object's engine physics instance, or null. A rigid object
    /// ([`RigidObject`](crate::physics::RigidObject)) returns its one body's wrapper, a [`PfxRigidBody`](crate::physics::PfxRigidBody), whose
    /// [`PfxInstance::m_GameObject`](crate::physics::PfxInstance::m_GameObject) is the object.
    pub GetIPfxInstance: unsafe extern "system" fn(
        this: *mut crate::physics::PhysicsGameObject,
    ) -> *mut crate::physics::PfxInstance,
    _vfunc_2: unsafe extern "system" fn(this: *mut crate::physics::PhysicsGameObject),
    _vfunc_3: unsafe extern "system" fn(this: *mut crate::physics::PhysicsGameObject),
    _vfunc_4: unsafe extern "system" fn(this: *mut crate::physics::PhysicsGameObject),
    _vfunc_5: unsafe extern "system" fn(this: *mut crate::physics::PhysicsGameObject),
    _vfunc_6: unsafe extern "system" fn(this: *mut crate::physics::PhysicsGameObject),
    _vfunc_7: unsafe extern "system" fn(this: *mut crate::physics::PhysicsGameObject),
    _vfunc_8: unsafe extern "system" fn(this: *mut crate::physics::PhysicsGameObject),
    _vfunc_9: unsafe extern "system" fn(this: *mut crate::physics::PhysicsGameObject),
    _vfunc_10: unsafe extern "system" fn(this: *mut crate::physics::PhysicsGameObject),
    _vfunc_11: unsafe extern "system" fn(this: *mut crate::physics::PhysicsGameObject),
    _vfunc_12: unsafe extern "system" fn(this: *mut crate::physics::PhysicsGameObject),
    /// How the grapple treats the object; see [`GrapplingMode`](crate::physics::GrapplingMode). The player aim reads it for
    /// the grapple slot's direct hit
    /// ([`PlayerAimControl::m_DirectGrapplePosIsGrapplable`](crate::aim::aim::PlayerAimControl::m_DirectGrapplePosIsGrapplable)),
    /// and a wire end's attach refuses an ungrapplable object
    /// ([`WireEnd::SetupProxyForAttachment`](crate::equipment::wire_end::WireEnd::SetupProxyForAttachment)).
    pub GetGrapplingMode: unsafe extern "system" fn(
        this: *const crate::physics::PhysicsGameObject,
    ) -> crate::physics::GrapplingMode,
    _vfunc_14: unsafe extern "system" fn(this: *mut crate::physics::PhysicsGameObject),
    _vfunc_15: unsafe extern "system" fn(this: *mut crate::physics::PhysicsGameObject),
    _vfunc_16: unsafe extern "system" fn(this: *mut crate::physics::PhysicsGameObject),
    _vfunc_17: unsafe extern "system" fn(this: *mut crate::physics::PhysicsGameObject),
    _vfunc_18: unsafe extern "system" fn(this: *mut crate::physics::PhysicsGameObject),
    _vfunc_19: unsafe extern "system" fn(this: *mut crate::physics::PhysicsGameObject),
    /// Takes the damage in `msg` and returns the damage dealt. Each kind of object overrides
    /// it: a character (`CCharacter::OnDamage`) takes the health, sends the hit effects, and
    /// runs its hit reaction from the message's impulse, position, and direction; a rigid
    /// object (`CRigidObject::OnDamage`) takes the health of its damage controller.
    pub OnDamage: unsafe extern "system" fn(
        this: *mut crate::physics::PhysicsGameObject,
        msg: *mut crate::damage::DamageMsg,
    ) -> i16,
}
fn _PhysicsGameObjectVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xA8], PhysicsGameObjectVftable>([0u8; 0xA8]);
    }
    unreachable!()
}
impl PhysicsGameObjectVftable {}
impl std::convert::AsRef<PhysicsGameObjectVftable> for PhysicsGameObjectVftable {
    fn as_ref(&self) -> &PhysicsGameObjectVftable {
        self
    }
}
impl std::convert::AsMut<PhysicsGameObjectVftable> for PhysicsGameObjectVftable {
    fn as_mut(&mut self) -> &mut PhysicsGameObjectVftable {
        self
    }
}
#[repr(C, align(8))]
/// The physics system (`CPhysicsSystem`), a `Base::CSingle` singleton that owns the Havok world and
/// serves the engine's scene queries. The queries are sim-phase calls against the live Havok world.
/// Only some members are bound.
pub struct PhysicsSystem {
    _field_0: [u8; 48],
    /// The Havok world.
    pub m_World: *mut crate::physics::hknpWorld,
    _field_38: [u8; 1278348],
    /// The world-origin shift: the Havok world is simulated about a moving origin, and a Havok-space
    /// position plus this offset is the game-space position. [`CastRay`](crate::physics::PhysicsSystem::CastRay) subtracts it
    /// from its game-space ray before the Havok query, and the engine adds it back to the positions
    /// a query reports. [`hknpWorld::applyBodyImpulseAt`](crate::physics::hknpWorld::applyBodyImpulseAt) takes a Havok-space position.
    pub m_AccumulatedShift: crate::types::math::Vector3,
}
fn _PhysicsSystem_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1381D0], PhysicsSystem>([0u8; 0x1381D0]);
    }
    unreachable!()
}
impl PhysicsSystem {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5417845024usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl PhysicsSystem {
    pub const Update_ADDRESS: usize = 0x1402D1B40;
    /// Advances the physics by one step. It fires the pre-update signal, clears the collision
    /// array, and updates the bodies that do not simulate; then, unless one of its two suspend
    /// flags is set (`CGameStateRun::Update` sets one from its update context's pause), and when
    /// the world exists and `dt` is above a small threshold, it fires the pre-simulation callbacks
    /// ([`FirePreSimulationCallbacks`](crate::physics::PhysicsSystem::FirePreSimulationCallbacks)) with the world and `dt`
    /// as given, then replaces `dt` with [`STEP_SECONDS`](crate::physics::PhysicsSystem::STEP_SECONDS) when they differ
    /// (the pin at [`UPDATE_STEP_PIN_OFFSET`](crate::physics::PhysicsSystem::UPDATE_STEP_PIN_OFFSET)), and steps the world
    /// once by it on the physics thread pool, joining the pool before it returns. The game states'
    /// updates (`CGameStateRun::Update` and the loading and front-end states' updates) call it
    /// inline on the thread that runs them, through the physics update
    /// ([`GameState::UpdatePhysics`](crate::game::GameState::UpdatePhysics) in the run state), which
    /// dispatches the queued collisions and the damage listeners' events after it.
    pub unsafe fn Update(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const FirePreSimulationCallbacks_ADDRESS: usize = 0x14029C9C0;
    /// Calls every pre-simulation callback's
    /// [`preSimulationCallback`](crate::physics::PfxPreSimulationCallback::preSimulationCallback) with `world` and
    /// `dt`, in the order they were added, compacting away the entries
    /// [`RemovePreSimulationCallback`](crate::physics::PhysicsSystem::RemovePreSimulationCallback) cleared as it walks.
    /// [`Update`](crate::physics::PhysicsSystem::Update) calls it before the step with the time it was given, before that
    /// time is pinned to [`STEP_SECONDS`](crate::physics::PhysicsSystem::STEP_SECONDS).
    pub unsafe fn FirePreSimulationCallbacks(
        &mut self,
        world: *mut crate::physics::hknpWorld,
        dt: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                world: *mut crate::physics::hknpWorld,
                dt: f32,
            ) = ::std::mem::transmute(Self::FirePreSimulationCallbacks_ADDRESS);
            f(self as *mut Self as _, world, dt)
        }
    }
    pub const AddPreSimulationCallback_ADDRESS: usize = 0x1402BEA00;
    /// Appends `callback` to the pre-simulation callbacks [`Update`](crate::physics::PhysicsSystem::Update) fires before
    /// each step, a plain `std::vector` push. The ragdolls, the grappling hook's wire, and the AI
    /// world register themselves this way. A callback added while the callbacks fire would
    /// invalidate the walk over them.
    pub unsafe fn AddPreSimulationCallback(
        &mut self,
        callback: *mut crate::physics::PfxPreSimulationCallback,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                callback: *mut crate::physics::PfxPreSimulationCallback,
            ) = ::std::mem::transmute(Self::AddPreSimulationCallback_ADDRESS);
            f(self as *mut Self as _, callback)
        }
    }
    pub const RemovePreSimulationCallback_ADDRESS: usize = 0x140286FC0;
    /// Clears every entry of the pre-simulation callbacks that is `callback`. The entry is not
    /// erased at once: the next walk over the callbacks (before a step, or on a world shift)
    /// compacts the cleared entries away, so a callback may remove itself, or another, while they
    /// fire.
    pub unsafe fn RemovePreSimulationCallback(
        &mut self,
        callback: *mut crate::physics::PfxPreSimulationCallback,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                callback: *mut crate::physics::PfxPreSimulationCallback,
            ) = ::std::mem::transmute(Self::RemovePreSimulationCallback_ADDRESS);
            f(self as *mut Self as _, callback)
        }
    }
    pub const CastRay_ADDRESS: usize = 0x140286740;
    /// The general ray query: casts `ray` (world space, normalized direction) from `min_fraction`
    /// out to `max_distance` along its direction against collision `layer`, returning whether it
    /// hit. `info`, when non-null, receives the closest hit; `filter`, when non-null, replaces the
    /// default hit filter (and can exclude materials or an instance); `flags` is a
    /// collision-filter-info word, `0` for a plain query. Backed by [`hknpWorld::castRay`](crate::physics::hknpWorld::castRay).
    pub unsafe fn CastRay(
        &mut self,
        ray: *const crate::physics::Ray3,
        min_fraction: f32,
        max_distance: f32,
        info: *mut crate::physics::CastRayInfoBase,
        filter: *mut crate::physics::RaycastFilter,
        flags: u32,
        layer: i32,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ray: *const crate::physics::Ray3,
                min_fraction: f32,
                max_distance: f32,
                info: *mut crate::physics::CastRayInfoBase,
                filter: *mut crate::physics::RaycastFilter,
                flags: u32,
                layer: i32,
            ) -> bool = ::std::mem::transmute(Self::CastRay_ADDRESS);
            f(
                self as *mut Self as _,
                ray,
                min_fraction,
                max_distance,
                info,
                filter,
                flags,
                layer,
            )
        }
    }
    pub const CastRaySimple_ADDRESS: usize = 0x140286BE0;
    /// Whether the segment from `start` to `end` is blocked: builds the ray from the two points and
    /// calls [`CastRay`](crate::physics::PhysicsSystem::CastRay) with no hit info, the default filter, and
    /// [`QUERY_LAYER`](crate::physics::PhysicsSystem::QUERY_LAYER).
    pub unsafe fn CastRaySimple(
        &mut self,
        start: *const crate::types::math::Vector3,
        end: *const crate::types::math::Vector3,
        flags: u32,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                start: *const crate::types::math::Vector3,
                end: *const crate::types::math::Vector3,
                flags: u32,
            ) -> bool = ::std::mem::transmute(Self::CastRaySimple_ADDRESS);
            f(self as *mut Self as _, start, end, flags)
        }
    }
    pub const CastRayTerrain_ADDRESS: usize = 0x140286CE0;
    /// A ray query against the terrain only, with the parameters of [`CastRay`](crate::physics::PhysicsSystem::CastRay).
    pub unsafe fn CastRayTerrain(
        &mut self,
        ray: *const crate::physics::Ray3,
        min_fraction: f32,
        max_distance: f32,
        info: *mut crate::physics::CastRayInfoBase,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ray: *const crate::physics::Ray3,
                min_fraction: f32,
                max_distance: f32,
                info: *mut crate::physics::CastRayInfoBase,
            ) -> bool = ::std::mem::transmute(Self::CastRayTerrain_ADDRESS);
            f(self as *mut Self as _, ray, min_fraction, max_distance, info)
        }
    }
    pub const CastRayWaterSurface_ADDRESS: usize = 0x14023C150;
    /// Intersects the segment from `start` to `end` with the water surface, writing the hit point
    /// to `hit` and returning whether there was one.
    pub unsafe fn CastRayWaterSurface(
        &mut self,
        start: *const crate::types::math::Vector3,
        end: *const crate::types::math::Vector3,
        hit: *mut crate::types::math::Vector3,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                start: *const crate::types::math::Vector3,
                end: *const crate::types::math::Vector3,
                hit: *mut crate::types::math::Vector3,
            ) -> bool = ::std::mem::transmute(Self::CastRayWaterSurface_ADDRESS);
            f(self as *mut Self as _, start, end, hit)
        }
    }
    pub const CollideBox_ADDRESS: usize = 0x14029A120;
    /// Collects every body overlapping a box of half extents `half_extents` placed by `transform`
    /// (game space), on collision `layer`, ignoring the bodies of collision system group
    /// `system_group`, into `collector` (an `hknpCollisionQueryCollector`); returns whether any
    /// overlapped. Each collected `hknpCollisionResult` carries the Havok-space position and the
    /// body id of one overlapping body. The player's melee hit message
    /// ([`Character::HandleTrackMessage`](crate::character::character::Character::HandleTrackMessage))
    /// runs it with an `hknpAllHitsCollector` to find the objects in front of the character when no
    /// character is the target, then damages each hit object and pushes its body.
    pub unsafe fn CollideBox(
        &mut self,
        half_extents: *const crate::types::math::Vector3,
        transform: *const crate::types::math::Matrix4,
        system_group: u32,
        layer: u32,
        collector: *mut ::std::ffi::c_void,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                half_extents: *const crate::types::math::Vector3,
                transform: *const crate::types::math::Matrix4,
                system_group: u32,
                layer: u32,
                collector: *mut ::std::ffi::c_void,
            ) -> bool = ::std::mem::transmute(Self::CollideBox_ADDRESS);
            f(
                self as *mut Self as _,
                half_extents,
                transform,
                system_group,
                layer,
                collector,
            )
        }
    }
    pub const CastSphere_ADDRESS: usize = 0x140202C90;
    /// Sweeps a sphere of `radius` from `start` to `end`, reporting hits to `collector` (an
    /// `hknpCollisionQueryCollector`).
    pub unsafe fn CastSphere(
        &mut self,
        start: *const crate::types::math::Vector3,
        end: *const crate::types::math::Vector3,
        radius: f32,
        p4: f32,
        collector: *mut ::std::ffi::c_void,
        filter_info: u32,
        p7: f32,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                start: *const crate::types::math::Vector3,
                end: *const crate::types::math::Vector3,
                radius: f32,
                p4: f32,
                collector: *mut ::std::ffi::c_void,
                filter_info: u32,
                p7: f32,
            ) -> bool = ::std::mem::transmute(Self::CastSphere_ADDRESS);
            f(self as *mut Self as _, start, end, radius, p4, collector, filter_info, p7)
        }
    }
    pub const CastShape_ADDRESS: usize = 0x1401F0890;
    /// Sweeps an arbitrary Havok shape (`hknpShape*`) with the rotation `rotation` (`hkRotationf`)
    /// from `start` to `end`, reporting hits to `collector`.
    pub unsafe fn CastShape(
        &mut self,
        start: *const crate::types::math::Vector3,
        end: *const crate::types::math::Vector3,
        p3: f32,
        shape: *const ::std::ffi::c_void,
        rotation: *const ::std::ffi::c_void,
        collector: *mut ::std::ffi::c_void,
        filter_info: u32,
        p8: f32,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                start: *const crate::types::math::Vector3,
                end: *const crate::types::math::Vector3,
                p3: f32,
                shape: *const ::std::ffi::c_void,
                rotation: *const ::std::ffi::c_void,
                collector: *mut ::std::ffi::c_void,
                filter_info: u32,
                p8: f32,
            ) -> bool = ::std::mem::transmute(Self::CastShape_ADDRESS);
            f(
                self as *mut Self as _,
                start,
                end,
                p3,
                shape,
                rotation,
                collector,
                filter_info,
                p8,
            )
        }
    }
}
impl PhysicsSystem {
    /// The collision layer [`CastRaySimple`](crate::physics::PhysicsSystem::CastRaySimple) hardcodes, and the one the
    /// player's direct-aim query passes to [`CastRay`](crate::physics::PhysicsSystem::CastRay).
    pub const QUERY_LAYER: i32 = 21;
    /// The simulation step, seconds. [`Update`](crate::physics::PhysicsSystem::Update) steps the world once per call by
    /// exactly this, whatever time it is given. The value is stored once in the image and shared:
    /// [`Update`](crate::physics::PhysicsSystem::Update) loads it to pin its step, and the vehicles' parallel post-solve
    /// update reads it too.
    pub const STEP_SECONDS: f32 = 0.033333335;
    /// The length in bytes of the `movaps` at [`UPDATE_STEP_PIN_OFFSET`](crate::physics::PhysicsSystem::UPDATE_STEP_PIN_OFFSET).
    pub const UPDATE_STEP_PIN_LENGTH: u64 = 3;
    /// The intra-[`Update`](crate::physics::PhysicsSystem::Update) site of the step pin: after comparing the time it was
    /// given with [`STEP_SECONDS`](crate::physics::PhysicsSystem::STEP_SECONDS) and skipping ahead when they are equal, a
    /// `movaps` copies the constant over the time, so every step uses the constant. Without the
    /// copy the world steps by the time it was given, and the step global the pre-collision
    /// handler reads carries that time too.
    pub const UPDATE_STEP_PIN_OFFSET: u64 = 195;
}
impl std::convert::AsRef<PhysicsSystem> for PhysicsSystem {
    fn as_ref(&self) -> &PhysicsSystem {
        self
    }
}
impl std::convert::AsMut<PhysicsSystem> for PhysicsSystem {
    fn as_mut(&mut self) -> &mut PhysicsSystem {
        self
    }
}
/// `CPhysicsSystem::s_WaterQueryProxy`: the water query singleton buoyancy, water interaction,
/// vehicle fin forces, and the water-surface ray query read water heights from.
pub unsafe fn get_PhysicsSystem_s_WaterQueryProxy() -> &'static mut *mut crate::physics::WaterProxy {
    unsafe { &mut *(0x142EDC230 as *mut *mut crate::physics::WaterProxy) }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
/// `CPfxCharacterInstance::EProxyState`: the character proxy's state, each with its own pre-built
/// collision shape. Every character has the shape of every state. The crouching shape is a
/// shortened capsule with the non-player standing radius; the player stands in
/// [`PLAYER_STANDING`](crate::physics::ProxyState::PLAYER_STANDING), whose capsule has the player's own radius.
pub enum ProxyState {
    STANDING = 0isize as _,
    CROUCHING = 1isize as _,
    SWIMMING = 2isize as _,
    ROTATED_AND_OFFSETTED = 3isize as _,
    ROTATING_CROUCHING = 4isize as _,
    QUADRUPED_LARGE = 5isize as _,
    QUADRUPED_SMALL = 6isize as _,
    QUADRUPED_TINY = 7isize as _,
    PLAYER_STANDING = 8isize as _,
    WINGSUITING = 9isize as _,
}
fn _ProxyState_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], ProxyState>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// The ragdoll's proxy phantom (`CRagdollProxyPhantom`), the shape set that stands in for a
/// character's ragdoll bodies in hit tests. Only one method is bound.
pub struct RagdollProxyPhantom {}
impl RagdollProxyPhantom {
    pub const GetHitNameHash_ADDRESS: usize = 0x1403E48E0;
    /// The name hash of the ragdoll part a ray hit (the hit's shape key matched against the
    /// phantom's shapes), or `0` when the hit is not one of them. `CCharacter::OnDamage` uses it to
    /// find the body part a hit struck.
    pub unsafe fn GetHitNameHash(
        &mut self,
        info: *const crate::physics::CastRayInfoBase,
    ) -> u32 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                info: *const crate::physics::CastRayInfoBase,
            ) -> u32 = ::std::mem::transmute(Self::GetHitNameHash_ADDRESS);
            f(self as *mut Self as _, info)
        }
    }
}
impl std::convert::AsRef<RagdollProxyPhantom> for RagdollProxyPhantom {
    fn as_ref(&self) -> &RagdollProxyPhantom {
        self
    }
}
impl std::convert::AsMut<RagdollProxyPhantom> for RagdollProxyPhantom {
    fn as_mut(&mut self) -> &mut RagdollProxyPhantom {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// A ray (`CRay3`): an origin and a direction, both world space when passed to
/// [`PhysicsSystem::CastRay`](crate::physics::PhysicsSystem::CastRay).
pub struct Ray3 {
    pub Origin: crate::types::math::Vector3,
    /// The ray direction; the queries expect it normalized.
    pub Direction: crate::types::math::Vector3,
}
fn _Ray3_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], Ray3>([0u8; 0x18]);
    }
    unreachable!()
}
impl Ray3 {}
impl std::convert::AsRef<Ray3> for Ray3 {
    fn as_ref(&self) -> &Ray3 {
        self
    }
}
impl std::convert::AsMut<Ray3> for Ray3 {
    fn as_mut(&mut self) -> &mut Ray3 {
        self
    }
}
#[repr(C, align(16))]
/// The engine's closest-hit ray filter (`CRaycastFilter`, derived from Havok's
/// `hknpClosestHitCollector`): collects the closest hit while skipping ignored materials and one
/// ignored instance.
pub struct RaycastFilter {
    _field_0: [u8; 144],
    /// Materials whose pass flags are in this mask do not count as hits.
    pub m_IgnoreMaterialMask: crate::physics::MaterialPassFlags,
    /// Materials whose pass flags are in this mask always count as hits.
    pub m_ForceMaterialMask: crate::physics::MaterialPassFlags,
    /// An instance the query never hits (`IPfxInstance*`), such as the caster's own proxy.
    pub m_IgnorePfxInstance: *mut ::std::ffi::c_void,
    _field_a0: [u8; 16],
    /// The Havok world the filter resolves bodies in; the physics system's world when constructed
    /// with null.
    pub m_World: *const crate::physics::hknpWorld,
    _field_b8: [u8; 8],
}
fn _RaycastFilter_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xC0], RaycastFilter>([0u8; 0xC0]);
    }
    unreachable!()
}
impl RaycastFilter {
    pub const Construct_ADDRESS: usize = 0x1401FCBA0;
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
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ignore_material_mask: crate::physics::MaterialPassFlags,
                force_material_mask: crate::physics::MaterialPassFlags,
                ignore_instance: *mut ::std::ffi::c_void,
                world: *const crate::physics::hknpWorld,
            ) -> *mut crate::physics::RaycastFilter = ::std::mem::transmute(
                Self::Construct_ADDRESS,
            );
            f(
                self as *mut Self as _,
                ignore_material_mask,
                force_material_mask,
                ignore_instance,
                world,
            )
        }
    }
}
impl std::convert::AsRef<RaycastFilter> for RaycastFilter {
    fn as_ref(&self) -> &RaycastFilter {
        self
    }
}
impl std::convert::AsMut<RaycastFilter> for RaycastFilter {
    fn as_mut(&mut self) -> &mut RaycastFilter {
        self
    }
}
#[repr(C, align(8))]
/// A rigid object (`CRigidObject`): a [`PhysicsGameObject`](crate::physics::PhysicsGameObject) with one rigid body, such as a crate,
/// a barrel, a gas canister, or a piece of debris. Its body may be static, fixed, keyframed, or
/// dynamic. Characters, vehicles, and weapons are not rigid objects. Only some members are
/// mapped.
pub struct RigidObject {
    _field_0: [u8; 1048],
    /// The object's model instance, or null while its graphics are not loaded. Created from
    /// [`m_Model`](crate::physics::RigidObject::m_Model) (`CRigidObject::CreateModelInstance`); `UnloadGraphics` releases
    /// it with a deferred
    /// [`ModelInstanceManager::AddToDeleteQueue`](crate::graphics_engine::model_instance::ModelInstanceManager::AddToDeleteQueue).
    pub m_ModelInstance: *mut crate::graphics_engine::model_instance::ModelInstance,
    /// The object's model, set by `CRigidObject::CacheModel` from a name
    /// ([`ModelManager::GetModel`](crate::graphics_engine::model_instance::ModelManager::GetModel)) or
    /// a compiled-name hash (the cache) and released by `UnloadGraphics`. The object holds it
    /// whether or not it is in the world or has a model instance.
    pub m_Model: crate::types::shared_ptr::SharedPtr<
        crate::graphics_engine::model_instance::Model,
    >,
}
fn _RigidObject_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x430], RigidObject>([0u8; 0x430]);
    }
    unreachable!()
}
impl RigidObject {
    pub const TYPE_ID_ADDRESS: usize = 0x14060F1F0;
    /// `CRigidObject::TYPE_ID`: the run-time type id of `CRigidObject`, for
    /// [`GameObject::IsType`](crate::game::GameObject::IsType). Registers it on first use.
    pub unsafe fn TYPE_ID() -> *const u32 {
        unsafe {
            let f: unsafe extern "system" fn() -> *const u32 = ::std::mem::transmute(
                Self::TYPE_ID_ADDRESS,
            );
            f()
        }
    }
}
impl std::convert::AsRef<RigidObject> for RigidObject {
    fn as_ref(&self) -> &RigidObject {
        self
    }
}
impl std::convert::AsMut<RigidObject> for RigidObject {
    fn as_mut(&mut self) -> &mut RigidObject {
        self
    }
}
#[repr(C, align(8))]
/// The character proxy's shared shape settings (`CPfxCharacterSettings`): a shape for each
/// [`ProxyState`](crate::physics::ProxyState), shared by the character proxies that use these settings. Only the shape table and
/// its corresponding flags are mapped.
pub struct SharedPfxCharacterSettings {
    /// The collision shape selected by each [`ProxyState`](crate::physics::ProxyState).
    pub m_CapsuleShape: [*mut crate::havok::hknpShape; 10],
    /// Per-state fake-contact flags corresponding to [`m_CapsuleShape`](crate::physics::SharedPfxCharacterSettings::m_CapsuleShape).
    pub m_FakeContact: [bool; 10],
    _field_5a: [u8; 6],
}
fn _SharedPfxCharacterSettings_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x60], SharedPfxCharacterSettings>([0u8; 0x60]);
    }
    unreachable!()
}
impl SharedPfxCharacterSettings {
    pub const Construct_ADDRESS: usize = 0x1402B0420;
    /// `CPfxCharacterSettings::CPfxCharacterSettings`: initializes the shared settings.
    pub unsafe fn Construct(
        &mut self,
    ) -> *mut crate::physics::SharedPfxCharacterSettings {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
            ) -> *mut crate::physics::SharedPfxCharacterSettings = ::std::mem::transmute(
                Self::Construct_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const Destruct_ADDRESS: usize = 0x14024D300;
    /// `CPfxCharacterSettings::~CPfxCharacterSettings`: releases the settings' owned resources.
    pub unsafe fn Destruct(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Destruct_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<SharedPfxCharacterSettings> for SharedPfxCharacterSettings {
    fn as_ref(&self) -> &SharedPfxCharacterSettings {
        self
    }
}
impl std::convert::AsMut<SharedPfxCharacterSettings> for SharedPfxCharacterSettings {
    fn as_mut(&mut self) -> &mut SharedPfxCharacterSettings {
        self
    }
}
#[repr(C, align(8))]
/// The water query interface (`CPhysicsSystem::IWaterProxy`) through which physics code reads
/// water heights, reached through [`PhysicsSystem_s_WaterQueryProxy`](get_PhysicsSystem_s_WaterQueryProxy). Only the first slot is bound.
pub struct WaterProxy {
    vftable: *const crate::physics::WaterProxyVftable,
}
fn _WaterProxy_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], WaterProxy>([0u8; 0x8]);
    }
    unreachable!()
}
impl WaterProxy {
    pub fn vftable(&self) -> *const crate::physics::WaterProxyVftable {
        self.vftable as *const crate::physics::WaterProxyVftable
    }
    /// The world sea level, the height of the open-sea water plane.
    /// [`PhysicsSystem::CastRayWaterSurface`](crate::physics::PhysicsSystem::CastRayWaterSurface) tests a segment against it after the water boxes.
    pub unsafe fn GetWorldSeaLevel(&mut self) -> f32 {
        unsafe {
            let f = (&raw const (*self.vftable()).GetWorldSeaLevel).read();
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<WaterProxy> for WaterProxy {
    fn as_ref(&self) -> &WaterProxy {
        self
    }
}
impl std::convert::AsMut<WaterProxy> for WaterProxy {
    fn as_mut(&mut self) -> &mut WaterProxy {
        self
    }
}
#[repr(C, align(8))]
pub struct WaterProxyVftable {
    /// The world sea level, the height of the open-sea water plane.
    /// [`PhysicsSystem::CastRayWaterSurface`](crate::physics::PhysicsSystem::CastRayWaterSurface) tests a segment against it after the water boxes.
    pub GetWorldSeaLevel: unsafe extern "system" fn(
        this: *mut crate::physics::WaterProxy,
    ) -> f32,
}
fn _WaterProxyVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], WaterProxyVftable>([0u8; 0x8]);
    }
    unreachable!()
}
impl WaterProxyVftable {}
impl std::convert::AsRef<WaterProxyVftable> for WaterProxyVftable {
    fn as_ref(&self) -> &WaterProxyVftable {
        self
    }
}
impl std::convert::AsMut<WaterProxyVftable> for WaterProxyVftable {
    fn as_mut(&mut self) -> &mut WaterProxyVftable {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
/// `hknpWorld::AdditionMode`: whether [`hknpWorld::createDynamicBody`](crate::physics::hknpWorld::createDynamicBody) adds the new body to the
/// simulation. Only the value that leaves it out is bound.
pub enum hknpAdditionMode {
    DO_NOT_ADD = 0isize as _,
}
fn _hknpAdditionMode_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], hknpAdditionMode>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(16))]
/// A Havok body (`hknpBody`): its transform, collision data, and the motion that moves it. Only
/// some members are bound.
pub struct hknpBody {
    /// The body's transform, Havok space: the frame a constraint's pivots are given in
    /// ([`hkpFixedConstraintData::setInBodySpace`](crate::havok::hkpFixedConstraintData::setInBodySpace)).
    pub m_transform: crate::havok::hkTransformf,
    /// The body's flags; see [`IS_DYNAMIC`](crate::physics::hknpBody::IS_DYNAMIC).
    pub m_flags: u32,
    /// The body's collision-filter word, read by the world's collision filter; see
    /// [`CustomGroupCollisionFilter`](crate::physics::CustomGroupCollisionFilter) for its fields.
    pub m_collisionFilterInfo: u32,
    /// The body's collision shape, in body space. The body holds a reference of its own to it,
    /// taken from the construction info's shape
    /// ([`hknpBodyCinfo::m_shape`](crate::havok::hknpBodyCinfo::m_shape)) when the body is created
    /// and released when it is destroyed, so several bodies may share one shape.
    pub m_shape: *mut crate::havok::hknpShape,
    /// The body's box in the broad phase, quantized into the world's integer space
    /// ([`hknpWorld::m_intSpaceUtil`](crate::physics::hknpWorld::m_intSpaceUtil)); [`hknpWorld::getBodyAabb`](crate::physics::hknpWorld::getBodyAabb) restores it.
    pub m_aabb: crate::havok::hkAabb16,
    _field_60: [u8; 8],
    /// The body's motion in [`hknpWorld::m_motions`](crate::physics::hknpWorld::m_motions); bodies attached to one another share it.
    pub m_motionId: u32,
    /// The body's entry in the broad phase, or
    /// [`INVALID_BROAD_PHASE_ID`](crate::physics::hknpBody::INVALID_BROAD_PHASE_ID) while it is not in the simulation.
    /// The engine's body wrapper reports a body as added to the world
    /// ([`PfxRigidBody::IsAddedToWorld`](crate::physics::PfxRigidBody::IsAddedToWorld)) when it has an entry, or an addition pending.
    pub m_broadPhaseId: u32,
    _field_70: [u8; 32],
}
fn _hknpBody_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x90], hknpBody>([0u8; 0x90]);
    }
    unreachable!()
}
impl hknpBody {}
impl hknpBody {
    /// The [`m_broadPhaseId`](crate::physics::hknpBody::m_broadPhaseId) of a body that is not in the broad phase: one
    /// created but not added to the simulation, or removed from it.
    pub const INVALID_BROAD_PHASE_ID: u32 = 4294967295;
    /// The [`m_motionId`](crate::physics::hknpBody::m_motionId) of a freed body slot.
    pub const INVALID_MOTION_ID: u32 = 2147483647;
    /// The [`m_flags`](crate::physics::hknpBody::m_flags) bit of a dynamic body, the only kind the velocity and impulse
    /// setters ([`hknpWorld::setBodyVelocity`](crate::physics::hknpWorld::setBodyVelocity), [`hknpWorld::applyBodyImpulseAt`](crate::physics::hknpWorld::applyBodyImpulseAt)) act on.
    pub const IS_DYNAMIC: u32 = 2;
    /// The [`m_flags`](crate::physics::hknpBody::m_flags) bit of a keyframed body: one moved along a set path that
    /// collisions do not push. `CPfxCharacterInstance::IsGroundBodyStaticOrKeyFramed` tests it with
    /// [`IS_STATIC`](crate::physics::hknpBody::IS_STATIC).
    pub const IS_KEYFRAMED: u32 = 4;
    /// The [`m_flags`](crate::physics::hknpBody::m_flags) bit of a static body. A slot with neither this nor
    /// [`IS_DYNAMIC`](crate::physics::hknpBody::IS_DYNAMIC) holds no valid body, which is how
    /// [`PfxCharacterInstance::IsGroundBodyValid`](crate::physics::PfxCharacterInstance::IsGroundBodyValid) tells a body from an empty slot.
    pub const IS_STATIC: u32 = 1;
    /// The [`m_flags`](crate::physics::hknpBody::m_flags) bit that has the solver raise a
    /// [`hknpContactImpulseEvent`](crate::havok::hknpContactImpulseEvent) for the body's contacts.
    /// [`PfxRigidBody::SetContactImpulseEventActive`](crate::physics::PfxRigidBody::SetContactImpulseEventActive) sets and clears it.
    pub const RAISE_CONTACT_IMPULSE_EVENTS: u32 = 128;
}
impl std::convert::AsRef<hknpBody> for hknpBody {
    fn as_ref(&self) -> &hknpBody {
        self
    }
}
impl std::convert::AsMut<hknpBody> for hknpBody {
    fn as_mut(&mut self) -> &mut hknpBody {
        self
    }
}
#[repr(C, align(8))]
/// One body property's values (`hknpBodyManager::PropertyBuffer`), indexed by body id: a body has
/// the property when its bit is set in [`m_occupancy`](crate::physics::hknpBodyPropertyBuffer::m_occupancy), and its value is at
/// that index of [`m_data`](crate::physics::hknpBodyPropertyBuffer::m_data), whose element size is the property's. Only some members
/// are bound.
pub struct hknpBodyPropertyBuffer {
    _field_0: [u8; 8],
    /// One bit per body id, 32 to a word.
    pub m_occupancy: *const u32,
    _field_10: [u8; 8],
    /// The number of body ids the buffer covers.
    pub m_capacity: u32,
    _field_1c: [u8; 4],
    /// The values.
    pub m_data: *mut u8,
}
fn _hknpBodyPropertyBuffer_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x28], hknpBodyPropertyBuffer>([0u8; 0x28]);
    }
    unreachable!()
}
impl hknpBodyPropertyBuffer {}
impl std::convert::AsRef<hknpBodyPropertyBuffer> for hknpBodyPropertyBuffer {
    fn as_ref(&self) -> &hknpBodyPropertyBuffer {
        self
    }
}
impl std::convert::AsMut<hknpBodyPropertyBuffer> for hknpBodyPropertyBuffer {
    fn as_mut(&mut self) -> &mut hknpBodyPropertyBuffer {
        self
    }
}
#[repr(C, align(8))]
/// The body manager's map from body property key to [`hknpBodyPropertyBuffer`](crate::physics::hknpBodyPropertyBuffer)
/// (`hkMap<hkUint16, hknpBodyManager::PropertyBuffer*>`), an open-addressed hash map. Only its
/// lookup is bound.
pub struct hknpBodyPropertyMap {
    _field_0: [u8; 16],
}
fn _hknpBodyPropertyMap_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], hknpBodyPropertyMap>([0u8; 0x10]);
    }
    unreachable!()
}
impl hknpBodyPropertyMap {
    pub const getWithDefault_ADDRESS: usize = 0x1401E87D0;
    /// The property buffer for property `key`, or `default` when there is none.
    pub unsafe fn getWithDefault(
        &self,
        key: u16,
        default: *mut crate::physics::hknpBodyPropertyBuffer,
    ) -> *mut crate::physics::hknpBodyPropertyBuffer {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                key: u16,
                default: *mut crate::physics::hknpBodyPropertyBuffer,
            ) -> *mut crate::physics::hknpBodyPropertyBuffer = ::std::mem::transmute(
                Self::getWithDefault_ADDRESS,
            );
            f(self as *const Self as _, key, default)
        }
    }
}
impl std::convert::AsRef<hknpBodyPropertyMap> for hknpBodyPropertyMap {
    fn as_ref(&self) -> &hknpBodyPropertyMap {
        self
    }
}
impl std::convert::AsMut<hknpBodyPropertyMap> for hknpBodyPropertyMap {
    fn as_mut(&mut self) -> &mut hknpBodyPropertyMap {
        self
    }
}
#[repr(C, align(16))]
/// A Havok character rigid body (`hknpCharacterRigidBody`): the dynamic body of a character proxy
/// ([`PfxCharacterInstance::m_CharacterRigidBody`](crate::physics::PfxCharacterInstance::m_CharacterRigidBody)), whose velocity the character context sets
/// before each step. Only some members are bound.
pub struct hknpCharacterRigidBody {
    _field_0: [u8; 16],
    /// The body's id in [`m_world`](crate::physics::hknpCharacterRigidBody::m_world).
    pub m_bodyId: u32,
    _field_14: [u8; 12],
    /// The world the body is in.
    pub m_world: *mut crate::physics::hknpWorld,
    _field_28: [u8; 8],
}
fn _hknpCharacterRigidBody_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x30], hknpCharacterRigidBody>([0u8; 0x30]);
    }
    unreachable!()
}
impl hknpCharacterRigidBody {
    pub const setLinearVelocity_ADDRESS: usize = 0x141521600;
    /// Sets the body's linear velocity to `velocity` (world space), keeping its angular velocity
    /// ([`hknpWorld::setBodyVelocity`](crate::physics::hknpWorld::setBodyVelocity)), and keeps the change over `timestep` as the body's
    /// acceleration. A change too small to matter only activates the body, and only with
    /// `force_activate`. The proxy's update
    /// ([`PfxCharacterInstance::UpdateContext`](crate::physics::PfxCharacterInstance::UpdateContext)) calls it with the character context's output. The
    /// disassembler mislabels it `CPfxCharacterInstance::CCharacterInput::SetWantedVelocity`, and
    /// gives this function's name to an unrelated lookup.
    pub unsafe fn setLinearVelocity(
        &mut self,
        velocity: *const crate::types::vector_math::AlignedVector3,
        timestep: *const crate::havok::hkSimdFloat32,
        force_activate: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                velocity: *const crate::types::vector_math::AlignedVector3,
                timestep: *const crate::havok::hkSimdFloat32,
                force_activate: bool,
            ) = ::std::mem::transmute(Self::setLinearVelocity_ADDRESS);
            f(self as *mut Self as _, velocity, timestep, force_activate)
        }
    }
}
impl std::convert::AsRef<hknpCharacterRigidBody> for hknpCharacterRigidBody {
    fn as_ref(&self) -> &hknpCharacterRigidBody {
        self
    }
}
impl std::convert::AsMut<hknpCharacterRigidBody> for hknpCharacterRigidBody {
    fn as_mut(&mut self) -> &mut hknpCharacterRigidBody {
        self
    }
}
#[repr(C, align(16))]
/// What a Havok character stands on (`hknpCharacterSurfaceInfo`), as its support check finds it.
pub struct hknpCharacterSurfaceInfo {
    /// Whether the surface is a dynamic body.
    pub m_isSurfaceDynamic: bool,
    /// The support: [`UNSUPPORTED`](crate::physics::hknpCharacterSurfaceInfo::UNSUPPORTED), [`SLIDING`](crate::physics::hknpCharacterSurfaceInfo::SLIDING), or
    /// [`SUPPORTED`](crate::physics::hknpCharacterSurfaceInfo::SUPPORTED).
    pub m_supportedState: u8,
    _field_2: [u8; 2],
    /// How far the character is above the surface beyond the support distance, metres.
    pub m_surfaceDistanceExcess: f32,
    _field_8: [u8; 8],
    /// The surface's normal, Havok space.
    pub m_surfaceNormal: crate::types::vector_math::AlignedVector3,
    /// The surface's velocity, world space. A supported proxy's update carries it into the
    /// velocity the proxy inherits from its ground.
    pub m_surfaceVelocity: crate::types::vector_math::AlignedVector3,
}
fn _hknpCharacterSurfaceInfo_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x30], hknpCharacterSurfaceInfo>([0u8; 0x30]);
    }
    unreachable!()
}
impl hknpCharacterSurfaceInfo {}
impl hknpCharacterSurfaceInfo {
    /// [`m_supportedState`](crate::physics::hknpCharacterSurfaceInfo::m_supportedState): on a surface too steep to stand on.
    pub const SLIDING: u8 = 1;
    /// [`m_supportedState`](crate::physics::hknpCharacterSurfaceInfo::m_supportedState): standing on a surface.
    pub const SUPPORTED: u8 = 2;
    /// [`m_supportedState`](crate::physics::hknpCharacterSurfaceInfo::m_supportedState): no support.
    pub const UNSUPPORTED: u8 = 0;
}
impl std::convert::AsRef<hknpCharacterSurfaceInfo> for hknpCharacterSurfaceInfo {
    fn as_ref(&self) -> &hknpCharacterSurfaceInfo {
        self
    }
}
impl std::convert::AsMut<hknpCharacterSurfaceInfo> for hknpCharacterSurfaceInfo {
    fn as_mut(&mut self) -> &mut hknpCharacterSurfaceInfo {
        self
    }
}
#[repr(C, align(8))]
/// A constraint slot (`hknpConstraint`) of a world's [`hknpConstraintManager`](crate::physics::hknpConstraintManager). A slot whose
/// constraint was destroyed has [`INVALID_BODY_ID`](crate::physics::hknpConstraint::INVALID_BODY_ID) as its first body. Only
/// some members are bound.
pub struct hknpConstraint {
    /// The constraint's first body.
    pub m_bodyIdA: u32,
    /// The constraint's second body.
    pub m_bodyIdB: u32,
    /// The constraint's data (`hkRefPtr<hkpConstraintData>`), which the constraint holds a
    /// reference to.
    pub m_data: *mut crate::havok::hkReferencedObject,
    /// The constraint's id.
    pub m_id: u32,
    _field_14: [u8; 2],
    /// The constraint's flags; see [`IS_DISABLED`](crate::physics::hknpConstraint::IS_DISABLED).
    pub m_flags: u8,
    _field_17: [u8; 33],
}
fn _hknpConstraint_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x38], hknpConstraint>([0u8; 0x38]);
    }
    unreachable!()
}
impl hknpConstraint {}
impl hknpConstraint {
    /// The [`m_bodyIdA`](crate::physics::hknpConstraint::m_bodyIdA) of a destroyed constraint's slot.
    pub const INVALID_BODY_ID: u32 = 2147483647;
    /// The [`m_flags`](crate::physics::hknpConstraint::m_flags) bit of a disabled constraint.
    pub const IS_DISABLED: u8 = 4;
}
impl std::convert::AsRef<hknpConstraint> for hknpConstraint {
    fn as_ref(&self) -> &hknpConstraint {
        self
    }
}
impl std::convert::AsMut<hknpConstraint> for hknpConstraint {
    fn as_mut(&mut self) -> &mut hknpConstraint {
        self
    }
}
#[repr(C, align(8))]
/// A Havok world's constraint manager (`hknpConstraintManager`): a buffer of constraint slots
/// indexed by constraint id. Only some members are bound.
pub struct hknpConstraintManager {
    _field_0: [u8; 8],
    /// The constraint slots, [`m_capacity`](crate::physics::hknpConstraintManager::m_capacity) of them.
    pub m_constraintBuffer: *mut crate::physics::hknpConstraint,
    /// The number of slots.
    pub m_capacity: u32,
    _field_14: [u8; 28],
}
fn _hknpConstraintManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x30], hknpConstraintManager>([0u8; 0x30]);
    }
    unreachable!()
}
impl hknpConstraintManager {}
impl hknpConstraintManager {
    /// The id the manager hands [`hknpWorld::createConstraint`](crate::physics::hknpWorld::createConstraint) when every slot is in use.
    pub const INVALID_CONSTRAINT_ID: u32 = 2147483647;
}
impl std::convert::AsRef<hknpConstraintManager> for hknpConstraintManager {
    fn as_ref(&self) -> &hknpConstraintManager {
        self
    }
}
impl std::convert::AsMut<hknpConstraintManager> for hknpConstraintManager {
    fn as_mut(&mut self) -> &mut hknpConstraintManager {
        self
    }
}
#[repr(C, align(16))]
/// A Havok motion (`hknpMotion`): the mass properties and velocities that move one body or a set
/// of attached bodies. Positions are Havok space: the game-space position less
/// [`PhysicsSystem::m_AccumulatedShift`](crate::physics::PhysicsSystem::m_AccumulatedShift). Only some members are bound.
pub struct hknpMotion {
    /// The centre of mass.
    pub m_centerOfMass: crate::types::vector_math::AlignedVector3,
    /// The orientation of the motion's frame, a unit quaternion `(x, y, z, w)`.
    pub m_orientation: crate::types::vector_math::AlignedQuat,
    /// The inverse inertia about the motion frame's axes and the inverse mass, as four `hkHalf`s
    /// (each the upper 16 bits of an `f32`): `x`, `y`, `z`, then the inverse mass. A zero inverse
    /// mass is infinite mass. The motion's frame is the body's principal frame, so the inertia is
    /// diagonal in it: the world-space inverse inertia is this diagonal turned by
    /// [`m_orientation`](crate::physics::hknpMotion::m_orientation).
    pub m_inverseInertia: [u16; 4],
    _field_28: [u8; 24],
    /// The linear velocity, world space.
    pub m_linearVelocity: crate::types::vector_math::AlignedVector3,
    /// The angular velocity, in the motion's frame.
    pub m_angularVelocity: crate::types::vector_math::AlignedVector3,
    _field_60: [u8; 32],
}
fn _hknpMotion_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x80], hknpMotion>([0u8; 0x80]);
    }
    unreachable!()
}
impl hknpMotion {}
impl std::convert::AsRef<hknpMotion> for hknpMotion {
    fn as_ref(&self) -> &hknpMotion {
        self
    }
}
impl std::convert::AsMut<hknpMotion> for hknpMotion {
    fn as_mut(&mut self) -> &mut hknpMotion {
        self
    }
}
#[repr(C, align(16))]
/// The Havok physics world (`hknpWorld`, Havok Physics 2014.1). Its body manager is embedded at
/// the start; bodies and motions live in arrays indexed by body id and motion id. Only some members
/// are bound.
pub struct hknpWorld {
    _field_0: [u8; 32],
    /// The body manager's body array, indexed by body id. A body id below or at
    /// [`m_peakBodyIndex`](crate::physics::hknpWorld::m_peakBodyIndex) has a slot; a freed slot's
    /// [`hknpBody::m_motionId`](crate::physics::hknpBody::m_motionId) is [`hknpBody::INVALID_MOTION_ID`](crate::physics::hknpBody::INVALID_MOTION_ID).
    pub m_bodies: *mut crate::physics::hknpBody,
    _field_28: [u8; 48],
    /// The body manager's property buffers: per-property arrays indexed by body id, looked up by
    /// property key ([`hknpBodyPropertyMap::getWithDefault`](crate::physics::hknpBodyPropertyMap::getWithDefault)).
    pub m_bodyProperties: crate::physics::hknpBodyPropertyMap,
    _field_68: [u8; 8],
    /// The highest body id the body manager has allocated.
    pub m_peakBodyIndex: u32,
    _field_74: [u8; 108],
    /// The motion manager's motion array, indexed by [`hknpBody::m_motionId`](crate::physics::hknpBody::m_motionId).
    pub m_motions: *mut crate::physics::hknpMotion,
    _field_e8: [u8; 56],
    /// The world's constraints.
    pub m_constraintManager: crate::physics::hknpConstraintManager,
    _field_150: [u8; 48],
    /// The mapping between Havok space and the integer space the broad phase keeps the bodies'
    /// boxes in ([`hknpBody::m_aabb`](crate::physics::hknpBody::m_aabb)).
    pub m_intSpaceUtil: crate::havok::hkIntSpaceUtil,
}
fn _hknpWorld_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1C0], hknpWorld>([0u8; 0x1C0]);
    }
    unreachable!()
}
impl hknpWorld {
    pub const enableBodyFlags_ADDRESS: usize = 0x1414CE4C0;
    /// Adds collision flags and rebuilds collision caches when `rebuild_mode` selects rebuilding.
    /// The disassembler labels this function as a rigid-body constructor.
    pub unsafe fn enableBodyFlags(&mut self, body: u32, flags: u32, rebuild_mode: i32) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                body: u32,
                flags: u32,
                rebuild_mode: i32,
            ) = ::std::mem::transmute(Self::enableBodyFlags_ADDRESS);
            f(self as *mut Self as _, body, flags, rebuild_mode)
        }
    }
    pub const disableBodyFlags_ADDRESS: usize = 0x1414CE550;
    /// Removes collision flags and rebuilds collision caches when `rebuild_mode` selects rebuilding.
    /// The disassembler labels this function as a signal subscription.
    pub unsafe fn disableBodyFlags(&mut self, body: u32, flags: u32, rebuild_mode: i32) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                body: u32,
                flags: u32,
                rebuild_mode: i32,
            ) = ::std::mem::transmute(Self::disableBodyFlags_ADDRESS);
            f(self as *mut Self as _, body, flags, rebuild_mode)
        }
    }
    pub const createConstraint_ADDRESS: usize = 0x1414E7200;
    /// Creates a constraint from `cinfo`, writes its id to `out`, and returns `out`. A constraint
    /// whose two bodies are the same, or whose data is of a type the world does not support, is
    /// created disabled; otherwise it is enabled at once. The constraint takes a reference of its
    /// own to the data (`hknpConstraint::init`, which the disassembler names
    /// `hknpWorldEx::handleConstraintActivationMode`), so the engine's callers, which allocate the
    /// data from the Havok heap, release their own reference once the constraint exists. The
    /// world's constraint collision filter keeps the two bodies of an enabled constraint from
    /// colliding. The engine's own users destroy their constraints before they replace or destroy
    /// the constrained bodies.
    pub unsafe fn createConstraint(
        &mut self,
        out: *mut u32,
        cinfo: *const crate::havok::hknpConstraintCinfo,
    ) -> *mut u32 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                out: *mut u32,
                cinfo: *const crate::havok::hknpConstraintCinfo,
            ) -> *mut u32 = ::std::mem::transmute(Self::createConstraint_ADDRESS);
            f(self as *mut Self as _, out, cinfo)
        }
    }
    pub const destroyConstraints_ADDRESS: usize = 0x1414E7370;
    /// Destroys the `count` constraints whose ids are at `ids`, firing the world's
    /// constraint-disabled signal for those that were enabled and a second signal for each. Each
    /// constraint releases its reference to its data, which destroys data no one else holds, and
    /// its slot goes onto the manager's destroyed list with its first body set to
    /// [`hknpConstraint::INVALID_BODY_ID`](crate::physics::hknpConstraint::INVALID_BODY_ID); the manager reuses destroyed ids once its free ones
    /// run out.
    pub unsafe fn destroyConstraints(&mut self, ids: *const u32, count: i32) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ids: *const u32,
                count: i32,
            ) = ::std::mem::transmute(Self::destroyConstraints_ADDRESS);
            f(self as *mut Self as _, ids, count)
        }
    }
    pub const stepSolve_ADDRESS: usize = 0x1414EDB40;
    /// The solve half of a step (`*solver_data` from the collide half, `hknpSolverData`): it
    /// generates the solve's task graphs, runs a graph of one task on the calling thread and any
    /// other on `task_queue` (`hkTaskQueue`), joining it, and repeats until no task remains. The
    /// last graph is the post-solve phase alone, which dispatches the step's events, so the
    /// world's event listeners run on the calling thread, inside the step.
    /// [`PhysicsSystem::Update`](crate::physics::PhysicsSystem::Update) calls it on the game thread.
    pub unsafe fn stepSolve(
        &mut self,
        solver_data: *mut *mut ::std::ffi::c_void,
        task_queue: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                solver_data: *mut *mut ::std::ffi::c_void,
                task_queue: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::stepSolve_ADDRESS);
            f(self as *mut Self as _, solver_data, task_queue)
        }
    }
    pub const createDynamicBody_ADDRESS: usize = 0x1401E9B20;
    /// Creates a dynamic body from `cinfo` with a new motion from `motion_cinfo`, writes the new
    /// body id to `out`, and returns `out`. The body takes its own reference to the shape. With
    /// [`DO_NOT_ADD`](crate::physics::hknpAdditionMode::DO_NOT_ADD) the body exists but is not in the simulation
    /// until it is added; any other `mode` adds it through the world's body addition with `flags`.
    /// The disassembler names the body creation it calls `hknpWorld::setBodyShape`.
    pub unsafe fn createDynamicBody(
        &mut self,
        out: *mut u32,
        cinfo: *const crate::havok::hknpBodyCinfo,
        motion_cinfo: *const crate::havok::hknpMotionCinfo,
        mode: crate::physics::hknpAdditionMode,
        flags: u8,
    ) -> *mut u32 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                out: *mut u32,
                cinfo: *const crate::havok::hknpBodyCinfo,
                motion_cinfo: *const crate::havok::hknpMotionCinfo,
                mode: crate::physics::hknpAdditionMode,
                flags: u8,
            ) -> *mut u32 = ::std::mem::transmute(Self::createDynamicBody_ADDRESS);
            f(self as *mut Self as _, out, cinfo, motion_cinfo, mode, flags)
        }
    }
    pub const castRay_ADDRESS: usize = 0x141500BE0;
    /// Casts a ray query (`hknpRayCastQuery`) and reports hits to `collector`
    /// (`hknpCollisionQueryCollector`).
    pub unsafe fn castRay(
        &self,
        query: *const ::std::ffi::c_void,
        collector: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                query: *const ::std::ffi::c_void,
                collector: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::castRay_ADDRESS);
            f(self as *const Self as _, query, collector)
        }
    }
    pub const applyBodyImpulseAt_ADDRESS: usize = 0x1414CCB30;
    /// Applies the linear impulse `impulse` to the body `body_id` at the Havok-space point
    /// `position` (the game-space point less [`PhysicsSystem::m_AccumulatedShift`](crate::physics::PhysicsSystem::m_AccumulatedShift)), changing both
    /// its linear and angular velocity, and wakes the body if it sleeps. It acts only on a dynamic
    /// body: a static or keyframed body is left alone. The linear velocity gains the impulse times
    /// the inverse mass; the angular velocity gains the impulse's moment about the centre of mass,
    /// turned into the motion's frame and scaled axis by axis by the inverse inertia
    /// ([`hknpMotion::m_inverseInertia`](crate::physics::hknpMotion::m_inverseInertia)).
    pub unsafe fn applyBodyImpulseAt(
        &mut self,
        body_id: u32,
        impulse: *const crate::types::vector_math::AlignedVector3,
        position: *const crate::types::vector_math::AlignedVector3,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                body_id: u32,
                impulse: *const crate::types::vector_math::AlignedVector3,
                position: *const crate::types::vector_math::AlignedVector3,
            ) = ::std::mem::transmute(Self::applyBodyImpulseAt_ADDRESS);
            f(self as *mut Self as _, body_id, impulse, position)
        }
    }
    pub const setBodyVelocity_ADDRESS: usize = 0x1414CC540;
    /// Sets the linear and angular velocity (world space, the angular one rotated into the
    /// motion's frame) of the dynamic body `body_id`'s motion, waking the body if it sleeps. A body
    /// that is not dynamic is left alone, and so is a call that changes neither velocity. The
    /// disassembler mislabels it `hknpWorld::getBodyVelocity`.
    pub unsafe fn setBodyVelocity(
        &mut self,
        body_id: u32,
        linear: *const crate::types::vector_math::AlignedVector3,
        angular: *const crate::types::vector_math::AlignedVector3,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                body_id: u32,
                linear: *const crate::types::vector_math::AlignedVector3,
                angular: *const crate::types::vector_math::AlignedVector3,
            ) = ::std::mem::transmute(Self::setBodyVelocity_ADDRESS);
            f(self as *mut Self as _, body_id, linear, angular)
        }
    }
    pub const setBodyLinearVelocity_ADDRESS: usize = 0x1414CC710;
    /// Sets the linear velocity (world space) of the dynamic body `body_id`'s motion, waking the
    /// body if it sleeps; as [`setBodyVelocity`](crate::physics::hknpWorld::setBodyVelocity) for the linear part.
    pub unsafe fn setBodyLinearVelocity(
        &mut self,
        body_id: u32,
        linear: *const crate::types::vector_math::AlignedVector3,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                body_id: u32,
                linear: *const crate::types::vector_math::AlignedVector3,
            ) = ::std::mem::transmute(Self::setBodyLinearVelocity_ADDRESS);
            f(self as *mut Self as _, body_id, linear)
        }
    }
    pub const setBodyAngularVelocity_ADDRESS: usize = 0x1414CC7D0;
    /// Sets the angular velocity (world space) of the dynamic body `body_id`'s motion, waking the
    /// body if it sleeps; as [`setBodyVelocity`](crate::physics::hknpWorld::setBodyVelocity) for the angular part.
    pub unsafe fn setBodyAngularVelocity(
        &mut self,
        body_id: u32,
        angular: *const crate::types::vector_math::AlignedVector3,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                body_id: u32,
                angular: *const crate::types::vector_math::AlignedVector3,
            ) = ::std::mem::transmute(Self::setBodyAngularVelocity_ADDRESS);
            f(self as *mut Self as _, body_id, angular)
        }
    }
    pub const setBodyShape_ADDRESS: usize = 0x1414CF370;
    /// Replaces the shape (`hknpShape*`) of the body `body_id` on the live world.
    pub unsafe fn setBodyShape(
        &mut self,
        body_id: u32,
        shape: *const ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                body_id: u32,
                shape: *const ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::setBodyShape_ADDRESS);
            f(self as *mut Self as _, body_id, shape)
        }
    }
    pub const getBodyAabb_ADDRESS: usize = 0x1414CC220;
    /// Writes the body `body_id`'s box as the broad phase holds it to `aabb`, Havok space: its
    /// [`hknpBody::m_aabb`](crate::physics::hknpBody::m_aabb) restored through [`m_intSpaceUtil`](crate::physics::hknpWorld::m_intSpaceUtil), each corner
    /// as the integer corner times the inverse scale less the low offset.
    pub unsafe fn getBodyAabb(&self, body_id: u32, aabb: *mut crate::havok::hkAabb) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                body_id: u32,
                aabb: *mut crate::havok::hkAabb,
            ) = ::std::mem::transmute(Self::getBodyAabb_ADDRESS);
            f(self as *const Self as _, body_id, aabb)
        }
    }
}
impl hknpWorld {
    /// The body property that holds a body's engine instance ([`PfxInstance`](crate::physics::PfxInstance)), as the physics
    /// queries read it to report the body's owner.
    pub const BODY_PROPERTY_PFX_INSTANCE: u16 = 1;
    /// Rebuild the body's collision caches after changing flags.
    pub const REBUILD_COLLISION_CACHES: i32 = 0;
}
impl std::convert::AsRef<hknpWorld> for hknpWorld {
    fn as_ref(&self) -> &hknpWorld {
        self
    }
}
impl std::convert::AsMut<hknpWorld> for hknpWorld {
    fn as_mut(&mut self) -> &mut hknpWorld {
        self
    }
}
pub const SweptSphereCast_ADDRESS: usize = 0x140738670;
/// `SweptSphereCast`: a free-function sphere sweep that fills `result` (`CCastResult`). The
/// meaning of its scalar parameters is not established.
pub unsafe fn SweptSphereCast(
    p0: f32,
    p1: f32,
    p2: f32,
    p3: f32,
    p4: f32,
    p5: f32,
    v0: *const crate::types::math::Vector3,
    v1: *const crate::types::math::Vector3,
    p8: f32,
    p9: bool,
    result: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            p0: f32,
            p1: f32,
            p2: f32,
            p3: f32,
            p4: f32,
            p5: f32,
            v0: *const crate::types::math::Vector3,
            v1: *const crate::types::math::Vector3,
            p8: f32,
            p9: bool,
            result: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(SweptSphereCast_ADDRESS);
        f(p0, p1, p2, p3, p4, p5, v0, v1, p8, p9, result)
    }
}
pub const SlerpAroundAxis_ADDRESS: usize = 0x140775620;
/// Rotates `from` toward `to` by the fraction `t` of the angle between them (about the axis of the
/// shortest-arc rotation), writing the result to `out`. When the two directions are within the
/// `0.99` dot-product threshold it writes `to` unchanged. The swim movement cores call it.
pub unsafe fn SlerpAroundAxis(
    from: *const crate::types::math::Vector3,
    to: *const crate::types::math::Vector3,
    t: f32,
    out: *mut crate::types::math::Vector3,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            from: *const crate::types::math::Vector3,
            to: *const crate::types::math::Vector3,
            t: f32,
            out: *mut crate::types::math::Vector3,
        ) = ::std::mem::transmute(SlerpAroundAxis_ADDRESS);
        f(from, to, t, out)
    }
}
pub const DoRotate_ADDRESS: usize = 0x1407754F0;
/// Rotates `from` toward `to` along the shortest arc, writing the result to `out`. It takes the
/// rotation from `from` to `to` as an angle-axis pair and applies
/// `min(angle / 0.2617994, 1) * rate` of it, so the step eases out as the two directions converge.
/// Note that the step is **not** clamped to the remaining angle: a `rate` above 0.2617994 radians
/// (15 degrees) overshoots the target, and one above twice that diverges. It logs `"UHOH!"` when
/// the two directions are more than about 162 degrees apart, where the rotation axis is
/// ill-conditioned.
///
/// The swim locomotion family calls it from two places per frame, for two different quantities:
///
/// - [`UpdateSurfaceMovement`](crate::input::swim::UpdateSurfaceMovement) and
///   [`UpdateUnderwaterMovement`](crate::input::swim::UpdateUnderwaterMovement) rotate the character's
///   current forward toward the desired direction (the world move direction from
///   [`ControllerUtility::TransformInputDirToWorldDir`](crate::input::controller_utility::ControllerUtility::TransformInputDirToWorldDir)
///   while moving, the aim-target direction while aiming, the grapple target while reeling, or the
///   current forward when the stick is neutral). The result is the `direction` argument to
///   `CMatrix4f::CreateOrientation`, so **this call is what turns the body**.
/// - [`ProcessMotion`](crate::physics::ProcessMotion) slerps the character's current velocity direction toward the wanted one.
pub unsafe fn DoRotate(
    from: *const crate::types::math::Vector3,
    to: *const crate::types::math::Vector3,
    rate: f32,
    out: *mut crate::types::math::Vector3,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            from: *const crate::types::math::Vector3,
            to: *const crate::types::math::Vector3,
            rate: f32,
            out: *mut crate::types::math::Vector3,
        ) = ::std::mem::transmute(DoRotate_ADDRESS);
        f(from, to, rate, out)
    }
}
pub const ProcessMotion_ADDRESS: usize = 0x140775760;
/// Turns a wanted velocity into the velocity actually applied this frame: it slerps the direction
/// of `current_vel` toward the direction of `wanted_vel` through [`DoRotate`](crate::physics::DoRotate), limited to
/// `slerp_delta_angle` radians, then rescales the result to the horizontal magnitude of
/// `wanted_vel`. Either direction falls back to the character's world-matrix forward when it is
/// degenerate, and `xz_only` flattens both onto the ground plane first. `dt` is unused: the
/// magnitude comes from `wanted_vel`, not from integration. The surface swim movement core uses it
/// to keep a swimmer's motion from snapping direction when the stick swings.
pub unsafe fn ProcessMotion(
    character: *mut crate::character::character::Character,
    dt: f32,
    current_vel: *const crate::types::math::Vector3,
    wanted_vel: *const crate::types::math::Vector3,
    slerp_delta_angle: f32,
    xz_only: bool,
    vel_out: *mut crate::types::math::Vector3,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            character: *mut crate::character::character::Character,
            dt: f32,
            current_vel: *const crate::types::math::Vector3,
            wanted_vel: *const crate::types::math::Vector3,
            slerp_delta_angle: f32,
            xz_only: bool,
            vel_out: *mut crate::types::math::Vector3,
        ) = ::std::mem::transmute(ProcessMotion_ADDRESS);
        f(character, dt, current_vel, wanted_vel, slerp_delta_angle, xz_only, vel_out)
    }
}
