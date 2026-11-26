//! The lock: while the reel carries the character toward the hook, holding the grapple button
//! holds the line's length, and the character swings from it. The mod side is in
//! `docs/mod/input/grapple-lock.md`, the engine side in "The reel's motion and endings" in
//! `docs/engine/gameplay/grapple-pipeline.md`.
//!
//! The lock stays inside the game's reel: the reel task keeps running, with its animation, its
//! turn toward the line, and the grapple comfort hold, while the reel's velocity detour
//! ([`crate::grapple::physical::reel`]) hands the reel the lock's velocity instead of the game's
//! ([`drive`]): the character's own velocity under gravity, which the reel's carries none of, with
//! the shared rope's pull from the hook's point ([`model::swing`]). The rope's relative motion is
//! measured against the hook's point's own velocity, so a lock onto a moving vehicle holds the
//! character at its offset from it. The reel's arrival and stuck check are held off while the
//! lock holds ([`hooks`]).
//!
//! The target feels the line's pull back: the rope shares its tension by inverse mass with the
//! body behind the hook's point, and the impulse it gave that body over the reel's ticks is applied
//! to it on the next input tick ([`lock_tick`]). A static target takes nothing, and a character is
//! never locked onto.
//!
//! On take, the lock turns the reel's rush into the swing: over the config's take time it turns the
//! character's speed toward the hook's point, measured against the hook's own motion, across the
//! line, keeping the speed as far as the line can hold the swing it starts, and lets the held
//! length follow the character in, so the line does not fall slack ([`model::swing`]).
//!
//! The grapple button's gestures ([`crate::controllers::bindings::grapple_button`]) say what the
//! player asks ([`LockButton`]), and [`lock_tick`] acts on them after the bindings' tables step:
//!
//! - pulling the grapple hand back takes line in ([`model::TakeIn`]), as does pulling back the free
//!   hand holding the line ahead of it ([`crate::grapple::line_grab`]);
//! - a quick release unlocks and resumes the reel, firing nothing: a fire onto the attached line
//!   can release it;
//! - a release after a longer hold lets go of the line, and the character flies on with the swing's
//!   velocity, captured before the retract and handed back after it;
//! - a yank resumes the reel.
//!
//! The lock also ends when the character lands (the line let go of), when the line overstretches
//! or the hook's target is gone (the line breaks), and when the reel ends by itself.
//!
//! The hang: a reel whose arrival would put the character in the game's upside-down ceiling
//! attach, which holds the body inverted against the surface, takes the lock instead
//! ([`hooks`]), with no press: the line pays out to a short length and the character hangs below
//! the hook, upright and swinging, the grapple hand pumping the swing and the other hand free. Any
//! release of the button, and a yank, let go of a hang, so the reel never resumes into the ceiling.

mod engine;
mod hooks;
mod model;
#[cfg(test)]
mod tests;

use std::{
    num::NonZeroUsize,
    time::{Duration, Instant},
};

use glam::Vec3;
use jc3gi::{
    character::character::Character,
    equipment::{grapple_point_proxy::AttachmentObjectInfo, grappling_hook::GrapplingHook},
    types::{math::Vector3, shared_ptr::SharedPtr},
};
use jc3vr_input::{
    frame::{self, Hand},
    haptics::cues::{Event, Sustain},
};
use jc3vr_view::{
    headpose::xr,
    vr::{VR_CONFIG, recenter_count},
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

pub use crate::controllers::bindings::grapple_button::LockButton;
use crate::{
    controllers::{
        bindings::yank::HandLine,
        facts, haptics,
        physical_hands::remote,
        physics::{GRAVITY_MPS2, grapple_hook as hook},
        weapons::engine::character_position,
    },
    grapple::{
        physical::{
            config::{GRAPPLE_CONFIG, LockConfig, PhysicalGrappleTuning, SwingTuning},
            lock::model::{FreeHand, Held, Pump, PumpSample, SwingInput, TakeIn},
        },
        rope::{self, RopeFault},
    },
    hooks::input::air,
};

/// The lock's hooks, aggregated by [`crate::grapple::physical::hook_library`].
pub(super) fn hook_library() -> HookLibrary {
    hooks::hook_library()
}

/// Why a lock ended.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum LockEnd {
    /// A quick release: the reel resumes.
    Tapped,
    /// The button let go of the line.
    Released,
    /// A yank: the reel resumes.
    Yanked,
    /// The character landed: the line is let go of.
    Landed,
    /// The line broke.
    Broke(BreakCause),
    /// The reel ended by itself, or the lock was switched off.
    ReelEnded,
    /// Y under a helicopter: the reel resumes into the game's hang below the airframe, which
    /// enters it.
    Entering,
}

