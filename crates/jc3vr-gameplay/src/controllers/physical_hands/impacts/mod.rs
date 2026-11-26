//! Solver-contact feedback and supplemental native damage for game-owned held bodies.

mod hooks;
mod policy;
mod state;
#[cfg(test)]
mod tests;

use std::{
    mem::MaybeUninit,
    num::NonZeroUsize,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Instant,
};

use glam::Vec3;
use jc3gi::{
    character::character::Character,
    damage::DamageFlags,
    havok::{hkcdManifold4, hknpContactImpulseEvent},
    physics::{PhysicsGameObject, hknpWorld},
};
use jc3vr_body::controllers::{
    haptics,
    physics::{
        bodies,
        refs::{Strong, Weak},
    },
    scheme,
};
use jc3vr_input::{frame::Hand, haptics::cues::Event};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::physical_hands::{
    carry_collision,
    config::PHYSICAL_HANDS_CONFIG,
    engine,
    impacts::{
        policy::{Motion, object_damage},
        state::{Contact, Enabled, FlagCall, Identity, Lease, Observation, State},
    },
    physics::{blows, blows::DamageContext, body, drawn_body},
    sounds,
    sounds::{Impact, ImpactKind},
    world::World,
};

/// Counters for held-prop contact observation and feedback.
#[derive(Debug, Default, Copy, Clone)]
pub struct Status {
    pub listening: usize,
    pub contacts: u64,
    pub feedback: u64,
    pub suppressed: u64,
    pub dropped: u64,
}

pub fn status() -> Status {
    STATE.lock().status
}
pub(crate) fn hook_library() -> HookLibrary {
    hooks::hook_library()
}

/// Publish current holders after the pickup tick. Both jointed and carried holds use this path.
pub(super) fn publish(held: [Option<(u32, NonZeroUsize)>; 2]) {
    STATE.lock().publish(held, Instant::now());
}

pub(super) fn obstruction(body: u32, owner: NonZeroUsize, lag: Vec3) -> Vec3 {
    let enabled = enabled();
    STATE
        .lock()
        .obstruction(body, owner, lag, Instant::now(), enabled)
}

/// Refresh motion and event leases immediately before the world step.
///
/// # Safety
/// The game thread, outside the physics step.
pub(crate) unsafe fn step() {
    unsafe { carry_collision::step() };
    let now = Instant::now();
    let enabled = enabled();
    let (wanted, leases) = {
        let state = STATE.lock();
        (state.wanted(now, enabled), state.leases)
    };
    let Some(mut world) = (unsafe { World::get() }) else {
        let mut state = STATE.lock();
        state.plan_step(now, enabled, &[]);
        HAS_LEASES.store(false, Ordering::Release);
        return;
    };
    let mut observations = Vec::with_capacity(4);
    for lease in leases.into_iter().flatten() {
        if let Some(observed) = unsafe {
            observe(
                &world,
                lease.identity.body,
                lease.identity.owner,
                Some(lease.motion),
            )
        } && observed.identity == lease.identity
        {
            observations.push(observed);
        }
    }
    for (body, owner) in wanted.into_iter().flatten() {
        if let Some(observed) = unsafe { observe(&world, body, owner, None) } {
            observations.push(observed);
        }
    }
    let changes = {
        let mut state = STATE.lock();
        let changes = state.plan_step(now, enabled, &observations);
        HAS_LEASES.store(state.status.listening != 0, Ordering::Release);
        changes
    };
    for call in changes {
        unsafe { hooks::apply(world.world_mut(), call) };
    }
}

/// Release leases and pending feedback before hook removal.
///
/// # Safety
/// The game thread, outside the physics step.
pub(crate) unsafe fn clear() {
    unsafe { carry_collision::clear() };
    STATE.lock().clear(Instant::now());
    RETAINED.lock().clear();
    unsafe { step() };
}

