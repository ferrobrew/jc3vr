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
/// The completion callback for [`Spawn`](crate::spawn_system::SpawnSystem::Spawn) and
/// [`SpawnTagIndex`](crate::spawn_system::SpawnSystem::SpawnTagIndex). Its signature is
/// `void (*)(const std::vector<boost::shared_ptr<CGameObject>>* spawned_objects, void* userdata)`.
/// The vector uses [`Vector`](crate::types::std_vector::Vector) of
/// [`SharedPtr`](crate::types::shared_ptr::SharedPtr) to [`GameObject`](crate::game::GameObject).
///
/// [`PreUpdate`](crate::spawn_system::SpawnSystem::PreUpdate) invokes the callback inline on its caller while holding the
/// request lock. The vector and its strong references are borrowed until the callback returns.
/// PreUpdate then releases those references and the vector storage. Retaining an object beyond the
/// callback requires an additional strong reference; copying a borrowed pointer pair does not take
/// one. The system copies the userdata value without taking ownership of it. Cancellation and an
/// invalid factory do not invoke the callback.
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
    _field_50: [u8; 248],
    /// The completion signal for the resource and despawn-ranking job run by
    /// [`Update`](crate::spawn_system::SpawnSystem::Update). `StartUpdate` clears it before scheduling that job.
    /// [`ResetSystem`](crate::spawn_system::SpawnSystem::ResetSystem) waits for it with
    /// [`CpuFragmentWaitUntilSignalIsNonZero`](crate::cpu_fragment::CpuFragmentWaitUntilSignalIsNonZero).
    /// [`PreUpdate`](crate::spawn_system::SpawnSystem::PreUpdate) and [`DespawnObject`](crate::spawn_system::SpawnSystem::DespawnObject) require that job to
    /// have finished; their internal locks do not establish worker completion.
    pub m_ThreadSignal: u32,
    _field_14c: [u8; 28],
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
    pub const PreUpdate_ADDRESS: usize = 0x140ECBF40;
    /// Services deferred despawns, queued cancellations, and pending spawn requests, in that order.
    /// The resource and despawn-ranking job must have finished before entry; see
    /// [`m_ThreadSignal`](crate::spawn_system::SpawnSystem::m_ThreadSignal). The request and despawn locks do not wait for that
    /// job.
    ///
    /// First releases the strong references queued for despawn. Then removes every pending request
    /// matching both fields of each [`CancelSpawn`](crate::spawn_system::SpawnSystem::CancelSpawn) record, destroys its factory,
    /// and clears the cancellation records without invoking callbacks. Finally services the remaining
    /// ready factories under the request lock. A successful request invokes its
    /// [`SpawnCallback`](crate::spawn_system::SpawnCallback) inline on the caller, destroys the factory, and removes the request.
    /// An invalid factory is removed without a callback; an unsuccessful spawn attempt can remain
    /// pending without invoking one.
    ///
    /// The run, load, and frontend state updates call this function. [`ResetSystem`](crate::spawn_system::SpawnSystem::ResetSystem)
    /// also calls it and can deliver callbacks while resetting. Callback delivery does not run in
    /// the separate ranking job in [`Update`](crate::spawn_system::SpawnSystem::Update).
    pub unsafe fn PreUpdate(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::PreUpdate_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
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
    /// request creates a spawn factory and streams the selected definition's exported entity in if
    /// needed. [`PreUpdate`](crate::spawn_system::SpawnSystem::PreUpdate) services it later and invokes the callback inline on
    /// its caller on success, with the borrowed objects and the given userdata. The transform is
    /// the world-space placement for the spawned object. `flags` is a combination of the
    /// `EFactoryFlags` constants ([`FF_DEFAULT`](crate::spawn_system::SpawnSystem::FF_DEFAULT) and friends) controlling the
    /// spawn checks and ownership. Unless [`FF_NO_SYSTEM_LOCK`](crate::spawn_system::SpawnSystem::FF_NO_SYSTEM_LOCK) is set,
    /// the resource definition retains the complete spawned group independently of the callback's
    /// references. Releasing the callback's references does not remove that persistent ownership.
    /// The callback may be null when the caller does not need the spawned objects.
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
    /// request lock and serviced by [`PreUpdate`](crate::spawn_system::SpawnSystem::PreUpdate), as for [`Spawn`](crate::spawn_system::SpawnSystem::Spawn).
    /// The request copies the transform, callback, userdata, and network identifier; it does not
    /// retain the transform pointer or take ownership of userdata. If factory creation fails, it
    /// returns without queueing a request or invoking the callback. A queued request can also be
    /// removed without a callback if its factory becomes invalid.
    ///
    /// On success PreUpdate invokes `callback` inline on its caller with the borrowed spawned group
    /// and `userdata`. The group has the persistent ownership described by [`Spawn`](crate::spawn_system::SpawnSystem::Spawn).
    /// The inventory's respawn of a slot passes
    /// [`FF_HIGH_PRIO_SPAWN_CHECKS`](crate::spawn_system::SpawnSystem::FF_HIGH_PRIO_SPAWN_CHECKS) for the player and
    /// [`FF_SECONDARY_SPAWN_CHECKS`](crate::spawn_system::SpawnSystem::FF_SECONDARY_SPAWN_CHECKS) otherwise, the character's
    /// transform, and an all-bits-set network identifier.
    pub unsafe fn SpawnTagIndex(
        &mut self,
        tag_index: i32,
        transform: *const crate::types::math::Matrix4,
        flags: u32,
        callback: *const u8,
        userdata: *mut u8,
        network_id: u32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                tag_index: i32,
                transform: *const crate::types::math::Matrix4,
                flags: u32,
                callback: *const u8,
                userdata: *mut u8,
                network_id: u32,
            ) = ::std::mem::transmute(Self::SpawnTagIndex_ADDRESS);
            f(
                self as *mut Self as _,
                tag_index,
                transform,
                flags,
                callback,
                userdata,
                network_id,
            )
        }
    }
    pub const CancelSpawn_ADDRESS: usize = 0x140EA3720;
    /// Queues a cancellation record containing `callback` and `userdata` under the request lock.
    /// It neither invokes the callback nor dereferences or releases userdata. Returning only
    /// confirms that the cancellation record is queued; it does not wait for request service or
    /// worker completion.
    ///
    /// [`PreUpdate`](crate::spawn_system::SpawnSystem::PreUpdate) removes all pending requests matching both values before
    /// servicing remaining requests. It destroys their factories without invoking callbacks.
    /// Once that PreUpdate has processed the record and returned, no matching request remains,
    /// provided no new request has reused the pair. A callback delivered before cancellation is
    /// unaffected, as are the objects it returned.
    pub unsafe fn CancelSpawn(&mut self, callback: *const u8, userdata: *mut u8) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                callback: *const u8,
                userdata: *mut u8,
            ) = ::std::mem::transmute(Self::CancelSpawn_ADDRESS);
            f(self as *mut Self as _, callback, userdata)
        }
    }
    pub const DespawnObject_ADDRESS: usize = 0x140EFFD40;
    /// Queues the complete spawn group containing `object` for deferred despawn. The caller must
    /// keep the object alive with a strong reference through the call, and the resource and
    /// despawn-ranking job must have finished; see [`m_ThreadSignal`](crate::spawn_system::SpawnSystem::m_ThreadSignal).
    /// Internal locks do not wait for that job.
    ///
    /// This is the raw-object overload of `CSpawnSystem::Despawn`. It takes a temporary weak
    /// reference and passes it by value to the weak-pointer overload. When the object belongs to
    /// a persistent spawned group, the system moves the whole group's strong references from the
    /// resource definition into the deferred despawn list and reduces the spawn and budget counts.
    /// [`PreUpdate`](crate::spawn_system::SpawnSystem::PreUpdate) later releases those deferred references. Other strong
    /// references can keep the objects alive, and final strong-reference disposal can itself queue
    /// engine deletion.
    ///
    /// Returns the number of objects queued, not whether destruction has completed. A zero result
    /// can mean that the object is protected, has no matching resource, or is no longer in the
    /// persistent list. `force` bypasses the critical-object check, but does not bypass missing
    /// membership. Objects spawned with [`FF_NO_SYSTEM_LOCK`](crate::spawn_system::SpawnSystem::FF_NO_SYSTEM_LOCK) are not in
    /// that list.
    pub unsafe fn DespawnObject(
        &mut self,
        object: *mut crate::game::GameObject,
        force: bool,
    ) -> i32 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                object: *mut crate::game::GameObject,
                force: bool,
            ) -> i32 = ::std::mem::transmute(Self::DespawnObject_ADDRESS);
            f(self as *mut Self as _, object, force)
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
    /// Resets the system: waits for its spawn job through [`m_ThreadSignal`](crate::spawn_system::SpawnSystem::m_ThreadSignal),
    /// sends `spawn.reset_all_spawn_rules` at once, and calls [`PreUpdate`](crate::spawn_system::SpawnSystem::PreUpdate). This
    /// service call can invoke successful pending spawn callbacks inline while the reset is in
    /// progress. It then destroys every remaining pending spawn request and its factory, releases
    /// the system's references to every spawned object of every resource definition and every
    /// object queued for despawn, and posts `spawn.post_reset_all_spawn_rules`, which its receivers
    /// handle later. An object that something else still holds a strong reference to outlives the
    /// reset, outside the system.
    /// The load state's reset step
    /// ([`GameStateLoad::ResetSystems`](crate::game::GameStateLoad::ResetSystems)) and the frontend's
    /// ([`GameStateFrontend::ResetSystems`](crate::game::GameStateFrontend::ResetSystems)) call it, so a
    /// death, a mission failure or checkpoint restart, a fast travel, a challenge's start, retry,
    /// restart, or abort, a load, and the quit to the menu all reset.
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
    /// `EFactoryFlags`: skip retaining the spawned group in the resource definition's persistent
    /// spawned-object list and skip its spawn and budget accounting. The callback output still
    /// receives the group. [`DespawnObject`](crate::spawn_system::SpawnSystem::DespawnObject) cannot find objects omitted from
    /// that persistent list. This flag does not remove the request lock used for queueing and
    /// callback delivery.
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
