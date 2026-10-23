//! The grapple snatch: the empty grapple hand pulls a weapon out of reach into itself. The design is
//! in `docs/mod/input/grapple-snatch.md`, the engine side in "Weapons and the grapple" in
//! `docs/engine/gameplay/grapple-pipeline.md`.
//!
//! The grapple's aim sees weapons lying in the world through a patch scoped to the grapple's pass
//! of the aim ray ([`crate::controllers::aim::grapple_weapons`]), and notes the guns in characters'
//! hands out to the snatch's range ([`crate::controllers::weapons::hold::steal`]). On each input
//! tick:
//!
//! 1. [`targets`] offers the grip one remote target ([`GrabVolume::Snatch`], ranked
//!    [`Tier::Remote`](crate::controllers::grab::Tier::Remote), taken only when nothing at all is
//!    in reach) for the empty grapple hand on foot, with the hook free and the grapple button up:
//!    the weapon under the grapple's aim, or the gun in a character's hands nearest the aim's line
//!    within its cone and in sight ([`rules::choose`]).
//! 2. [`finish_tick`] acts on the grip's grab of it, which arms the snatch on that target. A yank
//!    of the hand back toward the body while the grip is held fires it: a gun in a character's
//!    hands is first let go of, with the steal's drop and stagger; then the game's hook flies to
//!    the weapon as a ghost wire through the custom fire ([`engine::fire`]). Once it holds the
//!    weapon, the hold solve pulls the weapon's body along an arc toward the hand
//!    ([`rules::pull_point`]), and within the capture radius the wire is ended and the weapon taken
//!    into the hand through the loadout table, as a pickup is. Opening the grip at any point
//!    cancels: in flight the line comes away and the weapon keeps its momentum. A stall, a line
//!    run out, a missed hook, or a refused take fails the snatch and leaves the weapon in the world.
//!
//! The snatch's runtime state (armed, freeing, hook out, pulling) lives here alone; where the
//! weapon ends up is the loadout table's. The mod holds a strong reference to the weapon from the
//! fire to the end, holds the grapple button back from the game while its line is out, and ends
//! its own wire on every exit and stand-down.
//!
//! [`hooks`] makes the grapple button's tap and hold work on a weapon under the aim.

pub mod config;
mod engine;
mod hooks;
pub mod rules;

use std::{fmt, num::NonZeroUsize};

use glam::Vec3;
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::WeaponBase,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

pub use crate::controllers::snatch::config::SnatchConfig;
use crate::controllers::{
    aim::{
        self, grapple_weapons,
        ray::Ray,
        raycast::{self, CastOptions},
    },
    bindings::{controls::ButtonControl, hold::Desired, tables, yank::YankDetector},
    config::ControllersConfig,
    frame::Hand,
    grab::{GrabVolume, GripTarget, Mark, MarkAt, Provided},
    hands::ik,
    haptics::{self, Event, Sustain},
    loadout::slot_label,
    mode::{Detection, Mode},
    physical_hands::{
        self, BodyError,
        grasp::GripTarget as HoldPoint,
        hold::{self, HandHold, HoldTuning},
    },
    snatch::rules::{Candidate, PullLimits, StallWatch, Verdict},
    weapons::hold::{
        Cause,
        pickup::{self, TakeOptions},
        refs::{Strong, Weak},
        steal::{self, StealRefusal},
        world::{TargetKind, WorldTarget},
    },
};

/// The snatch's hooks, aggregated by [`crate::controllers::hook_library`].
pub(crate) fn hook_library() -> HookLibrary {
    hooks::hook_library()
}

