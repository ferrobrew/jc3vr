//! The root: who moves the character's root each simulation tick, the chase that keeps it under the
//! player's head, and the cockpit shift that keeps the camera on the head while it does
//! (`docs/mod/body/rig.md`).
//!
//! - [`owner`]: the one owner of the root per tick;
//! - [`chase`]: the gap to the ground point under the upright eyes and the velocity that closes it;
//! - [`ledger`]: what each step asked for and how much of the completed movement was the chase's;
//! - [`shift`]: the cockpit shift that takes the credited chase back out of the view;
//! - [`pushback`]: holding the view at a wall the chase cannot pass.
//!
//! The simulation half runs from a post-call detour on the on-foot locomotion task
//! (`NStateTask_MovementLocomotionTask::Update`), which writes the character proxy's wanted velocity
//! every on-foot tick, idle included. Velocity written there goes through the proxy's character
//! context, which handles stairs, slopes, and support. On each tick it:
//!
//! - keeps the teleport and warp bookkeeping (a teleport clears the ledger and forgets the probe);
//! - replaces the clip's velocity with the stick's ([`after_task`]) and adds the palms' push
//!   ([`physical_hands::push_body`]), which together are the task's movement;
//! - picks the root's owner ([`owner::select`]), and while the player owns it adds the chase
//!   velocity through the engine's world-velocity accessors (the proxy's wanted velocity is
//!   character-local) and submits the split to the ledger;
//! - steps the head-through-geometry probe, since scene queries run against the live physics world;
//! - requests or releases the crouch capsule, on the simulation thread where the engine applies it.
//!
//! The task writes nothing while the character is attached to a vehicle, and every other state
//! that moves the proxy (the grapple, jumps, stunts, ragdoll, swimming, the parachute, the wingsuit)
//! runs a different task, so the game owns the root then and the hook acts only on a tick where the
//! task set the task-defined motion state, which it does exactly when it wrote the velocity.
//!
//! The pose-finalization entry completes the tick's request against the root's displacement
//! ([`complete_movement`]). The frame half ([`frame`]), called once per rendered frame by the pose
//! composition, folds the credited chase into the cockpit shift, measures the gap and the lean,
//! slides the view back at an obstruction, and publishes the gap for the next ticks.

pub mod chase;
pub mod ledger;
pub mod owner;
pub mod pushback;
pub mod shift;

#[cfg(test)]
mod tests;

use std::{
    ffi::c_void,
    time::{Duration, Instant},
};

use detours_macro::detour;
use glam::{Quat, Vec2, Vec3};
use jc3gi::{
    character::character::Character, physics::PfxCharacterInstance, state::StateContext,
    types::math::Vector3,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        body::{
            BodyDimensions,
            procedural::{head::looks_level, state::animation_moves_body},
            rig::{
                collision::probe::{self, HeadProbe, TimedProbe},
                config::RigConfig,
                lean::{Lean, StandingHeight, pivot_height},
                root::{
                    chase::{
                        ChaseFrame, HeadVelocity, MeasureInput, Measurement, measure, to_cockpit,
                    },
                    ledger::{CompletedMovement, LedgerCounts, Movement, Requests},
                    owner::{OwnerInput, RootOwner},
                    pushback::{Pushback, PushbackInput, PushbackOutcome},
                    shift::{CockpitBasis, CockpitShift},
                },
                torso::head_heading,
            },
        },
        config::RoomscaleConfig,
        locomotion::after_task,
        physical_hands::{self, climb},
        roomscale::{
            self, SeatPhase,
            crouch::capsule_step,
            fade::{self, FadeSource},
            posture::EffectivePosture,
            teleport,
        },
    },
    headpose, hooks,
    vr::RecenterKind,
};

pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&LOCOMOTION_TASK_UPDATE_BINDER)
}

