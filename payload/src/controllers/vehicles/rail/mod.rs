//! Standing on a helicopter's skid by its door, free to walk along it, after a reel onto its door
//! side.
//!
//! A reel onto a helicopter's door side arrives in the game's door-side state, which keeps Rico
//! attached on the skid outside the door and carried by the airframe. Helicopters have no idle
//! there: once the arrival's clip ends, the state drops into the helicopter's stunt hang through
//! [`ValidateStuntPositionCondition::Evaluate`] on the transition to the side-to-hang state, unless
//! the player pressed Y in time to enter. The reviewer wants the skid as a place to stand and move
//! on with the full body, a door to open, and a step into the air to be a fall.
//!
//! So while the controllers act, that one transition is refused, and at the arrival clip's end the
//! rail takes Rico out of the helicopter's interaction graph, as the game takes a character off a
//! car's roof when they walk past its stunt area's edge ([`InteractionGraph::RemoveUser`], the
//! user's motion state back on, its graph state inactive, and the physics input's game-controlled
//! velocity off), and stands him on foot on the skid. Each physics step then carries his proxy
//! with the airframe's motion under his feet, as the hand climb carries it on a moving body
//! ([`engine::hold`]), plus his own walk measured against the airframe, along the skid's line
//! ([`solve`]): a sticky pull draws his feet back to the line, so a slow step outward holds and a
//! brisk one past the step-off distance leaves him falling with the airframe's velocity, which the
//! game takes into its fall and its skydive. The feet are held at the line's height. A hand climb
//! takes over the carry while a hand grips, so the hands can lower Rico off the skid to hang under
//! it; once his feet are well below the line the rail lets him go to the climb. The jump leaves the
//! rail as the game's own jump. Y is the game's on-foot entry, and the rail lets go once the game
//! takes him back into the helicopter's graph. While the rail holds, the game's automatic entry
//! into a stunt area on the vehicle below is refused for him ([`blocks_auto_attach`]).
//!
//! The airframe is the body the reel's line was attached to, noted at the door-side arrival
//! ([`note_airframe`]).
//!
//! [`ValidateStuntPositionCondition::Evaluate`]: jc3gi::animation::rule_system::ValidateStuntPositionCondition::Evaluate
//! [`InteractionGraph::RemoveUser`]: jc3gi::interaction::InteractionGraph::RemoveUser

mod solve;

use std::{
    sync::{
        LazyLock,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use detours_macro::detour;
use glam::{Affine3A, Vec3};
use jc3gi::{
    animation::rule_system::{StateMachineInstance, Transition, ValidateStuntPositionCondition},
    character::character::Character,
    types::math::Vector3,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        physical_hands::climb::{self, engine},
        scheme,
        vehicles::{
            config::RailConfig,
            rail::solve::{RailLine, RailMotion, RailStep, step, walk_from},
        },
        weapons::engine::character_position,
    },
    hash::hashlittle,
    hooks::animation::{active_state_type_id, body_rule_system},
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&EVALUATE_BINDER)
}

/// The rail's readout, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct RailStatus {
    /// Whether the rail holds Rico now.
    pub active: bool,
    /// How often the drop from the door side into the stunt hang was refused.
    pub drops_refused: u64,
    /// How often Rico came to stand on the rail.
    pub entries: u64,
    /// How the rail last let him go.
    pub last_release: Option<&'static str>,
    /// How far off the skid's line, level, his feet were at the last step, metres.
    pub off_line_m: f32,
    /// The walk the rail passed at the last step, metres per second.
    pub walk_mps: f32,
    /// Whether the character's own update carried the proxy with its support at the last step.
    pub update_carried: bool,
}

/// The rail's readout.
pub fn status() -> RailStatus {
    let mut status = STATE.lock().status;
    status.drops_refused = DROPS_REFUSED.load(Ordering::Relaxed);
    status
}

/// Note the body of the airframe a reel arriving at a helicopter's door side was attached to.
pub(crate) fn note_airframe(body: u32) {
    STATE.lock().airframe = Some((body, Instant::now()));
}