impl LockEnd {
    pub const fn label(self) -> &'static str {
        match self {
            LockEnd::Tapped => "tapped",
            LockEnd::Released => "released",
            LockEnd::Yanked => "yanked",
            LockEnd::Landed => "landed",
            LockEnd::Broke(BreakCause::Stretched) => "broke: overstretched",
            LockEnd::Broke(BreakCause::TargetGone) => "broke: target gone",
            LockEnd::Broke(BreakCause::Invalid) => "broke: bad sample",
            LockEnd::ReelEnded => "reel ended",
            LockEnd::Entering => "entering the helicopter",
        }
    }

    /// Whether the line is let go of: the character flies on.
    const fn lets_go(self) -> bool {
        matches!(
            self,
            LockEnd::Released | LockEnd::Landed | LockEnd::Broke(_)
        )
    }

    /// Whether the ending hands the character its swing velocity back. A button release and a
    /// line break do; a landing does not, since the game's own landing takes the character from
    /// there.
    const fn restores_velocity(self) -> bool {
        matches!(self, LockEnd::Released | LockEnd::Broke(_))
    }
}

/// Why the line broke.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum BreakCause {
    /// The character went farther past the held length than the line stretches.
    Stretched,
    /// The hook's target is gone: destroyed, freed, or no longer attached.
    TargetGone,
    /// A sample the swing needs was not finite.
    Invalid,
}

/// What the lock holds onto.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct LockTarget {
    /// What the far end is attached to, as the game names it.
    pub kind: &'static str,
    /// The body's mass, or `None` for a target the line does not move.
    pub mass_kg: Option<f32>,
}

/// The lock's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct LockStatus {
    pub locked: bool,
    /// Whether the take still runs: the character coasts in toward the hook with the line
    /// following it.
    pub braking: bool,
    /// Whether the lock is a hang below a ceiling attach rather than a swing the button took.
    pub hanging: bool,
    /// Whether the free hand holds the line ahead of the grapple hand.
    pub free_hand: bool,
    /// The held length, metres.
    pub length_m: f32,
    /// How far the character is past the held length, metres: negative while slack.
    pub stretch_m: f32,
    /// The line's tension and its strength, newtons.
    pub tension_n: f32,
    pub max_tension_n: f32,
    pub target: Option<LockTarget>,
    /// The hook's point and its speed, and the character's position, world space.
    pub hook: Vec3,
    pub hook_speed_mps: f32,
    pub rider: Vec3,
    /// Locks taken and lines broken since the payload loaded.
    pub locks: u64,
    pub breaks: u64,
    pub last_end: Option<LockEnd>,
}

impl LockStatus {
    /// The tension as a share of the line's strength, `0..=1`.
    pub fn share(&self) -> f32 {
        rope::tension_share(self.tension_n, self.max_tension_n)
    }
}

/// The lock's readout. Safe from any thread: it takes only a leaf lock.
pub fn status() -> LockStatus {
    SHARED.lock().status
}

/// Whether the lock holds the line.
pub(crate) fn holds() -> bool {
    facts::lock::holds()
}

/// Whether the lock hangs the character below a helicopter, where Y enters it.
pub(crate) fn hangs_under_helicopter() -> bool {
    holds()
        && SHARED
            .lock()
            .active
            .is_some_and(|active| active.under_helicopter)
}

/// Whether Y under a helicopter has just pointed the resumed reel at its door side.
pub(crate) fn entering_helicopter() -> bool {
    HANG_GATE
        .lock()
        .entering
        .is_some_and(|at| at.elapsed() <= ENTERING_FOR)
}

/// Whether the lock could take hold now: the switch is on, and the local player's hook reels in
/// along an attached line to something other than a character.
///
/// # Safety
///
/// The game thread.
pub(crate) unsafe fn lockable(config: &LockConfig) -> bool {
    config.enabled
        && unsafe { hook::local() }.is_some_and(|(_, hook)| {
            unsafe { engine::reeling_far_end(hook) }
                .is_some_and(|proxy| engine::lockable_kind(proxy.m_AttachmentObjectInfo))
        })
}