/// One rendered frame's inputs to the frame half.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FrameInputs {
    /// The head's cockpit position, with the composed height in place of the raw one.
    pub raw: Vec3,
    /// The head's cockpit orientation.
    pub head: Quat,
    /// The body rotation the cockpit composes with.
    pub view: Quat,
    pub world_scale: f32,
    pub posture: EffectivePosture,
    /// The seat phase as of the previous frame.
    pub seat: SeatPhase,
    /// Whether the character is on foot, so a level gaze measures the standing height.
    pub on_foot: bool,
    /// The runtime's count of floor jumps.
    pub floor_jumps: u32,
    /// Seconds since the last frame.
    pub dt: f32,
    /// The torso's heading, cockpit frame, when the torso estimate has one; the head's otherwise.
    pub torso_yaw: Option<f32>,
    pub config: RigConfig,
}

/// The cockpit positions to compose the two sides of the pose pair with; the height is the
/// input's.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FrameOutput {
    pub prev: Vec3,
    pub cur: Vec3,
}

/// The frame half: fold the credited chase into the cockpit shift, measure the gap and the lean,
/// and publish them. Called once per rendered VR frame on the game thread, outside the world
/// update, from the pose composition ([`crate::controllers::roomscale::compose_cockpit`]).
pub fn frame(inputs: &FrameInputs) -> FrameOutput {
    let config = inputs.config;
    let torso_yaw = inputs
        .torso_yaw
        .or_else(|| head_heading(inputs.head))
        .unwrap_or(0.0);
    let epoch = teleport::epoch();
    let now = Instant::now();
    let basis = CockpitBasis {
        body: inputs.view,
        world_scale: inputs.world_scale,
    };
    let mut frame = FRAME.lock();
    if epoch != frame.seen_epoch {
        frame.seen_epoch = epoch;
        let completed = clear_movement();
        frame.shift.synchronize(completed);
        frame.head_velocity.reset();
        frame.pushback.end_episode();
        if inputs.posture == EffectivePosture::Standing {
            frame.shift.rebase_horizontal(inputs.raw);
            frame.status.teleport_rebases += 1;
        }
    } else {
        frame.shift.observe(&basis, completed_movement());
    }

    let standing =
        inputs.on_foot && inputs.posture != EffectivePosture::Seated && looks_level(inputs.head);
    let drop_m = frame.standing.observe(
        pivot_height(inputs.raw, inputs.head),
        standing,
        inputs.dt,
        inputs.floor_jumps,
    );
    let mut input = MeasureInput {
        raw: inputs.raw,
        shift: frame.shift.current(),
        observed: frame.shift.observed(),
        head: inputs.head,
        torso_yaw,
        view: inputs.view,
        world_scale: inputs.world_scale,
        drop_m,
        dt: inputs.dt,
    };
    let state = &mut *frame;
    let mut measurement = measure(&input, &mut state.head_velocity, &config.lean);

    // Out of a seat, the body walks to a head that is near; a head further off than the chase
    // should cover is put back over the body.
    let exited = state.last_seat == SeatPhase::Exiting && inputs.seat == SeatPhase::Out;
    state.last_seat = inputs.seat;
    if exited
        && inputs.posture == EffectivePosture::Standing
        && measurement.gap_m() * inputs.world_scale >= EXIT_REBASE_M
    {
        state.shift.rebase_horizontal(inputs.raw);
        state.status.exit_rebases += 1;
        input.shift = state.shift.current();
        input.dt = 0.0;
        measurement = measure(&input, &mut state.head_velocity, &config.lean);
    }

    // Where the chase cannot follow, the view is held at the obstruction.
    let (owner, stopped) = owner_and_obstruction(now);
    let pushback = state.pushback.step(
        &PushbackInput {
            gap: measurement.frame.gap,
            owner,
            stopped,
            dt: inputs.dt,
        },
        &config.pushback,
    );
    if pushback.push != Vec2::ZERO {
        state
            .shift
            .slide_view(to_cockpit(pushback.push, inputs.view, inputs.world_scale));
        input.shift = state.shift.current();
        input.dt = 0.0;
        measurement = measure(&input, &mut state.head_velocity, &config.lean);
    }
    fade::set_target(FadeSource::PushBack, pushback.fade);

    state.published = Some((measurement.frame, now));
    state.status = FrameReadout {
        at: Some(now),
        measurement,
        torso_yaw,
        standing_m: state.standing.reference(),
        world_scale: inputs.world_scale,
        shift: state.shift.current(),
        pushback,
        debt_m: state.pushback.debt(),
        pushbacks: state.pushback.episodes(),
        ..state.status
    };
    let (prev, cur) = state.shift.apply(inputs.raw);
    FrameOutput { prev, cur }
}

