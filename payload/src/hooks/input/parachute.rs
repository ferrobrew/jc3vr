//! Parachute locomotion detours: drop the camera-relative look-steer from the parachute's
//! steering while the head is decoupled.
//!
//! The parachute steering helper `NParachuteMovement::GetParachuteSteeringValues` mixes the
//! *camera input matrix* (`GameCameraManager::GetInputMatrix`) into the parachute's steering.
//! The camera-relative yaw/pitch deltas are shaped by the look-steer block
//! (`m_LookSteerDeadZone`, `m_LookSteerMaxYaw`, `m_LookSteerExponential`) into `look_steer_out`,
//! which is then mixed into the steering values and the yaw/pitch velocity springs. In VR the
//! camera follows the HMD, so turning the head past the deadzone turns the chute with no stick
//! input -- the "parachute yaws with the head" issue. The on-foot air-born suppression
//! ([`crate::hooks::input::locomotion`]) covers the jump/fall via `UpdateFallSteering`, but the
//! parachute is a separate movement family that never routes through that helper.
//!
//! The fix subtracts the look-steer contribution from `steering_values_out` (the direct
//! steering) and zeroes `look_steer_out` (which feeds the yaw/pitch velocity springs) while the
//! head is decoupled. The stick input (`m_XInputYawPitchRollAmount`/`m_YInputYawPitchRollAmount`)
//! and the velocity-alignment steering are untouched, so the chute still steers from the stick and
//! banks with velocity.
//!
//! The look steer is also the only thing that turns the canopy toward a grapple line it reels
//! along (see [`UpdateParachutePhysics`]): the game's own turn toward the line is switched off, and
//! the slingshot pulls only along the canopy's facing, so a line fired beside the player added a
//! little speed and no turn once the head no longer steered. While the hook reels in under the
//! canopy, the look steer the game would give a camera looking along the line is put back in its
//! place ([`line_turn`]), so the canopy turns toward the line through the game's own look-steer
//! shaping and springs, as it does in the game when the player looks at the line they fired.
//!
//! [`UpdateParachutePhysics`]: jc3gi::input::parachute::UpdateParachutePhysics

use std::{
    f32::consts::{PI, TAU},
    sync::atomic::{AtomicU64, Ordering},
};

use detours_macro::detour;
use jc3gi::{
    character::character::Character,
    equipment::grappling_hook::GrapplingHookState,
    input::parachute::{CharacterAirMovementSettings, CharacterParachuteSettings},
    types::math::{Vector2, Vector3},
};
use re_utilities::hook_library::HookLibrary;

use crate::{config::Config, grapple::hook, hooks::input::locomotion::head_decoupled};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&GET_PARACHUTE_STEERING_VALUES_BINDER)
}

/// How often the parachute look-steer was removed for the local player while the head was
/// decoupled. Read by the Game tab's movement readout.
pub static PARACHUTE_LOOK_STEER_SUPPRESSIONS: AtomicU64 = AtomicU64::new(0);

/// How often the canopy was turned toward a grapple line it reels along. Read by the Air feature's
/// readout.
pub static PARACHUTE_LINE_TURNS: AtomicU64 = AtomicU64::new(0);

#[detour(address = jc3gi::input::parachute::GetParachuteSteeringValues_ADDRESS)]
fn get_parachute_steering_values(
    character: *mut Character,
    para_settings: *const CharacterParachuteSettings,
    input: *const Vector2,
    dt: f32,
    steering_values_out: *mut Vector3,
    wanted_char_yawpitchroll_out: *mut Vector3,
    look_steer_out: *mut Vector2,
) {
    GET_PARACHUTE_STEERING_VALUES.get().unwrap().call(
        character,
        para_settings,
        input,
        dt,
        steering_values_out,
        wanted_char_yawpitchroll_out,
        look_steer_out,
    );
    if should_suppress(character) {
        remove_look_steer(para_settings, steering_values_out, look_steer_out);
        let canopy = Config::lock_query(|c| c.controllers.vehicles.canopy);
        if canopy.line_turn {
            // SAFETY: the game passes live pointers for the duration of the call.
            unsafe {
                turn_toward_line(
                    character,
                    para_settings,
                    input,
                    wanted_char_yawpitchroll_out,
                    steering_values_out,
                    look_steer_out,
                    canopy.line_turn_scale,
                )
            };
        }
    }
}

/// Whether to remove the parachute look-steer this call: the character is a head-decoupled (VR)
/// local player.
fn should_suppress(character: *mut Character) -> bool {
    unsafe { character.as_ref() }.is_some_and(|c| c.m_IsLocalCharacter && head_decoupled(c))
}

