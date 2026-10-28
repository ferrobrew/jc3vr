//! Solver-contact feedback for game-owned held bodies. Native handlers, damage, materials,
//! collision filters, and constraint forces remain unchanged.

mod hooks;
mod policy;
mod state;
#[cfg(test)]
mod tests;

use std::{
    mem::MaybeUninit,
    num::NonZeroUsize,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use glam::Vec3;
use jc3gi::{
    character::character::Character,
    havok::{hkcdManifold4, hknpContactImpulseEvent},
    physics::hknpWorld,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        frame::Hand,
        haptics::{self, Event},
        physical_hands::{
            bodies,
            impacts::{
                policy::Motion,
                state::{Enabled, FlagCall, Identity, Observation, State},
            },
            physics::body,
            sounds::{self, Impact, ImpactKind},
            world::World,
        },
    },
};

/// Counters for held-prop contact observation and feedback.
#[derive(Debug, Default, Copy, Clone)]
pub(crate) struct Status {
    pub listening: usize,
    pub contacts: u64,
    pub feedback: u64,
    pub suppressed: u64,
    pub dropped: u64,
}

pub(crate) fn status() -> Status {
    STATE.lock().status
}
pub(crate) fn hook_library() -> HookLibrary {
    hooks::hook_library()
}

/// Publish current holders after the pickup tick. Both jointed and carried holds use this path.
pub(super) fn publish(held: [Option<(u32, NonZeroUsize)>; 2]) {
    STATE.lock().publish(held, Instant::now());
}

/// Refresh motion and event leases immediately before the world step.
///
/// # Safety
/// The game thread, outside the physics step.
pub(crate) unsafe fn step() {
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
    STATE.lock().clear(Instant::now());
    unsafe { step() };
}

/// Deliver only feedback. Character collision damage and reactions remain native.
///
/// # Safety
/// The game thread, outside the physics step.
pub(crate) unsafe fn deliver() {
    let enabled = enabled();
    if !enabled.active() {
        unsafe { clear() };
        return;
    }
    let config = Config::lock_query(|c| c.controllers.physical_hands);
    let leases = STATE.lock().leases;
    let valid: Vec<_> = unsafe { World::get() }
        .map(|world| {
            leases
                .into_iter()
                .flatten()
                .filter_map(|l| unsafe { identity(&world, l.identity.body, l.identity.owner) })
                .collect()
        })
        .unwrap_or_default();
    let feedback = STATE.lock().deliver(Instant::now(), enabled, &valid);
    for feedback in feedback {
        let event = feedback.contact;
        for hand in Hand::BOTH {
            if feedback.hands[hand.index()] {
                haptics::play_scaled(hand, Event::HandStrike, feedback.share);
            }
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
    let Some(world) = (unsafe { World::get() }) else {
        return;
    };
    let mut contacts = Vec::with_capacity(2);
    for lease in leases.into_iter().flatten() {
        let Some(side) = event
            .m_bodyIds
            .iter()
            .position(|b| *b == lease.identity.body)
        else {
            continue;
        };
        let other = event.m_bodyIds[1 - side];
        let other_owner = unsafe { bodies::owner_of(other) };
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
        contacts.push(lease.contact(at, normal, impulse, character));
    }
    let mut state = STATE.lock();
    let now = Instant::now();
    for contact in contacts {
        state.queue(contact, now);
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

static STATE: Mutex<State> = Mutex::new(State::new());
// A false value skips only observer work. Native dispatch and flag calls always continue.
static HAS_LEASES: AtomicBool = AtomicBool::new(false);

fn enabled() -> Enabled {
    Config::lock_query(|c| Enabled {
        controllers: c.controllers.enabled,
        hands: c.controllers.physical_hands.enabled,
        pickup: c.controllers.physical_hands.pickup.enabled,
        feature: c.controllers.physical_hands.held_impacts,
        gameplay: crate::hooks::in_gameplay(),
        running: !crate::is_shutting_down(),
    })
}

unsafe fn identity(world: &World, body: u32, owner: NonZeroUsize) -> Option<Identity> {
    if unsafe { bodies::owner_of(body) } != Some(owner) {
        return None;
    }
    Some(Identity {
        world: world.world() as *const hknpWorld as usize,
        body,
        owner,
        instance: unsafe { body::instance_of(world.world(), body) }?,
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
