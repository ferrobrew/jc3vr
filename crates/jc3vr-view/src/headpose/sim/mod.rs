//! The flatscreen headpose simulation: a latching mouse-look scheme.
//!
//! This module owns everything that is *simulation-specific*: mouse-look input handling, the
//! latch state machine, mode detection, body-yaw target computation, and the per-tick
//! [`on_input_tick`] that produces a [`HeadPose`](crate::headpose::HeadPose) and publishes it via
//! [`crate::headpose::set_pose`].
//!
//! The key insight: in VR, the player's head and body are independent — the head moves freely (HMD),
//! and the body is yawed via a stick. In flatscreen, we don't have independent head/body controls,
//! so the latch unifies them: within a configurable cone the head moves freely (decoupled), past
//! the threshold the body yaw tracks the head direction. This coupling is entirely a sim concern —
//! the headpose abstraction knows nothing about it.
//!
//! The accumulated yaw is **body-relative**: the head's yaw offset from the character's facing.
//! On foot, the offset is compensated when the body turns (the head stays world-anchored while the
//! body catches up to it, which is what makes the latch converge and disengage); in other modes
//! (vehicles, wingsuit) the offset rides the body frame, giving cockpit-relative free-look that
//! turns with the vehicle. The published orientation composes the offset onto the body's *full*
//! world rotation, so the body's pitch and roll carry the head with them (a banking wingsuit rolls
//! the view), and the free-look limits stay centred on the body's facing rather than on a fixed
//! world direction.
//!
//! Input arrives on the engine's fixed-rate sim tick (the device poll in
//! `InputDeviceManager::Update`), not per rendered frame. The whole sim step ([`on_input_tick`])
//! runs inside that hook, in the engine's input job in play, on the engine's own tick timeline: the
//! published pose pair ([`crate::headpose::snapshot_prev`]) rotates at the exact moment the engine
//! resets its sub-frame interpolation fraction, so the camera hook's previous/current pair is
//! phase-aligned with the `dtf` lerp that smooths it across the frames between ticks. (Deferring
//! the step to the next frame's update left the render lerping a stale pair at `dtf ≈ 0` on every
//! tick frame — a per-tick jerk.)

use body_rig::math::angle::{direction, heading, wrap};
use glam::{Quat, Vec3};
use jc3vr_core::published::Stamp;
use parking_lot::Mutex;

use crate::{
    headpose::{
        HeadPose,
        config::HEADPOSE_CONFIG,
        participants::head_participants,
        xr::{YawTick, advance_body_yaw, follow_turn},
    },
    tuning::VIEW,
};

/// The latch state (on-foot only).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum LatchState {
    /// The head moves freely within the decoupled cone; the body does not follow.
    Decoupled,
    /// The head has exceeded the latch threshold; the body is yawing to follow.
    BodyFollowing,
}

/// The current head mode, detected from the locomotion orientation evaluator's liveness.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum HeadMode {
    /// On foot: the latch applies.
    OnFoot,
    /// Other (vehicle, wingsuit, etc.): free-look with clamping. The default, since it is the
    /// mode that assumes least: the on-foot latch and body-turn coupling only apply once the
    /// locomotion evaluator has actually been seen to tick.
    #[default]
    Other,
}

