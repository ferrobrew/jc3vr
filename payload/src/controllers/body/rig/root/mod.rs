//! The root: who moves the character's root each simulation tick, the step drive that keeps it
//! under the player's head, and the cockpit shift that keeps the camera on the head while it does
//! (`docs/mod/body/rig.md`).
//!
//! - [`owner`]: the one owner of the root per tick;
//! - [`drive`]: the step drive's gap between the frame half and the ticks, and the warp test;
//! - [`ledger`]: what each step asked for and how much of the completed movement was the drive's;
//! - [`shift`]: the cockpit shift that takes the credited movement back out of the view;
//! - [`pushback`]: holding the view at a wall the body cannot pass;
//! - [`trace`]: each tick's move against what the previous tick asked for, for the recorder and the
//!   `rig: the root jumped` log.
//!
//! The simulation half runs from a post-call detour on the on-foot locomotion task
//! (`NStateTask_MovementLocomotionTask::Update`), which writes the character proxy's wanted velocity
//! every on-foot tick, idle included. Velocity written there goes through the proxy's character
//! context, which handles stairs, slopes, and support. On each tick it:
//!
//! - keeps the teleport and warp bookkeeping (a teleport clears the ledger);
//! - replaces the clip's velocity with the stick's ([`after_task`]) and adds the palms' push
//!   ([`physical_hands::push_body`]), which together are the task's movement;
//! - picks the root's owner ([`owner::select`]), and while the player owns it adds the step drive's
//!   velocity through the engine's world-velocity accessors (the proxy's wanted velocity is
//!   character-local) and submits the split to the ledger;
//! - requests or releases the crouch capsule, on the simulation thread where the engine applies it.
//!
//! The task writes nothing while the character is attached to a vehicle, and every other state
//! that moves the proxy (the grapple, jumps, stunts, ragdoll, swimming, the parachute, the wingsuit)
//! runs a different task, so the game owns the root then and the hook acts only on a tick where the
//! task set the task-defined motion state, which it does exactly when it wrote the velocity.
//!
//! The pose-finalization entry completes the tick's request against the root's displacement
//! ([`complete_movement`]). The frame half ([`frame`]), called once per rendered frame by the pose
//! composition, folds the credited movement into the cockpit shift, advances the grounded rig
//! ([`crate::controllers::body::rig::grounded`]), slides the view back at an obstruction, and
//! publishes the step drive's gap for the next ticks.

mod completion;
pub mod drive;
pub mod ledger;
pub mod owner;
pub mod pushback;
pub mod shift;
pub mod trace;

#[cfg(test)]
mod tests;

use std::{
    ffi::c_void,
    time::{Duration, Instant},
};

