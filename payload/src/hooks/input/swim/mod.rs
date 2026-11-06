//! Swim locomotion detours: the right stick owns the heading in water, behind
//! [`smooth_swim_yaw`](crate::hooks::input::MovementConfig::smooth_swim_yaw).
//!
//! The swim family is a separate locomotion pipeline that never touches the on-foot orientation
//! machinery, so nothing in [`crate::hooks::input::locomotion`] applies here. It turns the body two
//! ways the player does not own: the movement cores rotate toward the *camera*-relative stick, and
//! past a 65-degree threshold the input tasks dispatch 120-degree turn clips whose animated slerp
//! replaces the rotation outright. In VR the view rides the body frame, so both rotate the world
//! under the player. See `docs/engine/gameplay/swim-locomotion.md` for the engine side and
//! `docs/mod/body/swim-yaw.md` for the design.
//!
//! Four detours put the right stick in charge, all scoped to the local player:
//!
//! - [`do_rotate`] aims the cores' heading rotation at the body-yaw target. This is what turns the
//!   body.
//! - [`transform_input_dir_to_world_dir`] substitutes the same heading for the camera-relative move
//!   direction.
//! - [`get_delta_angle_from_orientation`] reports zero, so the turn acts never queue and their slerp
//!   never takes the rotation away from [`do_rotate`].
//! - [`refuse_swim_moves`] clears `MOVE_BACKWARD`, since the swim family has no backstroke, and
//!   `MOVE_LEFT` and `MOVE_RIGHT`, since the right stick turns and nothing strafes.
//!
//! [`process_motion`] only marks its window, so [`do_rotate`] can tell the heading rotation from the
//! velocity slerp nested inside it.
//!
//! After each core runs, [`propel_strokes`] joins the arm strokes' swim speed
//! ([`strokes::propulsion`]) to the velocity the core wrote, along the way the swimmer faces. The
//! cores take their speed from the animation's root motion and only the direction from the stick,
//! so a stick written smaller than full does not swim slower; the strokes set the velocity
//! instead. `CPfxCharacterInstance::SetOrientation` is deliberately not
//! hooked: it receives the orientation composed with the frame's swimming capsule offset, which for
//! the surface crawl pose is a quarter turn about X, so a yaw rewrite there is at gimbal lock.

use std::{
    cell::Cell,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use detours_macro::detour;
use glam::{Mat4, Vec3};
use jc3gi::{
    character::character::Character,
    input::{
        controller_utility::ControllerUtility,
        input_action_map::{Action, EffectorState, LocalPlayerActionMap},
    },
    physics,
    types::math::{Matrix4, Vector2, Vector3},
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        locomotion::strokes::{self, model::propel},
        mode::CharacterSnapshot,
    },
    headpose::body_yaw_target,
};

mod telemetry;

pub(crate) use self::telemetry::log_tick;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&UPDATE_SURFACE_MOVEMENT_BINDER)
        .with_static_binder(&UPDATE_UNDERWATER_MOVEMENT_BINDER)
        .with_static_binder(&TRANSFORM_INPUT_DIR_TO_WORLD_DIR_BINDER)
        .with_static_binder(&GET_DELTA_ANGLE_FROM_ORIENTATION_BINDER)
        .with_static_binder(&DO_ROTATE_BINDER)
        .with_static_binder(&PROCESS_MOTION_BINDER)
}

/// How often a swim movement core ran for the local player. The headpose sim reads this the way it
/// reads the orientation evaluator's counter: an advance since the last input tick means the player
/// is swimming, which keeps the VR body-yaw accumulator seeded in water.
pub static SWIM_EVAL_CALLS: AtomicU64 = AtomicU64::new(0);
/// How often the move direction was substituted for the body-yaw target.
pub static SWIM_DIRECTION_OVERRIDES: AtomicU64 = AtomicU64::new(0);
/// How often the heading rotation was aimed at the body-yaw target.
pub static SWIM_ROTATE_OVERRIDES: AtomicU64 = AtomicU64::new(0);
/// How often the turn-angle probe was suppressed, keeping a turn act from queuing.
pub static SWIM_TURN_SUPPRESSIONS: AtomicU64 = AtomicU64::new(0);
/// How often backward or sideways input was cleared before the game could see it.
pub static SWIM_MOVE_REFUSALS: AtomicU64 = AtomicU64::new(0);
/// How often a swim movement core ran for the local player with the override on but no body-yaw
/// target to give it, so the game's own camera-relative heading turned the body.
pub static SWIM_UNTARGETED: AtomicU64 = AtomicU64::new(0);