/// Re-base the cockpit shift after the recenter baseline was re-based: a full recenter makes the
/// head the cockpit origin, so the shift is spent; a yaw-and-height recenter keeps the horizontal
/// position and turns the cockpit frame by `turn` (new frame from old).
pub fn on_recenter(kind: RecenterKind, turn: Quat) {
    let mut frame = FRAME.lock();
    let completed = clear_movement();
    frame.shift.synchronize(completed);
    frame.head_velocity.reset();
    frame.pushback.end_episode();
    match kind {
        RecenterKind::Full => frame.shift.clear(),
        RecenterKind::YawAndHeight => frame.shift.rotate(turn),
    }
}

/// The lean the chase leaves alone on the latest frame, cockpit `(x, z)`, while the frame half
/// runs.
pub fn lean_allowance() -> Option<Vec2> {
    fresh_readout().map(|r| r.measurement.lean.vector)
}

/// Complete the tick's chase request after the post-physics serial pass schedules pose
/// finalization, at the same boundary where the camera's root and animated-head pair are captured.
/// This is not the HumanIK solve.
///
/// # Safety
///
/// Called only at the local character's pose-finalization entry, before anchor capture.
pub(crate) unsafe fn complete_movement(character: *mut Character) {
    let Some(character) = (unsafe { character.as_ref() }).filter(|c| c.m_IsLocalCharacter) else {
        return;
    };
    let previous = glam::Mat4::from(character.m_WorldMatrixT0)
        .w_axis
        .truncate();
    let current = glam::Mat4::from(character.m_WorldMatrixT1)
        .w_axis
        .truncate();
    let mut sim = SIM.lock();
    if teleport::epoch() != sim.seen_epoch {
        sim.movement.clear();
    }
    sim.movement.complete(previous, current);
}

/// The root's readout, for the debug UI, the signals, and the diagnostics.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct RootStatus {
    pub frame: FrameReadout,
    pub sim: SimStatus,
}

/// What the frame half measured on its latest frame.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct FrameReadout {
    /// When the frame half last ran.
    pub at: Option<Instant>,
    pub measurement: Measurement,
    /// The torso heading the target and the lean used, cockpit frame, radians.
    pub torso_yaw: f32,
    /// The standing height the lean's drop is measured from, metres over the cockpit's origin.
    pub standing_m: f32,
    pub world_scale: f32,
    /// The cockpit shift, cockpit frame, metres.
    pub shift: Vec3,
    /// Teleports that re-based the horizontal offset.
    pub teleport_rebases: u64,
    /// Seat exits that re-based a head too far from the body.
    pub exit_rebases: u64,
    /// What the push-back did on the frame.
    pub pushback: PushbackOutcome,
    /// How far the push-back has slid the view in its current episode, metres.
    pub debt_m: f32,
    /// Push-back episodes since the payload loaded.
    pub pushbacks: u64,
}

impl FrameReadout {
    /// Whether the frame half ran recently.
    pub fn fresh(&self, now: Instant) -> bool {
        self.at
            .is_some_and(|at| now.saturating_duration_since(at) < FRAME_GAP)
    }

    /// How far the ground point under the upright eyes is from the root, world metres.
    pub fn gap_m(&self) -> f32 {
        self.measurement.gap_m() * self.world_scale
    }

    /// How far it is outside the lean, world metres: what the chase still has to close.
    pub fn residual_m(&self) -> f32 {
        self.measurement.residual_m() * self.world_scale
    }
}