/// Deliver feedback and missing native damage after validating both contact participants.
///
/// # Safety
/// The game thread, outside the physics step.
pub(crate) unsafe fn deliver() {
    let enabled = enabled();
    if !enabled.active() {
        unsafe { clear() };
        return;
    }
    let config = *PHYSICAL_HANDS_CONFIG.lock();
    let retained = std::mem::take(&mut *RETAINED.lock());
    let locked: Vec<_> = retained
        .iter()
        .filter_map(|entry| {
            let held = entry.held_ref.lock()?;
            let world = unsafe { World::get() }?;
            if !unsafe { participant_live(&world, entry.held, &held) } {
                return None;
            }
            let target = match entry.target.as_ref() {
                Some((identity, weak)) => {
                    let strong = weak.lock()?;
                    if !unsafe { participant_live(&world, *identity, &strong) } {
                        return None;
                    }
                    Some((*identity, strong))
                }
                None => None,
            };
            Some((entry.token, held, entry.held, target))
        })
        .collect();
    let valid: Vec<_> = locked
        .iter()
        .flat_map(|(_, _, held, target)| {
            std::iter::once(*held).chain(target.as_ref().map(|(identity, _)| *identity))
        })
        .collect();
    let feedback = STATE.lock().deliver(Instant::now(), enabled, &valid);
    for feedback in feedback {
        let event = feedback.contact;
        let Some((_, held, _, target)) = locked.iter().find(|entry| entry.0 == event.retention)
        else {
            continue;
        };
        let Some(world) = (unsafe { World::get() }) else {
            continue;
        };
        if !unsafe { participant_live(&world, event.identity, held) }
            || event.target != target.as_ref().map(|(identity, _)| *identity)
            || target.as_ref().is_some_and(|(identity, strong)| {
                !unsafe { participant_live(&world, *identity, strong) }
                    || unsafe { body::in_player_group(identity.body) }
                    || identity.owner.get()
                        == unsafe { Character::GetLocalPlayerCharacter() } as usize
            })
        {
            continue;
        }
        if let (Some(impulse), Some((_, target))) = (feedback.damage_impulse, target.as_ref()) {
            unsafe {
                damage(
                    event,
                    impulse,
                    feedback.native,
                    held,
                    target,
                    config.bodies.reactions,
                )
            };
        }
        for hand in Hand::BOTH {
            if feedback.hands[hand.index()] {
                haptics::play_scaled(hand, Event::HandStrike, feedback.share);
            }
        }
        if !feedback.hands.into_iter().any(|hand| hand) {
            continue;
        }
        let hand = if feedback.hands[0] {
            Hand::Left
        } else {
            Hand::Right
        };
        unsafe {
            sounds::play(
                Impact {
                    hand,
                    kind: ImpactKind::Strike,
                    at: event.at,
                    velocity: event.motion.velocity_at(event.at),
                    material: sounds::strike_material(event.character),
                    strength: feedback.share,
                },
                &config.sounds,
            )
        };
    }
}

/// Observe contact values while all bodies and the solver Jacobian are live.
pub(super) unsafe fn record(event: *const hknpContactImpulseEvent) {
    if !HAS_LEASES.load(Ordering::Acquire) {
        return;
    }
    let Some(event) = (unsafe { event.as_ref() }) else {
        return;
    };
    if !matches!(
        event.m_status,
        hknpContactImpulseEvent::STATUS_STARTED | hknpContactImpulseEvent::STATUS_IMMEDIATE
    ) || event.m_contactJacobian.is_null()
    {
        return;
    }
    let leases = STATE.lock().leases;
    record_leased(
        event.m_bodyIds,
        leases,
        |leases| unsafe { observe_contacts(event, leases) },
        |contacts| {
            let mut state = STATE.lock();
            let now = Instant::now();
            for contact in contacts.into_iter().flatten() {
                state.queue(contact, now);
            }
        },
    );
}

fn record_leased(
    bodies: [u32; 2],
    leases: [Option<Lease>; 2],
    observe: impl FnOnce([Option<Lease>; 2]) -> [Option<Contact>; 2],
    enqueue: impl FnOnce([Option<Contact>; 2]),
) {
    if !leases
        .iter()
        .flatten()
        .any(|l| bodies.contains(&l.identity.body))
    {
        return;
    }
    let contacts = observe(leases);
    if contacts.iter().any(Option::is_some) {
        enqueue(contacts);
    }
}