/// What the bindings hand the lock's tick.
pub(crate) struct LockTickInput<'a> {
    /// What the grapple button asks of the lock this tick.
    pub button: Option<LockButton>,
    /// The reel task carries the character toward the hook: the lock may take hold. Before it, in
    /// the reel's wind-up, nothing drives the character, and a press only arms the lock.
    pub carries: bool,
    /// The physical grapple's yank fired this tick: the lock's one source of it, whatever the
    /// button's gesture is doing.
    pub yank: bool,
    /// The jump was pressed this tick: a way off the line, as from any hang.
    pub jump: bool,
    /// Y was pressed this tick: under a helicopter, the way into it.
    pub enter: bool,
    /// The grapple hand's grip and aim, in the cockpit frame, or `None` while untracked.
    pub line: Option<HandLine>,
    /// The free hand's grip in the cockpit frame, with the line's direction there as its aim, while
    /// it holds the line ([`crate::grapple::line_grab::lock_hand`]), or `None`.
    pub free_hand: Option<HandLine>,
    /// The wingsuit or the parachute is out: a release's velocity is theirs to carry on.
    pub deployed: bool,
    /// The tick's length, real seconds: the hand's take-in.
    pub real_dt: f32,
    pub hand: Hand,
    pub config: &'a LockConfig,
}

/// Run the lock's part of the input tick, after the bindings' tables have stepped: apply the
/// target's reaction, take the lock, end it, or take line in.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn lock_tick(input: &LockTickInput) {
    HANG_GATE.lock().ticked = Some(Instant::now());
    let (pending, fault) = {
        let mut shared = SHARED.lock();
        (shared.pending.take(), shared.fault.take())
    };
    if let Some(Reaction {
        body,
        owner,
        impulse,
        at,
    }) = pending
        && let Err(error) = unsafe { remote::push(body, owner, impulse, at) }
    {
        tracing::debug!(target: "grapple", %error, "grapple lock: the reaction found no body");
    }
    let wants = arm(input.button);
    let (pump_sample, pump_tuning) = pump_sample(input);
    let Some((character, hook)) = (unsafe { hook::local() }) else {
        end(None, LockEnd::ReelEnded, input.hand);
        return;
    };
    // The retract's cancel scales the character's velocity down to about a tenth a world update
    // after the release, under the reel's cancel state. Hand the release's velocity back, carried
    // on under gravity, on every tick of a short window that finds the character slower than
    // that, which lands it after the cancel whichever tick the scale falls on, and stop once the
    // character carries it. Anything else that takes the character's motion over ends it too:
    // touching down (a wall's or the ground's stop is not the cancel's), a new reel, the wingsuit,
    // or the parachute.
    let release_velocity = SHARED.lock().release_velocity;
    if let Some((velocity, at)) = release_velocity {
        let elapsed = at.elapsed().as_secs_f32();
        let expected = velocity - Vec3::Y * GRAVITY_MPS2 * elapsed;
        let measured = Vec3::from(character.m_MeasuredWorldVelocity);
        // Measured along the release's direction: a character turned aside by a wall carries less
        // of it however fast it goes.
        let carried = measured.is_finite()
            && expected.try_normalize().is_none_or(|direction| {
                measured.dot(direction) >= expected.length() * HANDBACK_CARRIED
            });
        // Until the cancel has had its world update, a character still carrying the velocity
        // proves nothing.
        let settled = elapsed >= HANDBACK_SETTLE_S && carried;
        let taken_over = input.deployed
            || unsafe { engine::supported(character) }
            || unsafe { engine::reeling_far_end(hook) }.is_some();
        if elapsed > HANDBACK_FOR_S || settled || taken_over {
            let mut shared = SHARED.lock();
            if shared
                .release_velocity
                .is_some_and(|(_, started)| started == at)
            {
                shared.release_velocity = None;
            }
        } else if !carried && expected.is_finite() {
            unsafe { engine::set_wanted_velocity(character, expected) };
        }
    }
    // A reel that ends with the character in the air carries its speed into the fall's and the
    // wingsuit's speed cap; the speed is the character's on the reel's last carrying tick.
    {
        let mut reel = REEL_SPEED.lock();
        if input.carries {
            let measured = Vec3::from(character.m_MeasuredWorldVelocity);
            *reel = measured.is_finite().then(|| measured.length());
        } else if let Some(speed) = reel.take()
            && !unsafe { engine::supported(character) }
        {
            air::carry_speed(speed);
        }
    }
    if !holds() {
        if !input.carries {
            hooks::forget_motion();
        }
        let hang = HANG_GATE
            .lock()
            .wanted
            .take()
            .filter(|at| at.elapsed() <= HANG_WANTED_FOR)
            .is_some();
        let kind = if hang && input.config.enabled && input.config.hang {
            Some(Hold::Hang)
        } else if wants && input.carries && input.config.enabled {
            Some(Hold::Swing)
        } else {
            None
        };
        if let Some(kind) = kind {
            SHARED.lock().armed = false;
            unsafe { start(character, hook, input, kind, pump_sample, pump_tuning) };
        }
        return;
    }
    match unsafe { ending(character, hook, input, fault) } {
        Some(why) => {
            if why == LockEnd::Entering {
                HANG_GATE.lock().entering = Some(Instant::now());
                // The reel is pointed at the nearest door side, so it pulls Rico up onto the skid
                // rather than into the stunt hang.
                let pull = unsafe { engine::pull_to_door_side(character, hook) };
                tracing::info!(
                    target: "grapple",
                    ?pull,
                    "grapple lock: Y below a helicopter: the reel pulls up to the door"
                );
            }
            end(Some((character, hook)), why, input.hand)
        }
        None => {
            let (share, free_held) = {
                let mut shared = SHARED.lock();
                let Some(active) = shared.active.as_mut() else {
                    return;
                };
                let (free_taken, changed) =
                    active
                        .free
                        .step(input.real_dt, input.free_hand, input.config.reel_in_mps);
                let free_held = active.free.held();
                if changed {
                    tracing::info!(
                        target: "grapple",
                        length_m = active.held.rope.length_m,
                        "grapple lock: the free hand {} the line",
                        if free_held { "took hold of" } else { "let go of" }
                    );
                    if !free_held {
                        haptics::sustain(input.hand.other(), Sustain::GrappleTension, 0.0);
                    }
                }
                let taken =
                    active
                        .take_in
                        .step(input.real_dt, input.line, input.config.reel_in_mps)
                        + free_taken;
                active.pump.sample(pump_sample, input.real_dt, &pump_tuning);
                active.pump_tuning = pump_tuning;
                active.pump_at = Instant::now();
                active.config = *input.config;
                active.held.take_in(taken, input.real_dt, input.config);
                let length_m = active.held.rope.length_m;
                let braking = active.held.coasting();
                shared.status.length_m = length_m;
                shared.status.braking = braking;
                shared.status.free_hand = free_held;
                (shared.status.share(), free_held)
            };
            haptics::sustain(input.hand, Sustain::GrappleTension, share);
            if free_held {
                haptics::sustain(input.hand.other(), Sustain::GrappleTension, share);
            }
        }
    }
}

