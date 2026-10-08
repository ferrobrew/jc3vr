#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The opaque BFBC context: the per-system beam-frustum-based-culling state that `BFBCProcess` reads
/// and writes. Its layout is internal to the BFBC backend and not mapped here.
pub struct SBFBCContext {}
impl SBFBCContext {}
impl std::convert::AsRef<SBFBCContext> for SBFBCContext {
    fn as_ref(&self) -> &SBFBCContext {
        self
    }
}
impl std::convert::AsMut<SBFBCContext> for SBFBCContext {
    fn as_mut(&mut self) -> &mut SBFBCContext {
        self
    }
}
#[repr(C, align(8))]
/// The opaque BFBC process parameter block: the per-camera cull-frustum parameters produced by
/// [`GetBFBCFrustumParamsForCameraAndTime`](crate::graphics_engine::graphics_engine::OccluderCollectionManager::GetBFBCFrustumParamsForCameraAndTime)
/// and consumed by `BFBCProcess`. See
/// [`BFBCFrustumCullParameters`](crate::graphics_engine::graphics_engine::BFBCFrustumCullParameters) for the
/// one mapped field (`m_FrustumCount`).
pub struct SBFBCProcessParameter {}
impl SBFBCProcessParameter {}
impl std::convert::AsRef<SBFBCProcessParameter> for SBFBCProcessParameter {
    fn as_ref(&self) -> &SBFBCProcessParameter {
        self
    }
}
impl std::convert::AsMut<SBFBCProcessParameter> for SBFBCProcessParameter {
    fn as_mut(&mut self) -> &mut SBFBCProcessParameter {
        self
    }
}
#[repr(C, align(8))]
/// The opaque BFBC process state: the per-camera cache slot handle returned alongside the parameter
/// block.
pub struct SBFBCProcessState {}
impl SBFBCProcessState {}
impl std::convert::AsRef<SBFBCProcessState> for SBFBCProcessState {
    fn as_ref(&self) -> &SBFBCProcessState {
        self
    }
}
impl std::convert::AsMut<SBFBCProcessState> for SBFBCProcessState {
    fn as_mut(&mut self) -> &mut SBFBCProcessState {
        self
    }
}
#[repr(C, align(8))]
/// One per-tag spawn budget entry. `CSpawnSystem::Update` iterates these each frame; when the live
/// count exceeds the budget, the highest-despawn-rank object is despawned. Partial: only the budget
/// field is mapped.
pub struct SpawnBudgetEntry {
    _field_0: [u8; 64],
    /// The maximum number of live objects for this spawn tag. When
    /// `count - unloading_count >= budget`, the spawn system despawns the object with the highest
    /// despawn rank.
    pub m_Budget: u32,
    _field_44: [u8; 244],
}
fn _SpawnBudgetEntry_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x138], SpawnBudgetEntry>([0u8; 0x138]);
    }
    unreachable!()
}
impl SpawnBudgetEntry {}
impl std::convert::AsRef<SpawnBudgetEntry> for SpawnBudgetEntry {
    fn as_ref(&self) -> &SpawnBudgetEntry {
        self
    }
}
impl std::convert::AsMut<SpawnBudgetEntry> for SpawnBudgetEntry {
    fn as_mut(&mut self) -> &mut SpawnBudgetEntry {
        self
    }
}
/// The completion callback for [`Spawn`](crate::spawn_system::SpawnSystem::Spawn): a code pointer with the signature
/// `void (*)(const std::vector<boost::shared_ptr<CGameObject>>* spawned_objects, void* userdata)`,
/// invoked with the spawned objects and the caller's userdata once the request has been serviced.
/// Modeled as a raw pointer because the definition language does not yet express function-pointer
/// types.
pub type SpawnCallback = *const u8;
#[repr(C, align(8))]
/// `CSpawnSystem::CSpawnFactoryImpl`: the factory that vets and performs individual spawns.
pub struct SpawnFactoryImpl {}
impl SpawnFactoryImpl {
    pub const CheckInternal_ADDRESS: usize = 0x140F09430;
    /// Vets a spawn of `resource` at `transform`: checks mission and encounter state, landscape
    /// physics residency, world-position validity, the despawn rank, the camera-visibility gate
    /// (a BFBC frustum test against the camera's box-frustum parameters), and a physics shape
    /// overlap. Returns whether the spawn may proceed.
    pub unsafe fn CheckInternal(
        &mut self,
        resource: *mut crate::spawn_system::SpawnResourceDef,
        transform: *const crate::types::math::Matrix4,
        priority: i16,
        flags: u32,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                resource: *mut crate::spawn_system::SpawnResourceDef,
                transform: *const crate::types::math::Matrix4,
                priority: i16,
                flags: u32,
            ) -> bool = ::std::mem::transmute(Self::CheckInternal_ADDRESS);
            f(self as *mut Self as _, resource, transform, priority, flags)
        }
    }
}
impl std::convert::AsRef<SpawnFactoryImpl> for SpawnFactoryImpl {
    fn as_ref(&self) -> &SpawnFactoryImpl {
        self
    }
}
impl std::convert::AsMut<SpawnFactoryImpl> for SpawnFactoryImpl {
    fn as_mut(&mut self) -> &mut SpawnFactoryImpl {
        self
    }
}
#[repr(C, align(8))]
/// An opaque `CSpawnSystem::SResourceDef`: one spawnable resource's definition.
pub struct SpawnResourceDef {}
impl SpawnResourceDef {}
impl std::convert::AsRef<SpawnResourceDef> for SpawnResourceDef {
    fn as_ref(&self) -> &SpawnResourceDef {
        self
    }
}
impl std::convert::AsMut<SpawnResourceDef> for SpawnResourceDef {
    fn as_mut(&mut self) -> &mut SpawnResourceDef {
        self
    }
}
#[repr(C, align(8))]
/// The spawn system: manages streaming (de)spawn of dynamic objects around the player. Each
/// frame, [`Update`](crate::spawn_system::SpawnSystem::Update) builds a BFBC cull frustum from the camera manager's active
/// camera and uses it as a "don't (de)spawn while visible" gate in `CSpawnFactoryImpl::CheckInternal`, so
/// objects are not spawned or despawned inside the player's view.
///
/// Its resource definitions are loaded at startup from the per-pool spawn definition files
/// (`settings/spawn_*_defs.bin`) referenced by `settings/spawn_budget_pools.bin`. Each budget pool
/// (aircraft, animals, civilians, combatants, devices, props, road decorations, trains, vehicles,
/// weapons, effects, and encounters) names a definition file, and each definition therein carries
/// an exported-entity path (`.ee`), a space-separated tag string, a selection weight, and an
/// optional debug-menu category. Spawn requests select definitions by matching tags, so the system
/// governs the (de)spawn of every tag-addressable dynamic object, not only characters and
/// vehicles. World geometry is placed by the location system instead and does not pass through
/// here.
pub struct SpawnSystem {
    _field_0: [u8; 48],
    /// The per-tag spawn budget entries. Each entry tracks the max object count (budget), current
    /// count, and despawn-rank state for one spawn tag. When the count exceeds the budget, the
    /// highest-despawn-rank object is despawned.
    pub m_BudgetEntries: crate::types::std_vector::Vector<
        crate::spawn_system::SpawnBudgetEntry,
    >,
    _field_50: [u8; 280],
    /// The BFBC cull context the spawn system's visibility gate runs against.
    pub m_SpawnBFBC: *mut crate::spawn_system::SBFBCContext,
    /// The BFBC cull-frustum parameters, populated each frame by
    /// [`GetBFBCFrustumParamsForCameraAndTime`](crate::graphics_engine::graphics_engine::OccluderCollectionManager::GetBFBCFrustumParamsForCameraAndTime)
    /// against the active camera.
    pub m_SpawnBFBCParams: *const crate::spawn_system::SBFBCProcessParameter,
    /// The BFBC cache-slot state handle, populated alongside the parameters.
    pub m_SpawnBFBCState: *mut crate::spawn_system::SBFBCProcessState,
    _field_180: [u8; 4],
    /// The center position the spawn system streams around, set by
    /// [`SetCenterPosition`](crate::spawn_system::SpawnSystem::SetCenterPosition).
    pub m_CenterPos: crate::types::math::Vector3,
    /// The view position for the spawn visibility gate, set by
    /// [`SetCenterPosition`](crate::spawn_system::SpawnSystem::SetCenterPosition).
    pub m_ViewPos: crate::types::math::Vector3,
    /// The view direction for the spawn visibility gate, set by
    /// [`SetCenterPosition`](crate::spawn_system::SpawnSystem::SetCenterPosition).
    pub m_ViewDir: crate::types::math::Vector3,
}
fn _SpawnSystem_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1A8], SpawnSystem>([0u8; 0x1A8]);
    }
    unreachable!()
}
impl SpawnSystem {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5418092952usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl SpawnSystem {
    pub const Update_ADDRESS: usize = 0x140EFEE90;
    /// The per-frame spawn update: walks every resource definition, evaluates despawn ranks against the
    /// view position, and at its tail builds a BFBC cull frustum from the camera manager's active camera
    /// ([`CameraManager::m_ActiveCamera`](crate::camera::camera_manager::CameraManager::m_ActiveCamera)) via
    /// [`GetBFBCFrustumParamsForCameraAndTime`](crate::graphics_engine::graphics_engine::OccluderCollectionManager::GetBFBCFrustumParamsForCameraAndTime),
    /// storing the result in [`m_SpawnBFBCParams`](crate::spawn_system::SpawnSystem::m_SpawnBFBCParams) and
    /// [`m_SpawnBFBCState`](crate::spawn_system::SpawnSystem::m_SpawnBFBCState) for the factory's visibility gate.
    ///
    /// The despawn ranking visits every spawned object of every resource definition. An object
    /// whose own weak reference's control block (for a weapon,
    /// [`WeaponBase::m_WeakThis`](crate::equipment::weapon::WeaponBase::m_WeakThis)) has a strong count
    /// other than one, the spawn system's own reference, is held by something else: it is given
    /// rank zero and is never queued for despawn, nor chosen when its pool is over budget. Any
    /// other object is ranked by `CalcDespawnRank` from its position, whether it is dead, and its
    /// interest; its mission-critical flag protects it only while a mission or an encounter is
    /// active, and is cleared otherwise. An object whose rank exceeds its pool's despawn threshold
    /// is queued for despawn; otherwise the pool's highest-ranked object is remembered and queued
    /// when the pool's instance count is over its limit.
    pub unsafe fn Update(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const SetCenterPosition_ADDRESS: usize = 0x140DF4BE0;
    /// Sets the spawn center, view position, and view direction used by the visibility gate. Called
    /// once per frame by `CGameStateRun::UpdateCenterPosition` with the streaming center and the active
    /// camera's position and forward direction.
    pub unsafe fn SetCenterPosition(
        &mut self,
        centerpos: *const crate::types::math::Vector3,
        viewpos: *const crate::types::math::Vector3,
        viewdir: *const crate::types::math::Vector3,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                centerpos: *const crate::types::math::Vector3,
                viewpos: *const crate::types::math::Vector3,
                viewdir: *const crate::types::math::Vector3,
            ) = ::std::mem::transmute(Self::SetCenterPosition_ADDRESS);
            f(self as *mut Self as _, centerpos, viewpos, viewdir)
        }
    }
    pub const Spawn_ADDRESS: usize = 0x140E97DA0;
    /// Queues an asynchronous spawn request for the resource definitions matching the given tag
    /// string. Tags are separated by spaces, commas, or pipes; a definition matches when it
    /// carries every requested tag, and the matching set must belong to a single budget pool. The
    /// request creates a spawn factory, streams the selected definition's exported entity in if
    /// needed, and is serviced on a later frame by the spawn thread's update; on success the
    /// callback fires with the spawned objects and the given userdata. The transform is the
    /// world-space placement for the spawned object. `flags` is a combination of the
    /// `EFactoryFlags` constants ([`FF_DEFAULT`](crate::spawn_system::SpawnSystem::FF_DEFAULT) and friends) controlling which
    /// spawn checks apply. The callback may be null when the caller does not need the spawned
    /// objects.
    pub unsafe fn Spawn(
        &mut self,
        tags: *const u8,
        transform: *const crate::types::math::Matrix4,
        flags: u32,
        callback: *const u8,
        userdata: *mut u8,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                tags: *const u8,
                transform: *const crate::types::math::Matrix4,
                flags: u32,
                callback: *const u8,
                userdata: *mut u8,
            ) = ::std::mem::transmute(Self::Spawn_ADDRESS);
            f(self as *mut Self as _, tags, transform, flags, callback, userdata)
        }
    }
    pub const SpawnTagIndex_ADDRESS: usize = 0x140E97EE0;
    /// Queues an asynchronous spawn request for the spawn tag `tag_index`, a type as
    /// [`GameObject::GetUniqueSpawnTagIndex`](crate::game::GameObject::GetUniqueSpawnTagIndex) names it
    /// (the tag-index overload of `CSpawnSystem::Spawn`). The request is queued under the system's
    /// request lock and serviced on a later frame, as for [`Spawn`](crate::spawn_system::SpawnSystem::Spawn), and `callback`
    /// then fires with the spawned objects and `userdata`. The inventory's respawn of a slot
    /// passes [`FF_HIGH_PRIO_SPAWN_CHECKS`](crate::spawn_system::SpawnSystem::FF_HIGH_PRIO_SPAWN_CHECKS) for the player and
    /// [`FF_SECONDARY_SPAWN_CHECKS`](crate::spawn_system::SpawnSystem::FF_SECONDARY_SPAWN_CHECKS) otherwise, the character's
    /// transform, and `-1` for `p7`.
    pub unsafe fn SpawnTagIndex(
        &mut self,
        tag_index: i32,
        transform: *const crate::types::math::Matrix4,
        flags: u32,
        callback: *const u8,
        userdata: *mut u8,
        p7: i32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                tag_index: i32,
                transform: *const crate::types::math::Matrix4,
                flags: u32,
                callback: *const u8,
                userdata: *mut u8,
                p7: i32,
            ) = ::std::mem::transmute(Self::SpawnTagIndex_ADDRESS);
            f(
                self as *mut Self as _,
                tag_index,
                transform,
                flags,
                callback,
                userdata,
                p7,
            )
        }
    }
    pub const GetTagHashFromTagIndex_ADDRESS: usize = 0x140E0D2C0;
    /// The hash of the spawn tag `tag_index`, or zero for an index out of range.
    pub unsafe fn GetTagHashFromTagIndex(&self, tag_index: i32) -> u32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self, tag_index: i32) -> u32 = ::std::mem::transmute(
                Self::GetTagHashFromTagIndex_ADDRESS,
            );
            f(self as *const Self as _, tag_index)
        }
    }
    pub const GetTagIndexFromTagHash_ADDRESS: usize = 0x140E5D5A0;
    /// The index of the spawn tag whose hash is `tag_hash`, or `-1` when no loaded tag has it.
    pub unsafe fn GetTagIndexFromTagHash(&self, tag_hash: u32) -> i32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self, tag_hash: u32) -> i32 = ::std::mem::transmute(
                Self::GetTagIndexFromTagHash_ADDRESS,
            );
            f(self as *const Self as _, tag_hash)
        }
    }
    pub const ResetSystem_ADDRESS: usize = 0x140ECC450;
    /// Resets the system: waits for its spawn job, sends `spawn.reset_all_spawn_rules` at once,
    /// destroys every pending spawn request and its factory, releases the system's references to
    /// every spawned object of every resource definition and every object queued for despawn, and
    /// posts `spawn.post_reset_all_spawn_rules`, which its receivers handle later. An object that
    /// something else still holds a strong reference to outlives the reset, outside the system.
    pub unsafe fn ResetSystem(&mut self) -> i32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) -> i32 = ::std::mem::transmute(
                Self::ResetSystem_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl SpawnSystem {
    /// `EFactoryFlags`: mark the spawn as an ambient spawn.
    pub const FF_AMBIENT_SPAWN: u32 = 2048;
    /// `EFactoryFlags`: automatically place a despawn block on the spawned object.
    pub const FF_AUTO_DESPAWN_BLOCK: u32 = 32768;
    /// `EFactoryFlags`: run the debug-check variant of the spawn checks.
    pub const FF_DEBUG_CHECK: u32 = 262144;
    /// `EFactoryFlags`: mark the spawn as a debug spawn.
    pub const FF_DEBUG_SPAWN: u32 = 524288;
    /// `EFactoryFlags` combination used for debug spawns (no despawn, line-of-sight,
    /// player-proximity, or time checks, no debug-loop check, and high-priority load).
    pub const FF_DEBUG_SPAWN_CHECKS: u32 = 131388;
    /// `EFactoryFlags`: bypasses none of the spawn checks; the factory performs resource
    /// requesting, despawn, line-of-sight, player-proximity, spawn-block, time, spawn-pool,
    /// physics, and landscape checks as configured.
    pub const FF_DEFAULT: u32 = 0;
    /// `EFactoryFlags`: stream the definition's resources at high priority.
    pub const FF_HIGH_PRIORITY_LOAD: u32 = 131072;
    /// `EFactoryFlags` combination used for black-market and other high-priority player-facing
    /// spawns (no despawn, line-of-sight, player-proximity, spawn-block, or time checks, premium
    /// content allowed, and high-priority load). `CInventory::SpawnSecondaryAttachObject` passes
    /// this for the player and [`FF_SECONDARY_SPAWN_CHECKS`](crate::spawn_system::SpawnSystem::FF_SECONDARY_SPAWN_CHECKS)
    /// otherwise.
    pub const FF_HIGH_PRIO_SPAWN_CHECKS: u32 = 139388;
    /// `EFactoryFlags`: mark the spawn as mission critical, protecting it from despawn.
    pub const FF_MISSION_CRITICAL: u32 = 4096;
    /// `EFactoryFlags`: do not count the spawn against its pool budget.
    pub const FF_NO_BUDGET_COUNT: u32 = 512;
    /// `EFactoryFlags`: skip the spawn-loop debug check.
    pub const FF_NO_DEBUG_LOOP_CHECK: u32 = 65536;
    /// `EFactoryFlags`: skip the despawn check.
    pub const FF_NO_DESPAWN_CHECK: u32 = 4;
    /// `EFactoryFlags`: skip the landscape (ground validity) check.
    pub const FF_NO_LANDSCAPE_CHECK: u32 = 1024;
    /// `EFactoryFlags`: skip the line-of-sight check that blocks spawning inside the player's view.
    pub const FF_NO_LOS_CHECK: u32 = 8;
    /// `EFactoryFlags`: skip the active-physics-area check.
    pub const FF_NO_PHYSICS_CHECK: u32 = 256;
    /// `EFactoryFlags`: skip the player-proximity check.
    pub const FF_NO_PLAYER_PROX_CHECK: u32 = 16;
    /// `EFactoryFlags`: do not request resource streaming; spawn only if already loaded.
    pub const FF_NO_RESOURCE_REQUEST: u32 = 1;
    /// `EFactoryFlags`: skip the spawn-block volume check.
    pub const FF_NO_SPAWN_BLOCK_CHECK: u32 = 32;
    /// `EFactoryFlags`: skip the spawn-pool budget check.
    pub const FF_NO_SPAWN_POOL_CHECK: u32 = 128;
    /// `EFactoryFlags`: skip taking the system lock.
    pub const FF_NO_SYSTEM_LOCK: u32 = 2;
    /// `EFactoryFlags`: skip the frame time-budget check.
    pub const FF_NO_TIME_CHECK: u32 = 64;
    /// `EFactoryFlags`: allow definitions flagged as premium content.
    pub const FF_PREMIUM_SPAWN: u32 = 8192;
    /// `EFactoryFlags`: allow definitions flagged as prototype content.
    pub const FF_PROTOTYPE_SPAWN: u32 = 16384;
    /// `EFactoryFlags` combination used for secondary attach-object spawns (no despawn,
    /// line-of-sight, player-proximity, spawn-block, or time checks).
    pub const FF_SECONDARY_SPAWN_CHECKS: u32 = 380;
}
impl std::convert::AsRef<SpawnSystem> for SpawnSystem {
    fn as_ref(&self) -> &SpawnSystem {
        self
    }
}
impl std::convert::AsMut<SpawnSystem> for SpawnSystem {
    fn as_mut(&mut self) -> &mut SpawnSystem {
        self
    }
}
