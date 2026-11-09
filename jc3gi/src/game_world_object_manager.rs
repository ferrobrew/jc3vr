#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The game world object manager: owns the character occlusion BFBC state and drives the per-frame
/// character occlusion cull. Each frame, [`StartCommitAddRemove`](crate::game_world_object_manager::GameWorldObjectManager::StartCommitAddRemove)
/// copies the camera manager's active camera into [`m_OcclusionCamera`](crate::game_world_object_manager::GameWorldObjectManager::m_OcclusionCamera)
/// and dispatches [`ProcessCharacterOcclusion`](crate::game_world_object_manager::GameWorldObjectManager::ProcessCharacterOcclusion) via a
/// CPU fragment. That function builds a BFBC cull frustum from the active camera via
/// [`GetBFBCFrustumParamsForCameraAndTime`](crate::graphics_engine::graphics_engine::OccluderCollectionManager::GetBFBCFrustumParamsForCameraAndTime),
/// then runs `BFBCProcess` with `E_BFBC_PROCESSFUNCTION_FRUSTUM_CULL` to cull character occlusion —
/// hiding characters the frustum rejects. The frustum is built from the single active camera's
/// projection.
pub struct GameWorldObjectManager {
    /// The recursive critical section that guards [`m_RemoveQueue`](crate::game_world_object_manager::GameWorldObjectManager::m_RemoveQueue).
    pub m_RemoveQueueLock: *mut ::std::ffi::c_void,
    /// Objects waiting for delayed removal, each with its countdown.
    pub m_RemoveQueue: crate::types::std_vector::Vector<
        crate::game_world_object_manager::PendingRemove,
    >,
    _field_28: [u8; 2512],
    /// The camera the character occlusion cull builds its BFBC frustum against. Set each frame by
    /// [`StartCommitAddRemove`](crate::game_world_object_manager::GameWorldObjectManager::StartCommitAddRemove) from
    /// [`CameraManager::m_ActiveCamera`](crate::camera::camera_manager::CameraManager::m_ActiveCamera).
    pub m_OcclusionCamera: *const crate::camera::camera::Camera,
    _field_a00: [u8; 65680],
    /// The BFBC cull context for character occlusion.
    pub m_CharacterBFBCContext: *mut crate::spawn_system::SBFBCContext,
}
fn _GameWorldObjectManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10A98], GameWorldObjectManager>([0u8; 0x10A98]);
    }
    unreachable!()
}
impl GameWorldObjectManager {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5418086696usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl GameWorldObjectManager {
    pub const UpdateRemoveQueue_ADDRESS: usize = 0x1409FD0F0;
    /// Decrements every pending object's countdown by `dt`, fades an object through its last
    /// second, and compacts out the entries whose countdown reached zero. The caller holds
    /// [`m_RemoveQueueLock`](crate::game_world_object_manager::GameWorldObjectManager::m_RemoveQueueLock).
    pub unsafe fn UpdateRemoveQueue(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::UpdateRemoveQueue_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const StartCommitAddRemove_ADDRESS: usize = 0x1404BB8B0;
    /// Copies the camera manager's active camera into
    /// [`m_OcclusionCamera`](crate::game_world_object_manager::GameWorldObjectManager::m_OcclusionCamera), along with its frustum planes
    /// and transform, then dispatches
    /// [`ProcessCharacterOcclusion`](crate::game_world_object_manager::GameWorldObjectManager::ProcessCharacterOcclusion) via a CPU
    /// fragment (`CallProxy`).
    pub unsafe fn StartCommitAddRemove(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::StartCommitAddRemove_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const ProcessCharacterOcclusion_ADDRESS: usize = 0x1404BB7E0;
    /// Builds a BFBC cull frustum from [`m_OcclusionCamera`](crate::game_world_object_manager::GameWorldObjectManager::m_OcclusionCamera)
    /// via [`GetBFBCFrustumParamsForCameraAndTime`](crate::graphics_engine::graphics_engine::OccluderCollectionManager::GetBFBCFrustumParamsForCameraAndTime),
    /// then runs `BFBCProcess` with `E_BFBC_PROCESSFUNCTION_FRUSTUM_CULL` to cull character occlusion.
    /// Dispatched asynchronously from
    /// [`StartCommitAddRemove`](crate::game_world_object_manager::GameWorldObjectManager::StartCommitAddRemove).
    pub unsafe fn ProcessCharacterOcclusion(&mut self) -> u64 {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) -> u64 = ::std::mem::transmute(
                Self::ProcessCharacterOcclusion_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<GameWorldObjectManager> for GameWorldObjectManager {
    fn as_ref(&self) -> &GameWorldObjectManager {
        self
    }
}
impl std::convert::AsMut<GameWorldObjectManager> for GameWorldObjectManager {
    fn as_mut(&mut self) -> &mut GameWorldObjectManager {
        self
    }
}
#[repr(C, align(8))]
/// One object in [`GameWorldObjectManager::m_RemoveQueue`](crate::game_world_object_manager::GameWorldObjectManager::m_RemoveQueue): the object, its remaining countdown,
/// and whether it fades out over its last second.
pub struct PendingRemove {
    /// The object scheduled for removal.
    pub m_Object: crate::types::shared_ptr::SharedPtr<crate::game::GameObject>,
    /// Seconds left before the entry is compacted out of the queue, seconds.
    pub m_TimeLeft: f32,
    /// Whether the object fades out while its countdown is at most one second.
    pub m_Fade: bool,
    _field_15: [u8; 3],
}
fn _PendingRemove_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], PendingRemove>([0u8; 0x18]);
    }
    unreachable!()
}
impl PendingRemove {}
impl std::convert::AsRef<PendingRemove> for PendingRemove {
    fn as_ref(&self) -> &PendingRemove {
        self
    }
}
impl std::convert::AsMut<PendingRemove> for PendingRemove {
    fn as_mut(&mut self) -> &mut PendingRemove {
        self
    }
}
