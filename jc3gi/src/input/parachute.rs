#![cfg_attr(any(), rustfmt::skip)]
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// `SCharacterAirMovementSettings`: the shared tuning block for the airborne movement families
/// (parachute, wingsuit, and freefall). It covers the per-axis input/look yaw-pitch-roll mixing
/// (`m_XInputYawPitchRollAmount`, `m_XLookYawPitchRollAmount`), the heading-to-velocity springs
/// (`m_RotateCharYawToVelocity`), the camera-relative look steering (`m_LookSteer*`), the
/// slingshot boost (`m_Slingshot*`), and the near-ground lift. The parachute and wingsuit settings
/// embed one of these as their `m_AirControl`.
pub struct CharacterAirMovementSettings {
    pub m_Gravity: f32,
    pub m_MaxVelocity: f32,
    pub m_MaxVelocityXZ: f32,
    pub m_ClampVelocitySpeed: f32,
    pub m_Lift: [f32; 3],
    pub m_DragX: [f32; 3],
    pub m_DragY: [f32; 3],
    pub m_DragZ: [f32; 3],
    pub m_LiftPoint: [f32; 3],
    pub m_PitchLimit: [f32; 2],
    pub m_YawSteerAmount: f32,
    pub m_DrawDebugLines: u32,
    pub m_RotateCharYawToVelocity: crate::input::parachute::CharacterSpring,
    pub m_RotateCharPitchToVelocity: crate::input::parachute::CharacterSpring,
    pub m_PitchInputSpring: crate::input::parachute::CharacterSpring,
    pub m_YawInputSpring: crate::input::parachute::CharacterSpring,
    pub m_RollInputSpring: crate::input::parachute::CharacterSpring,
    pub m_XInputYawPitchRollAmount: [f32; 3],
    pub m_YInputYawPitchRollAmount: [f32; 3],
    pub m_XLookYawPitchRollAmount: [f32; 3],
    pub m_YLookYawPitchRollAmount: [f32; 3],
    pub m_RotateCharYawToSlingshot: crate::input::parachute::CharacterSpring,
    pub m_PersistYawPitchRoll: [f32; 3],
    pub m_CenterYawPitchRoll: [f32; 3],
    pub m_LiftInputSpring: crate::input::parachute::CharacterSpring,
    pub m_DragInputSpring: crate::input::parachute::CharacterSpring,
    pub m_AnimInputSpringX: crate::input::parachute::CharacterSpring,
    pub m_AnimInputSpringY: crate::input::parachute::CharacterSpring,
    pub m_NearGroundLiftDistance: f32,
    pub m_NearGroundMaxLift: f32,
    pub m_NearGroundLiftExponent: f32,
    pub m_NearGroundLiftDecayMax: f32,
    pub m_NearGroundLiftDecayMultiplier: f32,
    pub m_NearGroundLiftRecoverRate: f32,
    pub m_LookSteerDeadZone: [f32; 2],
    pub m_LookSteerMaxSpeed: [f32; 2],
    pub m_LookSteerMaxYaw: f32,
    pub m_LookSteerMaxPitch: [f32; 2],
    pub m_LookSteerExponential: [f32; 2],
    pub m_LookSteerAimingDeadZone: [f32; 2],
    pub m_LookSteerAimingMaxSpeed: [f32; 2],
    pub m_LookSteerAimingMaxYaw: f32,
    pub m_LookSteerAimingMaxPitch: [f32; 2],
    pub m_LookSteerAimingExponential: [f32; 2],
    pub m_SlingshotPitchAdjust: f32,
    pub m_SlingshotExtraLift: f32,
    pub m_SlingshotDirectAccel: f32,
    pub m_SlingshotForwardAccel: f32,
    pub m_SlingshotMaxSpeedAdjust: f32,
    pub m_SlingshotDragXYZAdjust: [f32; 3],
    pub m_SlingshotBreakPitch: f32,
    pub m_SlingshotBreakYawStart: f32,
    pub m_SlingshotBreakYawEnd: f32,
    pub m_SlingshotBreakYawBlendTime: f32,
    pub m_SlingshotBreakDistance: f32,
    pub m_RetractSlingshotSettings: crate::input::parachute::CharacterRetractSShotSettings,
}
fn _CharacterAirMovementSettings_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1C4], CharacterAirMovementSettings>([0u8; 0x1C4]);
    }
    unreachable!()
}
impl CharacterAirMovementSettings {}
impl std::convert::AsRef<CharacterAirMovementSettings> for CharacterAirMovementSettings {
    fn as_ref(&self) -> &CharacterAirMovementSettings {
        self
    }
}
impl std::convert::AsMut<CharacterAirMovementSettings> for CharacterAirMovementSettings {
    fn as_mut(&mut self) -> &mut CharacterAirMovementSettings {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// `SCharacterParachuteSettings`: the per-character parachute movement tuning block, embedded in
/// `SCustomMovementSettings` alongside the other families' settings. The first fields are the
/// angular motion limits and the reel-to-parachute transition; the rest is the shared
/// [`CharacterAirMovementSettings`](crate::input::parachute::CharacterAirMovementSettings) used for the flight dynamics and the camera-relative look
/// steering.
pub struct CharacterParachuteSettings {
    pub m_MaxAngularSpeed: f32,
    pub m_MaxAngularAccel: f32,
    pub m_VelocityAlignMinSpeed: f32,
    pub m_ParachuteToPivotDistance: f32,
    pub m_ReelToParaSettings: crate::input::parachute::CharacterReelToParaSettings,
    pub m_AirControl: crate::input::parachute::CharacterAirMovementSettings,
}
fn _CharacterParachuteSettings_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x228], CharacterParachuteSettings>([0u8; 0x228]);
    }
    unreachable!()
}
impl CharacterParachuteSettings {}
impl std::convert::AsRef<CharacterParachuteSettings> for CharacterParachuteSettings {
    fn as_ref(&self) -> &CharacterParachuteSettings {
        self
    }
}
impl std::convert::AsMut<CharacterParachuteSettings> for CharacterParachuteSettings {
    fn as_mut(&mut self) -> &mut CharacterParachuteSettings {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// `SCharacterReelToParaSettings`: the settings for the reel-in-to-parachute transition.
pub struct CharacterReelToParaSettings {
    pub m_ExtraLiftCutoff: f32,
    pub m_ExtraLift: [f32; 3],
    pub m_ExtraDownwardForce: [f32; 3],
    pub m_StopExtraLiftHeight: f32,
    pub m_DragX: [f32; 3],
    pub m_DragY: [f32; 3],
    pub m_DragZ: [f32; 3],
    pub m_UpwardVelocityClamp: f32,
    pub m_FakeSlingshotSpeedThreshold: f32,
    pub m_FakeSlingshotAccel: f32,
    pub m_FakeSlingshotMinPitch: f32,
}
fn _CharacterReelToParaSettings_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x54], CharacterReelToParaSettings>([0u8; 0x54]);
    }
    unreachable!()
}
impl CharacterReelToParaSettings {}
impl std::convert::AsRef<CharacterReelToParaSettings> for CharacterReelToParaSettings {
    fn as_ref(&self) -> &CharacterReelToParaSettings {
        self
    }
}
impl std::convert::AsMut<CharacterReelToParaSettings> for CharacterReelToParaSettings {
    fn as_mut(&mut self) -> &mut CharacterReelToParaSettings {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// `SCharacterRetractSShotSettings`: the settings for the parachute slingshot boost on reel-in
/// retract.
pub struct CharacterRetractSShotSettings {
    pub m_BlendInSpeed: f32,
    pub m_BlendOutSpeed: f32,
    pub m_MaxSpeedAdjust: f32,
    pub m_DirectAccel: f32,
    pub m_ForwardAccel: f32,
}
fn _CharacterRetractSShotSettings_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x14], CharacterRetractSShotSettings>([0u8; 0x14]);
    }
    unreachable!()
}
impl CharacterRetractSShotSettings {}
impl std::convert::AsRef<CharacterRetractSShotSettings>
for CharacterRetractSShotSettings {
    fn as_ref(&self) -> &CharacterRetractSShotSettings {
        self
    }
}
impl std::convert::AsMut<CharacterRetractSShotSettings>
for CharacterRetractSShotSettings {
    fn as_mut(&mut self) -> &mut CharacterRetractSShotSettings {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// `SCharacterSpring`: the per-axis spring tuning used throughout the air and parachute
/// movement settings. A spring is expressed with `m_Constant` (the stiffness), `m_Damping`, and
/// `m_Speed` (the per-second response); the air movement cores integrate the spring state per
/// frame from these.
pub struct CharacterSpring {
    pub m_Speed: f32,
    pub m_Constant: f32,
    pub m_Damping: f32,
}
fn _CharacterSpring_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xC], CharacterSpring>([0u8; 0xC]);
    }
    unreachable!()
}
impl CharacterSpring {}
impl std::convert::AsRef<CharacterSpring> for CharacterSpring {
    fn as_ref(&self) -> &CharacterSpring {
        self
    }
}
impl std::convert::AsMut<CharacterSpring> for CharacterSpring {
    fn as_mut(&mut self) -> &mut CharacterSpring {
        self
    }
}
#[repr(C, align(8))]
/// The parachute handle IK task (`NParachuteHandleIKTask`). An animation state task that, while
/// active, moves each hand its [`ParachuteHandleIKProperties::m_LeftHand`](crate::input::parachute::ParachuteHandleIKProperties::m_LeftHand) /
/// [`ParachuteHandleIKProperties::m_RightHand`](crate::input::parachute::ParachuteHandleIKProperties::m_RightHand) flags select onto the parachute rig's handle,
/// rather than leaving it to the base animation.
pub struct NParachuteHandleIKTask {}
impl NParachuteHandleIKTask {
    pub const Update_ADDRESS: usize = 0x14082E510;
    /// The task's per-frame update. Derives the blend weight from the blend-in and blend-out
    /// segments' local time (ramping up from the former, down across the latter) and writes it to
    /// [`ParachuteHandleIKInstanceProperties::m_Weight`](crate::input::parachute::ParachuteHandleIKInstanceProperties::m_Weight). For each selected hand, blends between the
    /// parachute rig's handle-link bone and the character's hand and second hand-attach bones to
    /// derive a world-space target position for the wrist, then writes that position to the wrist's
    /// `SECONDARY`-pass position key
    /// ([`ObjectBlackboard::SECONDARY_HAND_POSITION_WS_LEFT_ID`](crate::blackboard::ObjectBlackboard::SECONDARY_HAND_POSITION_WS_LEFT_ID)
    /// or its right twin) and the blend weight to the matching weight key
    /// ([`ObjectBlackboard::SECONDARY_HAND_WEIGHT_LEFT_ID`](crate::blackboard::ObjectBlackboard::SECONDARY_HAND_WEIGHT_LEFT_ID)
    /// or its right twin), both with
    /// [`PersistenceType::PERSISTENCE_SESSION`](crate::blackboard::PersistenceType::PERSISTENCE_SESSION) —
    /// unlike most blackboard writers, which use
    /// [`PersistenceType::PERSISTENCE_LOCAL_FRAME`](crate::blackboard::PersistenceType::PERSISTENCE_LOCAL_FRAME).
    /// [`Character::UpdateSecondaryHandIKPass`](crate::character::character::Character::UpdateSecondaryHandIKPass)
    /// reads the keys and queues the wrist target on the `SECONDARY` pass.
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
    pub const OnExit_ADDRESS: usize = 0x14082E490;
    /// The task's exit: removes all four `SECONDARY`-pass hand keys
    /// ([`ObjectBlackboard::SECONDARY_HAND_POSITION_WS_LEFT_ID`](crate::blackboard::ObjectBlackboard::SECONDARY_HAND_POSITION_WS_LEFT_ID),
    /// [`ObjectBlackboard::SECONDARY_HAND_POSITION_WS_RIGHT_ID`](crate::blackboard::ObjectBlackboard::SECONDARY_HAND_POSITION_WS_RIGHT_ID),
    /// [`ObjectBlackboard::SECONDARY_HAND_WEIGHT_LEFT_ID`](crate::blackboard::ObjectBlackboard::SECONDARY_HAND_WEIGHT_LEFT_ID),
    /// and [`ObjectBlackboard::SECONDARY_HAND_WEIGHT_RIGHT_ID`](crate::blackboard::ObjectBlackboard::SECONDARY_HAND_WEIGHT_RIGHT_ID))
    /// through [`RemoveBlackBoardValue`](crate::blackboard::RemoveBlackBoardValue), regardless of which
    /// hand or hands the instance drove.
    pub unsafe fn OnExit(
        ctx: *mut crate::state::StateContext,
        p1: *mut ::std::ffi::c_void,
        p2: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                ctx: *mut crate::state::StateContext,
                p1: *mut ::std::ffi::c_void,
                p2: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::OnExit_ADDRESS);
            f(ctx, p1, p2)
        }
    }
    pub const Load_ADDRESS: usize = 0x140835D40;
    /// Loads [`ParachuteHandleIKProperties`](crate::input::parachute::ParachuteHandleIKProperties) from the state definition's runtime container.
    pub unsafe fn Load(
        properties: *mut crate::input::parachute::ParachuteHandleIKProperties,
        container: *const ::std::ffi::c_void,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                properties: *mut crate::input::parachute::ParachuteHandleIKProperties,
                container: *const ::std::ffi::c_void,
            ) -> bool = ::std::mem::transmute(Self::Load_ADDRESS);
            f(properties, container)
        }
    }
}
impl std::convert::AsRef<NParachuteHandleIKTask> for NParachuteHandleIKTask {
    fn as_ref(&self) -> &NParachuteHandleIKTask {
        self
    }
}
impl std::convert::AsMut<NParachuteHandleIKTask> for NParachuteHandleIKTask {
    fn as_mut(&mut self) -> &mut NParachuteHandleIKTask {
        self
    }
}
#[repr(C, align(8))]
/// The input state task active while the character is parachuting. Delegates the per-frame
/// movement to [`UpdateParachutePhysics`](crate::input::parachute::UpdateParachutePhysics).
pub struct NStateTask_MovementParachuteTask {}
impl NStateTask_MovementParachuteTask {
    pub const Update_ADDRESS: usize = 0x14082AB10;
    /// The per-frame update. Reads the move half-axes from the avatar's action map as a stick
    /// vector (`MOVE_RIGHT` minus `MOVE_LEFT` across, `MOVE_BACKWARD` minus `MOVE_FORWARD` along,
    /// so the stick pulled back is positive), calls [`UpdateParachutePhysics`](crate::input::parachute::UpdateParachutePhysics) with it, then
    /// handles the cancel input that closes the parachute.
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
impl std::convert::AsRef<NStateTask_MovementParachuteTask>
for NStateTask_MovementParachuteTask {
    fn as_ref(&self) -> &NStateTask_MovementParachuteTask {
        self
    }
}
impl std::convert::AsMut<NStateTask_MovementParachuteTask>
for NStateTask_MovementParachuteTask {
    fn as_mut(&mut self) -> &mut NStateTask_MovementParachuteTask {
        self
    }
}
#[repr(C, align(8))]
/// The wingsuit's per-frame update. It reads the move half-axes as a stick vector (`MOVE_RIGHT`
/// minus `MOVE_LEFT` across, `MOVE_BACKWARD` minus `MOVE_FORWARD` along) and hands it to
/// [`NAirMovement_UpdateAirPhysics`](crate::input::parachute::NAirMovement_UpdateAirPhysics) with the wingsuit's air settings. The function carries the
/// build's tamper-check markers.
pub struct NStateTask_MovementWingSuitTask {}
impl NStateTask_MovementWingSuitTask {
    pub const Update_ADDRESS: usize = 0x14082DDD0;
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
impl std::convert::AsRef<NStateTask_MovementWingSuitTask>
for NStateTask_MovementWingSuitTask {
    fn as_ref(&self) -> &NStateTask_MovementWingSuitTask {
        self
    }
}
impl std::convert::AsMut<NStateTask_MovementWingSuitTask>
for NStateTask_MovementWingSuitTask {
    fn as_mut(&mut self) -> &mut NStateTask_MovementWingSuitTask {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// `SParachuteActionParams`: the per-character parachute animation/physics state, embedded in
/// `CCharacter` alongside the other movement families' action params. The four proxy matrices
/// describe the parachute rig's pivot (the suspension point), the character, and the canopy, plus
/// the blended steering proxy that interpolates between them; the floats track the parachute
/// blend-in, bump timers, and the velocity-steering blend.
pub struct ParachuteActionParams {
    pub m_IsValid: bool,
    _field_1: [u8; 15],
    pub m_PivoProxy: crate::types::math::Matrix4,
    pub m_CharacterProxy: crate::types::math::Matrix4,
    pub m_ParachuteProxy: crate::types::math::Matrix4,
    pub m_SteeringProxy: crate::types::math::Matrix4,
    pub m_BlendOutPivotProxyCS: crate::types::math::Matrix4,
    pub m_OriginalReelDir: crate::types::math::Vector3,
    pub m_OriginalReelPos: crate::types::math::Vector3,
    pub m_OriginalReelTarget: crate::types::math::Vector3,
    pub m_PivotYOffset: f32,
    pub m_CharacterMaxSwing: f32,
    pub m_SlingshotTime: f32,
    pub m_SlinghotWeight: f32,
    pub m_ParachuteBlendIn: f32,
    pub m_ParachuteBumpTimer: f32,
    pub m_ParachuteBumpCooldownTimer: f32,
    pub m_VelocitySteeringBlendScale: f32,
    pub m_ParaFromAttachedResidueVelocity: crate::types::math::Vector3,
}
fn _ParachuteActionParams_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1A0], ParachuteActionParams>([0u8; 0x1A0]);
    }
    unreachable!()
}
impl ParachuteActionParams {}
impl std::convert::AsRef<ParachuteActionParams> for ParachuteActionParams {
    fn as_ref(&self) -> &ParachuteActionParams {
        self
    }
}
impl std::convert::AsMut<ParachuteActionParams> for ParachuteActionParams {
    fn as_mut(&mut self) -> &mut ParachuteActionParams {
        self
    }
}
#[repr(C, align(4))]
/// The per-instance runtime state of [`NParachuteHandleIKTask`](crate::input::parachute::NParachuteHandleIKTask)
/// (`NParachuteHandleIKTask::SInstanceProperties`): the task's current blend weight, written by
/// [`NParachuteHandleIKTask::Update`](crate::input::parachute::NParachuteHandleIKTask::Update) each frame.
pub struct ParachuteHandleIKInstanceProperties {
    pub m_Weight: f32,
}
fn _ParachuteHandleIKInstanceProperties_size_check() {
    unsafe {
        ::std::mem::transmute::<
            [u8; 0x4],
            ParachuteHandleIKInstanceProperties,
        >([0u8; 0x4]);
    }
    unreachable!()
}
impl ParachuteHandleIKInstanceProperties {}
impl std::convert::AsRef<ParachuteHandleIKInstanceProperties>
for ParachuteHandleIKInstanceProperties {
    fn as_ref(&self) -> &ParachuteHandleIKInstanceProperties {
        self
    }
}
impl std::convert::AsMut<ParachuteHandleIKInstanceProperties>
for ParachuteHandleIKInstanceProperties {
    fn as_mut(&mut self) -> &mut ParachuteHandleIKInstanceProperties {
        self
    }
}
#[repr(C, align(4))]
/// The per-instance loaded properties of [`NParachuteHandleIKTask`](crate::input::parachute::NParachuteHandleIKTask)
/// (`NParachuteHandleIKTask::SProperties`): the blend-in and blend-out animation segments that
/// gate the task's weight, and which hand or hands it drives.
pub struct ParachuteHandleIKProperties {
    /// The segment whose local time ramps the blend weight up from zero.
    pub m_BlendInSegmentId: u32,
    /// The segment whose local time ramps the blend weight back down to zero.
    pub m_BlendOutSegmentId: u32,
    /// Whether the task drives the left wrist.
    pub m_LeftHand: bool,
    /// Whether the task drives the right wrist.
    pub m_RightHand: bool,
    _field_a: [u8; 2],
}
fn _ParachuteHandleIKProperties_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xC], ParachuteHandleIKProperties>([0u8; 0xC]);
    }
    unreachable!()
}
impl ParachuteHandleIKProperties {}
impl std::convert::AsRef<ParachuteHandleIKProperties> for ParachuteHandleIKProperties {
    fn as_ref(&self) -> &ParachuteHandleIKProperties {
        self
    }
}
impl std::convert::AsMut<ParachuteHandleIKProperties> for ParachuteHandleIKProperties {
    fn as_mut(&mut self) -> &mut ParachuteHandleIKProperties {
        self
    }
}
pub const UpdateParachutePhysics_ADDRESS: usize = 0x1407E6DF0;
/// The parachute flight/steering core, called per frame by
/// [`NStateTask_MovementParachuteTask::Update`](crate::input::parachute::NStateTask_MovementParachuteTask::Update). Computes the parachute steering values, the
/// wanted character yaw/pitch/roll, and the camera-relative look steer via
/// [`GetParachuteSteeringValues`](crate::input::parachute::GetParachuteSteeringValues), then drives the parachute velocities and the character/
/// canopy orientation through the parachute action params' proxy matrices. Besides steering
/// through the input mixes, the input's along axis is the target of the lift and drag amounts,
/// which follow it through the air settings' `m_LiftInputSpring` and `m_DragInputSpring`; the
/// conventional velocity then takes its lift and drag between the settings' `m_Lift` and
/// `m_DragX`/`m_DragY`/`m_DragZ` entries by those amounts, the last entry with the stick fully
/// back and the first with it fully forward.
pub unsafe fn UpdateParachutePhysics(
    character: *mut crate::character::character::Character,
    dt: f32,
    input: *const crate::types::math::Vector2,
    para_settings: *const crate::input::parachute::CharacterParachuteSettings,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            character: *mut crate::character::character::Character,
            dt: f32,
            input: *const crate::types::math::Vector2,
            para_settings: *const crate::input::parachute::CharacterParachuteSettings,
        ) = ::std::mem::transmute(UpdateParachutePhysics_ADDRESS);
        f(character, dt, input, para_settings)
    }
}
pub const GetParachuteSteeringValues_ADDRESS: usize = 0x1407E6480;
/// The parachute steering helper, called from [`UpdateParachutePhysics`](crate::input::parachute::UpdateParachutePhysics). Computes the steering
/// values for the desired character yaw/pitch/roll from the input vector and the *camera input
/// matrix* (`GameCameraManager::GetInputMatrix`): the stick input is mixed through
/// `m_XInputYawPitchRollAmount`/`m_YInputYawPitchRollAmount`, while the camera-relative look is
/// shaped by `m_LookSteer*` (deadzone, max yaw, exponential) into `look_steer_out` and blended in.
/// Also computes the velocity-alignment steering toward the horizontal velocity.
pub unsafe fn GetParachuteSteeringValues(
    character: *mut crate::character::character::Character,
    para_settings: *const crate::input::parachute::CharacterParachuteSettings,
    input: *const crate::types::math::Vector2,
    dt: f32,
    steering_values_out: *mut crate::types::math::Vector3,
    wanted_char_yawpitchroll_out: *mut crate::types::math::Vector3,
    look_steer_out: *mut crate::types::math::Vector2,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            character: *mut crate::character::character::Character,
            para_settings: *const crate::input::parachute::CharacterParachuteSettings,
            input: *const crate::types::math::Vector2,
            dt: f32,
            steering_values_out: *mut crate::types::math::Vector3,
            wanted_char_yawpitchroll_out: *mut crate::types::math::Vector3,
            look_steer_out: *mut crate::types::math::Vector2,
        ) = ::std::mem::transmute(GetParachuteSteeringValues_ADDRESS);
        f(
            character,
            para_settings,
            input,
            dt,
            steering_values_out,
            wanted_char_yawpitchroll_out,
            look_steer_out,
        )
    }
}
pub const UpdateParachutingTransforms_ADDRESS: usize = 0x140792880;
/// Slerps the character and canopy proxy orientations in the parachute action params toward the
/// wanted orientation. Called from [`UpdateParachutePhysics`](crate::input::parachute::UpdateParachutePhysics).
pub unsafe fn UpdateParachutingTransforms(
    character: *mut crate::character::character::Character,
    params: *mut crate::input::parachute::ParachuteActionParams,
    wanted_yaw_pitch_roll: *const crate::types::math::Vector3,
    transform: *const crate::types::math::Matrix4,
    para_settings: *const crate::input::parachute::CharacterParachuteSettings,
    dt: f32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            character: *mut crate::character::character::Character,
            params: *mut crate::input::parachute::ParachuteActionParams,
            wanted_yaw_pitch_roll: *const crate::types::math::Vector3,
            transform: *const crate::types::math::Matrix4,
            para_settings: *const crate::input::parachute::CharacterParachuteSettings,
            dt: f32,
        ) = ::std::mem::transmute(UpdateParachutingTransforms_ADDRESS);
        f(character, params, wanted_yaw_pitch_roll, transform, para_settings, dt)
    }
}
pub const ApplyConventionalParachuteVelocities_ADDRESS: usize = 0x140792D10;
/// Computes the conventional (physics-driven) parachute velocity from the steering values, the
/// orientation, and the current velocity. Called from [`UpdateParachutePhysics`](crate::input::parachute::UpdateParachutePhysics).
pub unsafe fn ApplyConventionalParachuteVelocities(
    character: *mut crate::character::character::Character,
    dt: f32,
    para_settings: *const crate::input::parachute::CharacterParachuteSettings,
    transform: *const crate::types::math::Matrix4,
    blend: f32,
    steering_values: *const crate::types::math::Vector3,
    p6: *const crate::types::math::Vector3,
    p7: *const crate::types::math::Vector3,
    velocity_out: *mut crate::types::math::Vector3,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            character: *mut crate::character::character::Character,
            dt: f32,
            para_settings: *const crate::input::parachute::CharacterParachuteSettings,
            transform: *const crate::types::math::Matrix4,
            blend: f32,
            steering_values: *const crate::types::math::Vector3,
            p6: *const crate::types::math::Vector3,
            p7: *const crate::types::math::Vector3,
            velocity_out: *mut crate::types::math::Vector3,
        ) = ::std::mem::transmute(ApplyConventionalParachuteVelocities_ADDRESS);
        f(
            character,
            dt,
            para_settings,
            transform,
            blend,
            steering_values,
            p6,
            p7,
            velocity_out,
        )
    }
}
pub const ApplyAnimationVelocityWithSteering_ADDRESS: usize = 0x1407751E0;
/// Computes the parachute velocity from the animation root velocity with steering applied. Called
/// from [`UpdateParachutePhysics`](crate::input::parachute::UpdateParachutePhysics).
pub unsafe fn ApplyAnimationVelocityWithSteering(
    character: *mut crate::character::character::Character,
    dt: f32,
    para_settings: *const crate::input::parachute::CharacterParachuteSettings,
    transform: *const crate::types::math::Matrix4,
    blend: f32,
    velocity_out: *mut crate::types::math::Vector3,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            character: *mut crate::character::character::Character,
            dt: f32,
            para_settings: *const crate::input::parachute::CharacterParachuteSettings,
            transform: *const crate::types::math::Matrix4,
            blend: f32,
            velocity_out: *mut crate::types::math::Vector3,
        ) = ::std::mem::transmute(ApplyAnimationVelocityWithSteering_ADDRESS);
        f(character, dt, para_settings, transform, blend, velocity_out)
    }
}
pub const NAirMovement_UpdateAirPhysics_ADDRESS: usize = 0x1407E1680;
/// `NAirMovement::UpdateAirPhysics`: the air movement core shared by the wingsuit, falling, and
/// free-falling tasks. It maps the stick vector `input` through the settings'
/// `m_XInputYawPitchRollAmount` and `m_YInputYawPitchRollAmount` into a yaw, pitch, and roll
/// steering vector and, for the local player outside an alignment segment, adds the camera
/// look-steer from [`NAirMovement_UpdateStdLookSteering`](crate::input::parachute::NAirMovement_UpdateStdLookSteering) to it.
pub unsafe fn NAirMovement_UpdateAirPhysics(
    character: *mut crate::character::character::Character,
    dt: f32,
    input: *const crate::types::math::Vector2,
    settings: *const crate::input::parachute::CharacterAirMovementSettings,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            character: *mut crate::character::character::Character,
            dt: f32,
            input: *const crate::types::math::Vector2,
            settings: *const crate::input::parachute::CharacterAirMovementSettings,
        ) = ::std::mem::transmute(NAirMovement_UpdateAirPhysics_ADDRESS);
        f(character, dt, input, settings)
    }
}
pub const NAirMovement_UpdateStdLookSteering_ADDRESS: usize = 0x140791300;
/// `NAirMovement::UpdateStdLookSteering`: the camera look-steer of the air movement, called only
/// from [`NAirMovement_UpdateAirPhysics`](crate::input::parachute::NAirMovement_UpdateAirPhysics). It reads the camera input matrix
/// (`GameCameraManager::GetInputMatrix`, inlined), takes the pitch and yaw differences between the
/// camera and the character (`char_pitch`, `char_yaw`), shapes each through the settings'
/// look-steer deadzone, range, and exponent, and mixes them through `m_XLookYawPitchRollAmount` and
/// `m_YLookYawPitchRollAmount` into the yaw, pitch, and roll vector `out`. Each component is then
/// reduced against `target_steering` where the two oppose. It has no other side effects; the
/// caller adds `out` into its steering vector.
pub unsafe fn NAirMovement_UpdateStdLookSteering(
    out: *mut crate::types::math::Vector3,
    target_steering: *const crate::types::math::Vector3,
    char_pitch: f32,
    char_yaw: f32,
    settings: *const crate::input::parachute::CharacterAirMovementSettings,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            out: *mut crate::types::math::Vector3,
            target_steering: *const crate::types::math::Vector3,
            char_pitch: f32,
            char_yaw: f32,
            settings: *const crate::input::parachute::CharacterAirMovementSettings,
        ) = ::std::mem::transmute(NAirMovement_UpdateStdLookSteering_ADDRESS);
        f(out, target_steering, char_pitch, char_yaw, settings)
    }
}
pub const NParachuteObjectControl_SyncBones_ADDRESS: usize = 0x140CD74F0;
/// `NParachuteObjectControl::SyncBones`: moves the parachute object's own rig to track the pose of
/// `owner`, the character it is attached to, and, for the player, places the rig's handles in the
/// character's hands. `am` is the parachute's animated model, whose
/// [`m_AnimationController`](crate::character::character::AnimatedModel::m_AnimationController) holds the
/// rig's skeleton and joints; `props` is the parachute object's loaded properties.
///
/// Besides the rig-tracking pose (syncing the rig's `Base` bone toward the character's `BackAttach`
/// bone over a settling segment, re-deriving each rig joint from the rig's model-space pose, and
/// blending the joints toward their bind pose over another), it places each handle for the player.
/// Each side has three rig bones: the handle (`RightHandle`, `LeftHandle`), its rotation bone
/// (`RightHandleRotate`, `LeftHandleRotate`), and the link the handle's cord hangs from
/// (`RightLink`, `LeftLink`). A placed handle takes the world position of the character's second
/// hand-attach bone (`RightHandAttach2`, `LeftHandAttach2`) carried into the rig's model space as
/// its joint translation, the rotation bone takes that bone's rotation, and the link is drawn a
/// little toward the handle while the handle is further from it than in the bind pose. A handle not
/// placed sits on its link: its joint translation is the link's.
///
/// The right handle is not placed while the character wields a weapon or is within a
/// `SEG_NO_RIGHT_HANDLE_PLACEMENT` segment; the left one while the character fires the grapple,
/// has a grapple hook out, or is within a `SEG_NO_LEFT_HANDLE_PLACEMENT` segment. A non-zero first
/// word of `props` suspends both unless a `SEG_FORCE_HANDLE_PLACEMENT_OVERRIDE` segment is active.
/// So the handles are drawn in the character's hands wherever its hands are, independently of the
/// `SECONDARY` HumanIK pass that [`NParachuteHandleIKTask`](crate::input::parachute::NParachuteHandleIKTask) drives toward the links through the
/// blackboard.
pub unsafe fn NParachuteObjectControl_SyncBones(
    parachute: *mut ::std::ffi::c_void,
    am: *mut crate::character::character::AnimatedModel,
    owner: *mut crate::character::character::Character,
    props: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            parachute: *mut ::std::ffi::c_void,
            am: *mut crate::character::character::AnimatedModel,
            owner: *mut crate::character::character::Character,
            props: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(NParachuteObjectControl_SyncBones_ADDRESS);
        f(parachute, am, owner, props)
    }
}
pub const NParachuteObjectControl_DoSyncAndPlacement_ADDRESS: usize = 0x140CD87A0;
/// `NParachuteObjectControl::DoSyncAndPlacement`: sets the parachute object's transform from the
/// owning character's canopy proxy
/// ([`ParachuteActionParams::m_ParachuteProxy`](crate::input::parachute::ParachuteActionParams::m_ParachuteProxy)) and, once the object has finished initializing,
/// calls [`NParachuteObjectControl_SyncBones`](crate::input::parachute::NParachuteObjectControl_SyncBones).
pub unsafe fn NParachuteObjectControl_DoSyncAndPlacement(
    parachute: *mut ::std::ffi::c_void,
    props: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            parachute: *mut ::std::ffi::c_void,
            props: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(NParachuteObjectControl_DoSyncAndPlacement_ADDRESS);
        f(parachute, props)
    }
}