use detours_macro::detour;
use glam::{Mat4, Quat, Vec2, Vec3};
use jc3gi::{
    character::character::Character,
    physics::{PfxCharacterInstance, hknpCharacterSurfaceInfo},
    state::StateContext,
    types::math::Vector3,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

pub(crate) use crate::controllers::body::rig::root::completion::{
    CompletionSnapshot, CompletionSnapshotAvailability, CompletionToken, CompletionUnavailable,
    FrameSourceSnapshot, MovementCompletion,
};
#[cfg(test)]
pub(crate) use crate::controllers::body::rig::root::completion::{
    test_source, test_source_with_root,
};
use crate::{
    config::Config,
    controllers::{
        aim::raycast::CastOptions,
        body::{
            BodyDimensions,
            procedural::state::animation_moves_body,
            rig::{
                collision::probe,
                config::RigConfig,
                grounded::{self, GroundedFrame, GroundedInput},
                root::{
                    drive::{StepFrame, to_cockpit, to_world},
                    ledger::{CompletedMovement, LedgerCounts, Movement, Requests},
                    owner::{OwnerInput, RootOwner},
                    pushback::{Pushback, PushbackInput, PushbackOutcome},
                    shift::{CockpitBasis, CockpitShift},
                    trace::{Jump, ProxyRead, RootTrace, Split, Support, TickInput, Tracer},
                },
            },
        },
        clock::TimeScale,
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
    vr::{RecenterCause, RecenterKind},
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
    /// The eyes in the room frame: the cockpit frame with its floor at zero, in the player's
    /// metres.
    pub eyes: Vec3,
    /// The cockpit height of the room's floor.
    pub floor: f32,
    /// The tracked grips, room frame.
    pub hands: [Option<Vec3>; 2],
    /// Whether the stick moves the body.
    pub moving: bool,
    pub dimensions: BodyDimensions,
    /// Seconds since the last frame.
    pub dt: f32,
    pub config: RigConfig,
}

/// The cockpit positions to compose the two sides of the pose pair with; the height is the
/// input's.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FrameOutput {
    pub prev: Vec3,
    pub cur: Vec3,
    pub(crate) source: FrameSourceSnapshot,
}

/// The frame half: fold the credited movement into the cockpit shift, advance the grounded rig,
/// hold the view at a wall, and publish the step drive's gap. Called once per rendered VR frame on
/// the game thread, outside the world update, from the pose composition
/// ([`crate::controllers::roomscale::compose_cockpit`]).
pub fn frame(inputs: &FrameInputs) -> FrameOutput {
    let config = inputs.config;
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
        frame.pushback.end_episode();
        frame.shift.rebase_horizontal(inputs.raw);
        frame.status.teleport_rebases += 1;
        grounded::forget();
    }
    let movement = SIM.lock().frame_movement(now, epoch);
    frame.shift.observe(&basis, movement.movement());

    let (owner, stopped) = owner_and_obstruction(now);
    let shift = frame.shift.current();
    let rig_input = GroundedInput {
        eyes: inputs.eyes,
        head: inputs.head,
        hands: inputs.hands,
        root: Vec2::new(shift.x, shift.z),
        floor: inputs.floor,
        moving: inputs.moving,
        sample_height: inputs.posture == EffectivePosture::Standing,
        grounded: owner.is_some_and(RootOwner::drives),
        dimensions: inputs.dimensions,
        tuning: config.tuning(),
        dt: inputs.dt,
        now,
    };
    let mut rig = grounded::frame(&rig_input);
    let state = &mut *frame;
    if let Some(snap) = rig.and_then(|r| r.snap_to) {
        state
            .shift
            .rebase_horizontal(Vec3::new(snap.x, 0.0, snap.y));
        state.pushback.end_episode();
        state.status.snaps += 1;
    }

    // Out of a seat, the body steps to a head that is near; a head further off than a step should
    // cover is put back over the body.
    let exited = state.last_seat == SeatPhase::Exiting && inputs.seat == SeatPhase::Out;
    state.last_seat = inputs.seat;
    let gap_m = rig.map_or(0.0, |r| r.output.solve.offset.length() * inputs.world_scale);
    if exited && gap_m >= EXIT_REBASE_M {
        state.shift.rebase_horizontal(inputs.raw);
        state.status.exit_rebases += 1;
        grounded::request_snap();
        rig = None;
    }

    // Where the drive cannot follow, the view is held at the obstruction.
    let to_world = |v: Vec2| to_world(v, inputs.view, inputs.world_scale);
    let demand = rig.map_or(Vec2::ZERO, |r| to_world(r.output.solve.step_demand));
    let pushback = state.pushback.step(
        &PushbackInput {
            gap: demand,
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
    }
    fade::set_target(FadeSource::PushBack, pushback.fade);

    let drives = config.steps.enabled;
    state.published = rig.filter(|_| drives).map(|r| {
        (
            StepFrame {
                phase: r.output.phase,
                gap: to_world(r.output.gap),
                observed: state.shift.observed(),
            },
            now,
        )
    });
    state.status = FrameReadout {
        at: Some(now),
        rig,
        world_scale: inputs.world_scale,
        shift: state.shift.current(),
        pushback,
        debt_m: state.pushback.debt(),
        pushbacks: state.pushback.episodes(),
        ..state.status
    };
    let (prev, cur) = state.shift.apply(inputs.raw);
    FrameOutput {
        prev,
        cur,
        source: movement.compose(basis, &state.shift),
    }
}

/// Re-base the cockpit shift after the recenter baseline was re-based, for a recenter or a follow
/// bank of `cause`: a full recenter makes the head the cockpit origin, so the shift is spent; a
/// yaw-and-height re-base keeps the horizontal position and turns the cockpit frame by `turn` (new
/// frame from old). A recenter of the player's or a seat's then snaps the pelvis under the neck on
/// the next grounded frame ([`grounded::request_snap`]); a follow bank only turns the frame.
pub fn on_recenter(kind: RecenterKind, turn: Quat, cause: RecenterCause) {
    let mut frame = FRAME.lock();
    let completed = clear_movement();
    frame.shift.synchronize(completed);
    frame.pushback.end_episode();
    match kind {
        RecenterKind::Full => frame.shift.clear(),
        RecenterKind::YawAndHeight => frame.shift.rotate(turn),
    }
    grounded::rotate(turn);
    if cause != RecenterCause::FollowBank {
        grounded::request_snap();
    }
}

/// Complete the tick's request using the roots already read by the local pre-IK capture.
/// Only a publishable completion returns an identity; host time never joins sources.
pub(crate) fn complete_movement(
    previous_root: Mat4,
    current_root: Mat4,
) -> Option<CompletionToken> {
    SIM.lock().complete_movement(
        previous_root,
        current_root,
        Instant::now(),
        teleport::epoch(),
    )
}

/// Copy the bounded completion record without reading engine pointers or frame-half state.
pub(crate) fn completion_snapshot() -> CompletionSnapshot {
    SIM.lock().completion_snapshot(teleport::epoch())
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
    /// The grounded rig's frame, while Rico is grounded.
    pub rig: Option<GroundedFrame>,
    pub world_scale: f32,
    /// The cockpit shift, cockpit frame, metres.
    pub shift: Vec3,
    /// Teleports that re-based the horizontal offset.
    pub teleport_rebases: u64,
    /// Seat exits that re-based a head too far from the body.
    pub exit_rebases: u64,
    /// Recenters that snapped the pelvis under the neck.
    pub snaps: u64,
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

    /// How far the neck is out over the pelvis, world metres.
    pub fn gap_m(&self) -> f32 {
        self.rig
            .map_or(0.0, |r| r.output.solve.offset.length() * self.world_scale)
    }

    /// The step demand: how far the neck is out past what a lean explains, world metres.
    pub fn residual_m(&self) -> f32 {
        self.rig
            .map_or(0.0, |r| r.output.solve.demand_m() * self.world_scale)
    }
}

/// What the simulation half did on its last tick.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct SimStatus {
    /// When the last on-foot tick ran.
    pub at: Option<Instant>,
    /// Who owned the root on the last tick; `None` before the first.
    pub owner: Option<RootOwner>,
    /// The gap left after the drive completed since the frame, world `(x, z)`, metres.
    pub effective_gap: Vec2,
    /// The step drive's velocity added, world `(x, z)`, metres per second.
    pub step_velocity: Vec2,
    /// The locomotion task's own horizontal velocity with the palms' push, world `(x, z)`.
    pub task_velocity: Vec2,
    /// How the ledger credited the completed steps.
    pub ledger: LedgerCounts,
    /// Whether the mod holds the crouch capsule.
    pub capsule_owned: bool,
    /// Whether the last release of the capsule was held back for lack of headroom.
    pub capsule_blocked: bool,
    /// The last on-foot tick's trace, while the loop is live.
    pub trace: Option<RootTrace>,
}