/// Subtract the camera look-steer's contribution from the parachute steering values and zero the
/// look-steer output. The look-steer block mixed `m_XLookYawPitchRollAmount[axis] * look.x +
/// m_YLookYawPitchRollAmount[axis] * look.y` into each steering axis before the function
/// returned; removing exactly that leaves the stick input and the velocity alignment. `look_steer`
/// itself is read downstream by the caller (`UpdateParachutePhysics`) as the yaw/pitch velocity
/// spring inputs and the slingshot blend, so it is zeroed too.
fn remove_look_steer(
    para_settings: *const CharacterParachuteSettings,
    steering_values_out: *mut Vector3,
    look_steer_out: *mut Vector2,
) {
    let (Some(settings), Some(steering), Some(look)) = (
        unsafe { para_settings.as_ref() },
        unsafe { steering_values_out.as_mut() },
        unsafe { look_steer_out.as_mut() },
    ) else {
        return;
    };
    let air = &settings.m_AirControl;
    for axis in 0..3 {
        steering.data[axis] -= air.m_XLookYawPitchRollAmount[axis] * look.data[0]
            + air.m_YLookYawPitchRollAmount[axis] * look.data[1];
    }
    look.data = [0.0, 0.0];
    PARACHUTE_LOOK_STEER_SUPPRESSIONS.fetch_add(1, Ordering::Relaxed);
}

/// Put back the look steer a camera looking along the reeling line would give, scaled by `scale`,
/// while the local character's hook reels in. Called after [`remove_look_steer`], so the look steer
/// is the line's alone.
///
/// # Safety
///
/// The pointers are the game's live arguments to `GetParachuteSteeringValues`, after it returned.
unsafe fn turn_toward_line(
    character: *mut Character,
    para_settings: *const CharacterParachuteSettings,
    input: *const Vector2,
    wanted_char_yawpitchroll_out: *mut Vector3,
    steering_values_out: *mut Vector3,
    look_steer_out: *mut Vector2,
    scale: f32,
) {
    let (Some(character), Some(settings), Some(input), Some(wanted), Some(steering), Some(look)) =
        (unsafe {
            (
                character.as_ref(),
                para_settings.as_ref(),
                input.as_ref(),
                wanted_char_yawpitchroll_out.as_ref(),
                steering_values_out.as_mut(),
                look_steer_out.as_mut(),
            )
        })
    else {
        return;
    };
    // SAFETY: the local character's hook, read on the game thread during its parachute update.
    let Some(hook) = (unsafe { hook::of(character) })
        .filter(|hook| hook.m_State == GrapplingHookState::GHS_REELING_IN)
    else {
        return;
    };
    let mut direction = Vector3 { data: [0.0; 3] };
    // SAFETY: as above; the direction is written into a local.
    if !unsafe { hook.GetReelingInDirection(&raw mut direction) } {
        return;
    }
    let Some(yaw) = line_turn(
        &settings.m_AirControl,
        direction.data,
        wanted.data[0],
        input.data[0],
    ) else {
        return;
    };
    let yaw = yaw * scale;
    look.data[0] += yaw;
    for axis in 0..3 {
        steering.data[axis] += settings.m_AirControl.m_XLookYawPitchRollAmount[axis] * yaw;
    }
    PARACHUTE_LINE_TURNS.fetch_add(1, Ordering::Relaxed);
}

/// The look-steer yaw the game's look-steer block gives a camera looking along `direction` (the
/// reeling line, a unit vector), for a canopy whose wanted yaw is `wanted_yaw` radians and whose
/// stick reads `stick_x` across. `None` for a vertical line, which has no heading to turn toward.
///
/// It mirrors the block's yaw half: the yaw from the wanted heading to the look, past
/// `m_LookSteerDeadZone`, over `m_LookSteerMaxYaw`, raised to `m_LookSteerExponential`, clamped,
/// and scaled to `m_LookSteerMaxSpeed`, opposite in sign to the yaw. The block yields nothing while
/// the stick steers across, and so does this. Headings are the game's: the yaw of a direction
/// `(x, z)` is `atan2(-x, -z)`.
fn line_turn(
    air: &CharacterAirMovementSettings,
    direction: [f32; 3],
    wanted_yaw: f32,
    stick_x: f32,
) -> Option<f32> {
    let [x, _, z] = direction;
    if !(x.is_finite() && z.is_finite() && wanted_yaw.is_finite()) || x.hypot(z) < MIN_HEADING_XZ {
        return None;
    }
    if stick_x.abs() >= STICK_STEERS || air.m_LookSteerMaxYaw <= 0.0 {
        return Some(0.0);
    }
    let delta_deg = wrap_pi((-x).atan2(-z) - wanted_yaw).to_degrees();
    let past = (delta_deg.abs() - air.m_LookSteerDeadZone[0]).max(0.0) / air.m_LookSteerMaxYaw;
    let shaped = past.powf(air.m_LookSteerExponential[0]).clamp(0.0, 1.0);
    let yaw = -delta_deg.signum() * shaped * air.m_LookSteerMaxSpeed[0];
    yaw.is_finite().then_some(yaw)
}

/// `angle` wrapped into `-PI..PI`.
fn wrap_pi(angle: f32) -> f32 {
    (angle + PI).rem_euclid(TAU) - PI
}

/// The stick's across reading from which the look-steer block leaves the yaw to the stick, as the
/// game's block does.
const STICK_STEERS: f32 = 0.1;