/// What the simulation half did on its last tick.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct SimStatus {
    /// When the last on-foot tick ran.
    pub at: Option<Instant>,
    /// Who owned the root on the last tick; `None` before the first.
    pub owner: Option<RootOwner>,
    /// The gap left after the chase completed since the frame, world `(x, z)`, metres.
    pub effective_gap: Vec2,
    /// The chase velocity added, world `(x, z)`, metres per second.
    pub chase_velocity: Vec2,
    /// The locomotion task's own horizontal velocity with the palms' push, world `(x, z)`.
    pub task_velocity: Vec2,
    /// How the ledger credited the completed steps.
    pub ledger: LedgerCounts,
    /// The head probe's last result, while the probe runs.
    pub probe: Option<TimedProbe>,
    /// Whether the mod holds the crouch capsule.
    pub capsule_owned: bool,
    /// Whether the last release of the capsule was held back for lack of headroom.
    pub capsule_blocked: bool,
}

pub fn status() -> RootStatus {
    let frame = FRAME.lock().status;
    let mut sim = SIM.lock();
    sim.status.ledger = sim.movement.counts();
    RootStatus {
        frame,
        sim: sim.status,
    }
}

/// Release a capsule the mod holds, from outside the locomotion task (the task no longer runs, or
/// the capsule was switched off).
///
/// # Safety
///
/// Must run on the game thread outside the world update.
pub(crate) unsafe fn release_capsule() {
    let mut sim = SIM.lock();
    if !sim.status.capsule_owned {
        return;
    }
    match unsafe { Character::GetLocalPlayerCharacter().as_mut() } {
        Some(character) => unsafe { release_owned(&mut sim, character) },
        None => sim.status.capsule_owned = false,
    }
}

/// Whether the crouch capsule follows the head.
pub(crate) fn capsule_enabled(config: &RoomscaleConfig) -> bool {
    config.crouch.capsule
}

/// A gap between rendered frames longer than this means the headset stopped rendering, and the
/// frame half's measurement stops holding for the simulation half.
const FRAME_GAP: Duration = Duration::from_millis(500);

/// The gap at a seat exit past which the head is put back over the body instead of the chase
/// walking the body to it, world metres.
const EXIT_REBASE_M: f32 = 1.0;

struct FrameState {
    seen_epoch: u64,
    shift: CockpitShift,
    head_velocity: HeadVelocity,
    standing: StandingHeight,
    last_seat: SeatPhase,
    pushback: Pushback,
    /// The latest measurement for the simulation half, and when it was taken.
    published: Option<(ChaseFrame, Instant)>,
    status: FrameReadout,
}

static FRAME: Mutex<FrameState> = Mutex::new(FrameState {
    seen_epoch: 0,
    shift: CockpitShift::new(),
    head_velocity: HeadVelocity::new(),
    standing: StandingHeight::new(),
    last_seat: SeatPhase::Out,
    pushback: Pushback::new(),
    published: None,
    status: FrameReadout {
        at: None,
        measurement: Measurement {
            target: Vec2::ZERO,
            lean: Lean {
                drop_m: 0.0,
                allowance_m: 0.0,
                vector: Vec2::ZERO,
            },
            feet: Vec2::ZERO,
            frame: ChaseFrame {
                gap: Vec2::ZERO,
                observed: Vec2::ZERO,
                head_velocity: Vec2::ZERO,
            },
        },
        torso_yaw: 0.0,
        standing_m: 0.0,
        world_scale: 1.0,
        shift: Vec3::ZERO,
        teleport_rebases: 0,
        exit_rebases: 0,
        pushback: PushbackOutcome {
            push: Vec2::ZERO,
            threshold: None,
            obstructed: false,
            fade: 0.0,
        },
        debt_m: 0.0,
        pushbacks: 0,
    },
});

fn fresh_readout() -> Option<FrameReadout> {
    let readout = FRAME.lock().status;
    readout.fresh(Instant::now()).then_some(readout)
}

