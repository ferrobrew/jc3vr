#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(4))]
/// One aim effector of an arm aim IK task (`SAimEffectorProperties`, shared in shape by
/// `NRightArmAimIK` and `NReelInAimIK`). The task's `Update` fills the bone index, bone position, and
/// gain each frame; `UpdateAimEffector` writes the aim direction and rotation axis it derives.
pub struct AimEffectorProperties {
    /// The skeleton bone index the effector rotates (resolved from a safe-bone hash).
    pub m_BoneIndex: i32,
    /// The bone's position, in character-model space, taken from the pre-additive pose.
    pub m_BonePosition: crate::types::math::Vector3,
    /// The effector's rotation-reach weight: the task's aim weight times the per-effector gain.
    /// `UpdateAimEffector` writes it to [`HumanIK::m_TargetReachR`](crate::animation::ik::HumanIK::m_TargetReachR) for the effector.
    pub m_Gain: f32,
    /// The normalized direction from the bone to the aim target (model space).
    pub m_AimDirection: crate::types::math::Vector3,
    /// The rotation axis turning the reference axis onto [`m_AimDirection`](crate::animation::arm_aim_ik::AimEffectorProperties::m_AimDirection).
    pub m_RotationAxis: crate::types::math::Vector3,
}
fn _AimEffectorProperties_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x2C], AimEffectorProperties>([0u8; 0x2C]);
    }
    unreachable!()
}
impl AimEffectorProperties {}
impl std::convert::AsRef<AimEffectorProperties> for AimEffectorProperties {
    fn as_ref(&self) -> &AimEffectorProperties {
        self
    }
}
impl std::convert::AsMut<AimEffectorProperties> for AimEffectorProperties {
    fn as_mut(&mut self) -> &mut AimEffectorProperties {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
/// The aim effector index of an arm aim IK task (`NRightArmAimIK::EAimEffectorTypes`, which
/// `NReelInAimIK::EAimEffectorTypes` aliases). [`RightArmAimIK::Update`](crate::animation::arm_aim_ik::RightArmAimIK::Update) fills the effectors in
/// this order.
pub enum AimEffectorType {
    AET_RIGHT_ARM = 0isize as _,
    AET_RIGHT_HAND = 1isize as _,
    AET_HEAD = 2isize as _,
    AET_NOF_TYPES = 3isize as _,
}
fn _AimEffectorType_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], AimEffectorType>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// Shared helpers of the aim IK tasks (`NAimIKUtil`).
pub struct AimIKUtil {}
impl AimIKUtil {
    pub const GetIKWeightValue_ADDRESS: usize = 0x140802BA0;
    /// The aim IK weight envelope for the character, in `0..1`: the aim segment's local time (or
    /// `1` outside it, or when `force_full` is set), optionally overridden from the blackboard,
    /// minus the local time of the aim fade-out segment.
    pub unsafe fn GetIKWeightValue(
        character: *mut crate::character::character::Character,
        force_full: bool,
    ) -> f32 {
        unsafe {
            let f: unsafe extern "system" fn(
                character: *mut crate::character::character::Character,
                force_full: bool,
            ) -> f32 = ::std::mem::transmute(Self::GetIKWeightValue_ADDRESS);
            f(character, force_full)
        }
    }
}
impl std::convert::AsRef<AimIKUtil> for AimIKUtil {
    fn as_ref(&self) -> &AimIKUtil {
        self
    }
}
impl std::convert::AsMut<AimIKUtil> for AimIKUtil {
    fn as_mut(&mut self) -> &mut AimIKUtil {
        self
    }
}
#[repr(C, align(8))]
/// The grapple reel-in aim IK task (`NReelInAimIK`). A player-only animation state task, active in
/// the grapple reel-in state, that rotates the right arm toward the aim target and swings the chest
/// toward the grappling hook. Its per-instance properties are [`ReelInAimIKInstanceProperties`](crate::animation::arm_aim_ik::ReelInAimIKInstanceProperties).
pub struct ReelInAimIK {}
impl ReelInAimIK {
    pub const Update_ADDRESS: usize = 0x140841D70;
    /// The task's per-frame update. Gated like [`RightArmAimIK::Update`](crate::animation::arm_aim_ik::RightArmAimIK::Update) on the player and the
    /// [`AIM_IK_WEIGHT_ID`](crate::blackboard::ObjectBlackboard::AIM_IK_WEIGHT_ID) weight: aims the `RIGHT_ARM`
    /// effector at the player aim control's weapon aim point (weighted by
    /// [`AimIKUtil::GetIKWeightValue`](crate::animation::arm_aim_ik::AimIKUtil::GetIKWeightValue)), then measures the signed horizontal angle, about the `SPINE`
    /// bone, between that point and a position stored on the grappling hook, and past a threshold
    /// queues a chest-end rotation of up to
    /// [`MAX_CHEST_ROTATION_DEGREES`](crate::animation::arm_aim_ik::ReelInAimIK::MAX_CHEST_ROTATION_DEGREES), faded by the reel
    /// segment's local time.
    pub unsafe fn Update(
        ctx: *mut crate::state::StateContext,
        p1: *mut ::std::ffi::c_void,
        props: *mut crate::animation::arm_aim_ik::ReelInAimIKInstanceProperties,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                ctx: *mut crate::state::StateContext,
                p1: *mut ::std::ffi::c_void,
                props: *mut crate::animation::arm_aim_ik::ReelInAimIKInstanceProperties,
            ) = ::std::mem::transmute(Self::Update_ADDRESS);
            f(ctx, p1, props)
        }
    }
    pub const UpdateAimEffector_ADDRESS: usize = 0x140838A80;
    /// The reel-in variant of [`RightArmAimIK::UpdateAimEffector`](crate::animation::arm_aim_ik::RightArmAimIK::UpdateAimEffector), additionally given the
    /// character.
    pub unsafe fn UpdateAimEffector(
        hik: *mut crate::animation::ik::HumanIK,
        target: *const crate::types::math::Vector3,
        pole: *const crate::types::math::Vector3,
        props: *mut crate::animation::arm_aim_ik::AimEffectorProperties,
        character: *mut crate::character::character::Character,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                hik: *mut crate::animation::ik::HumanIK,
                target: *const crate::types::math::Vector3,
                pole: *const crate::types::math::Vector3,
                props: *mut crate::animation::arm_aim_ik::AimEffectorProperties,
                character: *mut crate::character::character::Character,
            ) = ::std::mem::transmute(Self::UpdateAimEffector_ADDRESS);
            f(hik, target, pole, props, character)
        }
    }
}
impl ReelInAimIK {
    /// The largest chest swing, in degrees, [`Update`](crate::animation::arm_aim_ik::ReelInAimIK::Update) applies toward the hook.
    pub const MAX_CHEST_ROTATION_DEGREES: f32 = 45.0;
}
impl std::convert::AsRef<ReelInAimIK> for ReelInAimIK {
    fn as_ref(&self) -> &ReelInAimIK {
        self
    }
}
impl std::convert::AsMut<ReelInAimIK> for ReelInAimIK {
    fn as_mut(&mut self) -> &mut ReelInAimIK {
        self
    }
}
#[repr(C, align(4))]
/// The instance properties of [`ReelInAimIK`](crate::animation::arm_aim_ik::ReelInAimIK) (`NReelInAimIK::SInstanceProperties`).
pub struct ReelInAimIKInstanceProperties {
    pub m_PoleVector: crate::types::math::Vector3,
    pub m_AimEffectors: [crate::animation::arm_aim_ik::AimEffectorProperties; 1],
}
fn _ReelInAimIKInstanceProperties_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x38], ReelInAimIKInstanceProperties>([0u8; 0x38]);
    }
    unreachable!()
}
impl ReelInAimIKInstanceProperties {}
impl std::convert::AsRef<ReelInAimIKInstanceProperties>
for ReelInAimIKInstanceProperties {
    fn as_ref(&self) -> &ReelInAimIKInstanceProperties {
        self
    }
}
impl std::convert::AsMut<ReelInAimIKInstanceProperties>
for ReelInAimIKInstanceProperties {
    fn as_mut(&mut self) -> &mut ReelInAimIKInstanceProperties {
        self
    }
}
#[repr(C, align(8))]
/// The right-arm (gun arm) aim IK task (`NRightArmAimIK`). A player-only animation state task,
/// active in the weapon-aim states, that rotates the right arm, the right hand, and the head toward
/// the aim target with rotation effectors on [`Pass::MAIN`](crate::animation::ik::Pass::MAIN) at
/// [`SolveStep::UPPER_BODY`](crate::animation::ik::SolveStep::UPPER_BODY). Its per-instance properties are
/// [`RightArmAimIKInstanceProperties`](crate::animation::arm_aim_ik::RightArmAimIKInstanceProperties).
pub struct RightArmAimIK {}
impl RightArmAimIK {
    pub const Update_ADDRESS: usize = 0x140845B10;
    /// The task's per-frame update. Runs only for the player and while the
    /// [`AIM_IK_WEIGHT_ID`](crate::blackboard::ObjectBlackboard::AIM_IK_WEIGHT_ID) weight is non-zero: brings the
    /// player aim control's weapon aim point into model space (the inverse of
    /// [`Character::m_WorldMatrixT1`](crate::character::character::Character::m_WorldMatrixT1)), fills the three effectors (`RIGHT_ARM`, `RIGHT_HAND`, and
    /// `HEAD`, weighted by [`RightArmAimIK_ArmGain`](get_RightArmAimIK_ArmGain), [`RightArmAimIK_HandGain`](get_RightArmAimIK_HandGain), and
    /// [`RightArmAimIK_HeadGain`](get_RightArmAimIK_HeadGain)), and calls [`UpdateAimEffector`](crate::animation::arm_aim_ik::RightArmAimIK::UpdateAimEffector) for
    /// each. The pole vector is the negated forward axis of the `ATTACH_HAND_RIGHT` bone.
    pub unsafe fn Update(
        ctx: *mut crate::state::StateContext,
        p1: *mut ::std::ffi::c_void,
        props: *mut crate::animation::arm_aim_ik::RightArmAimIKInstanceProperties,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                ctx: *mut crate::state::StateContext,
                p1: *mut ::std::ffi::c_void,
                props: *mut crate::animation::arm_aim_ik::RightArmAimIKInstanceProperties,
            ) = ::std::mem::transmute(Self::Update_ADDRESS);
            f(ctx, p1, props)
        }
    }
    pub const UpdateAimEffector_ADDRESS: usize = 0x140838EC0;
    /// Turns `props`'s bone axis toward `target`: computes the direction and the rotation axis
    /// against `pole`, queues a rotation target on the bone's effector
    /// ([`HumanIK::AddEffectorTargetRotationVector`](crate::animation::ik::HumanIK::AddEffectorTargetRotationVector), `UPPER_BODY`, `MAIN`, no interpolation or
    /// blend-out), and writes the effector's rotation reach from `m_Gain`. Skips a target within
    /// about 3 mm of the bone or already aligned with the pole.
    pub unsafe fn UpdateAimEffector(
        hik: *mut crate::animation::ik::HumanIK,
        target: *const crate::types::math::Vector3,
        pole: *const crate::types::math::Vector3,
        props: *mut crate::animation::arm_aim_ik::AimEffectorProperties,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                hik: *mut crate::animation::ik::HumanIK,
                target: *const crate::types::math::Vector3,
                pole: *const crate::types::math::Vector3,
                props: *mut crate::animation::arm_aim_ik::AimEffectorProperties,
            ) = ::std::mem::transmute(Self::UpdateAimEffector_ADDRESS);
            f(hik, target, pole, props)
        }
    }
}
impl std::convert::AsRef<RightArmAimIK> for RightArmAimIK {
    fn as_ref(&self) -> &RightArmAimIK {
        self
    }
}
impl std::convert::AsMut<RightArmAimIK> for RightArmAimIK {
    fn as_mut(&mut self) -> &mut RightArmAimIK {
        self
    }
}
#[repr(C, align(4))]
/// The instance properties of [`RightArmAimIK`](crate::animation::arm_aim_ik::RightArmAimIK) (`NRightArmAimIK::SInstanceProperties`).
pub struct RightArmAimIKInstanceProperties {
    pub m_PoleVector: crate::types::math::Vector3,
    /// The effectors, indexed by [`AimEffectorType`](crate::animation::arm_aim_ik::AimEffectorType).
    pub m_AimEffectors: [crate::animation::arm_aim_ik::AimEffectorProperties; 3],
}
fn _RightArmAimIKInstanceProperties_size_check() {
    unsafe {
        ::std::mem::transmute::<
            [u8; 0x90],
            RightArmAimIKInstanceProperties,
        >([0u8; 0x90]);
    }
    unreachable!()
}
impl RightArmAimIKInstanceProperties {}
impl std::convert::AsRef<RightArmAimIKInstanceProperties>
for RightArmAimIKInstanceProperties {
    fn as_ref(&self) -> &RightArmAimIKInstanceProperties {
        self
    }
}
impl std::convert::AsMut<RightArmAimIKInstanceProperties>
for RightArmAimIKInstanceProperties {
    fn as_mut(&mut self) -> &mut RightArmAimIKInstanceProperties {
        self
    }
}
/// The per-effector gain of the [`RightArmAimIK`](crate::animation::arm_aim_ik::RightArmAimIK) `RIGHT_ARM` effector (initially `0.75`).
pub unsafe fn get_RightArmAimIK_ArmGain() -> &'static mut f32 {
    unsafe { &mut *(0x142D66078 as *mut f32) }
}
/// The per-effector gain of the [`RightArmAimIK`](crate::animation::arm_aim_ik::RightArmAimIK) `RIGHT_HAND` effector (initially `0.6`).
pub unsafe fn get_RightArmAimIK_HandGain() -> &'static mut f32 {
    unsafe { &mut *(0x142D6607C as *mut f32) }
}
/// The per-effector gain of the [`RightArmAimIK`](crate::animation::arm_aim_ik::RightArmAimIK) `HEAD` effector (initially `0.15`).
pub unsafe fn get_RightArmAimIK_HeadGain() -> &'static mut f32 {
    unsafe { &mut *(0x142D66080 as *mut f32) }
}
pub const NAimIKTask_Update_ADDRESS: usize = 0x1408076D0;
/// The per-frame update of the aim IK task (`NAimIKTask::Update`): the animation state task that
/// orients the upper body toward the aim target while aiming a weapon or the grapple, queuing axis
/// rotation targets (the chest end among them) weighted by [`AimIKUtil::GetIKWeightValue`](crate::animation::arm_aim_ik::AimIKUtil::GetIKWeightValue).
pub unsafe fn NAimIKTask_Update(
    ctx: *mut crate::state::StateContext,
    p1: *mut ::std::ffi::c_void,
    p2: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::state::StateContext,
            p1: *mut ::std::ffi::c_void,
            p2: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(NAimIKTask_Update_ADDRESS);
        f(ctx, p1, p2)
    }
}
pub const NGrapplingFireArmIK_Update_ADDRESS: usize = 0x140814C80;
/// The per-frame update of the grapple-fire arm IK task (`NGrapplingFireArmIK::Update`): poses the
/// left arm toward the grapple target: reads the hook's last target position against the left
/// shoulder and hand and publishes the result through the character blackboard.
pub unsafe fn NGrapplingFireArmIK_Update(
    ctx: *mut crate::state::StateContext,
    p1: *mut ::std::ffi::c_void,
    p2: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::state::StateContext,
            p1: *mut ::std::ffi::c_void,
            p2: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(NGrapplingFireArmIK_Update_ADDRESS);
        f(ctx, p1, p2)
    }
}
pub const NLookAtIKTask_Update_ADDRESS: usize = 0x140827B00;
/// The per-frame update of the look-at IK task (`NLookAtIKTask::Update`).
pub unsafe fn NLookAtIKTask_Update(
    ctx: *mut crate::state::StateContext,
    p1: *mut ::std::ffi::c_void,
    p2: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::state::StateContext,
            p1: *mut ::std::ffi::c_void,
            p2: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(NLookAtIKTask_Update_ADDRESS);
        f(ctx, p1, p2)
    }
}
pub const NLocoIKTask_Update_ADDRESS: usize = 0x140826480;
/// The per-frame update of the locomotion IK task (`NLocoIKTask::Update`), which queues positional
/// effector targets through [`HumanIK::AddEffectorTargetPosition`](crate::animation::ik::HumanIK::AddEffectorTargetPosition).
pub unsafe fn NLocoIKTask_Update(
    ctx: *mut crate::state::StateContext,
    p1: *mut ::std::ffi::c_void,
    p2: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::state::StateContext,
            p1: *mut ::std::ffi::c_void,
            p2: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(NLocoIKTask_Update_ADDRESS);
        f(ctx, p1, p2)
    }
}
pub const NWeaponOffhandIKTask_Update_ADDRESS: usize = 0x1408516B0;
/// The per-frame update of the weapon off-hand IK task (`NWeaponOffhandIKTask::Update`). Registered
/// in [`Character::FINALIZE_POSE_STATE_TASK_RANGE`](crate::character::character::Character::FINALIZE_POSE_STATE_TASK_RANGE), so
/// it runs inside [`Character::UpdatePassFinalizePose_Parallel`](crate::character::character::Character::UpdatePassFinalizePose_Parallel) after the pass's entry and before
/// the IK solve. Unless
/// [`StateBitFlag::SB_DISABLE_WEAPON_OFFHAND_IK`](crate::state::StateBitFlag::SB_DISABLE_WEAPON_OFFHAND_IK)
/// is set, the task's segment (when it names one) is not playing on the base layer, or the
/// character's flags rule it out, it sets
/// [`StateBitFlag::SB_WEAPON_OFFHAND_IK_LEFT`](crate::state::StateBitFlag::SB_WEAPON_OFFHAND_IK_LEFT) or
/// [`StateBitFlag::SB_WEAPON_OFFHAND_IK_RIGHT`](crate::state::StateBitFlag::SB_WEAPON_OFFHAND_IK_RIGHT)
/// by the task's hand property and writes the one-frame
/// [`OFFHAND_IK_HAND_ID`](crate::blackboard::ObjectBlackboard::OFFHAND_IK_HAND_ID) and
/// [`OFFHAND_IK_WEIGHT_ID`](crate::blackboard::ObjectBlackboard::OFFHAND_IK_WEIGHT_ID) keys, the weight
/// ramping up from the state's entry when the task asks for it. Otherwise it clears both hand bits.
/// [`Character::UpdateSecondaryHandIKPass`](crate::character::character::Character::UpdateSecondaryHandIKPass) reads the keys.
pub unsafe fn NWeaponOffhandIKTask_Update(
    ctx: *mut crate::state::StateContext,
    p1: *mut ::std::ffi::c_void,
    p2: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::state::StateContext,
            p1: *mut ::std::ffi::c_void,
            p2: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(NWeaponOffhandIKTask_Update_ADDRESS);
        f(ctx, p1, p2)
    }
}