/// Let go of the lock without touching the line: the bindings stand down.
pub(crate) fn stand_down() {
    hooks::forget_motion();
    let mut shared = SHARED.lock();
    shared.armed = false;
    shared.release_velocity = None;
    if shared.active.take().is_some() {
        shared.status.locked = false;
        shared.status.free_hand = false;
        shared.status.last_end = Some(LockEnd::ReelEnded);
        facts::lock::publish(false);
        tracing::info!(target: "grapple", "grapple lock: the bindings stand down");
    }
}

/// The reel's velocity while the lock holds: advance the swing from the character's velocity and
/// write it to `velocity`, returning whether the lock drove it. A prediction step
/// (`is_simulating`) is left as it is while the lock holds.
///
/// # Safety
///
/// The reel task's own arguments, on the character update that owns them.
pub(crate) unsafe fn drive(
    character: *mut Character,
    hook: *const SharedPtr<GrapplingHook>,
    velocity: *mut Vector3,
    current_position: *const Vector3,
    dt: f32,
    is_simulating: bool,
) -> bool {
    if !holds() {
        return false;
    }
    let local = unsafe { character.as_ref() }.is_some_and(|c| c.m_IsLocalCharacter);
    if !local {
        return false;
    }
    if is_simulating {
        return true;
    }
    let (Some(hook), Some(velocity_out), Some(position)) = (
        unsafe { hook.as_ref() }.and_then(|h| unsafe { h.px.as_ref() }),
        unsafe { velocity.as_mut() },
        unsafe { current_position.as_ref() },
    ) else {
        return false;
    };
    let Some(mut active) = SHARED.lock().active else {
        return false;
    };
    let proxy = unsafe { engine::reeling_far_end(hook) }
        .filter(|proxy| std::ptr::from_ref(*proxy) as usize == active.proxy.get());
    let Some((hook_point, hook_velocity)) = proxy.and_then(|p| unsafe { engine::point(p) }) else {
        SHARED.lock().fault = Some(BreakCause::TargetGone);
        return false;
    };
    let rider = Vec3::from_array(position.data);
    let reaction = if active.config.reaction.is_finite() {
        active.config.reaction.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let target_inverse_mass = active.target.map_or(0.0, |t| t.inverse_mass * reaction);
    if active.pump_at.elapsed() > Duration::from_millis(100) {
        active.pump.expire();
    }
    let pump_velocity = active.pump.take(
        dt,
        rider - hook_point,
        active.held.rope.length_m,
        &active.pump_tuning,
    );
    if let Some(stored) = SHARED.lock().active.as_mut() {
        stored.pump = active.pump;
    }
    let swung = model::swing(
        &mut active.held,
        SwingInput {
            rider,
            rider_velocity: Vec3::from_array(velocity_out.data) + pump_velocity,
            rider_mass_kg: active.rider_mass_kg,
            hook: hook_point,
            hook_velocity,
            target_inverse_mass,
            dt,
        },
        &active.config,
    );
    let mut shared = SHARED.lock();
    let out = match swung {
        Ok(out) => out,
        Err(fault) => {
            shared.fault = Some(match fault {
                RopeFault::Stretched => BreakCause::Stretched,
                RopeFault::Invalid => BreakCause::Invalid,
            });
            return false;
        }
    };
    let Some(stored) = shared.active.as_mut() else {
        return false;
    };
    stored.held = active.held;
    velocity_out.data = out.velocity.to_array();
    shared.last_velocity = Some(out.velocity);
    if let Some(target) = active.target.filter(|_| target_inverse_mass > 0.0) {
        let pending = shared.pending.get_or_insert(Reaction {
            body: target.body,
            owner: target.owner,
            impulse: Vec3::ZERO,
            at: hook_point,
        });
        pending.impulse += out.target_impulse * reaction;
        pending.at = hook_point;
    }
    let status = &mut shared.status;
    status.braking = active.held.coasting();
    status.length_m = active.held.rope.length_m;
    status.stretch_m = out.stretch_m;
    status.tension_n = out.tension_n;
    status.max_tension_n = out.max_tension_n;
    status.hook = hook_point;
    status.hook_speed_mps = hook_velocity.length();
    status.rider = rider;
    true
}

fn pump_sample(input: &LockTickInput) -> (Option<PumpSample>, SwingTuning) {
    let scale = VR_CONFIG.lock().world_scale;
    let tuning = GRAPPLE_CONFIG.query(|c| PhysicalGrappleTuning::of(c).swing);
    let fresh = frame::with_latest(|frame| {
        frame.focused
            && frame.published_at.elapsed() <= Duration::from_millis(100)
            && frame.hand(input.hand).active
            && frame.hand(input.hand).grip.is_tracked()
    })
    .unwrap_or(false);
    let sample = if fresh {
        input
            .line
            .zip(xr::cockpit_pose())
            .map(|(line, head)| PumpSample {
                hand: line.grip,
                head: head.position,
                rotation: xr::body_rotation(),
                world_scale: scale,
                recenters: recenter_count(),
            })
    } else {
        None
    };
    (sample, tuning)
}

/// Whether the button asks the lock to take hold this tick ([`model::armed`]).
fn arm(button: Option<LockButton>) -> bool {
    let mut shared = SHARED.lock();
    shared.armed = model::armed(shared.armed, button);
    shared.armed
}

/// The lock's last velocity for the character, for a reel tick that skipped its motion.
fn last_velocity() -> Option<Vec3> {
    SHARED.lock().last_velocity
}

/// Ask the lock's next tick to hang the character below the hook: the reel's arrival would have
/// put it in the game's upside-down ceiling attach.
fn want_hang() {
    HANG_GATE.lock().wanted = Some(Instant::now());
}

/// Whether a hang can take the place of the reel's arrival: the lock's tick runs (it does not while
/// the bindings stand idle, such as under the gamepad scheme), and it has not just refused a hang.
/// Otherwise the arrival held off would only arrive again on every reel tick, with nothing to end
/// the reel.
fn hang_takes_over() -> bool {
    let gate = HANG_GATE.lock();
    gate.ticked
        .is_some_and(|at| at.elapsed() <= HANG_WANTED_FOR)
        && gate
            .refused
            .is_none_or(|at| at.elapsed() > HANG_REFUSED_FOR)
}

/// How the lock took hold.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Hold {
    /// The button took the line during the reel.
    Swing,
    /// The reel's arrival would have attached the character upside down to a ceiling.
    Hang,
}