/// Who owned the root on the latest on-foot tick, while the simulation half runs, and whether the
/// ledger finds the chase obstructed.
fn owner_and_obstruction(now: Instant) -> (Option<RootOwner>, bool) {
    let sim = SIM.lock();
    let fresh = sim
        .status
        .at
        .is_some_and(|at| now.saturating_duration_since(at) <= probe::STALE_AFTER);
    if fresh {
        (sim.status.owner, sim.movement.obstructed())
    } else {
        (None, false)
    }
}

/// The latest frame's measurement for the simulation half, while it is fresh.
fn published_chase(now: Instant) -> Option<ChaseFrame> {
    FRAME
        .lock()
        .published
        .filter(|(_, at)| now.saturating_duration_since(*at) < FRAME_GAP)
        .map(|(frame, _)| frame)
}

/// The ledger's running total, cleared first when the simulation half has gone quiet.
fn completed_movement() -> CompletedMovement {
    let mut sim = SIM.lock();
    if sim
        .status
        .at
        .is_none_or(|at| Instant::now().duration_since(at) > probe::STALE_AFTER)
    {
        sim.movement.clear();
    }
    sim.movement.completed()
}

/// Clear the ledger and return its running total.
fn clear_movement() -> CompletedMovement {
    let mut sim = SIM.lock();
    sim.movement.clear();
    sim.movement.completed()
}

struct Sim {
    probe: HeadProbe,
    /// The teleport epoch last acted on.
    seen_epoch: u64,
    /// The body's position on the last tick, for the warp test.
    last_position: Option<Vec3>,
    movement: Requests,
    status: SimStatus,
}