/// Whether the local player is swimming, by either of [`SwimSignals`]. Read by consumers outside
/// this module too (the VR frame loop levels the view frame on it).
///
/// Live state rather than a recency stamp: a stamp keeps reading "swimming" for its whole window
/// after the last core ran, and every gate below then leaks onto foot for that long -- the player
/// could not walk backwards out of the water, and the on-foot heading and aim-blend paths saw the
/// swim overrides.
pub fn swimming() -> bool {
    SwimSignals::local().is_some_and(SwimSignals::any)
}

/// The two signals that say the local player swims.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct SwimSignals {
    /// The character's motion state reads swimming, which both swim movement cores write each
    /// frame they run.
    pub motion_state: bool,
    /// The state bits read swimming, the signal the controllers' mode detector classifies by. They
    /// hold for every tick in the water, where the motion state follows the cores' cadence.
    pub state_bits: bool,
}

impl SwimSignals {
    /// The local player's signals, or `None` without a local player.
    pub(crate) fn local() -> Option<Self> {
        // SAFETY: the local player is live for the frame; only plain fields are read.
        let character = unsafe { Character::GetLocalPlayerCharacter().as_ref() }?;
        // SAFETY: as above; the control parameters are null or live with the character.
        let bits = unsafe { character.m_ControlParameters.as_ref() }.map(|control| {
            let vm = &control.m_UserVM;
            [
                vm.m_CurrentStateBitFlags0,
                vm.m_CurrentStateBitFlags1,
                vm.m_CurrentStateBitFlags2,
            ]
        });
        Some(Self::of(character.m_CurrentMotionState, bits))
    }

    fn of(motion_state: u32, state_bits: Option<[u64; 3]>) -> Self {
        Self {
            motion_state: motion_state == Character::MOTION_STATE_SWIMMING,
            state_bits: state_bits.is_some_and(|state_bits| {
                CharacterSnapshot {
                    state_bits,
                    ..CharacterSnapshot::default()
                }
                .swimming()
            }),
        }
    }

    pub(crate) const fn any(self) -> bool {
        self.motion_state || self.state_bits
    }
}

/// Clear the swim's refused moves ([`REFUSED_SWIM_MOVES`]) while swimming, so the game never sees
/// them: no act queues, no stroke animation plays, and the left stick only swims forward along the
/// heading the right stick turns. Called from the input-device update alongside the look effectors
/// the headpose consumes.
///
/// Backward cannot simply be reversed -- the swim family ships no backstroke, and pointing the move
/// direction against the heading made the character paddle in place, re-orient, or drift sideways.
/// Sideways would otherwise reach the game as forward motion, since [`transform_input_dir_to_world_dir`]
/// returns the heading for any stick direction.
pub(super) fn refuse_swim_moves(map: &mut LocalPlayerActionMap) {
    if engaged_target().is_none() {
        return;
    }
    for action in REFUSED_SWIM_MOVES {
        let Some(effector) = (unsafe { map.GetActionEffector(action).as_mut() }) else {
            continue;
        };
        if effector.m_Value == 0.0 {
            continue;
        }
        SWIM_MOVE_REFUSALS.fetch_add(1, Ordering::Relaxed);
        effector.m_Value = 0.0;
        effector.m_PrevValue = 0.0;
        effector.m_State = EffectorState::Idle;
    }
}

/// The moves the swim refuses: backward, which has no stroke, and both strafes, since the right
/// stick turns the body and the left stick's forward swims along the heading.
const REFUSED_SWIM_MOVES: [Action; 3] =
    [Action::MOVE_BACKWARD, Action::MOVE_LEFT, Action::MOVE_RIGHT];

#[detour(address = jc3gi::input::swim::UpdateSurfaceMovement_ADDRESS)]
fn update_surface_movement(
    character: *mut Character,
    dt: f32,
    target_angle_offset: f32,
    ang_corr_seg_id: *const u32,
    steering_seg_id: *const u32,
    warping_seg_id: *const u32,
) {
    with_swim_window(character, SwimCore::Surface, dt, || {
        UPDATE_SURFACE_MOVEMENT.get().unwrap().call(
            character,
            dt,
            target_angle_offset,
            ang_corr_seg_id,
            steering_seg_id,
            warping_seg_id,
        );
    });
}

