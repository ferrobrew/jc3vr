//! The roomscale loop's simulation-phase half, run from a post-call detour on the on-foot locomotion
//! task (`NStateTask_MovementLocomotionTask::Update`), which writes the character proxy's wanted
//! velocity every on-foot tick, idle included:
//!
//! - the teleport and warp bookkeeping (a teleport stops the chase and forgets the probe);
//! - the chase: the task's velocity with the chase velocity added, through the engine's own
//!   world-velocity accessors, because the proxy's wanted velocity is character-local;
//! - the head-through-geometry probe, since scene queries run against the live physics world;
//! - the crouch capsule, requested on the simulation thread where the engine applies it.
//!
//! The task writes nothing while the character is attached to a vehicle, and every other state
//! that moves the proxy (the grapple, jumps, stunts, ragdoll, swimming, the parachute, the wingsuit)
//! runs a different task, so none of them receives the chase. Within the task, the post-hook acts
//! only on a tick where the task set the task-defined motion state, which it does exactly when it
//! wrote the velocity.

use std::{ffi::c_void, time::Instant};

use detours_macro::detour;
use glam::{Vec2, Vec3};
use jc3gi::{
    character::character::Character,
    input::input_action_map::{Action, LocalPlayerActionMap},
    physics::PfxCharacterInstance,
    state::StateContext,
    types::math::Vector3,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        body::BodyDimensions,
        config::RoomscaleConfig,
        roomscale::{
            self, SeatPhase,
            chase::{self, Chaser},
            crouch::capsule_step,
            posture::EffectivePosture,
            probe::{self, HeadProbe, TimedProbe},
            teleport,
        },
    },
    headpose, hooks,
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&LOCOMOTION_TASK_UPDATE_BINDER)
}

/// What the simulation half did on its last tick, for the frame half and the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct SimStatus {
    /// When the last on-foot tick ran.
    pub at: Option<Instant>,
    /// The chase velocity added, world `(x, z)`, metres per second.
    pub chase_velocity: Vec2,
    /// The locomotion task's own horizontal velocity, world `(x, z)`.
    pub task_velocity: Vec2,
    pub chase_engaged: bool,
    /// The move stick's magnitude as the game read it, `0..=1`.
    pub stick: f32,
    /// The head probe's last result, while the probe runs.
    pub probe: Option<TimedProbe>,
    /// Whether the mod holds the crouch capsule.
    pub capsule_owned: bool,
    /// Whether the last release of the capsule was held back for lack of headroom.
    pub capsule_blocked: bool,
}

/// The last tick's status.
pub fn status() -> SimStatus {
    SIM.lock().status
}

/// The chase's share of the body's movement on the latest tick, consumed once by the frame half's
/// cockpit shift (see [`crate::controllers::roomscale::rebase`]).
pub(super) fn take_share() -> f32 {
    std::mem::take(&mut SIM.lock().share)
}

/// Release a capsule the mod holds, from outside the locomotion task (the task no longer runs, or
/// the capsule was switched off).
///
/// # Safety
///
/// Must run on the game thread outside the world update.
pub(super) unsafe fn release_capsule() {
    let mut sim = SIM.lock();
    if !sim.status.capsule_owned {
        return;
    }
    match unsafe { Character::GetLocalPlayerCharacter().as_mut() } {
        Some(character) => unsafe { release_owned(&mut sim, character) },
        None => sim.status.capsule_owned = false,
    }
}

struct Sim {
    chaser: Chaser,
    probe: HeadProbe,
    /// The teleport epoch last acted on.
    seen_epoch: u64,
    /// The body's position on the last tick, for the warp test.
    last_position: Option<Vec3>,
    share: f32,
    status: SimStatus,
}

static SIM: Mutex<Sim> = Mutex::new(Sim {
    chaser: Chaser::new(),
    probe: HeadProbe::new(),
    seen_epoch: 0,
    last_position: None,
    share: 0.0,
    status: SimStatus {
        at: None,
        chase_velocity: Vec2::ZERO,
        task_velocity: Vec2::ZERO,
        chase_engaged: false,
        stick: 0.0,
        probe: None,
        capsule_owned: false,
        capsule_blocked: false,
    },
});

