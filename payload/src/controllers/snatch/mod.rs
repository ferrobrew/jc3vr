//! The grapple snatch: the empty grapple hand pulls a thing out of reach into itself. The design is
//! in `docs/mod/input/grapple-snatch.md`, the engine side in "Weapons and the grapple" in
//! `docs/engine/gameplay/grapple-pipeline.md`.
//!
//! The grapple's aim sees weapons lying in the world through a patch scoped to the grapple's pass
//! of the aim ray ([`crate::controllers::aim::grapple_weapons`]), which also publishes what the
//! aim hit, and the steal notes the guns in characters' hands and holsters out to the snatch's
//! range ([`crate::controllers::weapons::hold::steal`]). On each input tick:
//!
//! 1. [`targets`] offers the grip one remote target ([`GrabVolume::Snatch`], ranked
//!    [`Tier::Remote`](crate::controllers::grab::Tier::Remote), taken only when nothing at all is
//!    in reach) for the empty grapple hand on foot, with the hook free and the grapple button up
//!    ([`target`]): the weapon, loose object, or car door under the grapple's aim, or the gun in a
//!    character's hands or holster nearest the aim's line within its cone and in sight.
//! 2. [`finish_tick`] acts on the grip's grab of it, which arms the snatch on that target. A yank
//!    of the hand back toward the body while the grip is held fires it ([`pull`]): a gun on a
//!    character is first let go of, with the steal's drop and stagger; then the game's hook flies
//!    to the target as a ghost wire through the custom fire ([`engine::fire`]). Once it holds a
//!    weapon or an object, the hold solve pulls the body along an arc toward the hand
//!    ([`rules::pull_point`]), and within the capture radius the wire is ended and the weapon taken
//!    into the hand through the loadout table, as a pickup is, or the object handed to the hand's
//!    ordinary hold. Once it holds a door, the door comes open and swings outward, and the wire is
//!    ended. Opening the grip at any point cancels: in flight the line comes away and the target
//!    keeps its momentum. A stall, a line run out, a missed hook, or a refused take fails the
//!    snatch and leaves the target in the world.
//!
//! The snatch's runtime state (armed, freeing, hook out, pulling) lives here alone; where a weapon
//! ends up is the loadout table's. The mod holds a strong reference to the target from the fire to
//! the end, holds the grapple button back from the game while its line is out, and ends its own
//! wire on every exit and stand-down.
//!
//! [`hooks`] makes the grapple button's tap and hold work on a weapon under the aim.

pub mod config;
mod engine;
mod hooks;
mod prearm;
mod pull;
pub mod rules;
mod target;

use std::{fmt, num::NonZeroUsize};

use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

pub use crate::controllers::snatch::config::SnatchConfig;
use crate::controllers::{
    aim::{self, grapple_weapons},
    bindings::{
        controls::ButtonControl,
        hold::Desired,
        tables,
        yank::{YankConfig, YankDetector},
    },
    config::ControllersConfig,
    frame::Hand,
    grab::{GrabVolume, GripTarget, Mark, MarkAt, Provided},
    haptics::{self, Event},
    loadout::slot_label,
    mode::{Detection, Mode},
    physical_hands::BodyError,
    snatch::{
        prearm::Prearm,
        pull::{Arrival, Attached, Fired, Grabbed},
        rules::StallWatch,
        target::Offer,
    },
    weapons::hold::{
        pickup,
        steal::{self, StealRefusal},
    },
};

/// The snatch's hooks, aggregated by [`crate::controllers::hook_library`].
pub(crate) fn hook_library() -> HookLibrary {
    hooks::hook_library()
}

