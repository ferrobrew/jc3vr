#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A keyed value store attached to game objects (`CObjectBlackboard`); the character's instance
/// lives at [`Character::m_Blackboard`](crate::character::character::Character::m_Blackboard) and carries
/// the locomotion values (the camera-relative move direction, the target face direction, the up
/// direction). Values are read and written by id through the template accessors below, under
/// [`m_BbLock`](crate::blackboard::ObjectBlackboard::m_BbLock).
pub struct ObjectBlackboard {
    /// The key table, [`m_KeyInfoCapacity`](crate::blackboard::ObjectBlackboard::m_KeyInfoCapacity) slots; keys are
    /// assigned as they are first written.
    pub m_KeyInfos: *mut crate::blackboard::ObjectBlackboardKeyInfo,
    pub m_KeyInfoCapacity: u16,
    _field_a: [u8; 6],
    /// The value arena; each entry lives at its key's
    /// [`m_DataOffset`](crate::blackboard::ObjectBlackboardKeyInfo::m_DataOffset).
    pub m_Data: *mut u8,
    pub m_DataCapacity: u16,
    /// The arena's bump-allocation cursor.
    pub m_CurrentOffset: u16,
    _field_1c: [u8; 4],
    /// The lock guarding every accessor (`SThreadMutex`, a one-pointer wrapper; the accessors
    /// enter the critical section directly).
    pub m_BbLock: *mut crate::graphics_engine::device::CRITICAL_SECTION,
    /// The dev dump cursor ("first line to dump"); unreferenced by the game code.
    pub m_FirstLineToDump: i32,
    _field_2c: [u8; 4],
}
fn _ObjectBlackboard_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x30], ObjectBlackboard>([0u8; 0x30]);
    }
    unreachable!()
}
impl ObjectBlackboard {
    pub const GetVector3_ADDRESS: usize = 0x1405C5AC0;
    /// The `GetBlackboardValue<CVector3f>` template instantiation. Returns whether the id was
    /// present; the value is written through `value`.
    pub unsafe fn GetVector3(
        &mut self,
        id: u32,
        value: *mut crate::types::math::Vector3,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                id: u32,
                value: *mut crate::types::math::Vector3,
            ) -> bool = ::std::mem::transmute(Self::GetVector3_ADDRESS);
            f(self as *mut Self as _, id, value)
        }
    }
    pub const GetFloat_ADDRESS: usize = 0x1405C5BA0;
    /// The `GetBlackboardValue<float>` template instantiation.
    pub unsafe fn GetFloat(&mut self, id: u32, value: *mut f32) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                id: u32,
                value: *mut f32,
            ) -> bool = ::std::mem::transmute(Self::GetFloat_ADDRESS);
            f(self as *mut Self as _, id, value)
        }
    }
    pub const GetInt_ADDRESS: usize = 0x1405C57E0;
    /// The `GetBlackboardValue<int>` template instantiation. Returns whether the id was present
    /// with an integer type; the value is written through `value`.
    pub unsafe fn GetInt(&mut self, id: u32, value: *mut i32) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                id: u32,
                value: *mut i32,
            ) -> bool = ::std::mem::transmute(Self::GetInt_ADDRESS);
            f(self as *mut Self as _, id, value)
        }
    }
    pub const SetFloat_ADDRESS: usize = 0x1405C6850;
    /// The `SetBlackboardValue<float>` template instantiation. Overwrites the value when the id is
    /// present, and otherwise creates the key with `persistence`; returns false only when the key
    /// table or the value arena is full.
    pub unsafe fn SetFloat(
        &mut self,
        id: u32,
        value: f32,
        persistence: crate::blackboard::PersistenceType,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                id: u32,
                value: f32,
                persistence: crate::blackboard::PersistenceType,
            ) -> bool = ::std::mem::transmute(Self::SetFloat_ADDRESS);
            f(self as *mut Self as _, id, value, persistence)
        }
    }
    pub const SetVector3_ADDRESS: usize = 0x1405C6A70;
    /// The `SetBlackboardValue<CVector3f>` template instantiation. `value` is by-value `CVector3f`
    /// at the C++ level, which the MSVC x64 ABI passes by reference — hence the pointer here.
    /// `persistence` is recorded when the call creates the key; it varies by caller rather than
    /// being fixed by the function — most writers pass
    /// [`PERSISTENCE_LOCAL_FRAME`](crate::blackboard::PersistenceType::PERSISTENCE_LOCAL_FRAME), some AI blackboard
    /// writes pass [`PERSISTENCE_GLOBAL_FRAME`](crate::blackboard::PersistenceType::PERSISTENCE_GLOBAL_FRAME), and the
    /// parachute handle IK task's `SECONDARY`-pass position keys
    /// ([`SECONDARY_HAND_POSITION_WS_LEFT_ID`](crate::blackboard::ObjectBlackboard::SECONDARY_HAND_POSITION_WS_LEFT_ID) and its
    /// right twin) are written with
    /// [`PERSISTENCE_SESSION`](crate::blackboard::PersistenceType::PERSISTENCE_SESSION) instead (see
    /// [`NParachuteHandleIKTask::Update`](crate::input::parachute::NParachuteHandleIKTask::Update)); `name`
    /// is an optional debug label, null in every observed call.
    pub unsafe fn SetVector3(
        &mut self,
        id: u32,
        value: *const crate::types::math::Vector3,
        persistence: crate::blackboard::PersistenceType,
        name: *const u8,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                id: u32,
                value: *const crate::types::math::Vector3,
                persistence: crate::blackboard::PersistenceType,
                name: *const u8,
            ) -> bool = ::std::mem::transmute(Self::SetVector3_ADDRESS);
            f(self as *mut Self as _, id, value, persistence, name)
        }
    }
}
impl ObjectBlackboard {
    /// The id of the float master weight (clamped to 0-1) of the player's per-arm aim IK, read by
    /// `NRightArmAimIK::Update` and `NReelInAimIK::Update` (the IK is skipped while it is zero). The
    /// animation data writes it, not the aim logic.
    pub const AIM_IK_WEIGHT_ID: u32 = 3893557566;
    /// The id of a float marker (written as `1.0` by the stunt set-target-direction task) whose
    /// presence makes `NStateTask_LocoUtil::EvaluateCharacterDisplacement` skip the animation
    /// root-motion branch and apply the [`MOVE_DIR_ID`](crate::blackboard::ObjectBlackboard::MOVE_DIR_ID) direction instantly.
    pub const APPLY_TARGET_DIR_INSTANTLY_ID: u32 = 3896772124;
    /// The id of the second float the input move task reads alongside
    /// [`SPEED_ID`](crate::blackboard::ObjectBlackboard::SPEED_ID); semantics unmapped, a candidate input-strength
    /// signal.
    pub const AUX_FLOAT_ID: u32 = 2217900102;
    /// The id of the constrained-ground movement direction (`CVector3f`), present on slopes (and
    /// other surface-constrained locomotion): when set, the movement task blends its displacement
    /// direction toward it instead of using the primary displacement output.
    pub const CONSTRAINED_DIR_ID: u32 = 2485695409;
    /// The id of the camera-relative world-space move direction (`CVector3f`), written each frame
    /// by `NStateTask_InputLocoSetTargetDirTask::SetupTargetDir`.
    pub const MOVE_DIR_ID: u32 = 2113030792;
    /// The id of the `int` naming the hand the weapon off-hand grip places: nonzero is the right
    /// hand, zero the left.
    pub const OFFHAND_IK_HAND_ID: u32 = 2602427023;
    /// The id of the float weight of the weapon off-hand grip, written by the weapon off-hand IK
    /// task ([`NWeaponOffhandIKTask_Update`](crate::animation::arm_aim_ik::NWeaponOffhandIKTask_Update),
    /// inside the pose-finalization pass, ramping up while its segment is active) and read by
    /// [`Character::UpdateSecondaryHandIKPass`](crate::character::character::Character::UpdateSecondaryHandIKPass),
    /// which places the off-hand on the weapon's grip only while it is positive.
    pub const OFFHAND_IK_WEIGHT_ID: u32 = 4135506754;
    /// The id of the previous target direction (`CVector3f`), written by
    /// `NStateTask_InputLocoSetTargetDirTask::SetupTargetDir` and read by
    /// `NStateTask_LocoUtil::EvaluateCharacterDisplacement` as the code-driven displacement
    /// direction inside its special segments.
    pub const PREVIOUS_TARGET_DIR_ID: u32 = 923417185;
    /// The id of the left wrist's model-space `SECONDARY`-pass target position (`CVector3f`).
    pub const SECONDARY_HAND_POSITION_LS_LEFT_ID: u32 = 2849370640;
    /// The id of the right wrist's model-space `SECONDARY`-pass target position (`CVector3f`).
    pub const SECONDARY_HAND_POSITION_LS_RIGHT_ID: u32 = 1684944817;
    /// The id of the left wrist's world-space `SECONDARY`-pass target position (`CVector3f`).
    pub const SECONDARY_HAND_POSITION_WS_LEFT_ID: u32 = 2514840215;
    /// The id of the right wrist's world-space `SECONDARY`-pass target position (`CVector3f`),
    /// preferred over [`SECONDARY_HAND_POSITION_LS_RIGHT_ID`](crate::blackboard::ObjectBlackboard::SECONDARY_HAND_POSITION_LS_RIGHT_ID)
    /// when present.
    pub const SECONDARY_HAND_POSITION_WS_RIGHT_ID: u32 = 1850587638;
    /// The id of the float weight of the left wrist's `SECONDARY`-pass position target (see
    /// [`SECONDARY_HAND_WEIGHT_RIGHT_ID`](crate::blackboard::ObjectBlackboard::SECONDARY_HAND_WEIGHT_RIGHT_ID)). The grapple
    /// fire-arm IK task and the parachute handle IK task write it.
    pub const SECONDARY_HAND_WEIGHT_LEFT_ID: u32 = 3591310625;
    /// The id of the float weight of the right wrist's `SECONDARY`-pass position target, read by
    /// [`Character::UpdateSecondaryHandIKPass`](crate::character::character::Character::UpdateSecondaryHandIKPass),
    /// which targets the wrist only while it is positive. The parachute handle IK task writes it.
    pub const SECONDARY_HAND_WEIGHT_RIGHT_ID: u32 = 1897716896;
    /// The id of the float speed value the input locomotion tasks branch on (`<= 0` routes into
    /// the stop acts).
    pub const SPEED_ID: u32 = 3396837917;
    /// The id of the float that replaces the result of
    /// [`EvaluateCharacterSpeed`](crate::input::locomotion::NStateTask_LocoUtil::EvaluateCharacterSpeed) when that function is
    /// asked to use the blackboard override and the key is present.
    pub const SPEED_OVERRIDE_ID: u32 = 1707123197;
    /// The id of the float turn angle (degrees) the swim input tasks write when they latch an
    /// animated turn: the XZ angle from the body forward to the desired direction, measured by
    /// `CControllerUtility::GetDeltaAngleFromOrientation` and stored alongside the
    /// `m_AnimatedTurnBaseDir`/`m_AnimatedTurnTargetDir` pair the swim movement actuators slerp
    /// between across the angle-correction animation segment.
    pub const SWIM_TURN_ANGLE_ID: u32 = 2828323117;
    /// The id of the target face direction (`CVector3f`): the desired body facing the orientation
    /// executor yaws toward in its tracking mode. Written per-state by the game's
    /// `SetUpTargetFaceDir` tasks.
    pub const TARGET_FACE_DIR_ID: u32 = 736589998;
}
impl std::convert::AsRef<ObjectBlackboard> for ObjectBlackboard {
    fn as_ref(&self) -> &ObjectBlackboard {
        self
    }
}
impl std::convert::AsMut<ObjectBlackboard> for ObjectBlackboard {
    fn as_mut(&mut self) -> &mut ObjectBlackboard {
        self
    }
}
#[repr(C, align(8))]
/// One key slot in an [`ObjectBlackboard`](crate::blackboard::ObjectBlackboard)'s table (`SObjectBlackboardKeyInfo`): the key hash, the
/// value's type tag and arena offset, and an optional debug label.
pub struct ObjectBlackboardKeyInfo {
    /// The key hash the accessors look up by.
    pub m_KeyHash: u32,
    /// The value's type tag.
    pub m_ValueType: crate::blackboard::ValueType,
    _field_5: [u8; 3],
    /// The persistence class.
    pub m_PersistenceType: crate::blackboard::PersistenceType,
    /// The value's byte offset within [`ObjectBlackboard::m_Data`](crate::blackboard::ObjectBlackboard::m_Data).
    pub m_DataOffset: u16,
    _field_e: [u8; 2],
    /// An optional debug label for the key; null at every call site.
    pub m_DebugName: *const u8,
}
fn _ObjectBlackboardKeyInfo_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], ObjectBlackboardKeyInfo>([0u8; 0x18]);
    }
    unreachable!()
}
impl ObjectBlackboardKeyInfo {}
impl std::convert::AsRef<ObjectBlackboardKeyInfo> for ObjectBlackboardKeyInfo {
    fn as_ref(&self) -> &ObjectBlackboardKeyInfo {
        self
    }
}
impl std::convert::AsMut<ObjectBlackboardKeyInfo> for ObjectBlackboardKeyInfo {
    fn as_mut(&mut self) -> &mut ObjectBlackboardKeyInfo {
        self
    }
}
#[repr(u32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
/// The persistence class of a blackboard key (`SObjectBlackboardKeyInfo::EPersistenceType`),
/// recorded in [`ObjectBlackboardKeyInfo::m_PersistenceType`](crate::blackboard::ObjectBlackboardKeyInfo::m_PersistenceType) when the key is first written.
pub enum PersistenceType {
    PERSISTENCE_SESSION = 0isize as _,
    PERSISTENCE_LOCAL_FRAME = 1isize as _,
    PERSISTENCE_GLOBAL_FRAME = 2isize as _,
}
fn _PersistenceType_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], PersistenceType>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(u8)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
/// The type tag of a blackboard value (`SObjectBlackboardKeyInfo::EValueType`), stored narrow in
/// [`ObjectBlackboardKeyInfo::m_ValueType`](crate::blackboard::ObjectBlackboardKeyInfo::m_ValueType).
pub enum ValueType {
    VALUE_NONE = 0isize as _,
    VALUE_FLOAT = 1isize as _,
    VALUE_VECTOR = 2isize as _,
    VALUE_UINT32 = 3isize as _,
    VALUE_INT32 = 4isize as _,
    VALUE_HASH = 5isize as _,
    VALUE_UINT64 = 6isize as _,
    VALUE_UINT8 = 7isize as _,
    VALUE_NOF_TYPES = 8isize as _,
}
fn _ValueType_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1], ValueType>([0u8; 0x1]);
    }
    unreachable!()
}
pub const RemoveBlackBoardValue_ADDRESS: usize = 0x1404D86D0;
/// Removes a key from a blackboard (`RemoveBlackBoardValue`), under
/// [`ObjectBlackboard::m_BbLock`](crate::blackboard::ObjectBlackboard::m_BbLock). Returns whether the id was present.
pub unsafe fn RemoveBlackBoardValue(
    bb: *mut crate::blackboard::ObjectBlackboard,
    id: u32,
) -> bool {
    unsafe {
        let f: unsafe extern "system" fn(
            bb: *mut crate::blackboard::ObjectBlackboard,
            id: u32,
        ) -> bool = ::std::mem::transmute(RemoveBlackBoardValue_ADDRESS);
        f(bb, id)
    }
}