/// Why a snatch did not happen, or ended before the weapon reached the hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// There is no local player or hook.
    NoPlayer,
    /// The grapple has a line out.
    LineOut,
    /// The hook is firing or reeling.
    HookBusy,
    /// The target is gone: despawned, picked up, or no longer in its character's hands.
    Gone,
    /// The weapon no longer lies in the world.
    NotDropped,
    /// The weapon has no body in the world.
    NoBody,
    /// The hook did not take the custom fire.
    FireRefused,
    /// The hook missed, ran out of line, or the game ended its line before it held the weapon.
    Missed,
    /// The hook flew for too long without holding the weapon.
    TimedOut,
    /// The weapon stopped coming: snagged on the world.
    Stalled,
    /// The weapon is farther from the hand than the line reaches.
    OutOfLine,
    /// The line came away while it pulled.
    LineLost,
    /// The weapon's body could not be read or driven.
    Body(BodyError),
    /// The character would not let go of its gun.
    Free(StealRefusal),
    /// The take into the hand refused the weapon.
    Take(pickup::Refusal),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self {
            Refusal::NoPlayer => "there is no local player or hook",
            Refusal::LineOut => "the grapple has a line out",
            Refusal::HookBusy => "the hook is firing or reeling",
            Refusal::Gone => "the target is gone",
            Refusal::NotDropped => "the weapon no longer lies in the world",
            Refusal::NoBody => "the weapon has no body in the world",
            Refusal::FireRefused => "the hook did not fire",
            Refusal::Missed => "the hook missed",
            Refusal::TimedOut => "the hook flew too long",
            Refusal::Stalled => "the weapon snagged",
            Refusal::OutOfLine => "the weapon is beyond the line",
            Refusal::LineLost => "the line came away",
            Refusal::Body(e) => return write!(f, "snatch: {e}"),
            Refusal::Free(e) => return write!(f, "snatch: {e}"),
            Refusal::Take(e) => return write!(f, "snatch: {e}"),
        };
        write!(f, "snatch: {why}")
    }
}

impl std::error::Error for Refusal {}

/// What the bindings hand [`targets`].
pub(crate) struct OfferInput<'a> {
    /// The grapple hand.
    pub hand: Hand,
    pub detection: &'a Detection,
    /// Whether the grapple button is held.
    pub button_held: bool,
    pub config: &'a ControllersConfig,
}

/// The remote target for the grapple hand's grip this tick, if there is one.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn targets(input: &OfferInput) -> Provided {
    let config = &input.config.weapons.snatch;
    let hold = &input.config.weapons.hold;
    let takes_held = hold.steal && hold.direct_pickup && hold.either_hand;
    grapple_weapons::set_enabled(config.enabled);
    steal::widen_notice(if config.enabled && takes_held {
        config.range_m
    } else {
        0.0
    });
    let mut state = STATE.lock();
    state.offer = None;
    state.status.live = true;
    if !config.enabled || !matches!(state.phase, Phase::Idle) {
        return Provided::default();
    }
    if let Err(why) = unsafe { offerable(input) } {
        state.status.target = None;
        state.status.unoffered = Some(why);
        return Provided::default();
    }
    state.status.unoffered = None;
    let offer = aim::status()
        .grapple
        .and_then(|grapple| unsafe { choose(&grapple.ray, config, takes_held) });
    state.status.target = offer.as_ref().map(|o| TargetReadout {
        kind: o.kind.label(),
        distance_m: o.distance,
    });
    let Some(offer) = offer else {
        return Provided::default();
    };
    let target = GripTarget::measured(
        GrabVolume::Snatch,
        input.hand,
        offer.distance,
        Mark::ring(MarkAt::World(offer.point), MARK_SIZE_M),
    );
    let mut notes: [Vec<String>; 2] = Default::default();
    notes[input.hand.index()].push(format!(
        "snatch: {} at {:.1} m",
        offer.kind.label(),
        offer.distance
    ));
    state.offer = Some(offer);
    Provided {
        targets: vec![target],
        notes,
    }
}

/// What the bindings hand [`finish_tick`].
pub(crate) struct FinishInput<'a> {
    /// The grapple hand.
    pub hand: Hand,
    /// The grabs the grips took this tick that the snatch acts on, by [`Hand::index`].
    pub grabs: [Option<GrabVolume>; 2],
    /// The grips, by [`Hand::index`].
    pub grips: [ButtonControl; 2],
    /// The grapple hand's distance from the body point a yank pulls toward, metres, or `None`
    /// while the hand is untracked.
    pub distance: Option<f32>,
    /// Whether the grapple button is held, as the tables inject it.
    pub button_held: bool,
    pub detection: &'a Detection,
    pub dt: f32,
    pub config: &'a ControllersConfig,
}

/// Act on the tick's grab of a remote target, step the snatch, and hold the grapple button back from
/// the game while its line is out.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn finish_tick(input: &FinishInput, desired: &mut Vec<Desired>) {
    let config = &input.config.weapons.snatch;
    hooks::publish(config.enabled, input.button_held);
    if let Some(weak) = hooks::take_toss() {
        unsafe { toss(&weak, input.hand) };
    }
    let mut state = STATE.lock();
    unsafe { state.step(input) };
    let line_out = matches!(
        state.phase,
        Phase::Freeing { .. } | Phase::HookOut { .. } | Phase::Pulling { .. }
    );
    state.status.phase = state.phase.label();
    drop(state);
    if line_out && let Some((_, actions)) = tables::grapple_press(input.detection.mode) {
        desired.retain(|d| !actions.fire.contains(&d.target.action));
    }
}

