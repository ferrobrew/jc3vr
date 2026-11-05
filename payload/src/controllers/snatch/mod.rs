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
//!    ([`target`]): the weapon, loose object, or car door under the grapple's aim, or the weapon
//!    lying in the world or gun in a character's hands or holster nearest the aim's line within its
//!    cone and in sight.
//! 2. [`finish_tick`] acts on the grip's grab of it, which arms the snatch on that target. A yank
//!    of the hand back along its aim at the grip edge while the grip is held fires it
//!    ([`ArmedLine`], [`pull`]): a gun on a
//!    character is first let go of, with the steal's drop and stagger; then the game's hook flies
//!    to the target as a ghost wire through the custom fire ([`engine::fire`]). Once it bites a
//!    weapon or an object, the line yanks the body toward the hand through the hold solve, pulling
//!    at the hook's point so the body spins, then over the final approach slows and pulls at the
//!    body's centre, and a couple of ticks out goes slack: the body flies on by its momentum, and
//!    when its path passes through a free hand whose grip is held, the grapple hand or the other,
//!    that hand has caught it ([`rules::verdict`]), the wire is ended, and the weapon is taken into
//!    that hand through the loadout table, as a pickup is, or the object handed to the hand's
//!    ordinary hold. While the line is out the other free hand's grip is held for the catch: it
//!    takes nothing else and drives no game action ([`reserved_hands`]). A body no hand is where it
//!    arrives flies past or hits the player and drops. Once the hook holds a
//!    door, the door comes open and swings outward, and the wire is ended. Opening the grip at any
//!    point cancels: in flight the line comes away and the target keeps its momentum. A stall, a
//!    line run out, a missed hook, a missed catch, or a refused take fails the snatch and leaves the
//!    target in the world. The grapple's own sounds and effects play throughout ([`juice`]).
//!
//! The snatch's runtime state (armed, freeing, hook out, pulling) lives here alone; where a weapon
//! ends up is the loadout table's. The mod holds a strong reference to the target from the fire to
//! the end, holds the grapple button back from the game while its line is out, and ends its own
//! wire on every exit and stand-down.
//!
//! A loose object too heavy for a hand to carry, up to the swing's ceiling, is never pulled in: once
//! the hook bites, the held line swings it ([`sling`]), a stiff, tension-only spring-damper from the
//! grapple hand whose strength scales with the target's mass, and which the other hand can brace at
//! the grapple hand's wrist ([`brace`]). Holding B after arming swings a weapon or a light object
//! the same way until B releases. [`sling_hooks`] keeps the native device endpoint on the tracked
//! forearm and excludes the owned wire's native constraint and tension path while the line swings.
//! Grip release writes no target velocity. [`hooks`] keeps ordinary grapple taps and holds working
//! on aimed weapons.

mod arrival;
mod brace;
pub mod config;
mod engine;
mod hooks;
mod juice;
pub mod landing;
mod latch;
mod moment;
mod prearm;
mod pull;
mod readout;
mod refusal;
pub mod rules;
mod sling;
mod sling_hooks;
mod target;

use std::num::NonZeroUsize;

use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

pub use crate::controllers::snatch::{
    config::{SNATCH, SnatchConfig, SwingConfig},
    readout::{SnatchStatus, SwingReadout, TargetReadout},
    refusal::Refusal,
};
use crate::controllers::{
    aim::{self, grapple_weapons},
    bindings::{
        controls::ButtonControl,
        hold::Desired,
        tables,
        yank::{ArmedLine, HandLine, YankConfig, YankDetector},
    },
    config::ControllersConfig,
    frame::Hand,
    grab::{GrabVolume, GripTarget, Mark, MarkAt, Provided, latch::GripLatch},
    haptics::{self, Event},
    loadout::slot_label,
    mode::{Detection, Mode},
    physical_hands,
    snatch::{
        arrival::ArrivalWatch,
        brace::Brace,
        moment::Moment,
        prearm::Prearm,
        pull::{Arrival, Attached, Fired, Flight, Grabbed, Step, Swinging},
        rules::PropHandling,
        target::{Offer, OfferKind},
    },
    weapons::hold::steal,
};