/// Take the lock on the reel under way, or refuse it.
///
/// # Safety
///
/// The game thread, before the world update.
unsafe fn start(
    character: &Character,
    hook: &GrapplingHook,
    input: &LockTickInput,
    hold: Hold,
    pump_sample: Option<PumpSample>,
    pump_tuning: SwingTuning,
) {
    let refuse = |why: &str| {
        if hold == Hold::Hang {
            HANG_GATE.lock().refused = Some(Instant::now());
        }
        tracing::info!(target: "grapple", why, "grapple lock: refused");
    };
    let Some(proxy) = (unsafe { engine::reeling_far_end(hook) }) else {
        return refuse("the hook does not reel along an attached line");
    };
    let kind = proxy.m_AttachmentObjectInfo;
    if !engine::lockable_kind(kind) {
        return refuse("the line holds a character");
    }
    let Some((hook_point, _)) = (unsafe { engine::point(proxy) }) else {
        return refuse("the hook's point is gone");
    };
    let Some(rider_mass_kg) = (unsafe { engine::rider_mass_kg(character) }) else {
        return refuse("the character's mass could not be read");
    };
    let rider = character_position(character);
    let held = match hold {
        Hold::Swing => Held::new(hook_point, rider, input.config),
        Hold::Hang => Held::hang(hook_point, rider, input.config.hang_length_m, input.config),
    };
    let Some(held) = held else {
        return refuse("the line's length is not finite");
    };
    let target = unsafe { engine::far_body(proxy) }.and_then(|(body, owner, state)| {
        let mass_kg = state.mass_kg?;
        Some(TargetBody {
            body,
            owner,
            inverse_mass: 1.0 / mass_kg,
        })
    });
    let Some(proxy_id) = NonZeroUsize::new(std::ptr::from_ref(proxy) as usize) else {
        return;
    };
    let mut pump = Pump::default();
    pump.sample(pump_sample, input.real_dt, &pump_tuning);
    let mut take_in = TakeIn::default();
    take_in.step(input.real_dt, input.line, input.config.reel_in_mps);
    let lock_target = LockTarget {
        kind: kind_label(kind),
        mass_kg: target.map(|t| 1.0 / t.inverse_mass),
    };
    {
        let mut shared = SHARED.lock();
        shared.active = Some(Active {
            held,
            proxy: proxy_id,
            rider_mass_kg,
            target,
            config: *input.config,
            take_in,
            free: FreeHand::default(),
            pump,
            pump_tuning,
            pump_at: Instant::now(),
            airborne: false,
            hang: hold == Hold::Hang,
            under_helicopter: hold == Hold::Hang
                && unsafe { engine::reeling_to_helicopter_hang(hook) },
        });
        shared.fault = None;
        shared.pending = None;
        shared.last_velocity = None;
        shared.release_velocity = None;
        let locks = shared.status.locks + 1;
        shared.status = LockStatus {
            locked: true,
            braking: held.coasting(),
            hanging: hold == Hold::Hang,
            free_hand: false,
            length_m: held.rope.length_m,
            target: Some(lock_target),
            hook: hook_point,
            rider,
            locks,
            ..shared.status
        };
    }
    facts::lock::publish(true);
    tracing::info!(
        target: "grapple",
        length_m = held.rope.length_m,
        kind = lock_target.kind,
        target_mass_kg = ?lock_target.mass_kg,
        rider_mass_kg,
        hang = hold == Hold::Hang,
        "grapple lock: the line locks"
    );
    haptics::play(input.hand, Event::LineLock);
}