/// Stand the snatch down: end its line, let go of its target, and turn the grapple's weapon aim
/// and button hooks off. Called from the input tick whenever the bindings stand down.
pub(crate) fn stand_down() {
    hooks::publish(false, false);
    grapple_weapons::set_enabled(false);
    let mut state = STATE.lock();
    if !matches!(state.phase, Phase::Idle) {
        tracing::info!(target: "controllers", phase = state.phase.label(), "snatch: stood down");
    }
    // SAFETY: the input tick runs on the main thread outside the world update.
    unsafe { state.phase.end_line() };
    state.phase = Phase::Idle;
    state.offer = None;
    state.status.live = false;
    state.status.phase = Phase::Idle.label();
    state.status.target = None;
}

/// The snatch's readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SnatchStatus {
    /// Whether the input tick ran it recently.
    pub live: bool,
    /// What the snatch is doing.
    pub phase: &'static str,
    /// The target offered to the grip on the last tick.
    pub target: Option<TargetReadout>,
    /// Why nothing was offered on the last tick, when the hand or the hook could not snatch.
    pub unoffered: Option<&'static str>,
    /// Why the last snatch failed.
    pub last_refusal: Option<String>,
    /// Snatches fired, taken into the hand, and failed, since the payload loaded.
    pub fired: u64,
    pub taken: u64,
    pub failed: u64,
    /// Weapons a tap of the grapple tossed to the player.
    pub tosses: u64,
}

/// The target offered, for the readout.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TargetReadout {
    pub kind: &'static str,
    pub distance_m: f32,
}

/// The latest readout. Safe from any thread: it takes only a leaf lock.
pub fn status() -> SnatchStatus {
    STATE.lock().status.clone()
}

/// The marker's diameter on a remote target, metres.
const MARK_SIZE_M: f32 = 0.3;
/// How far short of a gun in a character's hands its line of sight may be blocked, metres: the line
/// ends inside the character's hands.
const SIGHT_TOLERANCE_M: f32 = 0.3;
/// Where a tossed gun lands, metres in front of the player toward where it came from.
const TOSS_LAND_M: f32 = 1.2;
/// The toss's speed over the ground and its shortest and longest flight, metres per second and
/// seconds.
const TOSS_SPEED_MPS: f32 = 9.0;
const TOSS_FLIGHT_S: (f32, f32) = (0.45, 1.2);
/// Gravity's acceleration, metres per second squared.
const GRAVITY_MPS2: f32 = 9.81;

static STATE: Mutex<State> = Mutex::new(State {
    phase: Phase::Idle,
    offer: None,
    status: SnatchStatus {
        live: false,
        phase: "IDLE",
        target: None,
        unoffered: None,
        last_refusal: None,
        fired: 0,
        taken: 0,
        failed: 0,
        tosses: 0,
    },
});

struct State {
    phase: Phase,
    /// The target offered to the grip this tick.
    offer: Option<Offer>,
    status: SnatchStatus,
}

/// A remote target offered to the grip.
struct Offer {
    kind: OfferKind,
    /// Where the hook goes, world space.
    point: Vec3,
    /// How far it is from the grapple's ray origin, metres.
    distance: f32,
}

enum OfferKind {
    /// A weapon lying in the world.
    Ground { weapon: NonZeroUsize, weak: Weak },
    /// A gun in a character's hands.
    Held(WorldTarget),
}

impl OfferKind {
    const fn label(&self) -> &'static str {
        match self {
            OfferKind::Ground { .. } => "WEAPON ON THE GROUND",
            OfferKind::Held(_) => "WEAPON IN HANDS",
        }
    }
}

/// A weapon the snatch has fired at, held alive.
struct Grabbed {
    weapon: NonZeroUsize,
    _alive: Strong,
}