/// Handle one engine input tick, called from the `InputDeviceManager::Update` hook (in the poll
/// stage, [`crate::controllers::bindings::stage`]: the engine's input job in play, inside the
/// engine's fixed-rate sim tick): detect the mode, rotate the published pose
/// pair, integrate the look deltas, update the latch, and publish the headpose.
pub fn on_input_tick(look_x: f32, look_y: f32, look_x_delta: bool, dt: f32) {
    let config = *HEADPOSE_CONFIG.lock();
    if !config.enabled {
        return;
    }
    let participants = head_participants();
    // Read before the SIM lock: it takes the config lock and the torso estimate's slot.
    let follow = (participants.follow_settings)();

    let mut s = SIM.lock();

    // Mode detection, once per input tick: the locomotion orientation evaluator runs once per sim
    // tick while on foot (including idle — the move/aim task counters stop while idle), and never
    // in vehicles. This hook fires on the same sim-tick cadence, so between ticks the evaluator's
    // run advances exactly when the player is on foot. Comparing per rendered frame instead flickered
    // to `Other` on every frame without a sim tick, resetting the latch and letting the idle
    // camera pin leak through (the body tracked the head immediately).
    //
    // This runs regardless of the active source, *before* the VR early-return below: `sim::mode()`
    // is read by `head_decoupled_idle` (the body-turn suppression gate), so freezing it under VR
    // left the game's aim-relative body turn permanently unsuppressed, which the body-relative pose
    // composition (`body × cockpit`) turns into a runaway head spin.
    let on_foot_ran = (participants.on_foot_ran)();
    s.mode = detect_mode(s.last_on_foot_run, on_foot_ran);
    s.last_on_foot_run = on_foot_ran;

    // The in-water counterpart, from the swim movement cores (the swim family never runs the
    // orientation evaluator). Kept out of `HeadMode` deliberately: swimming only extends the VR
    // body-yaw accumulator below, while the flatscreen latch machinery stays off-foot in water.
    // The character's motion state counts too: the cores run on the character update's cadence,
    // which need not give every input tick a run, and a tick read as out of the water clears the
    // accumulator. Without a target the swim detours stand down for that tick and the core turns
    // the body toward the camera-relative stick; the view rides the body, so the camera turns with
    // it, and the next tick re-seeds the accumulator from the turned body, which compounds into a
    // circle.
    let swim_ran = (participants.swim_ran)();
    let swimming = swim_ran > s.last_swim_run || (participants.swimming)();
    s.last_swim_run = swim_ran;

    // Advance the grapple reel-in body-frame filter on this tick, likewise before the VR
    // early-return: the blend it smooths (and the pre-reel frame it holds) is applied by both
    // sources' body-frame composition (here at publish, and in `xr::body_rotation`). The VR frame
    // loop advances it per rendered frame as well, so under VR this tick advance is the flatscreen
    // fallback and the state read that keeps `sim` coverage.
    {
        // The view frame, with any turn of the body under the view taken out, so the frame the
        // filter holds is the one the view composes with.
        let body = read_body_rotation().unwrap_or(Quat::IDENTITY) * follow_turn(true);
        let rooted = crate::headpose::anchor_pair()
            .map(|pair| pair[1].view_head() - pair[1].root.w_axis.truncate());
        // `on_foot` is passed in rather than read back via `sim::mode()`: the SIM lock is held
        // here, and re-locking it inside `advance` deadlocks the input tick.
        (participants.advance_grapple)(body, rooted, s.mode == HeadMode::OnFoot, &config.grapple);
        (participants.log_grapple_tick)(body, dt);
    }

    // The VR source owns the pose while an OpenXR session is running (it publishes a fresh HMD pose
    // every rendered frame). The sim must not also publish, or the two would fight over the same
    // slot; skip the rest of the tick so the VR pose stands (mode detection above still ran). The
    // look effectors do not steer the HMD-driven head, so they are consumed here to turn the body
    // instead (the flatscreen head-yaw path below owns this for the sim source).
    if crate::headpose::source() == crate::headpose::Source::Vr {
        // Swimming keeps the accumulator seeded: the swim yaw override
        // (`crate::hooks::input::swim`) consumes the same body-yaw target, and clearing the
        // accumulator in water would hand the body back to the game's act-quantized turns.
        let vehicle_transition = (participants.vehicle_transition)();
        let body_thrown = (participants.body_thrown)();
        advance_body_yaw(
            YawTick {
                look_x,
                delta_based: look_x_delta,
                on_foot: s.mode == HeadMode::OnFoot,
                swimming,
                vehicle_transition,
                body_thrown,
                dt,
            },
            &config,
            &follow,
        );
        return;
    }

    let body_rotation = read_body_rotation();
    let body_yaw = body_rotation.map(heading);

    // Rotate the published pose pair on the engine's own tick timeline, phase-aligned with its
    // dtf reset (see the module docs).
    crate::headpose::snapshot_prev();

    // Negated: a positive net LOOK_RIGHT delta must turn the head clockwise from above, which is
    // a negative rotation about +Y (established in-game; the unnegated form turned the wrong way).
    let look = &VIEW.get().flatscreen_look;
    let yaw_delta = -(look_x * look.mouse_sensitivity).to_radians();
    let pitch_delta = (look_y * look.mouse_sensitivity).to_radians();

    match s.mode {
        HeadMode::OnFoot => {
            // Keep the head world-anchored while the body turns underneath it: remove the body's
            // rotation since the last tick from the body-relative offset. Without this, the offset
            // rides the turning body and the latch never converges.
            if let (Some(now), Some(then)) = (body_yaw, s.last_body_yaw) {
                s.yaw = wrap(s.yaw - wrap(now - then));
            }
            s.yaw = wrap(s.yaw + yaw_delta);
        }
        HeadMode::Other => {
            let yaw_limit = look.free_look_yaw_limit_deg.to_radians();
            s.yaw = (s.yaw + yaw_delta).clamp(-yaw_limit, yaw_limit);
        }
    }
    // The pitch is clamped in every mode: a real head cannot pitch past vertical, and letting the
    // euler pitch cross ±90° flips the yaw/roll decomposition.
    let pitch_limit = look.free_look_pitch_limit_deg.to_radians();
    s.pitch = (s.pitch + pitch_delta).clamp(-pitch_limit, pitch_limit);
    s.last_body_yaw = body_yaw;

    match s.mode {
        HeadMode::OnFoot => {
            s.latch = update_latch(s.latch, s.yaw.to_degrees(), s.mode);
            s.body_yaw_target = (s.latch == LatchState::BodyFollowing)
                .then(|| body_yaw.map(|b| direction(b + s.yaw)))
                .flatten();
        }
        HeadMode::Other => {
            s.latch = LatchState::Decoupled;
            s.body_yaw_target = None;
        }
    }

    // Publish: the head orientation rides the full body frame — the body-relative offset composed
    // onto the body's world rotation. On foot the body is upright and this reduces to a yaw
    // composition; in vehicles and the wingsuit it carries the body's pitch and roll too, so a
    // banking wingsuit rolls the view with it. The position anchors to the animated head bone.
    let head_offset = Quat::from_euler(glam::EulerRot::YXZ, s.yaw, s.pitch, s.roll);
    // The grapple filter cancels the body rotation the reel adds while reeling in (issue #36), so
    // the alignment toward the target never reaches the composed view; see `crate::grapple`.
    let body_rotation =
        (participants.filter_body_rotation)(body_rotation.unwrap_or(Quat::IDENTITY));
    let orientation = body_rotation * head_offset;
    let anchor = (participants.filter_anchor)(crate::headpose::anchor().unwrap_or(Vec3::ZERO));
    let position = anchor;
    drop(s);
    crate::headpose::set_pose(HeadPose {
        position,
        orientation,
    });
}