/// Whether the rail holds `character`, so the game's automatic entry into a stunt area on the
/// vehicle below is refused for him. Lock-free, for the detour on that entry.
pub(crate) fn blocks_auto_attach(character: *const Character) -> bool {
    let held = HELD.load(Ordering::Relaxed);
    held != 0 && held == character as usize
}

/// Step the rail on the input tick: stand Rico on the skid once the door-side arrival has ended,
/// let him go when the jump is pressed, he is no longer on foot (`on_foot`), or the game has
/// taken him back into the helicopter's graph, and step him off the skid with the jump while the
/// door-side state still holds him.
///
/// # Safety
///
/// The game stage, on the game thread before the world update.
pub(crate) unsafe fn input_tick(jump: bool, on_foot: bool) {
    let Some(character) = (unsafe { Character::GetLocalPlayerCharacter().as_mut() }) else {
        release("the local player went away");
        return;
    };
    let config = Config::lock_query(|c| c.controllers.vehicles.rail);
    if !enabled() {
        ENTER_REQUESTED.store(false, Ordering::Relaxed);
        release("the rail was switched off");
        return;
    }
    let held = { STATE.lock().active.map(|a| (a.character, a.since)) };
    if let Some((held, since)) = held {
        let settled = since.elapsed() >= MODE_GRACE;
        let reason = if held != std::ptr::from_mut(character) as usize {
            Some("the local player changed")
        } else if jump {
            Some("jumped")
        } else if !character.m_InteractionUserProxy.m_Graph.is_null() {
            Some("the game took him into the helicopter")
        } else if settled && !on_foot {
            Some("he is no longer on foot")
        } else {
            None
        };
        if let Some(reason) = reason {
            release(reason);
        }
        return;
    }
    if !unsafe { on_door_side(character) } {
        ENTER_REQUESTED.store(false, Ordering::Relaxed);
        return;
    }
    if jump {
        ENTER_REQUESTED.store(false, Ordering::Relaxed);
        unsafe {
            leave_graph(
                character,
                Vec3::from_array(character.m_MeasuredWorldVelocity.data),
            )
        };
        tracing::info!(target: "controllers", "rail: stepped off the helicopter's skid");
        return;
    }
    if ENTER_REQUESTED.swap(false, Ordering::Relaxed) {
        unsafe { enter(character, &config) };
    }
}

/// The rail's part of the pre-simulation step: carry Rico along the skid.
///
/// # Safety
///
/// The thread that steps the physics world, outside the step, after the characters' proxy
/// updates (the pre-simulation callback).
pub(crate) unsafe fn step_physics() {
    let mut state = STATE.lock();
    let Some(mut active) = state.active else {
        return;
    };
    // A gripping hand carries the proxy itself.
    if climb::climbing() {
        return;
    }
    let config = Config::lock_query(|c| c.controllers.vehicles.rail);
    let Some(frame) = (unsafe { engine::body_frame(active.body) }) else {
        drop(state);
        release("the airframe went away");
        return;
    };
    let Ok(place) = (unsafe { engine::player_place() }) else {
        return;
    };
    let surface = frame.point_velocity(place.feet);
    let game = unsafe { engine::proxy_velocity() }.unwrap_or(active.carried);
    let (walk, update_carried) = walk_from(game, active.surface);
    let into_frame = frame.transform.inverse();
    let feet = into_frame.transform_point3(place.feet);
    let walk_local = into_frame.transform_vector3(walk);
    let decision = step(&active.line, feet, walk_local, &motion(&config));
    let nearest = active.line.nearest(feet);
    state.status.off_line_m = Vec3::new(feet.x - nearest.x, 0.0, feet.z - nearest.z).length();
    state.status.walk_mps = walk.length();
    state.status.update_carried = update_carried;
    match decision {
        RailStep::Carry(relative) => {
            let velocity = surface + frame.transform.transform_vector3(relative);
            if unsafe { engine::hold(velocity, active.body, surface) }.is_err() {
                drop(state);
                release("the proxy could not be carried");
                return;
            }
            active.surface = surface;
            active.carried = velocity;
            state.active = Some(active);
        }
        RailStep::SteppedOff => {
            drop(state);
            release("stepped off the skid");
        }
        RailStep::Dropped => {
            drop(state);
            release("went off the skid's edge");
        }
    }
}