enum Phase {
    Idle,
    /// The grip holds the target; a yank fires.
    Armed {
        hand: Hand,
        offer: Offer,
        yank: YankDetector,
    },
    /// A character let go of the gun; the hook fires once it lies in the world.
    Freeing {
        hand: Hand,
        weapon: Grabbed,
        waited_s: f32,
    },
    /// The hook flies to the weapon.
    HookOut {
        hand: Hand,
        weapon: Grabbed,
        wire: NonZeroUsize,
        waited_s: f32,
    },
    /// The hook holds the weapon, and the pull brings it in.
    Pulling {
        hand: Hand,
        weapon: Grabbed,
        wire: NonZeroUsize,
        stall: StallWatch,
    },
}

impl Phase {
    const fn label(&self) -> &'static str {
        match self {
            Phase::Idle => "IDLE",
            Phase::Armed { .. } => "ARMED",
            Phase::Freeing { .. } => "FREEING",
            Phase::HookOut { .. } => "HOOK OUT",
            Phase::Pulling { .. } => "PULLING",
        }
    }

    const fn hand(&self) -> Option<Hand> {
        match self {
            Phase::Idle => None,
            Phase::Armed { hand, .. }
            | Phase::Freeing { hand, .. }
            | Phase::HookOut { hand, .. }
            | Phase::Pulling { hand, .. } => Some(*hand),
        }
    }

    /// End the snatch's line, when it has one out.
    unsafe fn end_line(&self) {
        if let Phase::HookOut { wire, .. } | Phase::Pulling { wire, .. } = self {
            unsafe { engine::cancel(*wire) };
        }
    }
}

impl State {
    /// One input tick of the snatch.
    unsafe fn step(&mut self, input: &FinishInput) {
        let config = &input.config.weapons.snatch;
        let on_foot =
            matches!(input.detection.mode, Mode::OnFoot(_)) && input.detection.traversal.is_none();
        if let Some(hand) = self.phase.hand() {
            if !config.enabled || !on_foot {
                self.cancel("the snatch is off or the player left their feet");
                return;
            }
            if !input.grips[hand.index()].held() {
                self.cancel("the grip opened");
                return;
            }
        }
        let phase = std::mem::replace(&mut self.phase, Phase::Idle);
        self.phase = match phase {
            Phase::Idle => self.arm(input),
            Phase::Armed {
                hand,
                offer,
                mut yank,
            } => match yank.step(input.dt, input.distance, true, &config.yank) {
                Some(pulled) => {
                    tracing::info!(
                        target: "controllers",
                        hand = hand.label(),
                        target_kind = offer.kind.label(),
                        distance_m = offer.distance,
                        yank_m = pulled.distance_m,
                        yank_mps = pulled.speed_mps,
                        "snatch: yanked; firing"
                    );
                    self.status.fired += 1;
                    match unsafe { fire_at(hand, offer) } {
                        Ok(phase) => phase,
                        Err(refusal) => self.failed(hand, refusal),
                    }
                }
                None => Phase::Armed { hand, offer, yank },
            },
            Phase::Freeing {
                hand,
                weapon,
                waited_s,
            } => match unsafe { engine::dropped(weapon.weapon) } {
                Ok((dropped, body)) => match unsafe { body_centre(body, weapon.weapon) }
                    .and_then(|at| unsafe { engine::fire(dropped, at) })
                {
                    Ok(wire) => {
                        haptics::play(hand, Event::GrappleFire);
                        Phase::HookOut {
                            hand,
                            weapon,
                            wire,
                            waited_s: 0.0,
                        }
                    }
                    Err(_) if waited_s + input.dt < config.free_timeout_s => Phase::Freeing {
                        hand,
                        weapon,
                        waited_s: waited_s + input.dt,
                    },
                    Err(refusal) => self.failed(hand, refusal),
                },
                Err(refusal) => self.failed(hand, refusal),
            },
            Phase::HookOut {
                hand,
                weapon,
                wire,
                waited_s,
            } => match unsafe { engine::read_wire(wire) }.0 {
                engine::WireReading::Attached => {
                    haptics::play(hand, Event::GrappleAttach);
                    tracing::info!(target: "controllers", hand = hand.label(), "snatch: the hook holds the weapon; pulling");
                    Phase::Pulling {
                        hand,
                        weapon,
                        wire,
                        stall: StallWatch::default(),
                    }
                }
                engine::WireReading::Flying if waited_s + input.dt < config.attach_timeout_s => {
                    Phase::HookOut {
                        hand,
                        weapon,
                        wire,
                        waited_s: waited_s + input.dt,
                    }
                }
                engine::WireReading::Flying => {
                    unsafe { engine::cancel(wire) };
                    self.failed(hand, Refusal::TimedOut)
                }
                engine::WireReading::Gone => self.failed(hand, Refusal::Missed),
            },
            Phase::Pulling {
                hand,
                weapon,
                wire,
                mut stall,
            } => match unsafe { pull(hand, &weapon, wire, &mut stall, input) } {
                Ok(None) => Phase::Pulling {
                    hand,
                    weapon,
                    wire,
                    stall,
                },
                Ok(Some(slot)) => {
                    self.status.taken += 1;
                    haptics::play(hand, Event::Take);
                    tracing::info!(
                        target: "controllers",
                        hand = hand.label(),
                        slot = slot_label(slot),
                        "snatch: the weapon came into the {} hand",
                        hand.label()
                    );
                    Phase::Idle
                }
                Err(refusal) => {
                    unsafe { engine::cancel(wire) };
                    self.failed(hand, refusal)
                }
            },
        };
    }