/// The current latch state, for UI display and the body-yaw hook.
pub fn latch_state() -> LatchState {
    SIM.lock().latch
}

/// The current head mode, for UI display and the body-yaw hook.
pub fn mode() -> HeadMode {
    SIM.lock().mode
}

/// The desired body forward on the ground plane when latched; `None` when decoupled. Read by the
/// locomotion hook, not by the headpose.
pub fn body_yaw_target() -> Option<Vec3> {
    SIM.lock().body_yaw_target
}

/// The current accumulated body-relative yaw/pitch/roll (radians), for UI display.
pub fn euler_angles() -> (f32, f32, f32) {
    let s = SIM.lock();
    (s.yaw, s.pitch, s.roll)
}

/// Reset the sim's accumulated state, re-centering the head on the body's facing. The last body
/// yaw is kept so the on-foot compensation stays continuous across the reset.
pub fn reset() {
    let mut s = SIM.lock();
    s.yaw = 0.0;
    s.pitch = 0.0;
    s.roll = 0.0;
    s.latch = LatchState::Decoupled;
    s.body_yaw_target = None;
}

struct SimState {
    /// Accumulated body-relative yaw (radians): the head's yaw offset from the character's facing.
    yaw: f32,
    /// Accumulated pitch (radians).
    pitch: f32,
    /// Accumulated roll (radians).
    roll: f32,
    /// The latch state (on-foot only).
    latch: LatchState,
    /// The current head mode.
    mode: HeadMode,
    /// The orientation evaluator's last run as of the last input tick (for mode detection).
    last_on_foot_run: Option<Stamp>,
    /// The swim movement cores' last run as of the last input tick (for the in-water body-yaw
    /// seeding).
    last_swim_run: Option<Stamp>,
    /// The body yaw at the last processed input tick, for the on-foot compensation.
    last_body_yaw: Option<f32>,
    /// The last computed body-yaw target, for [`body_yaw_target`].
    body_yaw_target: Option<Vec3>,
}