/// Why the lock ends this tick, if it does.
///
/// # Safety
///
/// The game thread, before the world update.
unsafe fn ending(
    character: &Character,
    hook: &GrapplingHook,
    input: &LockTickInput,
    fault: Option<BreakCause>,
) -> Option<LockEnd> {
    let active = SHARED.lock().active?;
    // A hang's reel would only arrive at the ceiling again: every way out of it lets go.
    match input.button {
        Some(LockButton::Tapped) if active.hang => return Some(LockEnd::Released),
        Some(LockButton::Tapped) => return Some(LockEnd::Tapped),
        Some(LockButton::LetGo) => return Some(LockEnd::Released),
        _ if input.yank && active.hang => return Some(LockEnd::Released),
        _ if input.yank => return Some(LockEnd::Yanked),
        _ if input.jump => return Some(LockEnd::Released),
        _ if input.enter && active.under_helicopter => return Some(LockEnd::Entering),
        _ => {}
    }
    if !input.config.enabled || (active.hang && !input.config.hang) {
        return Some(LockEnd::ReelEnded);
    }
    let Some(proxy) = (unsafe { engine::attached_far_end(hook) }) else {
        return Some(LockEnd::ReelEnded);
    };
    if std::ptr::from_ref(proxy) as usize != active.proxy.get() || !unsafe { proxy.IsOk() } {
        return Some(LockEnd::Broke(BreakCause::TargetGone));
    }
    if let Some(fault) = fault {
        return Some(LockEnd::Broke(fault));
    }
    if let Some(target) = active.target
        && unsafe { engine::far_body(proxy) }.is_none_or(|(body, _, _)| body != target.body)
    {
        return Some(LockEnd::Broke(BreakCause::TargetGone));
    }
    // A held swing holds for as long as the button does, across the ground too: touching down
    // drags Rico along on the line rather than letting go of it. A hang's floor below its ceiling
    // ends it.
    let supported = unsafe { engine::supported(character) };
    if supported && active.airborne && active.hang {
        return Some(LockEnd::Landed);
    }
    if !supported && let Some(active) = SHARED.lock().active.as_mut() {
        active.airborne = true;
    }
    None
}