    /// Arm the snatch on the offered target, when the grapple hand's grip took it this tick.
    fn arm(&mut self, input: &FinishInput) -> Phase {
        let hand = input.hand;
        if input.grabs[hand.index()] != Some(GrabVolume::Snatch) {
            return Phase::Idle;
        }
        let Some(offer) = self.offer.take() else {
            return Phase::Idle;
        };
        tracing::info!(
            target: "controllers",
            hand = hand.label(),
            target_kind = offer.kind.label(),
            distance_m = offer.distance,
            "snatch: armed; a yank fires"
        );
        haptics::play(hand, Event::SnatchArm);
        Phase::Armed {
            hand,
            offer,
            yank: YankDetector::new(),
        }
    }

    /// Cancel the snatch under way: the line comes away, and the weapon keeps its motion.
    fn cancel(&mut self, why: &str) {
        let phase = std::mem::replace(&mut self.phase, Phase::Idle);
        // SAFETY: the input tick runs on the main thread outside the world update.
        unsafe { phase.end_line() };
        tracing::info!(target: "controllers", phase = phase.label(), why, "snatch: cancelled");
    }

    /// Record a failed snatch of `hand`, and go idle.
    fn failed(&mut self, hand: Hand, refusal: Refusal) -> Phase {
        tracing::info!(target: "controllers", hand = hand.label(), %refusal, "snatch: failed");
        haptics::play(hand, Event::SnatchRefused);
        self.status.failed += 1;
        self.status.last_refusal = Some(refusal.to_string());
        Phase::Idle
    }
}

/// Why the grapple hand cannot snatch now, if it cannot.
///
/// # Safety
///
/// The main thread, outside the world update.
unsafe fn offerable(input: &OfferInput) -> Result<(), &'static str> {
    if !matches!(input.detection.mode, Mode::OnFoot(_)) || input.detection.traversal.is_some() {
        return Err("NOT ON FOOT");
    }
    let i = input.hand.index();
    if !physical_hands::free_hands()[i] || physical_hands::held_props()[i].is_some() {
        return Err("HAND NOT EMPTY");
    }
    if input.button_held {
        return Err("GRAPPLE BUTTON HELD");
    }
    let hook = unsafe { engine::local_hook() }.ok_or("NO HOOK")?;
    match engine::busy(hook) {
        Some(Refusal::LineOut) => Err("LINE OUT"),
        Some(_) => Err("HOOK BUSY"),
        None => Ok(()),
    }
}