const SIM_DEFAULT: SimState = SimState {
    yaw: 0.0,
    pitch: 0.0,
    roll: 0.0,
    latch: LatchState::Decoupled,
    mode: HeadMode::Other,
    last_on_foot_run: None,
    last_swim_run: None,
    last_body_yaw: None,
    body_yaw_target: None,
};

static SIM: Mutex<SimState> = Mutex::new(SIM_DEFAULT);

/// Detect the head mode from the orientation evaluator's last run: when it advanced since the last
/// input tick, the on-foot locomotion orientation executor ran, so the player is on foot.
fn detect_mode(old: Option<Stamp>, new: Option<Stamp>) -> HeadMode {
    if new > old {
        HeadMode::OnFoot
    } else {
        HeadMode::Other
    }
}

/// Update the latch state given the body-relative yaw and the current mode. On-foot uses the
/// hysteresis latch; other modes are always decoupled.
fn update_latch(latch: LatchState, relative_yaw_deg: f32, mode: HeadMode) -> LatchState {
    if mode != HeadMode::OnFoot {
        return LatchState::Decoupled;
    }
    let look = &VIEW.get().flatscreen_look;
    match latch {
        LatchState::Decoupled => {
            if relative_yaw_deg.abs() >= look.latch_threshold_deg {
                LatchState::BodyFollowing
            } else {
                LatchState::Decoupled
            }
        }
        LatchState::BodyFollowing => {
            if relative_yaw_deg.abs() <= look.latch_disengage_threshold_deg {
                LatchState::Decoupled
            } else {
                LatchState::BodyFollowing
            }
        }
    }
}

/// Compose the latest input state in a captured current body frame without advancing input.
pub(crate) fn orientation_at(root: glam::Mat4) -> Quat {
    let (_, body, _) = root.to_scale_rotation_translation();
    let body = (head_participants().filter_body_rotation)(body);
    let state = SIM.lock();
    body * Quat::from_euler(glam::EulerRot::YXZ, state.yaw, state.pitch, state.roll)
}

/// Read the local player character's world rotation from `m_WorldMatrixT1`.
fn read_body_rotation() -> Option<Quat> {
    unsafe {
        let character = jc3gi::character::character::Character::GetLocalPlayerCharacter();
        let character = character.as_ref()?;
        let world = glam::Mat4::from(character.m_WorldMatrixT1);
        let (_, rotation, _) = world.to_scale_rotation_translation();
        Some(rotation)
    }
}

#[cfg(test)]
mod tests;