/// The snatch's hooks, aggregated by [`crate::controllers::hook_library`].
pub(crate) fn hook_library() -> HookLibrary {
    hooks::hook_library()
        .with_hook_library(sling_hooks::hook_library())
        .with_hook_library(juice::hook_library())
}

/// Only an already-gripped remote target can claim a subsequent fresh B press. A heavy object swings
/// without it, and the claimed press does nothing to it, rather than firing the game's grapple from
/// under the armed snatch.
pub(crate) fn can_claim_button() -> bool {
    matches!(STATE.lock().phase, Phase::Armed { .. })
}

/// A copied phase-specific instruction for the current usable-controls provider.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct Hint {
    /// The grapple hand.
    pub hand: Hand,
    /// Whether the line swings the target.
    pub swinging: bool,
    /// Whether B acts on the target: it swings a weapon or a light object, and releasing it brings
    /// one swung to the hand. A heavy object swings without it and never comes to the hand.
    pub modifier: bool,
}

/// The snatch's current instruction, while it has one.
pub(crate) fn hint() -> Option<Hint> {
    let state = STATE.lock();
    match state.phase {
        Phase::Armed {
            hand, ref offer, ..
        } => Some(Hint {
            hand,
            swinging: false,
            modifier: !heavy(offer),
        }),
        Phase::Pulling {
            hand,
            rope: Some(_),
            ref target,
            ..
        } => Some(Hint {
            hand,
            swinging: true,
            modifier: !target.kind.tethered(),
        }),
        _ => None,
    }
}

/// Whether `offer` is an object too heavy to come to the hand, which the line swings.
const fn heavy(offer: &Offer) -> bool {
    matches!(
        offer.kind,
        OfferKind::Prop {
            handling: PropHandling::Swing,
            ..
        } | OfferKind::Ragdoll { .. }
    )
}

/// The hands whose grips the snatch holds for its catch, by [`Hand::index`]: while its line is out,
/// the free, empty hand other than the grapple hand. A press of such a grip closes the hand on
/// nothing, ready to catch, rather than taking what is in reach or driving the game action bound to
/// it.
pub(crate) fn reserved_hands() -> [bool; 2] {
    let Some(hand) = STATE.lock().phase.in_flight() else {
        return [false; 2];
    };
    let free = physical_hands::free_hands();
    let props = physical_hands::held_props();
    Hand::BOTH.map(|h| h != hand && free[h.index()] && props[h.index()].is_none())
}

/// The hand other than the grapple hand that caught the snatch's target on the last tick, with the
/// grab its grip now holds the target by, as the grapple hand's grip holds a snatched target.
pub(crate) fn take_other_catch() -> Option<(Hand, GrabVolume)> {
    STATE.lock().other_catch.take()
}

/// What the bindings hand [`targets`].
pub(crate) struct OfferInput<'a> {
    /// The grapple hand.
    pub hand: Hand,
    pub detection: &'a Detection,
    /// Whether the grapple button is held.
    pub button_held: bool,
    /// The grapple hand's grip after input filtering.
    pub grip: ButtonControl,
    /// The tick's length, real seconds: how long the grapple hand's aim keeps a target.
    pub dt: f32,
    pub config: &'a ControllersConfig,
}