/// The target the grapple's aim picks: the weapon under it, or a gun in a character's hands near
/// its line, within the range, in sight.
///
/// # Safety
///
/// The main thread, outside the world update.
unsafe fn choose(ray: &Ray, config: &SnatchConfig, takes_held: bool) -> Option<Offer> {
    #[derive(Copy, Clone)]
    enum Kind {
        Ground,
        Held(usize),
    }
    let ground = grapple_weapons::aimed();
    // A rack's weapons are the grab's alone.
    let held: Vec<WorldTarget> = if takes_held {
        steal::targets(|p| p)
            .into_iter()
            .filter(|t| matches!(t.kind, TargetKind::Held { .. }))
            .collect()
    } else {
        Vec::new()
    };
    let candidates = ground
        .iter()
        .map(|(aimed, _)| Candidate {
            kind: Kind::Ground,
            point: aimed.point,
        })
        .chain(held.iter().enumerate().map(|(i, target)| Candidate {
            kind: Kind::Held(i),
            point: target.points[0],
        }));
    let chosen = rules::choose(
        ray,
        candidates,
        config.cone_deg.to_radians(),
        config.range_m,
    )?;
    let point = chosen.candidate.point;
    let kind = match chosen.candidate.kind {
        Kind::Ground => {
            let (aimed, _alive) = ground.as_ref()?;
            // SAFETY: the strong reference keeps the weapon, and its weak reference, alive.
            let weapon = unsafe { &*(aimed.object.get() as *const WeaponBase) };
            OfferKind::Ground {
                weapon: aimed.object,
                weak: unsafe { Weak::of(&weapon.m_WeakThis) },
            }
        }
        Kind::Held(i) => {
            let target = held[i];
            let TargetKind::Held { character } = target.kind else {
                return None;
            };
            let player = unsafe { Character::GetLocalPlayerCharacter() } as usize;
            let options = CastOptions {
                ignore: [player, character],
                see_through: true,
                material: false,
            };
            // A ghost hook flies through walls: the snatch checks the sight line itself.
            if !unsafe { raycast::line_of_sight(ray.origin, point, SIGHT_TOLERANCE_M, &options) } {
                return None;
            }
            OfferKind::Held(target)
        }
    };
    Some(Offer {
        kind,
        point,
        distance: chosen.distance,
    })
}

/// Fire at the armed target: free a gun from its character's hands first, or fire the hook at a
/// weapon on the ground.
///
/// # Safety
///
/// The main thread, outside the world update.
unsafe fn fire_at(hand: Hand, offer: Offer) -> Result<Phase, Refusal> {
    match offer.kind {
        OfferKind::Ground { weapon, weak } => {
            let alive = weak.lock().ok_or(Refusal::Gone)?;
            let grabbed = Grabbed {
                weapon,
                _alive: alive,
            };
            let (dropped, body) = unsafe { engine::dropped(weapon) }?;
            // What the hand could not take is refused before the hook flies.
            let player = unsafe { Character::GetLocalPlayerCharacter().as_ref() }
                .ok_or(Refusal::NoPlayer)?;
            unsafe { pickup::takeable(player, dropped) }.map_err(Refusal::Take)?;
            unsafe { pickup::route(dropped, Some(hand)) }.map_err(Refusal::Take)?;
            let at = unsafe { body_centre(body, weapon) }?;
            let wire = unsafe { engine::fire(dropped, at) }?;
            haptics::play(hand, Event::GrappleFire);
            Ok(Phase::HookOut {
                hand,
                weapon: grabbed,
                wire,
                waited_s: 0.0,
            })
        }
        OfferKind::Held(target) => {
            let freed = unsafe { steal::release(hand, &target) }.map_err(Refusal::Free)?;
            let weapon = NonZeroUsize::new(freed.weapon as usize).ok_or(Refusal::Gone)?;
            tracing::info!(target: "controllers", hand = hand.label(), "snatch: the character let go of its gun");
            Ok(Phase::Freeing {
                hand,
                weapon: Grabbed {
                    weapon,
                    _alive: freed.alive,
                },
                waited_s: 0.0,
            })
        }
    }
}

/// The weapon's body's centre of mass, world space.
///
/// # Safety
///
/// The main thread, outside the physics step.
unsafe fn body_centre(body: u32, weapon: NonZeroUsize) -> Result<Vec3, Refusal> {
    unsafe { physical_hands::body_state(body, weapon) }
        .map(|state| state.pose.centre)
        .map_err(Refusal::Body)
}