#[detour(address = jc3gi::input::swim::UpdateUnderwaterMovement_ADDRESS)]
fn update_underwater_movement(
    character: *mut Character,
    dt: f32,
    target_angle_offset: f32,
    ang_corr_seg_id: *const u32,
    steering_seg_id: *const u32,
    p6: bool,
) {
    with_swim_window(character, SwimCore::Underwater, dt, || {
        UPDATE_UNDERWATER_MOVEMENT.get().unwrap().call(
            character,
            dt,
            target_angle_offset,
            ang_corr_seg_id,
            steering_seg_id,
            p6,
        );
    });
}

/// Substitute the body-yaw target for the camera-relative world move direction inside the surface
/// core, taking the camera out of where the swimmer goes. The underwater core builds its own
/// direction inline, so underwater the heading comes from [`do_rotate`] alone.
///
/// Gated on the swim window because this helper is general: fifteen call sites across locomotion,
/// jump, melee, grapple, aim, and AI steering, only two of them swim. The swim *input* tasks call it
/// outside the window and are deliberately left alone -- their output picks a turn act, which
/// [`get_delta_angle_from_orientation`] already suppresses, and feeds the dive check, which is
/// better served by the real camera-relative direction.
#[detour(address = ControllerUtility::TransformInputDirToWorldDir_ADDRESS)]
fn transform_input_dir_to_world_dir(
    out: *mut Vector3,
    input_dir: *const Vector3,
    input: *mut Vector2,
) -> *mut Vector3 {
    if let Some(target) = SWIM_WINDOW.with(Cell::get).and_then(|_| engaged_target()) {
        SWIM_DIRECTION_OVERRIDES.fetch_add(1, Ordering::Relaxed);
        if let Some(out) = unsafe { out.as_mut() } {
            out.data = target.to_array();
        }
        return out;
    }
    TRANSFORM_INPUT_DIR_TO_WORLD_DIR
        .get()
        .unwrap()
        .call(out, input_dir, input)
}

/// Aim the cores' heading rotation at the body-yaw target: the seam that turns the body, since the
/// cores hand this output straight to `CMatrix4f::CreateOrientation`. It also covers what the
/// direction override cannot reach -- a neutral stick aims the core at its own forward, aiming aims
/// it at the weapon target, and the underwater core never routes through the override at all.
///
/// The rate is replaced too. The shipped rates (~69 deg/s at the surface, ~30 underwater) cannot keep
/// up with the stick's ~250, and a target the body never catches wraps past 180 degrees, where the
/// shortest arc reverses. The replacement is capped at [`DO_ROTATE_EASE_ANGLE`] per call, because
/// [`physics::DoRotate`] scales its step by `min(error / 15 deg, 1)` without clamping to the error:
/// a larger step overshoots the target, and twice that diverges.
///
/// [`physics::DoRotate`] has three call sites: this one, the underwater core's, and the velocity
/// slerp nested in [`ProcessMotion`](physics::ProcessMotion), which the [`process_motion`] guard
/// excludes. The single surface site carries every branch the core can aim at, so a grapple or
/// planted-explosive warp taken while swimming is faced at the right-stick heading too.
#[detour(address = physics::DoRotate_ADDRESS)]
fn do_rotate(from: *const Vector3, to: *const Vector3, rate: f32, out: *mut Vector3) {
    let call = DO_ROTATE.get().unwrap();
    let window = SWIM_WINDOW
        .with(Cell::get)
        .filter(|_| !IN_PROCESS_MOTION.with(Cell::get));
    let (Some(window), Some(target)) = (window, engaged_target()) else {
        call.call(from, to, rate, out);
        if window.is_some() {
            record_facing(out);
        }
        return;
    };
    SWIM_ROTATE_OVERRIDES.fetch_add(1, Ordering::Relaxed);
    // SAFETY: the core passes a live vector or null.
    let game_to = unsafe { to.as_ref() }.map(|v| Vec3::from_array(v.data));
    let target = match window.core {
        SwimCore::Surface => {
            *SWIM_PITCH.lock() = 0.0;
            target
        }
        SwimCore::Underwater => pitched(target, advance_swim_pitch(window.dt).sin()),
    };
    let rate = (Config::lock_query(|c| c.movement.swim_turn_rate_deg_s)
        .max(0.0)
        .to_radians()
        * window.dt)
        .clamp(f32::EPSILON, DO_ROTATE_EASE_ANGLE);
    let given = target;
    let target = Vector3 {
        data: target.to_array(),
    };
    call.call(from, &target, rate, out);
    record_facing(out);
    // SAFETY: the core passes live vectors, and the original has written `out`.
    let read = |v: *const Vector3| unsafe { v.as_ref() }.map(|v| Vec3::from_array(v.data));
    if let (Some(from), Some(game_to), Some(out)) = (read(from), game_to, read(out)) {
        telemetry::record_rotate(window.core, from, game_to, given, out);
    }
}