#[detour(address = jc3gi::input::locomotion::NStateTask_MovementLocomotionTask::Update_ADDRESS)]
fn locomotion_task_update(ctx: *mut StateContext, properties: *mut c_void, state: *mut c_void) {
    LOCOMOTION_TASK_UPDATE
        .get()
        .unwrap()
        .call(ctx, properties, state);

    // SAFETY: the task's context names its character; the post-call runs on the task's own thread,
    // inside the character's serial state-machine pass.
    unsafe {
        let Some(dt) = ctx.as_ref().map(|c| c.m_Dt) else {
            return;
        };
        let Some(character) = crate::hooks::input::locomotion::character_from_context(ctx)
            .filter(|c| c.m_IsLocalCharacter)
        else {
            return;
        };
        after_locomotion(character, dt);
    }
}

/// The simulation half, for the local player after the locomotion task.
unsafe fn after_locomotion(character: &mut Character, dt: f32) {
    let config = Config::lock_query(|c| c.controllers.roomscale);
    let frame = roomscale::frame_shared();
    let mut sim = SIM.lock();
    let now = Instant::now();
    // A gap means the task stopped running (a vehicle, a swim, a fall), and the body may have moved
    // any distance in between, which is no warp.
    if sim
        .status
        .at
        .is_none_or(|at| now.duration_since(at) > probe::STALE_AFTER)
    {
        sim.last_position = None;
    }
    sim.status.at = Some(now);
    sim.share = 0.0;

    let wrote_velocity = character.m_CurrentMotionState == Character::MOTION_STATE_TASK_DEFINED
        && !unsafe { character.IsAttachedToVehicle() };
    let driven = headpose::source() == headpose::Source::Vr
        && hooks::in_gameplay()
        && frame.is_some_and(|f| f.fresh(now));
    let live = driven && wrote_velocity;
    let position = glam::Mat4::from(character.m_WorldMatrixT1)
        .w_axis
        .truncate();
    let proxy = unsafe { character.m_PfxCharacter.px.as_ref() };

    // The teleport bookkeeping runs whether or not the loop is live, so that it never mistakes the
    // first live tick after a teleport for a warp.
    let epoch = teleport::epoch();
    let teleported = epoch != sim.seen_epoch;
    if let (Some(last), false) = (sim.last_position, teleported) {
        let speed = proxy.map_or(0.0, |p| unsafe { velocity(p) }.length());
        if chase::is_warp(
            (position - last).length(),
            speed,
            dt,
            config.chase.warp_min_m,
        ) {
            teleport::record_warp();
        }
    }
    sim.last_position = Some(position);
    let epoch = teleport::epoch();
    if epoch != sim.seen_epoch {
        sim.seen_epoch = epoch;
        sim.chaser.reset();
        sim.probe.reset();
        sim.status.chase_velocity = Vec2::ZERO;
        return;
    }

    let (Some(frame), true) = (frame, live) else {
        sim.chaser.reset();
        sim.probe.reset();
        sim.status.chase_velocity = Vec2::ZERO;
        sim.status.chase_engaged = false;
        sim.status.probe = None;
        // Without the headset driving the view, nothing asks for the crouch any more, and the frame
        // half's release does not run (no VR frames) or sees this tick as fresh.
        if !driven {
            unsafe { release_owned(&mut sim, character) };
        }
        return;
    };

    let seated_out = frame.seat == SeatPhase::Out;
    if config.chase.enabled && frame.posture == EffectivePosture::Roomscale && seated_out {
        unsafe { chase_tick(&mut sim, character, frame.chase_offset, &config) };
    } else {
        sim.chaser.reset();
        sim.status.chase_velocity = Vec2::ZERO;
        sim.status.chase_engaged = false;
    }

    sim.status.probe = if config.head_fade.enabled {
        let ignore = [character as *const Character as usize, 0];
        let head = headpose::query().position;
        let anchor = headpose::anchor().unwrap_or(head);
        let result = sim.probe.step(head, anchor, |from, to| {
            // SAFETY: the simulation phase, and the ignored object is the live local character.
            unsafe { probe::scene_cast(from, to, ignore) }.unwrap_or_else(|error| {
                tracing::debug!(target: "controllers", "roomscale: head probe: {error}");
                None
            })
        });
        Some(TimedProbe { result, at: now })
    } else {
        sim.probe.reset();
        None
    };

    if config.crouch.capsule && config.crouch.visual {
        unsafe { capsule_tick(&mut sim, character, frame.crouched, position) };
    }
}