/// One tick of the pull: drive the weapon toward the hand, and take it into the hand once it
/// arrives. Returns the slot it went to once taken.
///
/// # Safety
///
/// The main thread, outside the world update and the physics step.
unsafe fn pull(
    hand: Hand,
    weapon: &Grabbed,
    wire: NonZeroUsize,
    stall: &mut StallWatch,
    input: &FinishInput,
) -> Result<Option<WeaponSlot>, Refusal> {
    let config = &input.config.weapons.snatch;
    let (reading, line_m) = unsafe { engine::read_wire(wire) };
    if reading == engine::WireReading::Gone {
        return Err(Refusal::LineLost);
    }
    let (dropped, body) = unsafe { engine::dropped(weapon.weapon) }?;
    let state =
        unsafe { physical_hands::body_state(body, weapon.weapon) }.map_err(Refusal::Body)?;
    let Some(palm) = ik::tick_records().current[hand.index()].map(|t| t.wrist.position) else {
        // An untracked hand keeps the line where it is.
        return Ok(None);
    };
    let centre = state.pose.centre;
    let distance = centre.distance(palm);
    let stalled = stall.step(
        input.dt,
        centre,
        config.stall_window_s,
        config.stall_distance_m,
    );
    let limits = PullLimits {
        capture_radius_m: config.capture_radius_m,
        line_m: line_m.unwrap_or(config.range_m),
    };
    match rules::verdict(distance, stalled, &limits) {
        Verdict::Pulling => {}
        Verdict::Arrived => {
            unsafe { engine::cancel(wire) };
            let hold = &input.config.weapons.hold;
            let options = TakeOptions {
                drop_replaced: hold.drop_replaced,
                slide_s: if hold.take_slide { hold.snap_s } else { 0.0 },
                cause: Cause::Snatch,
                touch: None,
            };
            return unsafe { pickup::take(hand, dropped, options) }
                .map(Some)
                .map_err(Refusal::Take);
        }
        Verdict::Stalled => return Err(Refusal::Stalled),
        Verdict::OutOfLine => return Err(Refusal::OutOfLine),
    }
    let mass = state.mass_kg.unwrap_or(1.0);
    let tuning = HoldTuning {
        stiffness_per_s: config.stiffness_per_s.max(0.0),
        angular_stiffness_per_s: 0.0,
        max_speed_mps: config.max_speed_mps.max(0.0),
        max_angular_speed_rps: 0.0,
        hand_force_n: mass * config.max_accel_mps2.max(0.0),
        hand_torque_nm: 0.0,
        two_hand_angular_share: 1.0,
        gravity: Vec3::new(0.0, -GRAVITY_MPS2, 0.0),
    };
    let rigid = state.rigid();
    let hold = HandHold {
        anchor: centre,
        target: HoldPoint {
            point: rules::pull_point(centre, palm, config.arc),
            orientation: state.pose.orientation,
        },
        velocity: Vec3::ZERO,
        angular_velocity: rigid.velocities.angular,
    };
    let mut holds = [None; 2];
    holds[hand.index()] = Some(hold);
    let solution = hold::solve(&rigid, holds, input.dt, &tuning);
    unsafe {
        physical_hands::set_body_velocities(
            body,
            weapon.weapon,
            solution.velocities.linear,
            solution.velocities.angular,
        )
    }
    .map_err(Refusal::Body)?;
    haptics::sustain(hand, Sustain::GrappleTension, solution.loads[hand.index()]);
    Ok(None)
}

/// Toss the weapon a tap of the grapple reeled in to land in front of the player.
///
/// # Safety
///
/// The main thread, outside the world update and the physics step.
unsafe fn toss(weak: &Weak, hand: Hand) {
    let Some(alive) = weak.lock() else {
        return;
    };
    let address = alive.object() as usize - WeaponBase::GAME_OBJECT_BASE_OFFSET as usize;
    let Some(weapon) = NonZeroUsize::new(address) else {
        return;
    };
    let result = (|| {
        let (_, body) = unsafe { engine::dropped(weapon) }?;
        let from = unsafe { body_centre(body, weapon) }?;
        let player = unsafe { engine::player_position() }.ok_or(Refusal::NoPlayer)?;
        let toward = Vec3::new(from.x - player.x, 0.0, from.z - player.z)
            .try_normalize()
            .unwrap_or(Vec3::ZERO);
        let land = player + toward * TOSS_LAND_M;
        let flight = rules::toss_flight(
            from.distance(land),
            TOSS_SPEED_MPS,
            TOSS_FLIGHT_S.0,
            TOSS_FLIGHT_S.1,
        );
        let velocity = rules::toss_velocity(from, land, flight, Vec3::new(0.0, -GRAVITY_MPS2, 0.0));
        unsafe { physical_hands::set_body_velocities(body, weapon, velocity, Vec3::ZERO) }
            .map_err(Refusal::Body)
    })();
    match result {
        Ok(()) => {
            haptics::play(hand, Event::GrappleAttach);
            STATE.lock().status.tosses += 1;
            tracing::info!(target: "controllers", "snatch: tossed a weapon to the player's feet");
        }
        Err(refusal) => {
            tracing::info!(target: "controllers", %refusal, "snatch: the tap's toss did not happen");
        }
    }
}