/// End the lock for `why`, letting go of the line through `hook` when it does. A release or a
/// break hands the character the swing's last velocity, before the retract and again after it: the
/// retract detaches the wire, and the reel task's cancel can clear the velocity the swing gave the
/// character.
fn end(local: Option<(&mut Character, &mut GrapplingHook)>, why: LockEnd, hand: Hand) {
    hooks::forget_motion();
    let (status, last_velocity, free_held) = {
        let mut shared = SHARED.lock();
        if local.is_none() {
            // The character the captured velocity belonged to is gone; never hand it to a later
            // one, even when the lock already ended.
            shared.release_velocity = None;
        }
        let Some(active) = shared.active.take() else {
            return;
        };
        shared.pending = None;
        shared.fault = None;
        let last_velocity = shared.last_velocity.take();
        shared.status.locked = false;
        shared.status.braking = false;
        shared.status.hanging = false;
        shared.status.free_hand = false;
        shared.status.tension_n = 0.0;
        shared.status.last_end = Some(why);
        if matches!(why, LockEnd::Broke(_)) {
            shared.status.breaks += 1;
        }
        (shared.status, last_velocity, active.free.held())
    };
    facts::lock::publish(false);
    if why.lets_go()
        && let Some((character, hook)) = local
    {
        let velocity = if why.restores_velocity() {
            // The swing's own last velocity is what the reel handed the character. The wanted
            // velocity read back from the character reads near zero while the lock drives the
            // reel, and a release that took it stopped the character dead in the air.
            // SAFETY: the game thread before the world update, the local player's live character.
            last_velocity.or_else(|| unsafe { engine::wanted_velocity(character) })
        } else {
            None
        };
        // SAFETY: the game thread before the world update, the local player's live hook.
        unsafe { engine::let_go(hook) };
        if let Some(velocity) = velocity {
            // SAFETY: as above; the character is the local player's.
            unsafe { engine::set_wanted_velocity(character, velocity) };
            // The reel's cancel runs on its next update; hand the velocity back once more after
            // it, in case the cancel cleared it.
            SHARED.lock().release_velocity = Some((velocity, Instant::now()));
            air::carry_speed(velocity.length());
        }
    }
    tracing::info!(
        target: "grapple",
        why = why.label(),
        length_m = status.length_m,
        stretch_m = status.stretch_m,
        "grapple lock: the lock ends"
    );
    haptics::sustain(hand, Sustain::GrappleTension, 0.0);
    if free_held {
        haptics::sustain(hand.other(), Sustain::GrappleTension, 0.0);
    }
    match why {
        LockEnd::Broke(_) => haptics::play(hand, Event::LineSnap),
        LockEnd::Released | LockEnd::Landed => haptics::play(hand, Event::LineRelease),
        LockEnd::Tapped | LockEnd::Yanked | LockEnd::ReelEnded | LockEnd::Entering => {}
    }
}

/// What the far end is attached to, as the readout names it.
const fn kind_label(kind: AttachmentObjectInfo) -> &'static str {
    match kind {
        AttachmentObjectInfo::AOI_Vehicle => "vehicle",
        AttachmentObjectInfo::AOI_Static => "static",
        AttachmentObjectInfo::AOI_Generic => "object",
        AttachmentObjectInfo::AOI_Character => "character",
        AttachmentObjectInfo::AOI_Invalid => "invalid",
    }
}