unsafe fn observe_contacts(
    event: &hknpContactImpulseEvent,
    leases: [Option<Lease>; 2],
) -> [Option<Contact>; 2] {
    let mut contacts = [None; 2];
    let Some(world) = (unsafe { World::get() }) else {
        return contacts;
    };
    for (slot, lease) in contacts.iter_mut().zip(leases) {
        let Some(lease) = lease else { continue };
        let Some(side) = event
            .m_bodyIds
            .iter()
            .position(|b| *b == lease.identity.body)
        else {
            continue;
        };
        let other = event.m_bodyIds[1 - side];
        let other_owner = unsafe { bodies::owner_of(other) };
        let target = match other_owner {
            Some(owner) => {
                let Some(target) = (unsafe { identity(&world, other, owner) }) else {
                    continue;
                };
                Some(target)
            }
            None => None,
        };
        // Rico's own torso capsules are the player's body, not something the held object struck:
        // the hold is told it rests on them, and no feedback or push comes of the contact.
        if lease.armed && drawn_body::is_proxy(other) {
            drawn_body::note_contact(lease.identity.body);
            continue;
        }
        let player = unsafe { Character::GetLocalPlayerCharacter() } as usize;
        if !lease.armed
            || other_owner.is_some_and(|owner| owner.get() == player)
            || unsafe { identity(&world, lease.identity.body, lease.identity.owner) }
                != Some(lease.identity)
            || leases.iter().flatten().any(|l| l.identity.body == other)
            || unsafe { body::in_player_group(other) }
        {
            continue;
        }
        let mut manifold = MaybeUninit::<hkcdManifold4>::zeroed();
        unsafe { event.calculateContactPointPositions(world.world(), manifold.as_mut_ptr()) };
        let manifold = unsafe { manifold.assume_init() };
        let points = manifold.m_numPoints.clamp(0, 4) as usize;
        let Some(point) = (0..points)
            .filter(|i| event.m_contactImpulses[*i].is_finite())
            .max_by(|a, b| event.m_contactImpulses[*a].total_cmp(&event.m_contactImpulses[*b]))
        else {
            continue;
        };
        let impulse: f32 = event.m_contactImpulses[..points]
            .iter()
            .copied()
            .filter(|v| v.is_finite() && *v > 0.0)
            .sum();
        let at = Vec3::from_array(
            manifold.m_positions[point].data[..3]
                .try_into()
                .unwrap_or([f32::NAN; 3]),
        ) + world.shift;
        let normal = Vec3::from_array(
            manifold.m_normal.data[..3]
                .try_into()
                .unwrap_or([f32::NAN; 3]),
        );
        let normal = if side == 0 { -normal } else { normal };
        let character = other_owner.is_some_and(|owner| unsafe {
            crate::controllers::physical_hands::engine::is_character(owner)
        });
        let mut contact = lease.contact(at, normal, impulse, character);
        contact.target = target;
        contact.target_motion = target.and_then(|target| {
            unsafe { observe(&world, other, target.owner, None) }
                .filter(|observation| observation.identity == target)
                .map(|observation| observation.motion)
        });
        let mut retained = RETAINED.lock();
        if retained.len() >= 32 {
            continue;
        }
        let token = NEXT_CONTACT.fetch_add(1, Ordering::Relaxed);
        if token == u64::MAX {
            std::process::abort();
        }
        let held_ref =
            unsafe { Weak::of(&(*bodies::game_object_of(lease.identity.owner)).m_WeakThis) };
        let target = target.map(|identity| {
            let weak = unsafe { Weak::of(&(*bodies::game_object_of(identity.owner)).m_WeakThis) };
            (identity, weak)
        });
        retained.push(Retained {
            token,
            held: lease.identity,
            target,
            held_ref,
        });
        contact.retention = token;
        *slot = Some(contact);
    }
    contacts
}

/// Record a native collision conversion before supplemental delivery.
///
/// # Safety
/// The native caller keeps `recipient` alive through the collision conversion.
pub(super) unsafe fn native_hit(recipient: *mut PhysicsGameObject) {
    if !HAS_LEASES.load(Ordering::Acquire) {
        return;
    }
    let Some(world) = (unsafe { World::get() }) else {
        return;
    };
    let pairs: Vec<_> = RETAINED
        .lock()
        .iter()
        .filter_map(|entry| {
            let (target_identity, target_ref) = entry.target.as_ref()?;
            let target = if entry.held.owner.get() == recipient as usize {
                entry.held
            } else if target_identity.owner.get() == recipient as usize {
                *target_identity
            } else {
                return None;
            };
            let held = entry.held_ref.lock()?;
            let other = target_ref.lock()?;
            (unsafe { participant_live(&world, entry.held, &held) }
                && unsafe { participant_live(&world, *target_identity, &other) })
            .then_some(([entry.held, *target_identity], target))
        })
        .collect();
    let mut state = STATE.lock();
    for (pair, target) in pairs {
        // The opaque native hit info cannot prove the pair. Suppress conservatively by recipient.
        state.native(pair, target, Instant::now());
    }
}

/// Preserve native flag intent while a held-body lease keeps events enabled.
fn native_flags(call: FlagCall) -> FlagCall {
    if !HAS_LEASES.load(Ordering::Acquire) {
        return call;
    }
    let lease = STATE
        .lock()
        .leases
        .into_iter()
        .flatten()
        .find(|l| l.identity.world == call.world && l.identity.body == call.body);
    let identity = lease.and_then(|l| {
        unsafe { World::get() }
            .and_then(|world| unsafe { identity(&world, l.identity.body, l.identity.owner) })
    });
    STATE.lock().intercept_flags(call, identity)
}

struct Retained {
    token: u64,
    held: Identity,
    target: Option<(Identity, Weak)>,
    held_ref: Weak,
}