static SIM: Mutex<Sim> = Mutex::new(Sim {
    probe: HeadProbe::new(),
    seen_epoch: 0,
    last_position: None,
    movement: Requests::new(),
    status: SimStatus {
        at: None,
        owner: None,
        effective_gap: Vec2::ZERO,
        chase_velocity: Vec2::ZERO,
        task_velocity: Vec2::ZERO,
        ledger: LedgerCounts {
            exact: 0,
            projected: 0,
        },
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
    let (config, rig) = Config::lock_query(|c| (c.controllers.roomscale, c.controllers.body.rig));
    let now = Instant::now();
    let shared = roomscale::frame_shared();
    let chase_frame = published_chase(now);
    let climbing = climb::climbing();
    let mut sim = SIM.lock();
    // A gap means the task stopped running (a vehicle, a swim, a fall), and the body may have moved
    // any distance in between, which is no warp.
    if sim
        .status
        .at
        .is_none_or(|at| now.duration_since(at) > probe::STALE_AFTER)
    {
        sim.last_position = None;
        sim.movement.clear();
    }
    sim.status.at = Some(now);

    let wrote_velocity = character.m_CurrentMotionState == Character::MOTION_STATE_TASK_DEFINED
        && !unsafe { character.IsAttachedToVehicle() };
    let driven = headpose::source() == headpose::Source::Vr
        && hooks::in_gameplay()
        && shared.is_some_and(|f| f.fresh(now));
    let live = driven && wrote_velocity;
    let position = glam::Mat4::from(character.m_WorldMatrixT1)
        .w_axis
        .truncate();
    let proxy = unsafe { character.m_PfxCharacter.px.as_ref() };

    // The teleport bookkeeping runs whether or not the loop is live, so that it never mistakes the
    // first live tick after a teleport for a warp.
    let teleported = teleport::epoch() != sim.seen_epoch;
    if let (Some(last), false) = (sim.last_position, teleported) {
        let speed = proxy.map_or(0.0, |p| unsafe { velocity(p) }.length());
        if chase::is_warp((position - last).length(), speed, dt, rig.chase.warp_min_m) {
            teleport::record_warp();
        }
    }
    sim.last_position = Some(position);
    let epoch = teleport::epoch();
    if epoch != sim.seen_epoch {
        sim.seen_epoch = epoch;
        sim.movement.clear();
        sim.probe.reset();
        stand_down(&mut sim.status, RootOwner::Game);
        return;
    }

    let (Some(shared), true) = (shared, live) else {
        sim.movement.clear();
        sim.probe.reset();
        stand_down(&mut sim.status, RootOwner::Game);
        sim.status.probe = None;
        // Without the headset driving the view, nothing asks for the crouch any more, and the frame
        // half's release does not run (no VR frames) or sees this tick as fresh.
        if !driven {
            unsafe { release_owned(&mut sim, character) };
        }
        return;
    };

    let authored = unsafe { animation_moves_body(character, now) };
    // The stick's velocity replaces the clip's root velocity first; the palms' push off what they
    // press into then counts as the task's own movement, so it is added before the chase reads the
    // task's velocity.
    unsafe { after_task(character, dt) };
    unsafe { physical_hands::push_body(character) };

    let owner = owner::select(&OwnerInput {
        task_wrote: true,
        climbing,
        authored,
        posture: shared.posture,
        seat: shared.seat,
    });
    sim.status.owner = Some(owner);
    match chase_frame.filter(|_| owner.chases()) {
        Some(frame) => unsafe { chase_tick(&mut sim, character, &frame, &rig, dt) },
        None => {
            // The ledger records nothing for a tick the player did not move the root on, so a climb
            // or an authored motion is never credited as chase.
            sim.movement.clear();
            stand_down(&mut sim.status, owner);
        }
    }

    sim.status.probe = if config.head_fade.enabled {
        let ignore = [character as *const Character as usize, 0];
        let pose = headpose::query();
        let head = pose.position;
        let anchor = headpose::anchor().unwrap_or(head);
        let right = pose.orientation * Vec3::X;
        let right = Vec3::new(right.x, 0.0, right.z).normalize_or_zero();
        let result = sim.probe.step(head, anchor, right, |from, to| {
            // SAFETY: the simulation phase, and the ignored object is the live local character.
            unsafe { probe::scene_cast(from, to, ignore) }.unwrap_or_else(|error| {
                tracing::debug!(target: "controllers", "rig: head probe: {error}");
                None
            })
        });
        Some(TimedProbe { result, at: now })
    } else {
        sim.probe.reset();
        None
    };

    if capsule_enabled(&config) {
        unsafe { capsule_tick(&mut sim, character, shared.crouched, position) };
    }
}

/// Zero the chase's readout for a tick it did not run on.
fn stand_down(status: &mut SimStatus, owner: RootOwner) {
    status.owner = Some(owner);
    status.chase_velocity = Vec2::ZERO;
    status.effective_gap = Vec2::ZERO;
}

/// Add the chase velocity to the task's and submit the split.
unsafe fn chase_tick(
    sim: &mut Sim,
    character: &mut Character,
    frame: &ChaseFrame,
    rig: &RigConfig,
    dt: f32,
) {
    let completed = sim.movement.completed();
    let chase = chase::velocity(frame, completed, dt, &rig.chase);
    let mut wanted = Vector3 { data: [0.0; 3] };
    unsafe { character.GetWantedWorldVelocity(&mut wanted) };
    let [x, y, z] = wanted.data;
    let task = Vec2::new(x, z);
    let movement = Movement::new(task, chase, dt);
    if chase != Vec2::ZERO {
        let sum = movement.velocity(dt);
        let world = Vector3 {
            data: [sum.x, y, sum.y],
        };
        unsafe { character.SetWantedWorldVelocity(&world) };
    }
    sim.movement.submit(movement);
    sim.status.effective_gap = frame.effective_gap(completed.current);
    sim.status.chase_velocity = chase;
    sim.status.task_velocity = task;
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

unsafe fn velocity(proxy: &PfxCharacterInstance) -> Vec3 {
    let mut out = Vector3 { data: [0.0; 3] };
    unsafe { proxy.GetVelocity(&mut out) };
    Vec3::from_array(out.data)
}
