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
//! The grapple button's gestures ([`crate::controllers::bindings::grapple_button`]) say what the
//! player asks ([`LockButton`]), and [`lock_tick`] acts on them after the bindings' tables step:
//!
//! - a press locks the line at the distance it is at; for [`LockConfig::brake_s`] the lock brakes
//!   the reel's rush toward the hook, keeping the speed across the line, and the held length
//!   follows the character in. A press in the reel's wind-up, before the reel task carries the
//!   character, arms the lock, and the lock takes hold as the reel starts if the button is still
//!   held;
//! - pulling the grapple hand back takes line in ([`model::TakeIn`]);
//! - a quick release unlocks and resumes the reel, the tap going on to the game, which fires again
//!   from the reel when its upgrade is bought;
//! - a release after a longer hold lets go of the line, and the character flies on;
//! - a yank resumes the reel.
//!
//! The lock also ends when the character lands (the line let go of), when the line overstretches
//! or the hook's target is gone (the line breaks), and when the reel ends by itself.

mod engine;
mod hooks;
mod model;
#[cfg(test)]
mod tests;

use std::{
    num::NonZeroUsize,
    sync::atomic::{AtomicBool, Ordering},
};

use glam::Vec3;
use jc3gi::{
    character::character::Character,
    equipment::{grapple_point_proxy::AttachmentObjectInfo, grappling_hook::GrapplingHook},
    types::{math::Vector3, shared_ptr::SharedPtr},
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

pub use crate::controllers::bindings::grapple_button::LockButton;
use crate::{
    controllers::{
        bindings::yank::HandLine,
        frame::Hand,
        haptics::{self, Event, Sustain},
        physical_hands::remote,
        weapons::engine::character_position,
    },
    grapple::{
        hook,
        physical::{
            config::LockConfig,
            lock::model::{Held, SwingInput, TakeIn},
        },
        rope::{self, RopeFault},
    },
};

/// The lock's hooks, aggregated by [`crate::grapple::physical::hook_library`].
pub(super) fn hook_library() -> HookLibrary {
    hooks::hook_library()
}

/// Why a lock ended.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum LockEnd {
    /// A quick release: the reel resumes and the tap goes on to the game.
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
        }
    }

    /// Whether the line is let go of: the character flies on.
    const fn lets_go(self) -> bool {
        matches!(
            self,
            LockEnd::Released | LockEnd::Landed | LockEnd::Broke(_)
        )
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
    /// Whether the brake still turns the reel's rush into the swing.
    pub braking: bool,
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
    LOCKED.load(Ordering::Acquire)
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
    /// The grapple hand's grip and aim, in the cockpit frame, or `None` while untracked.
    pub line: Option<HandLine>,
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
    let Some((character, hook)) = (unsafe { hook::local() }) else {
        end(None, LockEnd::ReelEnded, input.hand);
        return;
    };
    if !holds() {
        if wants && input.carries && input.config.enabled {
            SHARED.lock().armed = false;
            unsafe { start(character, hook, input) };
        }
        return;
    }
    match unsafe { ending(character, hook, input, fault) } {
        Some(why) => end(Some(hook), why, input.hand),
        None => {
            let share = {
                let mut shared = SHARED.lock();
                let Some(active) = shared.active.as_mut() else {
                    return;
                };
                let taken =
                    active
                        .take_in
                        .step(input.real_dt, input.line, input.config.reel_in_mps);
                active.config = *input.config;
                active.held.take_in(taken, input.config);
                let length_m = active.held.rope.length_m;
                shared.status.length_m = length_m;
                shared.status.share()
            };
            haptics::sustain(input.hand, Sustain::GrappleTension, share);
        }
    }
}