/// The remote target for the grapple hand's grip this tick, if there is one.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn targets(input: &OfferInput) -> Provided {
    let config = &input.config.weapons.snatch;
    let hold = &input.config.weapons.hold;
    let takes_held = hold.either_hand;
    grapple_weapons::set_enabled(config.enabled);
    steal::widen_notice(if config.enabled && takes_held {
        SNATCH.range_m
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
    state.status.aimed_mass_kg = unsafe { target::aimed_mass() };
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
        mass_kg: match offer.kind {
            OfferKind::Prop { mass_kg, .. } | OfferKind::Ragdoll { mass_kg, .. } => Some(mass_kg),
            _ => None,
        },
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

/// Separate consumers of the routed grapple hold.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct ButtonHolds {
    pub ordinary: bool,
    pub modifier: bool,
}

impl ButtonHolds {
    pub(crate) fn routed(held: bool, owned: bool) -> Self {
        Self {
            ordinary: held && !owned,
            modifier: held && owned,
        }
    }
}

#[cfg(test)]
pub(crate) use hooks::should_reel_weapon;

/// What the bindings hand [`finish_tick`].
pub(crate) struct FinishInput<'a> {
    /// The grapple hand.
    pub hand: Hand,
    /// The grabs the grips took this tick that the snatch acts on, by [`Hand::index`].
    pub grabs: [Option<GrabVolume>; 2],
    /// The grips, by [`Hand::index`].
    pub grips: [ButtonControl; 2],
    /// What each grip is latched on after the tables, by [`Hand::index`]: a hand catches only with
    /// its grip closed on nothing or on the snatch.
    pub latches: [GripLatch; 2],
    /// The grapple hand's grip and aim, or `None` while the hand is untracked.
    pub line: Option<HandLine>,
    /// The routed ordinary grapple hold, independent of the snatch modifier.
    pub ordinary_button_held: bool,
    /// The grapple hold owned by the snatch after a grip-first claim.
    pub modifier_held: bool,
    pub detection: &'a Detection,
    /// The tick's length, game seconds: the wire's flight and the free target's wait.
    pub dt: f32,
    /// The tick's length, real seconds: the hand's yank.
    pub real_dt: f32,
    pub config: &'a ControllersConfig,
}

/// Act on the tick's grab of a remote target, step the snatch, and hold the grapple button back from
/// the game while its line is out.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn finish_tick(input: &FinishInput, desired: &mut Vec<Desired>) {
    let config = &input.config.weapons.snatch;
    hooks::publish(config.enabled, input.ordinary_button_held);
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
    if let Some(watch) = &mut state.watch
        && !unsafe { watch.tick() }
    {
        state.watch = None;
    }
    let (from, hand) = (state.phase.label(), state.phase.hand());
    unsafe { state.step(input) };
    let to = state.phase.label();
    let hand = hand.or(state.phase.hand());
    state.moment.note(hand, from, to);
    let overlay = &input.config.overlay;
    match (&state.phase, state.status.swing) {
        _ if !(overlay.enabled && overlay.markers) => latch::clear(),
        (Phase::Armed { hand, offer, .. }, _) => latch::submit(*hand, offer.point),
        (Phase::Pulling { hand, .. }, Some(swing)) => latch::submit_line(*hand, &swing),
        _ => latch::clear(),
    }
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
    latch::clear();
    let mut state = STATE.lock();
    if !matches!(state.phase, Phase::Idle) {
        tracing::info!(target: "controllers", phase = state.phase.label(), "snatch: stood down");
        let (from, hand) = (state.phase.label(), state.phase.hand());
        state
            .moment
            .ended("stood down", "the bindings stood down".to_owned());
        state.moment.note(hand, from, Phase::Idle.label());
    }
    // SAFETY: the game stage runs on the game thread before the world update.
    unsafe { state.phase.end_line() };
    state.phase = Phase::Idle;
    state.prearm.clear();
    state.other_catch = None;
    state.status.live = false;
    state.status.phase = Phase::Idle.label();
    state.status.target = None;
    state.status.swing = None;
}

/// The latest readout. Safe from any thread: it takes only a leaf lock.
pub fn status() -> SnatchStatus {
    STATE.lock().status.clone()
}

