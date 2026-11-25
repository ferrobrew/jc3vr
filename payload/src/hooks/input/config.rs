//! Input hook configuration: the movement settings. The rest of the movement hooks' behaviour is
//! fixed in the hook modules that own it.

use serde::{Deserialize, Serialize};

/// Movement settings.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct MovementConfig {
    /// Give the right stick the swim heading. The swim family bypasses the on-foot orientation
    /// executor and steers by the *camera*-relative stick in act-sized chunks, which in VR -- where
    /// the view rides the body frame -- snaps the whole world. This aims both the move direction and
    /// the cores' heading rotation at the right-stick body-yaw target instead, and refuses backward
    /// input (there is no backstroke). See `crate::hooks::input::swim`. Inert without a headpose
    /// body-yaw target, so flatscreen keeps the native behaviour.
    pub smooth_swim_yaw: bool,
    /// The rate, in degrees per second, the body is turned toward the swim heading. Must stay above
    /// the look input's own rate, or the body trails the target and the turn continues after the
    /// stick is released; the shipped rates (~69 at the surface, ~30 underwater) are far below it.
    /// Capped per frame by the angle `DoRotate` eases out over, so it cannot overshoot.
    pub swim_turn_rate_deg_s: f32,
    /// Suppress the head-driven body turn during a jump. The airborne actuator
    /// (`NStateTask_MovementJumpTask::Update`) faces the body at the weapon-aim target while
    /// [`m_AimingWeapon`](jc3gi::character::character::AimState::m_AimingWeapon) is set, and in VR
    /// that target follows the HMD gaze -- so turning your head yaws your body mid-jump with no stick
    /// input. This clears the aim bit around the jump update for the local player while the head is
    /// decoupled (the VR source), routing the jump through its non-aiming fallback (current forward
    /// plus stick-gated steer). Restored immediately after. See `crate::hooks::input::locomotion`.
    pub suppress_air_aim_facing: bool,
}
impl MovementConfig {
    pub const fn new() -> Self {
        Self {
            smooth_swim_yaw: true,
            swim_turn_rate_deg_s: 360.0,
            suppress_air_aim_facing: true,
        }
    }
}