/// The angle below which [`physics::DoRotate`] eases its step out, and the largest step that cannot
/// overshoot the target. Radians.
const DO_ROTATE_EASE_ANGLE: f32 = 0.261_799_4;

/// Keep the heading the core's rotation just produced, which the core builds the swimmer's
/// orientation from, for [`propel_strokes`].
fn record_facing(out: *const Vector3) {
    // SAFETY: the core passes a live vector or null, and the original has written it.
    let facing = unsafe { out.as_ref() }.map(|v| Vec3::from_array(v.data));
    FACING.with(|f| f.set(facing.filter(|v| v.is_finite())));
}

/// Report the turn angle as zero so the input tasks stay below their act threshold and never queue a
/// 120-degree turn clip. That matters beyond the animation: while such an act runs, the core replaces
/// its heading rotation with the animated-turn slerp, which [`do_rotate`] cannot reach.
#[detour(address = ControllerUtility::GetDeltaAngleFromOrientation_ADDRESS)]
fn get_delta_angle_from_orientation(character: *const Character, dir: *const Vector3) -> f32 {
    let local = unsafe { character.as_ref() }.is_some_and(|c| c.m_IsLocalCharacter);
    if local && engaged_target().is_some() {
        SWIM_TURN_SUPPRESSIONS.fetch_add(1, Ordering::Relaxed);
        return 0.0;
    }
    GET_DELTA_ANGLE_FROM_ORIENTATION
        .get()
        .unwrap()
        .call(character, dir)
}

/// Mark the velocity-slerp window so [`do_rotate`] leaves the nested rotation alone.
#[detour(address = physics::ProcessMotion_ADDRESS)]
fn process_motion(
    character: *mut Character,
    dt: f32,
    current_vel: *const Vector3,
    wanted_vel: *const Vector3,
    slerp_delta_angle: f32,
    xz_only: bool,
    vel_out: *mut Vector3,
) {
    let call = PROCESS_MOTION.get().unwrap();
    IN_PROCESS_MOTION.with(|c| c.set(true));
    call.call(
        character,
        dt,
        current_vel,
        wanted_vel,
        slerp_delta_angle,
        xz_only,
        vel_out,
    );
    IN_PROCESS_MOTION.with(|c| c.set(false));
}

/// The body-yaw target while the override is engaged: the toggle is on and the headpose has a
/// heading (the VR accumulator is seeded, or the flatscreen latch is following). `None` leaves every
/// detour inert and the native swim behaviour standing.
fn engaged_target() -> Option<Vec3> {
    if !(swimming() && Config::lock_query(|c| c.movement.smooth_swim_yaw)) {
        return None;
    }
    body_yaw_target()
}

/// Which swim movement core is running: the surface core flattens the heading against the water
/// plane, the underwater core takes it whole and swims along it, so only the latter needs a pitch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SwimCore {
    Surface,
    Underwater,
}

impl SwimCore {
    const fn label(self) -> &'static str {
        match self {
            SwimCore::Surface => "surface",
            SwimCore::Underwater => "underwater",
        }
    }
}

/// The core running for the local player on this thread and the frame delta it was called with.
#[derive(Clone, Copy)]
struct SwimWindow {
    core: SwimCore,
    dt: f32,
}