/// The marker's diameter on a remote target, metres.
const MARK_SIZE_M: f32 = 0.3;

static STATE: Mutex<State> = Mutex::new(State {
    phase: Phase::Idle,
    moment: Moment::new(),
    prearm: Prearm::new(),
    other_catch: None,
    watch: None,
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
        aimed_mass_kg: None,
        swing: None,
    },
});

struct State {
    phase: Phase,
    /// What the step under way saw, for its recorded phase change.
    moment: Moment,
    /// The recent aimed target retained across the grip edge.
    prearm: Prearm<Offer>,
    /// The other hand's catch on the last tick, for the bindings to latch its grip on.
    other_catch: Option<(Hand, GrabVolume)>,
    /// The watch over a caught gun's first ticks in the hand.
    watch: Option<ArrivalWatch>,
    status: SnatchStatus,
}

enum Phase {
    Idle,
    /// The grip holds the target; a yank back along `line`, the hand's aim at the grip edge, fires.
    Armed {
        hand: Hand,
        offer: Offer,
        line: ArmedLine,
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
    /// The hook holds the target, and the line yanks it toward the hand, then goes slack for the
    /// catch.
    Pulling {
        hand: Hand,
        target: Grabbed,
        wire: NonZeroUsize,
        flight: Flight,
        rope: Option<sling::Sling>,
        brace: Brace,
    },
}

impl Phase {
    const fn label(&self) -> &'static str {
        match self {
            Phase::Idle => "IDLE",
            Phase::Armed { .. } => "ARMED",
            Phase::Freeing { .. } => "FREEING",
            Phase::HookOut { .. } => "HOOK OUT",
            Phase::Pulling { rope: Some(_), .. } => "SWINGING",
            Phase::Pulling { flight, .. } if flight.slack() => "INBOUND",
            Phase::Pulling { .. } => "PULLING",
        }
    }

