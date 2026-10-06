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
#[repr(C, align(8))]
/// `CPfxCharacterInstance`: the character's physics-side body instance (the Havok character proxy),
/// owned by `CCharacter` through a `boost::shared_ptr`. Only some members are bound.
pub struct PfxCharacterInstance {
    _field_0: [u8; 56],
    /// The proxy input the physics step reads; see [`CharacterInput`](crate::physics::CharacterInput).
    pub m_Input: crate::physics::CharacterInput,
    _field_48: [u8; 124],
    /// Whether the proxy is standing on stairs. The on-foot locomotion task takes its velocity
    /// along the blackboard's slope direction while it is set.
    pub m_IsSupportedOnStairs: bool,
    _field_c5: [u8; 3],
    /// The current proxy state, a [`ProxyState`](crate::physics::ProxyState) value, which selects the proxy's collision shape.
    /// Written by [`SetProxyState`](crate::physics::PfxCharacterInstance::SetProxyState).
    pub m_ProxyState: i32,
    _field_cc: [u8; 4],
    /// The Havok character rigid body (`hknpCharacterRigidBody*`), which holds the body id that
    /// [`GetCharacterRigidBodyId`](crate::physics::PfxCharacterInstance::GetCharacterRigidBodyId) reads.
    pub m_CharacterRigidBody: *mut ::std::ffi::c_void,
    _field_d8: [u8; 16],
    /// The character rigid-body listener (`CCustomCharacterRBListener*`).
    pub m_CharacterRBListener: *mut ::std::ffi::c_void,
    /// The owning character (`IPfxCharacter*`). Its `GetSharedPfxCharacterSettings` virtual returns
    /// the shared settings that hold the per-state shape table.
    pub m_Owner: *mut ::std::ffi::c_void,
}
fn _PfxCharacterInstance_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xF8], PfxCharacterInstance>([0u8; 0xF8]);
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
/// An engine physics instance (`IPfxInstance`), the object behind a body's
/// [`BODY_PROPERTY_PFX_INSTANCE`](crate::physics::hknpWorld::BODY_PROPERTY_PFX_INSTANCE) property. Only one member
/// is bound.
pub struct PfxInstance {
    _field_0: [u8; 40],
    /// The instance's owner (`IPfxGameObject*`): the [`PhysicsGameObject`](crate::physics::PhysicsGameObject) a physics query reports
    /// for a hit on the instance's bodies ([`CastRayInfo::m_GameObject`](crate::physics::CastRayInfo::m_GameObject)).
    pub m_GameObject: *mut ::std::ffi::c_void,
}
fn _PfxInstance_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x30], PfxInstance>([0u8; 0x30]);
    }
    unreachable!()
}
impl PfxInstance {}
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
    /// the world and the time the update was given, outside the step: the world may be changed
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
    /// the world and the time the update was given, outside the step: the world may be changed
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
}
impl PfxRigidBody {
    /// The [`Destruct`](crate::physics::PfxRigidBody::Destruct) flag that frees the wrapper's memory.
    pub const DESTRUCT_FREE: u8 = 1;
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
}
fn _PfxRigidBodyVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x80], PfxRigidBodyVftable>([0u8; 0x80]);
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
    _vfunc_1: unsafe extern "system" fn(this: *mut crate::physics::PhysicsGameObject),
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
    _vfunc_13: unsafe extern "system" fn(this: *mut crate::physics::PhysicsGameObject),
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
    /// ([`AddPreSimulationCallback`](crate::physics::PhysicsSystem::AddPreSimulationCallback)) with the world and `dt`, and
    /// steps the world once by [`STEP_SECONDS`](crate::physics::PhysicsSystem::STEP_SECONDS) on the physics thread pool,
    /// joining the pool before it returns. The game states' updates
    /// (`CGameStateRun::Update` and the loading and front-end states' updates) call it inline on
    /// the thread that runs them, through the physics update, which dispatches the queued
    /// collisions and the damage listeners' events after it.
    pub unsafe fn Update(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _, dt)
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
    /// exactly this, whatever time it is given.
    pub const STEP_SECONDS: f32 = 0.033333335;
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
/// dynamic. Characters, vehicles, and weapons are not rigid objects. Only its type id is bound.
pub struct RigidObject {}
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
    _field_0: [u8; 64],
    /// The body's flags; see [`IS_DYNAMIC`](crate::physics::hknpBody::IS_DYNAMIC).
    pub m_flags: u32,
    /// The body's collision-filter word, read by the world's collision filter; see
    /// [`CustomGroupCollisionFilter`](crate::physics::CustomGroupCollisionFilter) for its fields.
    pub m_collisionFilterInfo: u32,
    _field_48: [u8; 32],
    /// The body's motion in [`hknpWorld::m_motions`](crate::physics::hknpWorld::m_motions); bodies attached to one another share it.
    pub m_motionId: u32,
    _field_6c: [u8; 36],
}
fn _hknpBody_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x90], hknpBody>([0u8; 0x90]);
    }
    unreachable!()
}
impl hknpBody {}
impl hknpBody {
    /// The [`m_motionId`](crate::physics::hknpBody::m_motionId) of a freed body slot.
    pub const INVALID_MOTION_ID: u32 = 2147483647;
    /// The [`m_flags`](crate::physics::hknpBody::m_flags) bit of a dynamic body, the only kind the velocity and impulse
    /// setters ([`hknpWorld::setBodyVelocity`](crate::physics::hknpWorld::setBodyVelocity), [`hknpWorld::applyBodyImpulseAt`](crate::physics::hknpWorld::applyBodyImpulseAt)) act on.
    pub const IS_DYNAMIC: u32 = 2;
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
#[repr(C, align(8))]
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
}
fn _hknpWorld_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xE8], hknpWorld>([0u8; 0xE8]);
    }
    unreachable!()
}
impl hknpWorld {
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
}
impl hknpWorld {
    /// The body property that holds a body's engine instance ([`PfxInstance`](crate::physics::PfxInstance)), as the physics
    /// queries read it to report the body's owner.
    pub const BODY_PROPERTY_PFX_INSTANCE: u16 = 1;
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