/// Why a snatch did not happen, or ended before the target reached the hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// There is no local player or hook.
    NoPlayer,
    /// The grapple has a line out.
    LineOut,
    /// The hook is firing or reeling.
    HookBusy,
    /// The target is gone: despawned, picked up, or no longer where it was.
    Gone,
    /// The weapon no longer lies in the world.
    NotDropped,
    /// The weapon has no body in the world.
    NoBody,
    /// The hook did not take the custom fire.
    FireRefused,
    /// The hook missed, ran out of line, or the game ended its line before it held the target.
    Missed,
    /// The hook flew for too long without holding the target.
    TimedOut,
    /// The target stopped coming: snagged on the world.
    Stalled,
    /// The target is farther from the hand than the line reaches.
    OutOfLine,
    /// The line came away while it pulled.
    LineLost,
    /// The target's body could not be read or driven.
    Body(BodyError),
    /// The character would not let go of its gun.
    Free(StealRefusal),
    /// The take into the hand refused the weapon.
    Take(pickup::Refusal),
    /// The door would not come open.
    Door(String),
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
            Refusal::Stalled => "the target snagged",
            Refusal::OutOfLine => "the target is beyond the line",
            Refusal::LineLost => "the line came away",
            Refusal::Body(e) => return write!(f, "snatch: {e}"),
            Refusal::Free(e) => return write!(f, "snatch: {e}"),
            Refusal::Take(e) => return write!(f, "snatch: {e}"),
            Refusal::Door(e) => return write!(f, "snatch: the door: {e}"),
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
    /// The grapple hand's grip after input filtering.
    pub grip: ButtonControl,
    pub dt: f32,
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
    state.status.live = true;
    if !config.enabled || !matches!(state.phase, Phase::Idle) {
        state.prearm.clear();
        return Provided::default();
    }
    if let Err(why) = unsafe { target::offerable(input) } {
        state.prearm.clear();
        state.status.target = None;
        state.status.unoffered = Some(why);
        return Provided::default();
    }
    state.status.unoffered = None;
    let grapple = aim::status().grapple;
    let current = grapple
        .as_ref()
        .and_then(|grapple| unsafe { target::choose(&grapple.ray, input.config) });
    let Some(offer) = state
        .prearm
        .observe(current, input.grip, input.dt, |offer| {
            grapple.as_ref().is_some_and(|grapple| unsafe {
                target::refresh(offer, &grapple.ray, input.config)
            })
        })
    else {
        state.status.target = None;
        return Provided::default();
    };
    let readout = TargetReadout {
        kind: offer.kind.label(),
        distance_m: offer.distance,
    };
    let target = GripTarget::measured(
        GrabVolume::Snatch(offer.kind.remote()),
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
    state.status.target = Some(readout);
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
        match unsafe { pull::toss(&weak) } {
            Ok(()) => {
                haptics::play(input.hand, Event::GrappleAttach);
                STATE.lock().status.tosses += 1;
                tracing::info!(target: "controllers", "snatch: tossed a weapon to the player's feet");
            }
            Err(refusal) => {
                tracing::info!(target: "controllers", %refusal, "snatch: the tap's toss did not happen");
            }
        }
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
    state.prearm.clear();
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
    /// Snatches fired, brought to the hand (or doors opened), and failed, since the payload loaded.
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

static STATE: Mutex<State> = Mutex::new(State {
    phase: Phase::Idle,
    prearm: Prearm::new(),
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
    /// The recent aimed target retained across the grip edge.
    prearm: Prearm<Offer>,
    status: SnatchStatus,
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
        target: Grabbed,
        waited_s: f32,
    },
    /// The hook flies to the target.
    HookOut {
        hand: Hand,
        target: Grabbed,
        wire: NonZeroUsize,
        waited_s: f32,
    },
    /// The hook holds the target, and the pull brings it in.
    Pulling {
        hand: Hand,
        target: Grabbed,
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

fn armed_yank(distance: Option<f32>, config: &YankConfig) -> YankDetector {
    let mut yank = YankDetector::new();
    yank.step(0.0, distance, true, config);
    yank
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
                    match unsafe { pull::fire_at(hand, offer) } {
                        Ok(Fired::Freeing(target)) => Phase::Freeing {
                            hand,
                            target,
                            waited_s: 0.0,
                        },
                        Ok(Fired::HookOut(target, wire)) => {
                            haptics::play(hand, Event::GrappleFire);
                            Phase::HookOut {
                                hand,
                                target,
                                wire,
                                waited_s: 0.0,
                            }
                        }
                        Err(refusal) => self.failed(hand, refusal),
                    }
                }
                None => Phase::Armed { hand, offer, yank },
            },
            Phase::Freeing {
                hand,
                target,
                waited_s,
            } => match unsafe { pull::fire_hook(&target) } {
                Ok(wire) => {
                    haptics::play(hand, Event::GrappleFire);
                    Phase::HookOut {
                        hand,
                        target,
                        wire,
                        waited_s: 0.0,
                    }
                }
                // The gun's body may take a tick to lie in the world.
                Err(Refusal::NoBody | Refusal::Body(_) | Refusal::NotDropped)
                    if waited_s + input.dt < config.free_timeout_s =>
                {
                    Phase::Freeing {
                        hand,
                        target,
                        waited_s: waited_s + input.dt,
                    }
                }
                Err(refusal) => self.failed(hand, refusal),
            },
            Phase::HookOut {
                hand,
                target,
                wire,
                waited_s,
            } => match unsafe { engine::read_wire(wire) }.0 {
                engine::WireReading::Attached => {
                    haptics::play(hand, Event::GrappleAttach);
                    match unsafe { pull::attached(&target, config.door_swing_mps) } {
                        Ok(Attached::Pull) => {
                            tracing::info!(target: "controllers", hand = hand.label(), "snatch: the hook holds the target; pulling");
                            Phase::Pulling {
                                hand,
                                target,
                                wire,
                                stall: StallWatch::default(),
                            }
                        }
                        Ok(Attached::Opened) => {
                            unsafe { engine::cancel(wire) };
                            self.status.taken += 1;
                            tracing::info!(target: "controllers", hand = hand.label(), "snatch: the line pulled a car door open");
                            Phase::Idle
                        }
                        Err(refusal) => {
                            unsafe { engine::cancel(wire) };
                            self.failed(hand, refusal)
                        }
                    }
                }
                engine::WireReading::Flying if waited_s + input.dt < config.attach_timeout_s => {
                    Phase::HookOut {
                        hand,
                        target,
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
                target,
                wire,
                mut stall,
            } => match unsafe { pull::pull(hand, &target, wire, &mut stall, input) } {
                Ok(None) => Phase::Pulling {
                    hand,
                    target,
                    wire,
                    stall,
                },
                Ok(Some(arrival)) => {
                    self.status.taken += 1;
                    haptics::play(hand, Event::Take);
                    match arrival {
                        Arrival::Weapon(slot) => tracing::info!(
                            target: "controllers",
                            hand = hand.label(),
                            slot = slot_label(slot),
                            "snatch: the weapon came into the {} hand",
                            hand.label()
                        ),
                        Arrival::Prop => tracing::info!(
                            target: "controllers",
                            hand = hand.label(),
                            "snatch: the object came into the {} hand's hold",
                            hand.label()
                        ),
                    }
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
        if !matches!(input.grabs[hand.index()], Some(GrabVolume::Snatch(_))) {
            return Phase::Idle;
        }
        let Some(offer) = self.prearm.take() else {
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
            yank: armed_yank(input.distance, &input.config.weapons.snatch.yank),
        }
    }

    /// Cancel the snatch under way: the line comes away, and the target keeps its motion.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_held_tick_measures_the_pull_from_the_grip_edge() {
        let config = SnatchConfig::new().yank;
        let mut yank = armed_yank(Some(0.6), &config);
        assert!(yank.step(0.05, Some(0.42), true, &config).is_some());
        assert!(yank.step(0.05, Some(0.3), true, &config).is_none());
    }
}