    /// The grapple hand while the snatch's line is out: from the fire to the catch or the miss.
    const fn in_flight(&self) -> Option<Hand> {
        match self {
            Phase::Freeing { hand, .. }
            | Phase::HookOut { hand, .. }
            | Phase::Pulling { hand, .. } => Some(*hand),
            Phase::Idle | Phase::Armed { .. } => None,
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

    /// End the snatch's line, when it has one out, with the line's retract.
    unsafe fn end_line(&self) {
        sling_hooks::clear();
        sling_hooks::exclude(None);
        unsafe { juice::set_reeling(false) };
        if let Phase::HookOut { wire, .. } | Phase::Pulling { wire, .. } = self {
            unsafe { pull::end_line(*wire) };
        }
    }

    /// Whether the line is reeling its target in, for the reel's loop.
    fn reeling(&self) -> bool {
        matches!(self, Phase::Pulling { rope: None, flight, .. } if !flight.slack())
    }
}

/// The hands that may catch the snatch's target this tick, by [`Hand::index`]: the grapple hand
/// `hand`, and the other hand when it is free and empty, each with its grip held and closed on
/// nothing or on the snatch.
fn catchers(hand: Hand, input: &FinishInput) -> [bool; 2] {
    let free = physical_hands::free_hands();
    let props = physical_hands::held_props();
    Hand::BOTH.map(|h| {
        let i = h.index();
        let latched = match input.latches[i] {
            GripLatch::Fist { .. } => true,
            GripLatch::Grab(GrabVolume::Snatch(_)) => h == hand,
            GripLatch::Open | GripLatch::Grab(_) | GripLatch::Default => false,
        };
        input.grips[i].held() && latched && free[i] && props[i].is_none()
    })
}

fn armed_yank(reach: Option<f32>, config: &YankConfig) -> YankDetector {
    let mut yank = YankDetector::new();
    yank.step(0.0, reach, true, config);
    yank
}

/// Log the held line taking its target.
fn log_swing_start(swinging: &Swinging, tethered: bool) {
    tracing::info!(
        target: "controllers",
        length_m = swinging.length_m,
        mass_kg = swinging.mass_kg,
        max_tension_n = swinging.max_tension_n,
        tethered,
        "snatch: swinging on the held line"
    );
}

impl State {
    /// One input tick of the snatch.
    unsafe fn step(&mut self, input: &FinishInput) {
        let config = &input.config.weapons.snatch;
        self.status.swing = None;
        if let Phase::HookOut { target, .. } | Phase::Pulling { target, .. } = &self.phase
            && physical_hands::held_props().contains(&Some(target.object))
        {
            self.cancel("another hand took the target");
            return;
        }
        let on_foot =
            matches!(input.detection.mode, Mode::OnFoot(_)) && input.detection.traversal.is_none();
        if let Some(hand) = self.phase.hand() {
            let continuation = sling::Continuation {
                enabled: config.enabled,
                on_foot,
                tracked: input.line.is_some(),
                same_hand: hand == input.hand,
                empty_hand: physical_hands::free_hands()[hand.index()]
                    && physical_hands::held_props()[hand.index()].is_none(),
                grip_held: input.grips[hand.index()].held(),
            };
            if let Some(why) = continuation.cancellation() {
                self.cancel(why);
                return;
            }
            let swings = match &self.phase {
                Phase::Pulling { rope, target, .. } => rope.is_some() || target.kind.tethered(),
                Phase::HookOut { target, .. } => target.kind.tethered(),
                _ => false,
            };
            if swings && !config.sling {
                self.cancel("swinging on the line is off");
                return;
            }
        }
        let phase = std::mem::replace(&mut self.phase, Phase::Idle);
        self.phase = match phase {
            Phase::Idle => self.arm(input),
            Phase::Armed {
                hand,
                offer,
                mut line,
                mut yank,
            } => match yank.step(input.real_dt, line.reach(input.line), true, &config.yank) {
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
                    self.moment.target(offer.kind.label(), offer.distance);
                    self.moment.yank(pulled.distance_m, pulled.speed_mps);
                    haptics::play(hand, Event::SnatchFire);
                    self.status.fired += 1;
                    match unsafe { pull::fire_at(hand, offer) } {
                        Ok(Fired::Freeing(target)) => Phase::Freeing {
                            hand,
                            target,
                            waited_s: 0.0,
                        },
                        Ok(Fired::HookOut(target, wire)) => {
                            haptics::play(hand, Event::GrappleFire);
                            unsafe { juice::fire() };
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
                None => Phase::Armed {
                    hand,
                    offer,
                    line,
                    yank,
                },
            },
            Phase::Freeing {
                hand,
                target,
                waited_s,
            } => match unsafe { pull::fire_hook(&target) } {
                Ok(wire) => {
                    haptics::play(hand, Event::GrappleFire);
                    unsafe { juice::fire() };
                    Phase::HookOut {
                        hand,
                        target,
                        wire,
                        waited_s: 0.0,
                    }
                }
                // The gun's body may take a tick to lie in the world.
                Err(Refusal::NoBody | Refusal::Body(_) | Refusal::NotDropped)
                    if waited_s + input.dt < SNATCH.free_timeout_s =>
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
                    match unsafe { pull::attached(&target, SNATCH.door_swing_mps) } {
                        Ok(Attached::Pull) => {
                            haptics::play(hand, Event::SnatchHit);
                            let mut rope = None;
                            let mut brace = Brace::default();
                            let tethered = target.kind.tethered();
                            let result =
                                unsafe { pull::bite(hand, &target, wire) }.and_then(|flight| {
                                    if config.sling && (input.modifier_held || tethered) {
                                        let swinging = unsafe {
                                            pull::sling(
                                                hand, &target, wire, &mut rope, &mut brace, input,
                                            )
                                        }?;
                                        self.status.swing =
                                            Some(SwingReadout::new(swinging, tethered));
                                        log_swing_start(&swinging, tethered);
                                    }
                                    Ok(flight)
                                });
                            match result {
                                Ok(flight) => Phase::Pulling {
                                    hand,
                                    target,
                                    wire,
                                    flight,
                                    rope,
                                    brace,
                                },
                                Err(refusal) => {
                                    unsafe { pull::end_line(wire) };
                                    self.failed(hand, refusal)
                                }
                            }
                        }
                        Ok(Attached::Opened) => {
                            haptics::play(hand, Event::GrappleAttach);
                            unsafe { pull::end_line(wire) };
                            self.status.taken += 1;
                            tracing::info!(target: "controllers", hand = hand.label(), "snatch: the line pulled a car door open");
                            self.moment
                                .ended("taken", "the line pulled a car door open".to_owned());
                            Phase::Idle
                        }
                        Err(refusal) => {
                            unsafe { pull::end_line(wire) };
                            self.failed(hand, refusal)
                        }
                    }
                }
                engine::WireReading::Flying if waited_s + input.dt < SNATCH.attach_timeout_s => {
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
                mut flight,
                mut rope,
                mut brace,
            } => {
                let tethered = target.kind.tethered();
                let mode = sling::attached_mode(
                    config.sling,
                    input.modifier_held,
                    rope.is_some(),
                    tethered,
                );
                let result = match mode {
                    sling::AttachedMode::Swing => {
                        let starting = rope.is_none();
                        let was_braced = brace.braced();
                        unsafe { pull::sling(hand, &target, wire, &mut rope, &mut brace, input) }
                            .map(|swinging| {
                                if starting {
                                    log_swing_start(&swinging, tethered);
                                }
                                if swinging.braced != was_braced {
                                    tracing::info!(
                                        target: "controllers",
                                        braced = swinging.braced,
                                        max_tension_n = swinging.max_tension_n,
                                        "snatch: the other hand {} the line",
                                        if swinging.braced { "braced" } else { "let go of" }
                                    );
                                }
                                self.status.swing = Some(SwingReadout::new(swinging, tethered));
                                Step::Flying
                            })
                    }
                    sling::AttachedMode::Resume | sling::AttachedMode::Pull => {
                        if mode == sling::AttachedMode::Resume {
                            rope = None;
                            brace = Brace::default();
                            sling_hooks::clear();
                            flight.resume();
                            tracing::info!(target: "controllers", "snatch: released B; pulling to the hand");
                        }
                        let catchers = catchers(hand, input);
                        unsafe { pull::pull(hand, &target, wire, &mut flight, catchers, input) }
                    }
                    sling::AttachedMode::Cancel => Err(Refusal::Gone),
                };
                match result {
                    Ok(step @ (Step::Flying | Step::WentSlack)) => {
                        if matches!(step, Step::WentSlack) {
                            tracing::debug!(target: "controllers", hand = hand.label(), "snatch: the line went slack; the target flies to the hand");
                        }
                        Phase::Pulling {
                            hand,
                            target,
                            wire,
                            flight,
                            rope,
                            brace,
                        }
                    }
                    Ok(Step::Caught(catcher, arrival)) => {
                        self.status.taken += 1;
                        self.moment.ended(
                            "taken",
                            match &arrival {
                                Arrival::Weapon { slot, .. } => {
                                    format!(
                                        "a weapon came into the {} hand's {} slot",
                                        catcher.label(),
                                        slot_label(*slot)
                                    )
                                }
                                Arrival::Prop => {
                                    format!(
                                        "an object came into the {} hand's hold",
                                        catcher.label()
                                    )
                                }
                                Arrival::Grenade => {
                                    format!("a grenade came to the {} hand", catcher.label())
                                }
                            },
                        );
                        if catcher != hand
                            && let GripLatch::Grab(volume) = input.latches[hand.index()]
                        {
                            self.other_catch = Some((catcher, volume));
                        }
                        haptics::play(catcher, Event::SnatchCatch);
                        if let Arrival::Weapon { weapon, .. } = arrival {
                            self.watch = Some(ArrivalWatch::new(weapon, catcher));
                        }
                        match arrival {
                            Arrival::Weapon { slot, .. } => tracing::info!(
                                target: "controllers",
                                hand = catcher.label(),
                                line_hand = hand.label(),
                                slot = slot_label(slot),
                                "snatch: the weapon came into the {} hand",
                                catcher.label()
                            ),
                            Arrival::Prop => tracing::info!(
                                target: "controllers",
                                hand = catcher.label(),
                                line_hand = hand.label(),
                                "snatch: the object came into the {} hand's hold",
                                catcher.label()
                            ),
                            Arrival::Grenade => tracing::info!(
                                target: "controllers",
                                hand = catcher.label(),
                                line_hand = hand.label(),
                                "snatch: the grenade came to the {} hand",
                                catcher.label()
                            ),
                        }
                        Phase::Idle
                    }
                    Err(refusal) => {
                        unsafe { pull::end_line(wire) };
                        sling_hooks::clear();
                        self.failed(hand, refusal)
                    }
                }
            }
        };
        if matches!(self.phase, Phase::Idle) {
            sling_hooks::clear();
            sling_hooks::exclude(None);
        }
        unsafe { juice::set_reeling(self.phase.reeling()) };
        let keeps_sling = match &self.phase {
            Phase::HookOut { target, .. } | Phase::Pulling { target, .. } => {
                input.modifier_held || target.kind.tethered()
            }
            _ => input.modifier_held,
        };
        if keeps_sling && config.sling {
            if let Phase::HookOut {
                hand,
                wire,
                ref target,
                ..
            } = self.phase
                && !matches!(target.kind, pull::Pulled::Door { .. })
                && let Err(refusal) = unsafe { pull::prepare_sling(hand, wire) }
            {
                unsafe { engine::cancel(wire) };
                sling_hooks::clear();
                self.phase = self.failed(hand, refusal);
            }
        } else {
            sling_hooks::clear();
        }
    }

    /// Arm the snatch on the offered target, when the grapple hand's grip took it this tick.
    fn arm(&mut self, input: &FinishInput) -> Phase {
        let hand = input.hand;
        if !matches!(input.grabs[hand.index()], Some(GrabVolume::Snatch(_))) {
            return Phase::Idle;
        }
        // An untracked hand has no aim to yank back along; the continuation cancels it next tick.
        if input.line.is_none() {
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
            mass_kg = match offer.kind {
                OfferKind::Prop { mass_kg, .. } => Some(mass_kg),
                _ => None,
            },
            "snatch: armed; a yank fires"
        );
        self.moment.target(offer.kind.label(), offer.distance);
        haptics::play(hand, Event::SnatchArm);
        let mut line = ArmedLine::new();
        let reach = line.reach(input.line);
        Phase::Armed {
            hand,
            offer,
            line,
            yank: armed_yank(reach, &input.config.weapons.snatch.yank),
        }
    }

    /// Cancel the snatch under way: the line comes away, and the target keeps its motion.
    fn cancel(&mut self, why: &str) {
        let phase = std::mem::replace(&mut self.phase, Phase::Idle);
        // SAFETY: the game stage runs on the game thread before the world update.
        unsafe { phase.end_line() };
        tracing::info!(target: "controllers", phase = phase.label(), why, "snatch: cancelled");
        self.moment.ended("cancelled", why.to_owned());
    }

    /// Record a failed snatch of `hand`, and go idle.
    fn failed(&mut self, hand: Hand, refusal: Refusal) -> Phase {
        tracing::info!(target: "controllers", hand = hand.label(), %refusal, "snatch: failed");
        self.moment.ended("failed", refusal.to_string());
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