#[detour(address = jc3gi::animation::rule_system::ValidateStuntPositionCondition::Evaluate_ADDRESS)]
fn evaluate(
    condition: *const ValidateStuntPositionCondition,
    machine: *const StateMachineInstance,
    transition: *const Transition,
) -> bool {
    // SAFETY: the state machine evaluates its own transition's condition on its own thread.
    if enabled() && unsafe { drops_to_hang(machine, transition) } {
        if DROPS_REFUSED.fetch_add(1, Ordering::Relaxed) == 0 {
            tracing::info!(
                target: "controllers",
                "rail: holding Rico on the helicopter's skid rather than dropping into the hang"
            );
        }
        ENTER_REQUESTED.store(true, Ordering::Relaxed);
        return false;
    }
    EVALUATE.get().unwrap().call(condition, machine, transition)
}

/// Whether the rail acts: the controllers' scheme, and its switch on.
fn enabled() -> bool {
    scheme::is_controllers()
        && Config::lock_query(|c| c.controllers.enabled && c.controllers.vehicles.helicopter_rail)
}

/// The rail's motion from its settings.
fn motion(config: &RailConfig) -> RailMotion {
    RailMotion {
        step_off_m: config.step_off_m,
        pull_s: config.pull_s,
        height_s: HEIGHT_S,
        max_walk_mps: MAX_WALK_MPS,
        max_drop_m: MAX_DROP_M,
    }
}

/// Stand `character`, at the end of the door-side arrival, on the skid of the airframe the reel
/// was attached to.
///
/// # Safety
///
/// `character` is the live local player, on the game thread before the world update.
unsafe fn enter(character: &mut Character, config: &RailConfig) {
    let airframe = STATE
        .lock()
        .airframe
        .filter(|(_, at)| at.elapsed() <= AIRFRAME_FRESH)
        .map(|(body, _)| body);
    let Some((body, frame)) =
        airframe.and_then(|body| unsafe { engine::body_frame(body) }.map(|f| (body, f)))
    else {
        tracing::warn!(
            target: "controllers",
            noted = airframe.is_some(),
            "rail: no airframe to stand on; the door-side state holds Rico"
        );
        return;
    };
    let feet = character_position(character);
    let surface = frame.point_velocity(feet);
    let into_frame: Affine3A = frame.transform.inverse();
    let line = RailLine::through(
        into_frame.transform_point3(feet),
        into_frame.transform_point3(frame.centre),
        config.half_length_m,
    );
    unsafe { leave_graph(character, surface) };
    let mut state = STATE.lock();
    state.active = Some(Active {
        character: std::ptr::from_mut(character) as usize,
        body,
        line,
        since: Instant::now(),
        surface,
        carried: surface,
    });
    state.status.active = true;
    state.status.entries += 1;
    HELD.store(std::ptr::from_mut(character) as usize, Ordering::Relaxed);
    tracing::info!(
        target: "controllers",
        body,
        surface_mps = surface.length(),
        "rail: standing on the helicopter's skid"
    );
}

/// Let go of Rico, for `reason`: the rail no longer carries him, so he moves as the game moves
/// him, with the velocity the last step gave him.
fn release(reason: &'static str) {
    let mut state = STATE.lock();
    if state.active.take().is_none() {
        return;
    }
    HELD.store(0, Ordering::Relaxed);
    state.status.active = false;
    state.status.last_release = Some(reason);
    tracing::info!(target: "controllers", reason, "rail: let go of Rico");
}

/// Take `character` out of the helicopter's interaction graph, as the game takes a character off a
/// car's roof, moving at `velocity`.
///
/// # Safety
///
/// `character` is the live local player, on the game thread before the world update.
unsafe fn leave_graph(character: &mut Character, velocity: Vec3) {
    let proxy = &raw mut character.m_InteractionUserProxy;
    // SAFETY: the proxy is the character's own, live with it; the graph is live while it has the
    // user.
    unsafe {
        if let Some(graph) = (*proxy).m_Graph.as_mut() {
            graph.RemoveUser(proxy);
        }
        (*proxy).m_MotionStateEnabled = true;
        (*proxy).SetGraphUserState(GRAPH_USER_INACTIVE);
        if let Some(pfx) = character.m_PfxCharacter.px.as_mut() {
            pfx.m_Input.gameControledVelocity = false;
        }
        let velocity = Vector3 {
            data: velocity.to_array(),
        };
        character.SetWantedWorldVelocity(&velocity);
    }
}