/// Add the chase velocity to the task's.
unsafe fn chase_tick(
    sim: &mut Sim,
    character: &mut Character,
    offset: Vec2,
    config: &RoomscaleConfig,
) {
    let stick = unsafe { move_stick() };
    let chase = sim.chaser.velocity(offset, stick, &config.chase);
    let mut wanted = Vector3 { data: [0.0; 3] };
    unsafe { character.GetWantedWorldVelocity(&mut wanted) };
    let [x, y, z] = wanted.data;
    let task = Vec2::new(x, z);
    if chase != Vec2::ZERO {
        let sum = chase::combine(task, chase, config.chase.max_speed_mps);
        let world = Vector3 {
            data: [sum.x, y, sum.y],
        };
        unsafe { character.SetWantedWorldVelocity(&world) };
    }
    sim.share = chase::share(task, chase);
    sim.status.chase_velocity = chase;
    sim.status.task_velocity = task;
    sim.status.chase_engaged = sim.chaser.engaged();
    sim.status.stick = stick;
}

/// Hand an owned crouch capsule back to the character's default state.
unsafe fn release_owned(sim: &mut Sim, character: &mut Character) {
    if !sim.status.capsule_owned {
        return;
    }
    let step = capsule_step(
        false,
        true,
        character.m_PendingProxyState,
        character.m_DefaultProxyState,
    );
    if let Some(value) = step.write {
        character.m_PendingProxyState = value;
    }
    sim.status.capsule_owned = false;
    sim.status.capsule_blocked = false;
}

/// Request or release the crouch capsule. Standing up waits for the headroom to stand in.
unsafe fn capsule_tick(sim: &mut Sim, character: &mut Character, crouched: bool, feet: Vec3) {
    let owned = sim.status.capsule_owned;
    let pending = character.m_PendingProxyState;
    let default = character.m_DefaultProxyState;
    let blocked = !crouched && owned && unsafe { headroom_blocked(character, feet) };
    let step = capsule_step(crouched || blocked, owned, pending, default);
    if let Some(value) = step.write {
        character.m_PendingProxyState = value;
    }
    sim.status.capsule_owned = step.owned;
    sim.status.capsule_blocked = blocked;
}

/// Whether something above the crouched character would stop it standing: a ray up from knee
/// height to the top of a standing character.
unsafe fn headroom_blocked(character: &Character, feet: Vec3) -> bool {
    let from = feet + Vec3::Y * HEADROOM_RAY_START_M;
    let to = feet + Vec3::Y * BodyDimensions::CHARACTER_ESTIMATE.height;
    let ignore = [character as *const Character as usize, 0];
    // SAFETY: the simulation phase; the ignored object is the live local character.
    matches!(unsafe { probe::scene_cast(from, to, ignore) }, Ok(Some(_)))
}

/// Where the headroom ray starts above the feet, metres: above a step the character stands on.
const HEADROOM_RAY_START_M: f32 = 0.5;

/// The move stick's magnitude as the game reads it, from the player action map's move half-axes.
unsafe fn move_stick() -> f32 {
    let Some(map) = (unsafe { LocalPlayerActionMap::get() }) else {
        return 0.0;
    };
    let mut value =
        |action| unsafe { map.GetActionEffector(action).as_ref() }.map_or(0.0, |e| e.m_Value);
    let stick = Vec2::new(
        value(Action::MOVE_RIGHT) - value(Action::MOVE_LEFT),
        value(Action::MOVE_FORWARD) - value(Action::MOVE_BACKWARD),
    );
    let magnitude = stick.length();
    if magnitude.is_finite() {
        magnitude.min(1.0)
    } else {
        0.0
    }
}

unsafe fn velocity(proxy: &PfxCharacterInstance) -> Vec3 {
    let mut out = Vector3 { data: [0.0; 3] };
    unsafe { proxy.GetVelocity(&mut out) };
    Vec3::from_array(out.data)
}