/// Let go of the lock without touching the line: the bindings stand down.
pub(crate) fn stand_down() {
    let mut shared = SHARED.lock();
    shared.armed = false;
    if shared.active.take().is_some() {
        shared.status.locked = false;
        shared.status.last_end = Some(LockEnd::ReelEnded);
        LOCKED.store(false, Ordering::Release);
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
    let swung = model::swing(
        &mut active.held,
        SwingInput {
            rider,
            rider_velocity: Vec3::from_array(velocity_out.data),
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
    status.braking = active.held.braking();
    status.length_m = active.held.rope.length_m;
    status.stretch_m = out.stretch_m;
    status.tension_n = out.tension_n;
    status.max_tension_n = out.max_tension_n;
    status.hook = hook_point;
    status.hook_speed_mps = hook_velocity.length();
    status.rider = rider;
    true
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

/// Take the lock on the reel under way, or refuse it.
///
/// # Safety
///
/// The game thread, before the world update.
unsafe fn start(character: &Character, hook: &GrapplingHook, input: &LockTickInput) {
    let refuse = |why: &str| {
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
    let Some(held) = Held::new(hook_point, rider, input.config) else {
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
            take_in: TakeIn::default(),
            airborne: false,
        });
        shared.fault = None;
        shared.pending = None;
        shared.last_velocity = None;
        let locks = shared.status.locks + 1;
        shared.status = LockStatus {
            locked: true,
            braking: true,
            length_m: held.rope.length_m,
            target: Some(lock_target),
            hook: hook_point,
            rider,
            locks,
            ..shared.status
        };
    }
    LOCKED.store(true, Ordering::Release);
    tracing::info!(
        target: "grapple",
        length_m = held.rope.length_m,
        kind = lock_target.kind,
        target_mass_kg = ?lock_target.mass_kg,
        rider_mass_kg,
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
    match input.button {
        Some(LockButton::Tapped) => return Some(LockEnd::Tapped),
        Some(LockButton::LetGo) => return Some(LockEnd::Released),
        _ if input.yank => return Some(LockEnd::Yanked),
        _ => {}
    }
    if !input.config.enabled {
        return Some(LockEnd::ReelEnded);
    }
    let active = SHARED.lock().active?;
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
    let supported = unsafe { engine::supported(character) };
    if supported && active.airborne {
        return Some(LockEnd::Landed);
    }
    if !supported && let Some(active) = SHARED.lock().active.as_mut() {
        active.airborne = true;
    }
    None
}

/// End the lock for `why`, letting go of the line through `hook` when it does.
fn end(hook: Option<&mut GrapplingHook>, why: LockEnd, hand: Hand) {
    let status = {
        let mut shared = SHARED.lock();
        if shared.active.take().is_none() {
            return;
        }
        shared.pending = None;
        shared.fault = None;
        shared.last_velocity = None;
        shared.status.locked = false;
        shared.status.braking = false;
        shared.status.tension_n = 0.0;
        shared.status.last_end = Some(why);
        if matches!(why, LockEnd::Broke(_)) {
            shared.status.breaks += 1;
        }
        shared.status
    };
    LOCKED.store(false, Ordering::Release);
    if why.lets_go()
        && let Some(hook) = hook
    {
        // SAFETY: the game thread before the world update, the local player's live hook.
        unsafe { engine::let_go(hook) };
    }
    tracing::info!(
        target: "grapple",
        why = why.label(),
        length_m = status.length_m,
        stretch_m = status.stretch_m,
        "grapple lock: the lock ends"
    );
    haptics::sustain(hand, Sustain::GrappleTension, 0.0);
    match why {
        LockEnd::Broke(_) => haptics::play(hand, Event::LineSnap),
        LockEnd::Released | LockEnd::Landed => haptics::play(hand, Event::LineRelease),
        LockEnd::Tapped | LockEnd::Yanked | LockEnd::ReelEnded => {}
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
    /// Whether the character has left the ground since the lock took hold.
    airborne: bool,
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
    /// A press is held that has not yet taken the lock or been refused.
    armed: bool,
}

/// A leaf lock: nothing else is taken while it is held. The input tick and the reel task both run
/// on the game thread, so it is never contended in play.
static SHARED: Mutex<Shared> = Mutex::new(Shared {
    active: None,
    status: LockStatus {
        locked: false,
        braking: false,
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
    armed: false,
});

/// Whether the lock holds the line, for the detours, which must not wait on [`SHARED`].
static LOCKED: AtomicBool = AtomicBool::new(false);