/// Run a wrapped swim movement core inside the per-thread window the nested detours gate on. NPC
/// characters run the core without the window.
fn with_swim_window(character: *mut Character, core: SwimCore, dt: f32, call: impl FnOnce()) {
    if !unsafe { character.as_ref() }.is_some_and(|c| c.m_IsLocalCharacter) {
        call();
        return;
    }
    SWIM_EVAL_CALLS.fetch_add(1, Ordering::Relaxed);
    if Config::lock_query(|c| c.movement.smooth_swim_yaw) && body_yaw_target().is_none() {
        SWIM_UNTARGETED.fetch_add(1, Ordering::Relaxed);
    }
    SWIM_WINDOW.with(|w| w.set(Some(SwimWindow { core, dt })));
    FACING.with(|f| f.set(None));
    call();
    SWIM_WINDOW.with(|w| w.set(None));
    // SAFETY: the core has returned; the local character is live for the rest of its update.
    if let Some(character) = unsafe { character.as_mut() } {
        unsafe { propel_strokes(character, core) };
    }
}

/// Join the arm strokes' swim speed to the velocity the core just wrote to the proxy, along the way
/// the swimmer faces: level at the surface, so the core's buoyancy keeps the vertical, and as the
/// core pitched it underwater, where the swimmer moves where it faces.
///
/// The core writes the proxy's wanted velocity in the frame of the orientation it then hands to
/// `SetOrientation`, the capsule frame, so the velocity is read back through the proxy's transform
/// and written through [`Character::SetWantedWorldVelocity`], which converts with the same one.
///
/// # Safety
///
/// `character` must be the live local character, right after its swim core ran.
unsafe fn propel_strokes(character: &mut Character, core: SwimCore) {
    let Some(speed) = strokes::propulsion(Instant::now()) else {
        return;
    };
    let Some(proxy) = (unsafe { character.m_PfxCharacter.px.as_ref() }) else {
        return;
    };
    let facing = FACING
        .with(Cell::get)
        .unwrap_or_else(|| -Mat4::from(character.m_WorldMatrixT1).z_axis.truncate());
    let direction = match core {
        SwimCore::Surface => Vec3::new(facing.x, 0.0, facing.z),
        SwimCore::Underwater => facing,
    }
    .normalize_or_zero();
    if direction == Vec3::ZERO {
        return;
    }
    let mut transform = Matrix4::IDENTITY;
    unsafe { proxy.GetTransform(&mut transform) };
    let local = Vec3::from_array(proxy.m_Input.wantedVelocity.data);
    let wrote = Mat4::from(transform).transform_vector3(local);
    if !wrote.is_finite() {
        return;
    }
    let velocity = propel(wrote, direction, speed);
    strokes::record_applied(wrote.dot(direction), velocity.dot(direction));
    if velocity != wrote {
        let world = Vector3 {
            data: velocity.to_array(),
        };
        unsafe { character.SetWantedWorldVelocity(&world) };
    }
}

thread_local! {
    /// The local player's swim movement core running on this thread; `None` outside it. Per-thread
    /// because character updates run on worker threads.
    static SWIM_WINDOW: Cell<Option<SwimWindow>> = const { Cell::new(None) };

    /// Whether this thread is inside [`physics::ProcessMotion`], whose nested [`physics::DoRotate`]
    /// rotates the velocity direction rather than the heading.
    static IN_PROCESS_MOTION: Cell<bool> = const { Cell::new(false) };

    /// The heading the running core's rotation produced this run, world space; `None` until it
    /// rotates (a core playing an animated turn, or outside its steering segment, keeps its
    /// forward without rotating).
    static FACING: Cell<Option<Vec3>> = const { Cell::new(None) };
}

/// The head pitch, in degrees, within which the underwater dive control does nothing, so a resting
/// head does not slowly dive or surface the swimmer.
const SWIM_PITCH_DEADZONE_DEG: f32 = 12.0;

/// The furthest the underwater dive control will pitch the body, in degrees. Kept short of vertical,
/// where a heading is undefined.
const SWIM_PITCH_LIMIT_DEG: f32 = 55.0;

/// How fast, in degrees per second, the underwater dive pitch follows the head. Slower than the yaw
/// rate on purpose: a dive that snaps as fast as a turn reads as the world tipping over.
const SWIM_PITCH_RATE_DEG_S: f32 = 60.0;

