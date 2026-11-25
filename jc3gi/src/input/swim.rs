#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The input state task active while the character swims at the water surface. Unlike the on-foot
/// locomotion family, the swim tasks do not route through
/// [`NStateTask_LocoUtil::EvaluateCharacterOrientation`](crate::input::locomotion::NStateTask_LocoUtil::EvaluateCharacterOrientation)
/// or the target-face-dir blackboard: body turning is dispatched as discrete animation acts, and
/// the actual heading change is applied by the movement actuators below.
pub struct NStateTask_InputSurfaceSwimTask {}
impl NStateTask_InputSurfaceSwimTask {
    pub const Update_ADDRESS: usize = 0x140830BA0;
    /// The per-frame update. Computes the camera-relative move direction (via
    /// `GameCameraManager::GetInputMatrix`) and, for the player, the direction to the aim-target
    /// position, then dispatches swim acts by the angle from the body forward
    /// ([`ControllerUtility::GetDeltaAngleFromOrientation`](crate::input::controller_utility::ControllerUtility::GetDeltaAngleFromOrientation)):
    /// below [`SWIM_TURN_ACT_MIN_ANGLE`](crate::input::swim::SWIM_TURN_ACT_MIN_ANGLE) the forward crawl-start act, from there up to
    /// [`SWIM_TURN_ACT_MAX_ANGLE`](crate::input::swim::SWIM_TURN_ACT_MAX_ANGLE) the directional 120-degree turn acts
    /// (`ACT_SURFACE_SWIM_START_120R/L` while moving,
    /// `ACT_SURFACE_IDLE_QUICKTURN_120R/L` while idle and wielding a weapon, measured against the
    /// aim direction). When a turn is latched it writes the angle to the blackboard
    /// ([`SWIM_TURN_ANGLE_ID`](crate::blackboard::ObjectBlackboard::SWIM_TURN_ANGLE_ID)) and stores the
    /// base/target direction pair in the character's swim action parameters for the actuator's
    /// segment-gated slerp. Also handles the surface-to-underwater dive transition (camera pitched
    /// down, sufficient depth, facing within 65 degrees of the move direction).
    ///
    /// The stick's magnitude only picks acts: at or above
    /// [`CharacterSwimSettings::m_SurfaceSwimFullDeflection`](crate::input::locomotion::CharacterSwimSettings::m_SurfaceSwimFullDeflection)
    /// a character that is not already starting or crawling gets the start act; otherwise the
    /// task queues the crawl act until the stick has stayed below a tenth of full deflection for
    /// [`CharacterSwimSettings::m_SurfaceSwimNoInputToStopTime`](crate::input::locomotion::CharacterSwimSettings::m_SurfaceSwimNoInputToStopTime),
    /// then the idle act. Weapon input, a grenade throw, or a planted explosive held past that
    /// time divert to the idle and quick-turn acts.
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
impl std::convert::AsRef<NStateTask_InputSurfaceSwimTask>
for NStateTask_InputSurfaceSwimTask {
    fn as_ref(&self) -> &NStateTask_InputSurfaceSwimTask {
        self
    }
}
impl std::convert::AsMut<NStateTask_InputSurfaceSwimTask>
for NStateTask_InputSurfaceSwimTask {
    fn as_mut(&mut self) -> &mut NStateTask_InputSurfaceSwimTask {
        self
    }
}
#[repr(C, align(8))]
/// The input state task active while the character swims underwater. Mirrors
/// [`NStateTask_InputSurfaceSwimTask`](crate::input::swim::NStateTask_InputSurfaceSwimTask)'s act dispatch, with `ACT_UW_SWIM_IDLE_TURN_120R/L` as the
/// discrete turn acts and the surfacing transition in place of the dive.
pub struct NStateTask_InputUnderWaterSwimTask {}
impl NStateTask_InputUnderWaterSwimTask {
    pub const Update_ADDRESS: usize = 0x140823FC0;
    /// The per-frame update.
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
impl std::convert::AsRef<NStateTask_InputUnderWaterSwimTask>
for NStateTask_InputUnderWaterSwimTask {
    fn as_ref(&self) -> &NStateTask_InputUnderWaterSwimTask {
        self
    }
}
impl std::convert::AsMut<NStateTask_InputUnderWaterSwimTask>
for NStateTask_InputUnderWaterSwimTask {
    fn as_mut(&mut self) -> &mut NStateTask_InputUnderWaterSwimTask {
        self
    }
}
#[repr(C, align(8))]
/// The movement actuator task for surface swimming; delegates to [`UpdateSurfaceMovement`](crate::input::swim::UpdateSurfaceMovement) for the
/// player and `UpdateSurfaceMovementNPC` otherwise.
pub struct NStateTask_MovementSurfaceSwimTask {}
impl NStateTask_MovementSurfaceSwimTask {
    pub const Update_ADDRESS: usize = 0x14082C940;
    /// The per-frame update.
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
impl std::convert::AsRef<NStateTask_MovementSurfaceSwimTask>
for NStateTask_MovementSurfaceSwimTask {
    fn as_ref(&self) -> &NStateTask_MovementSurfaceSwimTask {
        self
    }
}
impl std::convert::AsMut<NStateTask_MovementSurfaceSwimTask>
for NStateTask_MovementSurfaceSwimTask {
    fn as_mut(&mut self) -> &mut NStateTask_MovementSurfaceSwimTask {
        self
    }
}
#[repr(C, align(8))]
/// The movement actuator task for underwater swimming; delegates to [`UpdateUnderwaterMovement`](crate::input::swim::UpdateUnderwaterMovement).
pub struct NStateTask_MovementUnderWaterSwimTask {}
impl NStateTask_MovementUnderWaterSwimTask {
    pub const Update_ADDRESS: usize = 0x14082D470;
    /// The per-frame update.
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
impl std::convert::AsRef<NStateTask_MovementUnderWaterSwimTask>
for NStateTask_MovementUnderWaterSwimTask {
    fn as_ref(&self) -> &NStateTask_MovementUnderWaterSwimTask {
        self
    }
}
impl std::convert::AsMut<NStateTask_MovementUnderWaterSwimTask>
for NStateTask_MovementUnderWaterSwimTask {
    fn as_mut(&mut self) -> &mut NStateTask_MovementUnderWaterSwimTask {
        self
    }
}
/// The fixed surface-swim facing turn rate (degrees per second) while aiming or planting
/// explosives.
pub const SWIM_AIM_TURN_RATE: f32 = 300.0;
/// The upper bound (degrees) of the swim input tasks' turn-act angle range.
pub const SWIM_TURN_ACT_MAX_ANGLE: f32 = 180.0;
/// The absolute angle (degrees) from the body forward below which the swim input tasks queue the
/// forward crawl-start act, and at or above which they queue a directional 120-degree turn act.
pub const SWIM_TURN_ACT_MIN_ANGLE: f32 = 65.0;
/// The factor the swim movement cores multiply a turn rate by to get degrees per second: the
/// surface core scales the movement settings' swim slerp speeds by it, and the underwater core
/// uses it as a fixed rate.
pub const SWIM_TURN_RATE_SCALE: f32 = 30.0;
pub const UpdateSurfaceMovement_ADDRESS: usize = 0x1407E9440;
/// The surface-swim movement core, called per frame by
/// [`NStateTask_MovementSurfaceSwimTask::Update`](crate::input::swim::NStateTask_MovementSurfaceSwimTask::Update) for the player. Computes buoyancy and the swim
/// velocity, and applies the body orientation: the heading slerps from the latched animated-turn
/// base direction to the target direction with the interpolant driven by the local time of the
/// angle-correction animation segment (`ang_corr_seg_id`), is held at the current forward outside
/// the steering segment (`steering_seg_id`), is blended with the water-surface plane for roll, and
/// is written through [`PfxCharacterInstance::SetOrientation`](crate::physics::PfxCharacterInstance::SetOrientation) -- so the heading changes in
/// act-sized steps rather than continuously. `warping_seg_id` drives the position warp toward a
/// grapple or planted-explosive target. The segment ids are the task's loaded properties, passed
/// as `idstring<SSegmentIdTag>` references.
///
/// The swim speed is the horizontal magnitude of the animation's raw root velocity
/// ([`AnimationController::GetRawRootVelocity`](crate::character::character::AnimationController::GetRawRootVelocity));
/// the stick contributes only its direction, normalized, so how fast the swimmer goes is set by
/// the act the input task queued and the clip it plays. With the stick at rest the speed is kept
/// along the measured world velocity's direction. The velocity, with buoyancy, wave push, and the
/// transition speed added, is rotated into the frame of the orientation passed to
/// [`PfxCharacterInstance::SetOrientation`](crate::physics::PfxCharacterInstance::SetOrientation) (the transpose of that orientation applied to it) and
/// written to the proxy's [`CharacterInput::wantedVelocity`](crate::physics::CharacterInput::wantedVelocity)
/// just before that call.
pub unsafe fn UpdateSurfaceMovement(
    character: *mut crate::character::character::Character,
    dt: f32,
    target_angle_offset: f32,
    ang_corr_seg_id: *const u32,
    steering_seg_id: *const u32,
    warping_seg_id: *const u32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            character: *mut crate::character::character::Character,
            dt: f32,
            target_angle_offset: f32,
            ang_corr_seg_id: *const u32,
            steering_seg_id: *const u32,
            warping_seg_id: *const u32,
        ) = ::std::mem::transmute(UpdateSurfaceMovement_ADDRESS);
        f(
            character,
            dt,
            target_angle_offset,
            ang_corr_seg_id,
            steering_seg_id,
            warping_seg_id,
        )
    }
}
pub const UpdateUnderwaterMovement_ADDRESS: usize = 0x1407A5DC0;
/// The underwater-swim movement core, called per frame by
/// [`NStateTask_MovementUnderWaterSwimTask::Update`](crate::input::swim::NStateTask_MovementUnderWaterSwimTask::Update). Same orientation structure as
/// [`UpdateSurfaceMovement`](crate::input::swim::UpdateSurfaceMovement) -- segment-gated animated-turn slerp applied through
/// [`PfxCharacterInstance::SetOrientation`](crate::physics::PfxCharacterInstance::SetOrientation) -- with the pitch following the three-dimensional swim
/// direction instead of the surface plane. Its speed is likewise the animation's root speed.
pub unsafe fn UpdateUnderwaterMovement(
    character: *mut crate::character::character::Character,
    dt: f32,
    target_angle_offset: f32,
    ang_corr_seg_id: *const u32,
    steering_seg_id: *const u32,
    p6: bool,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            character: *mut crate::character::character::Character,
            dt: f32,
            target_angle_offset: f32,
            ang_corr_seg_id: *const u32,
            steering_seg_id: *const u32,
            p6: bool,
        ) = ::std::mem::transmute(UpdateUnderwaterMovement_ADDRESS);
        f(character, dt, target_angle_offset, ang_corr_seg_id, steering_seg_id, p6)
    }
}
pub const UpdateSurfaceMovementNPC_ADDRESS: usize = 0x1407A5710;
/// The surface-swim movement core for NPCs, called by
/// [`NStateTask_MovementSurfaceSwimTask::Update`](crate::input::swim::NStateTask_MovementSurfaceSwimTask::Update) when the character is not the player.
pub unsafe fn UpdateSurfaceMovementNPC(
    character: *mut crate::character::character::Character,
    dt: f32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            character: *mut crate::character::character::Character,
            dt: f32,
        ) = ::std::mem::transmute(UpdateSurfaceMovementNPC_ADDRESS);
        f(character, dt)
    }
}