/// Whether `transition` of `machine` is the local player's drop from a helicopter's door side into
/// its stunt hang.
///
/// # Safety
///
/// The machine and the transition are live for the condition's evaluation.
unsafe fn drops_to_hang(
    machine: *const StateMachineInstance,
    transition: *const Transition,
) -> bool {
    let Some(character) = (unsafe { Character::GetLocalPlayerCharacter().as_ref() }) else {
        return false;
    };
    let local_machine = unsafe { body_rule_system(character) }
        .is_some_and(|rules| std::ptr::eq(rules.m_StateMachineInstance.px.cast_const(), machine));
    if !local_machine {
        return false;
    }
    let Some(destination) = (unsafe { transition.as_ref() })
        .and_then(|transition| unsafe { transition.GetDestination().as_ref() })
    else {
        return false;
    };
    TO_HANG.contains(&destination.m_HashID.m_Hash)
}

/// Whether `character` is in a helicopter's door-side state.
///
/// # Safety
///
/// `character` is live, on the game thread.
unsafe fn on_door_side(character: &Character) -> bool {
    unsafe { active_state_type_id(character) }.is_some_and(|state| DOOR_SIDE.contains(&state))
}

/// The rail holding Rico.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Active {
    character: usize,
    body: u32,
    line: RailLine,
    since: Instant,
    /// The airframe's velocity under the feet at the last step.
    surface: Vec3,
    /// The velocity the last step gave the proxy.
    carried: Vec3,
}

struct State {
    airframe: Option<(u32, Instant)>,
    active: Option<Active>,
    status: RailStatus,
}

static STATE: Mutex<State> = Mutex::new(State {
    airframe: None,
    active: None,
    status: RailStatus {
        active: false,
        drops_refused: 0,
        entries: 0,
        last_release: None,
        off_line_m: 0.0,
        walk_mps: 0.0,
        update_carried: false,
    },
});

/// The character the rail holds, or zero.
static HELD: AtomicUsize = AtomicUsize::new(0);

/// Set when the door-side arrival's drop into the hang was refused: its clip has ended.
static ENTER_REQUESTED: AtomicBool = AtomicBool::new(false);

static DROPS_REFUSED: AtomicU64 = AtomicU64::new(0);

/// The states a helicopter's door side drops into the stunt hang through.
static TO_HANG: LazyLock<[u32; 2]> = LazyLock::new(|| {
    [
        hashlittle(b"S_DRIVER_SIDE_TO_HANG_VEHICLE"),
        hashlittle(b"S_PASSENGER_SIDE_TO_HANG_VEHICLE"),
    ]
});

/// The door-side states a reel onto a helicopter arrives in.
static DOOR_SIDE: LazyLock<[u32; 2]> = LazyLock::new(|| {
    [
        hashlittle(b"S_GRAPPLE_REEL_TO_HELI_DRIVER_SIDE"),
        hashlittle(b"S_GRAPPLE_REEL_TO_HELI_PASSENGER_SIDE"),
    ]
});

/// The interaction graph user's inactive state.
const GRAPH_USER_INACTIVE: i32 = 0;

/// How long after its arrival a noted airframe stands for the door side's, seconds.
const AIRFRAME_FRESH: Duration = Duration::from_secs(30);

/// How long after the rail takes Rico the input tick's mode is trusted: the mode reads the fall
/// the graph's exit starts until the first carried step lands him.
const MODE_GRACE: Duration = Duration::from_millis(500);

/// The time constant that holds the feet at the skid's height, seconds.
const HEIGHT_S: f32 = 0.1;

/// The fastest walk the rail passes on, metres per second.
const MAX_WALK_MPS: f32 = 3.0;

/// How far below the skid's line the feet go before the rail lets them go, metres.
const MAX_DROP_M: f32 = 0.5;