/// The lock while it holds the line.
#[derive(Copy, Clone)]
struct Active {
    held: Held,
    /// The far end's proxy the lock took hold through: another proxy is another line.
    proxy: NonZeroUsize,
    rider_mass_kg: f32,
    /// The dynamic body behind the hook's point, or `None` for one the line does not move.
    target: Option<TargetBody>,
    config: LockConfig,
    take_in: TakeIn,
    free: FreeHand,
    pump: Pump,
    pump_tuning: SwingTuning,
    pump_at: Instant,
    /// Whether the character has left the ground since the lock took hold.
    airborne: bool,
    /// Whether the lock is a hang below a ceiling attach.
    hang: bool,
    /// Whether the hang is below a helicopter, whose hang entry the reel would have arrived at.
    under_helicopter: bool,
}

/// The dynamic body behind the hook's point.
#[derive(Debug, Copy, Clone, PartialEq)]
struct TargetBody {
    body: u32,
    owner: NonZeroUsize,
    inverse_mass: f32,
}

/// The line's impulse on the target over the reel ticks since the last input tick.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Reaction {
    body: u32,
    owner: NonZeroUsize,
    impulse: Vec3,
    at: Vec3,
}

struct Shared {
    active: Option<Active>,
    status: LockStatus,
    pending: Option<Reaction>,
    fault: Option<BreakCause>,
    last_velocity: Option<Vec3>,
    /// The exit velocity a release captured and when, to hand back after the reel's cancel has
    /// run.
    release_velocity: Option<(Vec3, Instant)>,
    /// A press is held that has not yet taken the lock or been refused.
    armed: bool,
}

/// How long after a release its velocity is handed back while the character is slower than it,
/// seconds: past the reel's cancel and the state change after it.
const HANDBACK_FOR_S: f32 = 0.35;

/// How long after a release a character carrying its velocity counts as having kept it, seconds:
/// a few input ticks, past the world update in which the reel's cancel scales it.
const HANDBACK_SETTLE_S: f32 = 0.15;

/// The share of the release's velocity, carried on under gravity, at which the character counts
/// as carrying it.
const HANDBACK_CARRIED: f32 = 0.8;

/// A leaf lock: nothing else is taken while it is held. The input tick and the reel task both run
/// on the game thread, so it is never contended in play.
static SHARED: Mutex<Shared> = Mutex::new(Shared {
    active: None,
    status: LockStatus {
        locked: false,
        braking: false,
        hanging: false,
        free_hand: false,
        length_m: 0.0,
        stretch_m: 0.0,
        tension_n: 0.0,
        max_tension_n: 0.0,
        target: None,
        hook: Vec3::ZERO,
        hook_speed_mps: 0.0,
        rider: Vec3::ZERO,
        locks: 0,
        breaks: 0,
        last_end: None,
    },
    pending: None,
    fault: None,
    last_velocity: None,
    release_velocity: None,
    armed: false,
});

/// The character's speed on the reel's last carrying tick, until the reel ends.
static REEL_SPEED: Mutex<Option<f32>> = Mutex::new(None);

/// When the reel's arrival last asked for a hang ([`want_hang`]), when the lock last ticked, and
/// when it last refused a hang ([`hang_takes_over`]): a leaf lock the arrival's detours and the
/// input tick take in turn.
static HANG_GATE: Mutex<HangGate> = Mutex::new(HangGate {
    wanted: None,
    ticked: None,
    refused: None,
    entering: None,
});

struct HangGate {
    wanted: Option<Instant>,
    ticked: Option<Instant>,
    refused: Option<Instant>,
    /// When Y under a helicopter let the reel resume into the game's hang.
    entering: Option<Instant>,
}

/// How long after Y under a helicopter its pull up to the door lasts, for the log of its arrival
/// and the bindings' hold on the wingsuit: the pull and the door-side arrival's clip.
const ENTERING_FOR: Duration = Duration::from_secs(3);

/// How long an arrival's ask for a hang stands: the arrival asks again on every reel tick it
/// arrives, so an ask older than a few ticks belongs to a reel that has ended.
const HANG_WANTED_FOR: Duration = Duration::from_millis(250);

/// How long after a refused hang the reel's arrival is left to the game, so that the reel ends.
const HANG_REFUSED_FOR: Duration = Duration::from_secs(2);