/// The smallest horizontal length of a unit line direction that still has a heading.
const MIN_HEADING_XZ: f32 = 1e-3;

#[cfg(test)]
mod tests {
    use super::*;

    fn look_settings() -> CharacterParachuteSettings {
        let mut settings = unsafe { std::mem::zeroed::<CharacterParachuteSettings>() };
        let air = &mut settings.m_AirControl;
        air.m_LookSteerDeadZone = [10.0, 10.0];
        air.m_LookSteerMaxYaw = 80.0;
        air.m_LookSteerExponential = [1.0, 1.0];
        air.m_LookSteerMaxSpeed = [2.0, 2.0];
        settings
    }

    /// A line beside the canopy turns it toward the line, opposite in sign to the yaw as the
    /// game's look steer is, and the two sides mirror each other.
    #[test]
    fn a_line_to_the_side_turns_toward_it() {
        let settings = look_settings();
        let air = &settings.m_AirControl;
        // Facing yaw 0 looks along -Z; a line along -X is a quarter turn to positive yaw.
        let left = line_turn(air, [-1.0, 0.0, 0.0], 0.0, 0.0).unwrap();
        let right = line_turn(air, [1.0, 0.0, 0.0], 0.0, 0.0).unwrap();
        // (90 - 10) / 80 = 1, scaled to the max speed and negated.
        assert!((left + 2.0).abs() < 1e-4, "{left}");
        assert!((right - 2.0).abs() < 1e-4, "{right}");
    }

    /// A line ahead, inside the dead zone, or while the stick steers across turns nothing; a
    /// vertical line has no heading.
    #[test]
    fn a_line_ahead_the_stick_or_a_vertical_line_does_not_turn() {
        let settings = look_settings();
        let air = &settings.m_AirControl;
        assert_eq!(line_turn(air, [0.0, 0.0, -1.0], 0.0, 0.0), Some(0.0));
        let near = (5.0f32).to_radians();
        assert_eq!(
            line_turn(air, [-near.sin(), 0.0, -near.cos()], 0.0, 0.0),
            Some(0.0)
        );
        assert_eq!(line_turn(air, [-1.0, 0.0, 0.0], 0.0, 0.5), Some(0.0));
        assert_eq!(line_turn(air, [0.0, 1.0, 0.0], 0.0, 0.0), None);
    }

    /// The yaw is measured from the wanted heading across the wrap at a half turn.
    #[test]
    fn the_yaw_wraps_across_a_half_turn() {
        let settings = look_settings();
        let air = &settings.m_AirControl;
        // Wanted yaw just short of +PI, line just past -PI: a small turn, not most of a circle.
        let wanted = PI - 0.05;
        let line = -PI + 0.05;
        let direction = [-line.sin(), 0.0, -line.cos()];
        assert_eq!(line_turn(air, direction, wanted, 0.0), Some(0.0));
    }

    fn settings_with_look(x: [f32; 3], y: [f32; 3]) -> CharacterParachuteSettings {
        let mut settings = unsafe { std::mem::zeroed::<CharacterParachuteSettings>() };
        settings.m_AirControl.m_XLookYawPitchRollAmount = x;
        settings.m_AirControl.m_YLookYawPitchRollAmount = y;
        settings
    }

    /// A pure look-steer contribution is removed exactly from each steering axis, and the
    /// look-steer output is zeroed.
    #[test]
    fn removes_look_steer_from_steering() {
        let settings = settings_with_look([1.0, 0.5, 0.25], [0.1, 0.2, 0.3]);
        let mut steering = Vector3 {
            data: [10.0, 20.0, 30.0],
        };
        let mut look = Vector2 { data: [2.0, 4.0] };
        remove_look_steer(&raw const settings, &raw mut steering, &raw mut look);
        // axis 0: 10 - (1*2 + 0.1*4) = 10 - 2.4 = 7.6
        // axis 1: 20 - (0.5*2 + 0.2*4) = 20 - 1.8 = 18.2
        // axis 2: 30 - (0.25*2 + 0.3*4) = 30 - 1.7 = 28.3
        assert!((steering.data[0] - 7.6).abs() < 1e-5);
        assert!((steering.data[1] - 18.2).abs() < 1e-5);
        assert!((steering.data[2] - 28.3).abs() < 1e-5);
        assert!(look.data[0] == 0.0 && look.data[1] == 0.0);
    }

    /// With zero look-steer the steering is unchanged and the output stays zero.
    #[test]
    fn zero_look_steer_leaves_steering() {
        let settings = settings_with_look([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let mut steering = Vector3 {
            data: [5.0, 6.0, 7.0],
        };
        let mut look = Vector2 { data: [0.0, 0.0] };
        remove_look_steer(&raw const settings, &raw mut steering, &raw mut look);
        assert!((steering.data[0] - 5.0).abs() < 1e-6);
        assert!((steering.data[1] - 6.0).abs() < 1e-6);
        assert!((steering.data[2] - 7.0).abs() < 1e-6);
    }
}