static RETAINED: Mutex<Vec<Retained>> = Mutex::new(Vec::new());
static NEXT_CONTACT: AtomicU64 = AtomicU64::new(1);
static STATE: Mutex<State> = Mutex::new(State::new());
// A false value skips only observer work. Native dispatch and flag calls always continue.
static HAS_LEASES: AtomicBool = AtomicBool::new(false);

fn enabled() -> Enabled {
    let controllers = scheme::enabled();
    let (hands, pickup) = PHYSICAL_HANDS_CONFIG.query(|c| (c.enabled, c.pickup.enabled));
    Enabled {
        controllers,
        hands,
        pickup,
        gameplay: jc3vr_core::hooks::in_gameplay(),
        running: !jc3vr_core::is_shutting_down(),
    }
}

unsafe fn participant_live(world: &World, id: Identity, strong: &Strong) -> bool {
    strong.object() == bodies::game_object_of(id.owner)
        && unsafe { identity(world, id.body, id.owner) } == Some(id)
}

unsafe fn damage(
    contact: Contact,
    impulse: f32,
    native: [bool; 2],
    held: &Strong,
    target: &Strong,
    reactions: bool,
) {
    let (Some(target_identity), Some(target_motion)) = (contact.target, contact.target_motion)
    else {
        return;
    };
    let Some(attacker) = (unsafe { Character::GetLocalPlayerCharacter().as_ref() }) else {
        return;
    };
    let Some(world) = (unsafe { World::get() }) else {
        return;
    };
    if !unsafe { participant_live(&world, contact.identity, held) }
        || !unsafe { participant_live(&world, target_identity, target) }
    {
        return;
    }
    // A character always takes the calibrated strike. Its capsule turns every contact into a
    // native hit reaction call, so deferring to that call skipped nearly every held prop's blow,
    // while the native reaction does nothing visible for a hand-held prop: on the 14 November take
    // a roof panel swung at 7 to 11 m/s through a soldier moved nobody.
    if reactions && contact.character && unsafe { engine::is_character(target_identity.owner) } {
        let victim = unsafe { &mut *(target_identity.owner.get() as *mut Character) };
        // A native reaction that also acted is told, so that a take shows any blow the character
        // took twice.
        if native[1] {
            tracing::info!(
                target: "controllers",
                impulse,
                "held impacts: the native hit reaction acted on the struck character as well"
            );
        }
        unsafe {
            blows::held_strike(
                attacker,
                victim,
                contact.at,
                contact.normal.normalize(),
                impulse,
            )
        };
    }
    // A target reaction may replace either physics instance. Revalidate both before the next call.
    if !unsafe { participant_live(&world, contact.identity, held) }
        || !unsafe { participant_live(&world, target_identity, target) }
    {
        return;
    }
    // Object-object contacts also feed the separate native damage-controller listener.
    // Its queued damage is not observable through DoReactToHit, so those contacts stay native.
    if !native[0]
        && contact.character
        && let Some(amount) = object_damage(impulse)
    {
        let source = unsafe { &(*target.object()).m_WeakThis };
        let dealt = unsafe {
            blows::dispatch_damage(
                contact.identity.owner.get() as *mut PhysicsGameObject,
                DamageContext {
                    source,
                    body: Some(target_identity.body),
                    source_position: target_motion.centre,
                    player: None,
                    flags: DamageFlags::DMG_TYPE_IMPACT,
                },
                contact.at,
                -contact.normal.normalize(),
                amount,
                0.0,
            )
        };
        tracing::debug!(
            dealt,
            amount,
            impulse,
            "held impacts: a held object received impact damage"
        );
    }
}

unsafe fn identity(world: &World, body: u32, owner: NonZeroUsize) -> Option<Identity> {
    if unsafe { bodies::owner_of(body) } != Some(owner) {
        return None;
    }
    Some(Identity {
        world: world.world() as *const hknpWorld as usize,
        body,
        owner,
        instance: unsafe { bodies::instance_of(world.world(), body) }?,
        control: unsafe { (*bodies::game_object_of(owner)).m_WeakThis.pn } as usize,
    })
}

unsafe fn observe(
    world: &World,
    body: u32,
    owner: NonZeroUsize,
    fallback: Option<Motion>,
) -> Option<Observation> {
    let identity = unsafe { identity(world, body, owner) }?;
    let motion = unsafe { bodies::read(body, owner) }
        .ok()
        .map(|m| Motion {
            centre: m.pose.centre,
            linear: m.velocities.linear,
            angular: m.velocities.angular,
            mass_kg: m.mass_kg.unwrap_or(0.0),
        })
        .or(fallback)?;
    let flags = unsafe { (*world.world().m_bodies.add(body as usize)).m_flags };
    Some(Observation {
        identity,
        flags,
        motion,
    })
}