pub fn status() -> RootStatus {
    status_from(&FRAME, &SIM)
}

/// Copy the frame readout without waiting for its owner. `None` means the frame lock is busy.
pub(crate) fn try_frame_readout() -> Option<FrameReadout> {
    try_frame_readout_from(&FRAME)
}

/// Copy the simulation readout without waiting for its owner. `None` means the simulation lock is
/// busy. The ledger counts are projected into the copy without changing the stored status.
pub(crate) fn try_sim_readout() -> Option<SimStatus> {
    try_sim_readout_from(&SIM)
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

fn status_from(frame: &Mutex<FrameState>, sim: &Mutex<Sim>) -> RootStatus {
    let frame = frame.lock().status;
    let sim = sim.lock().readout();
    RootStatus { frame, sim }
}

fn try_frame_readout_from(frame: &Mutex<FrameState>) -> Option<FrameReadout> {
    frame.try_lock().map(|frame| frame.status)
}

fn try_sim_readout_from(sim: &Mutex<Sim>) -> Option<SimStatus> {
    sim.try_lock().map(|sim| sim.readout())
}

/// A gap between rendered frames longer than this means the headset stopped rendering, and the
/// frame half's measurement stops holding for the simulation half.
const FRAME_GAP: Duration = Duration::from_millis(500);

/// The gap at a seat exit past which the head is put back over the body instead of the body
/// stepping to it, world metres.
const EXIT_REBASE_M: f32 = 1.0;

/// The fastest the drive and the task together may ask the proxy to move, world metres per second:
/// a sanity bound, not a tuning.
const MAX_WANTED_MPS: f32 = 12.0;

struct FrameState {
    seen_epoch: u64,
    shift: CockpitShift,
    last_seat: SeatPhase,
    pushback: Pushback,
    /// The latest step frame for the simulation half, and when it was taken.
    published: Option<(StepFrame, Instant)>,
    status: FrameReadout,
}

static FRAME: Mutex<FrameState> = Mutex::new(FrameState {
    seen_epoch: 0,
    shift: CockpitShift::new(),
    last_seat: SeatPhase::Out,
    pushback: Pushback::new(),
    published: None,
    status: FrameReadout {
        at: None,
        rig: None,
        world_scale: 1.0,
        shift: Vec3::ZERO,
        teleport_rebases: 0,
        exit_rebases: 0,
        snaps: 0,
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

/// Who owned the root on the latest on-foot tick, while the simulation half runs, and whether the
/// ledger finds the drive obstructed.
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

/// The latest frame's step for the simulation half, while it is fresh.
fn published_step(now: Instant) -> Option<StepFrame> {
    FRAME
        .lock()
        .published
        .filter(|(_, at)| now.saturating_duration_since(*at) < FRAME_GAP)
        .map(|(frame, _)| frame)
}

/// Clear the ledger and return its running total.
fn clear_movement() -> CompletedMovement {
    let mut sim = SIM.lock();
    sim.clear_movement(CompletionUnavailable::Cleared);
    sim.movement.completed()
}

struct Sim {
    /// The teleport epoch last acted on.
    seen_epoch: u64,
    /// The body's position on the last tick, for the warp test.
    last_position: Option<Vec3>,
    movement: Requests,
    tracer: Tracer,
    completion_sequence: u64,
    completion: CompletionSnapshot,
    status: SimStatus,
}

impl Sim {
    const fn new() -> Self {
        Self {
            seen_epoch: 0,
            last_position: None,
            movement: Requests::new(),
            tracer: Tracer::new(),
            completion_sequence: 0,
            completion: CompletionSnapshot::unavailable(CompletionUnavailable::NotCompleted),
            status: SimStatus {
                at: None,
                owner: None,
                effective_gap: Vec2::ZERO,
                step_velocity: Vec2::ZERO,
                task_velocity: Vec2::ZERO,
                ledger: LedgerCounts {
                    exact: 0,
                    projected: 0,
                },
                capsule_owned: false,
                capsule_blocked: false,
                trace: None,
            },
        }
    }

    fn readout(&self) -> SimStatus {
        SimStatus {
            ledger: self.movement.counts(),
            ..self.status
        }
    }
}

static SIM: Mutex<Sim> = Mutex::new(Sim::new());

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
    let step_frame = published_step(now);
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
        sim.tracer.forget();
        sim.clear_movement(CompletionUnavailable::SourceUnavailable);
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
        if drive::is_warp((position - last).length(), speed, dt, drive::WARP_MIN_M) {
            teleport::record_warp();
        }
    }
    sim.last_position = Some(position);
    let epoch = teleport::epoch();
    if epoch != sim.seen_epoch {
        sim.seen_epoch = epoch;
        sim.clear_movement(CompletionUnavailable::Teleported);
        sim.tracer.forget();
        sim.status.trace = None;
        stand_down(&mut sim.status, RootOwner::Game);
        return;
    }

    let (Some(shared), true) = (shared, live) else {
        sim.clear_movement(CompletionUnavailable::SourceUnavailable);
        sim.tracer.forget();
        sim.status.trace = None;
        stand_down(&mut sim.status, RootOwner::Game);
        // Without the headset driving the view, nothing asks for the crouch any more, and the frame
        // half's release does not run (no VR frames) or sees this tick as fresh.
        if !driven {
            unsafe { release_owned(&mut sim, character) };
        }
        return;
    };

    let authored = unsafe { animation_moves_body(character, now) };
    // The stick's velocity replaces the clip's root velocity first; the palms' push off what they
    // press into then counts as the task's own movement, so it is added before the drive reads the
    // task's velocity.
    let task = horizontal(unsafe { wanted_velocity(character) });
    unsafe { after_task(character, dt) };
    let stick = horizontal(unsafe { wanted_velocity(character) });
    unsafe { physical_hands::push_body(character) };
    let palms = horizontal(unsafe { wanted_velocity(character) }) - stick;

    let owner = owner::select(&OwnerInput {
        task_wrote: true,
        climbing,
        authored,
        posture: shared.posture,
        seat: shared.seat,
    });
    sim.status.owner = Some(owner);
    match step_frame.filter(|_| owner.drives()) {
        Some(frame) => unsafe { drive_tick(&mut sim, character, &frame, dt, owner, &rig) },
        None => {
            // The ledger records nothing for a tick the player did not move the root on, so a climb
            // or an authored motion is never credited as a step.
            sim.clear_movement(CompletionUnavailable::Cleared);
            stand_down(&mut sim.status, owner);
        }
    }

    if capsule_enabled(&config) {
        unsafe { capsule_tick(&mut sim, character, shared.crouched, position) };
    }

    let input = TickInput {
        position,
        dt,
        split: Split {
            task,
            stick,
            palms,
            chase: sim.status.step_velocity,
            wanted: unsafe { wanted_velocity(character) },
        },
        proxy: unsafe { read_proxy(character) },
        authored,
        capsule_owned: sim.status.capsule_owned,
        capsule_blocked: sim.status.capsule_blocked,
    };
    let (trace, jump) = sim.tracer.step(input, now);
    sim.status.trace = Some(trace);
    if let Some(jump) = jump {
        log_jump(&jump, owner);
    }
}

/// The character's wanted velocity, world metres per second.
///
/// # Safety
///
/// `character` must be live, on the thread running its state machines.
unsafe fn wanted_velocity(character: &Character) -> Vec3 {
    let mut wanted = Vector3 { data: [0.0; 3] };
    unsafe { character.GetWantedWorldVelocity(&mut wanted) };
    Vec3::from_array(wanted.data)
}

/// The world `(x, z)` part of `v`.
fn horizontal(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.z)
}

/// The proxy's velocity, support, ground, and states, for the tick's trace.
///
/// # Safety
///
/// `character` must be live, on the thread running its state machines.
unsafe fn read_proxy(character: &Character) -> ProxyRead {
    let base = ProxyRead {
        pending_state: character.m_PendingProxyState,
        default_state: character.m_DefaultProxyState,
        ..ProxyRead::default()
    };
    let Some(proxy) = (unsafe { character.m_PfxCharacter.px.as_ref() }) else {
        return base;
    };
    let ground = &proxy.m_GroundInformation;
    ProxyRead {
        velocity: unsafe { velocity(proxy) },
        surface_velocity: Vec3::from_array(ground.m_surfaceVelocity.data),
        support: match ground.m_supportedState {
            hknpCharacterSurfaceInfo::UNSUPPORTED => Support::Unsupported,
            hknpCharacterSurfaceInfo::SLIDING => Support::Sliding,
            hknpCharacterSurfaceInfo::SUPPORTED => Support::Supported,
            _ => Support::Unknown,
        },
        surface_dynamic: ground.m_isSurfaceDynamic,
        ground_body: (proxy.m_GroundBodyId != PfxCharacterInstance::INVALID_BODY_ID)
            .then_some(proxy.m_GroundBodyId),
        state: proxy.m_ProxyState,
        ..base
    }
}

/// Log a tick whose root went further than the previous tick's wanted velocity explains, with what
/// that tick asked for, so the log names what pushed the root. At info level, since the rate limit
/// keeps it quiet and a session should carry it without a filter change; on its own target, so a
/// `root_jump=off` filter silences it alone.
fn log_jump(jump: &Jump, owner: RootOwner) {
    tracing::info!(
        target: "root_jump",
        moved = ?jump.moved,
        expected = ?jump.expected,
        task = ?jump.asked.task,
        stick = ?jump.asked.stick,
        palms = ?jump.asked.palms,
        chase = ?jump.asked.chase,
        wanted = ?jump.asked.wanted,
        proxy_velocity = ?jump.proxy.velocity,
        surface_velocity = ?jump.proxy.surface_velocity,
        support = jump.proxy.support.label(),
        surface_dynamic = jump.proxy.surface_dynamic,
        ground_body = ?jump.proxy.ground_body,
        proxy_state = jump.proxy.state,
        pending_proxy_state = jump.proxy.pending_state,
        owner = owner.label(),
        suppressed = jump.suppressed,
        "rig: the root jumped",
    );
}

/// Zero the step drive's readout for a tick it did not run on.
fn stand_down(status: &mut SimStatus, owner: RootOwner) {
    status.owner = Some(owner);
    status.step_velocity = Vec2::ZERO;
    status.effective_gap = Vec2::ZERO;
}

/// Add the step drive's velocity to the task's and submit the split. `dt` is the task's step, game
/// seconds, and the velocity is the world's per game second. The drive closes the gap to the
/// player's own neck, which moves in real time, so it is paced in real time and converted: in slow
/// motion the body keeps up with the player's steps.
unsafe fn drive_tick(
    sim: &mut Sim,
    character: &mut Character,
    frame: &StepFrame,
    dt: f32,
    owner: RootOwner,
    config: &RigConfig,
) {
    let completed = sim.movement.completed();
    let gap = frame.effective_gap(completed.current);
    // SAFETY: the locomotion task runs while the game runs.
    let time_scale = unsafe { TimeScale::current() };
    let real = body_rig::drive_velocity(frame.phase, gap, time_scale.real_dt(dt), &config.step);
    let step = (real / time_scale.get()).clamp_length_max(MAX_WANTED_MPS);
    let step = if step.is_finite() { step } else { Vec2::ZERO };
    let mut wanted = Vector3 { data: [0.0; 3] };
    unsafe { character.GetWantedWorldVelocity(&mut wanted) };
    let [x, y, z] = wanted.data;
    let task = Vec2::new(x, z);
    let movement = Movement::new(task, step, dt);
    if step != Vec2::ZERO {
        let sum = movement.velocity(dt).clamp_length_max(MAX_WANTED_MPS);
        let world = Vector3 {
            data: [sum.x, y, sum.y],
        };
        unsafe { character.SetWantedWorldVelocity(&world) };
    }
    sim.movement.submit_owned(movement, owner);
    sim.status.effective_gap = gap;
    sim.status.step_velocity = step;
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
    let blocked = !crouched && owned && unsafe { headroom_blocked(feet) };
    let step = capsule_step(crouched || blocked, owned, pending, default);
    if let Some(value) = step.write {
        character.m_PendingProxyState = value;
    }
    sim.status.capsule_owned = step.owned;
    sim.status.capsule_blocked = blocked;
}

/// Whether something above the crouched character would stop it standing: a ray up from knee
/// height to the top of a standing character.
unsafe fn headroom_blocked(feet: Vec3) -> bool {
    let from = feet + Vec3::Y * HEADROOM_RAY_START_M;
    let to = feet + Vec3::Y * BodyDimensions::CHARACTER_ESTIMATE.height;
    // SAFETY: the game thread.
    let options = unsafe { CastOptions::ignoring_player(0) };
    // SAFETY: the simulation phase; the ignored object is the live local character.
    matches!(
        unsafe { probe::scene_cast(from, to, &options) },
        Ok(Some(_))
    )
}

/// Where the headroom ray starts above the feet, metres: above a step the character stands on.
const HEADROOM_RAY_START_M: f32 = 0.5;

unsafe fn velocity(proxy: &PfxCharacterInstance) -> Vec3 {
    let mut out = Vector3 { data: [0.0; 3] };
    unsafe { proxy.GetVelocity(&mut out) };
    Vec3::from_array(out.data)
}