/// The held underwater dive pitch, radians. Zero at the surface.
static SWIM_PITCH: Mutex<f32> = Mutex::new(0.0);

/// Step the held dive pitch toward what the player's head is asking for, in radians.
///
/// The command is the head's pitch *relative to the body*, which the body's own motion cannot
/// change. Taking it from the camera's world pitch instead is a runaway, because the view rides the
/// body frame: pitching the body toward where the camera points pitches the camera further.
fn advance_swim_pitch(dt: f32) -> f32 {
    let (deadzone, limit, rate) = (
        SWIM_PITCH_DEADZONE_DEG.to_radians(),
        SWIM_PITCH_LIMIT_DEG.to_radians(),
        SWIM_PITCH_RATE_DEG_S.to_radians(),
    );
    let head = crate::headpose::body_relative_rotation() * Vec3::NEG_Z;
    let head_pitch = head.normalize_or_zero().y.clamp(-1.0, 1.0).asin();
    let wanted =
        ((head_pitch.abs() - deadzone).max(0.0) * head_pitch.signum()).clamp(-limit, limit);

    let mut held = SWIM_PITCH.lock();
    let step = (rate * dt.max(0.0)).max(0.0);
    *held += (wanted - *held).clamp(-step, step);
    *held
}

/// `flat` (a unit ground-plane direction) tilted out of the plane by `pitch_sin`, keeping its
/// heading.
fn pitched(flat: Vec3, pitch_sin: f32) -> Vec3 {
    let pitch_sin = pitch_sin.clamp(-1.0, 1.0);
    (flat * (1.0 - pitch_sin * pitch_sin).max(0.0).sqrt() + Vec3::Y * pitch_sin).normalize_or(flat)
}

#[cfg(test)]
mod tests {
    use jc3gi::state::StateBitFlag;

    use super::*;

    fn bits(flag: StateBitFlag) -> [u64; 3] {
        let bit = flag as u32 as usize;
        let mut words = [0; 3];
        words[bit / 64] |= 1 << (bit % 64);
        words
    }

    /// Either signal alone counts as swimming, so a tick on which the cores did not run still
    /// engages the swim steering while the state bits say the player is in the water.
    #[test]
    fn either_swim_signal_engages_the_swim() {
        let swimming = Character::MOTION_STATE_SWIMMING;
        let other = swimming + 1;
        assert!(SwimSignals::of(swimming, None).any());
        assert!(SwimSignals::of(other, Some(bits(StateBitFlag::SB_IS_SWIMMING))).any());
        assert!(SwimSignals::of(other, Some(bits(StateBitFlag::SB_IS_UW_SWIMMING))).any());
        assert!(!SwimSignals::of(other, Some(bits(StateBitFlag::SB_IS_WINGSUIT))).any());
        assert!(!SwimSignals::of(other, None).any());
    }

    /// The left stick swims only forward: back and both strafes are refused, and forward is not.
    #[test]
    fn the_swim_refuses_back_and_strafe_but_not_forward() {
        assert!(REFUSED_SWIM_MOVES.contains(&Action::MOVE_BACKWARD));
        assert!(REFUSED_SWIM_MOVES.contains(&Action::MOVE_LEFT));
        assert!(REFUSED_SWIM_MOVES.contains(&Action::MOVE_RIGHT));
        assert!(!REFUSED_SWIM_MOVES.contains(&Action::MOVE_FORWARD));
    }

    /// Tilting keeps the heading and produces the asked-for vertical component.
    #[test]
    fn pitched_keeps_heading_and_applies_pitch() {
        let flat = Vec3::new(0.6, 0.0, -0.8);
        let tilted = pitched(flat, 0.5);
        assert!((tilted.length() - 1.0).abs() < 1e-5);
        assert!((tilted.y - 0.5).abs() < 1e-5);
        let heading = Vec3::new(tilted.x, 0.0, tilted.z).normalize();
        assert!(heading.angle_between(flat) < 1e-4);
    }

    /// A zero pitch leaves the direction flat, so the surface case is a no-op.
    #[test]
    fn pitched_by_zero_is_unchanged() {
        let flat = Vec3::new(0.0, 0.0, -1.0);
        assert!(pitched(flat, 0.0).distance(flat) < 1e-6);
    }
}
