use std::{collections::HashSet, num::NonZeroUsize, time::Instant};

use jc3gi::{
    character::character::Character,
    havok::hkAabb,
    physics::{
        CustomGroupCollisionFilter as Filter, PhysicsSystem, hknpBody, hknpConstraint, hknpWorld,
    },
    vehicle::vehicle::Vehicle,
};
use jc3vr_body::controllers::{
    physics::{bodies, characters::proxy_body},
    scheme,
};
use parking_lot::Mutex;

use crate::controllers::physical_hands::{
    doors::engine::is_vehicle_part,
    physics::drawn_body,
    stale_after,
    tuning::{CarryCollisionTuning, PHYSICAL_HANDS},
    world::World,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Subject {
    pub body: u32,
    pub owner: NonZeroUsize,
    pub assembly: Assembly,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum Assembly {
    Body,
    Component,
    Character,
}

impl Subject {
    fn includes(self, body: u32, owner: Option<NonZeroUsize>, seed_group: u32, group: u32) -> bool {
        owner == Some(self.owner) && (self.assembly == Assembly::Character || self.body == body)
            || self.assembly == Assembly::Character && seed_group != 0 && seed_group == group
    }
}

pub(super) fn publish(held: [Option<Subject>; 2]) {
    let mut state = STATE.lock();
    state.wanted = held;
    state.published = Some(Instant::now());
}

pub(super) unsafe fn step() {
    let enabled = scheme::enabled()
        && scheme::is_controllers()
        && jc3vr_core::hooks::in_gameplay()
        && !jc3vr_core::is_shutting_down();
    let now = Instant::now();
    let tuning = &PHYSICAL_HANDS.get().carry_collision;
    let mut state = STATE.lock();
    let fresh = state
        .published
        .is_some_and(|at| now.saturating_duration_since(at) <= stale_after());
    let wanted = if enabled && fresh {
        state.wanted
    } else {
        [None; 2]
    };
    if state.leases.is_empty() && wanted.iter().all(Option::is_none) {
        return;
    }
    let Some(mut world) = (unsafe { World::get() }) else {
        return;
    };
    let Some(physics) = (unsafe { PhysicsSystem::get() }) else {
        return;
    };
    let Some(filter) = (unsafe { physics.m_PairCollisionFilter.as_mut() }) else {
        return;
    };
    let context = Context {
        world: world.world() as *const hknpWorld as usize,
        filter: filter as *mut _ as usize,
    };
    let subjects: Vec<_> = wanted
        .into_iter()
        .flatten()
        .filter_map(|subject| {
            if unsafe { bodies::body_owner(world.world(), subject.body) } != Some(subject.owner) {
                return None;
            }
            let slot = unsafe { live_body(world.world(), subject.body) }?;
            Some((
                subject,
                slot.m_collisionFilterInfo >> Filter::SYSTEM_GROUP_SHIFT,
                if subject.assembly == Assembly::Component {
                    unsafe { component(world.world(), subject) }
                } else {
                    HashSet::new()
                },
            ))
        })
        .collect();
    let player = unsafe { Character::GetLocalPlayerCharacter().as_ref() };
    let player_owner =
        player.and_then(|player| NonZeroUsize::new(player as *const Character as usize));
    let player_group = player
        .and_then(|player| unsafe { proxy_body(player) })
        .and_then(|body| unsafe { live_body(world.world(), body) })
        .map(|slot| slot.m_collisionFilterInfo >> Filter::SYSTEM_GROUP_SHIFT)
        .filter(|group| *group != 0);
    let mut carried = Vec::new();
    let mut players = Vec::new();
    for body in 0..=world.world().m_peakBodyIndex {
        let Some(slot) = (unsafe { live_body(world.world(), body) }) else {
            continue;
        };
        let owner = unsafe { bodies::body_owner(world.world(), body) };
        // The player proxy shares a vehicle's system group while standing in its stunt area, so
        // the group alone also takes in the vehicle's chassis and parts: those are never the
        // player's. Counted as the player's on the 14 November take, a jeep's deck let a yanked
        // gunner's ragdoll fall straight through it.
        let player_body = player_owner.is_some() && owner == player_owner
            || player_group.is_some_and(|group| {
                slot.m_collisionFilterInfo >> Filter::SYSTEM_GROUP_SHIFT == group
            }) && !owner.is_some_and(|owner| unsafe { vehicle_owned(owner) })
                // The drawn torso's capsules are what a held object is to meet.
                && !drawn_body::is_proxy(body);
        let held = subjects.iter().any(|(subject, group, component)| {
            component.contains(&body)
                || subject.includes(
                    body,
                    owner,
                    *group,
                    slot.m_collisionFilterInfo >> Filter::SYSTEM_GROUP_SHIFT,
                )
        });
        if !player_body && !held {
            continue;
        }
        let identity = unsafe { Identity::observe(world.world(), body) };
        if player_body {
            players.extend(identity);
        } else if held {
            carried.extend(identity);
        }
    }
    state.leases.retain_mut(|lease| {
        if lease.context != context {
            return false;
        }
        let current = unsafe { lease.a.current(world.world()) && lease.b.current(world.world()) };
        let held = current && carried.contains(&lease.a) && players.contains(&lease.b);
        let overlapping = current
            && !held
            && !unsafe { separated(world.world(), lease.a.body, lease.b.body, tuning) };
        if lease.retain(
            held,
            now,
            overlapping,
            jc3vr_core::is_shutting_down(),
            tuning,
        ) {
            return true;
        }
        unsafe { filter.enableCollisionsBetween(world.world_mut(), lease.a.body, lease.b.body) };
        false
    });
    for a in carried {
        for &b in &players {
            if state
                .leases
                .iter()
                .any(|lease| lease.a == a && lease.b == b)
            {
                continue;
            }
            unsafe { filter.disableCollisionsBetween(world.world_mut(), a.body, b.body) };
            state.leases.push(Lease {
                context,
                a,
                b,
                released: None,
            });
        }
    }
    let paired = lease_bodies(&state.leases);
    if paired != state.logged {
        tracing::info!(
            target: "controllers",
            carried = ?paired.0,
            player = ?paired.1,
            leases = state.leases.len(),
            "physical hands: carry collision: the held bodies kept off the player changed"
        );
        state.logged = paired;
    }
}

/// The distinct carried and player bodies `leases` pair, each sorted.
fn lease_bodies(leases: &[Lease]) -> (Vec<u32>, Vec<u32>) {
    let mut carried: Vec<u32> = leases.iter().map(|lease| lease.a.body).collect();
    let mut player: Vec<u32> = leases.iter().map(|lease| lease.b.body).collect();
    for bodies in [&mut carried, &mut player] {
        bodies.sort_unstable();
        bodies.dedup();
    }
    (carried, player)
}

pub(super) unsafe fn clear() {
    let mut state = STATE.lock();
    state.wanted = [None; 2];
    state.published = None;
    state.logged = (Vec::new(), Vec::new());
    let Some(physics) = (unsafe { PhysicsSystem::get() }) else {
        return;
    };
    let world = physics.m_World;
    if world.is_null() {
        return;
    }
    let Some(filter) = (unsafe { physics.m_PairCollisionFilter.as_mut() }) else {
        return;
    };
    let context = Context {
        world: world as usize,
        filter: filter as *mut _ as usize,
    };
    for lease in state
        .leases
        .drain(..)
        .filter(|lease| lease.context == context)
    {
        unsafe { filter.enableCollisionsBetween(world, lease.a.body, lease.b.body) };
    }
}

pub(super) unsafe fn destroying(world: *mut hknpWorld, ids: *const u32, count: i32) {
    if world.is_null() || ids.is_null() || count <= 0 {
        return;
    }
    let mut state = STATE.lock();
    let ids = unsafe { std::slice::from_raw_parts(ids, count as usize) };
    for subject in &mut state.wanted {
        if subject.is_some_and(|subject| ids.contains(&subject.body)) {
            *subject = None;
        }
    }
    if state.leases.is_empty() {
        return;
    }
    let Some(physics) = (unsafe { PhysicsSystem::get() }) else {
        return;
    };
    if physics.m_World != world {
        return;
    }
    let Some(filter) = (unsafe { physics.m_PairCollisionFilter.as_mut() }) else {
        return;
    };
    let context = Context {
        world: world as usize,
        filter: filter as *mut _ as usize,
    };
    state.leases.retain(|lease| {
        if lease.context != context {
            return false;
        }
        if ids.contains(&lease.a.body) || ids.contains(&lease.b.body) {
            unsafe { filter.enableCollisionsBetween(world, lease.a.body, lease.b.body) };
            false
        } else {
            true
        }
    });
}

#[derive(Copy, Clone, PartialEq, Eq)]
struct Context {
    world: usize,
    filter: usize,
}

#[derive(Copy, Clone, PartialEq, Eq)]
struct Identity {
    body: u32,
    owner: Option<NonZeroUsize>,
    instance: Option<usize>,
    shape: usize,
}

impl Identity {
    unsafe fn observe(world: &hknpWorld, body: u32) -> Option<Self> {
        let slot = unsafe { live_body(world, body) }?;
        Some(Self {
            body,
            owner: unsafe { bodies::body_owner(world, body) },
            instance: unsafe { bodies::instance_of(world, body) },
            shape: slot.m_shape as usize,
        })
    }

    unsafe fn current(self, world: &hknpWorld) -> bool {
        (unsafe { Self::observe(world, self.body) }) == Some(self)
    }
}

struct Lease {
    context: Context,
    a: Identity,
    b: Identity,
    released: Option<Instant>,
}

impl Lease {
    fn retain(
        &mut self,
        held: bool,
        now: Instant,
        overlapping: bool,
        force: bool,
        tuning: &CarryCollisionTuning,
    ) -> bool {
        if force {
            return false;
        }
        if held {
            self.released = None;
            return true;
        }
        let released = *self.released.get_or_insert(now);
        overlapping
            && now.saturating_duration_since(released).as_secs_f32() < tuning.release_grace_s
    }
}

struct State {
    wanted: [Option<Subject>; 2],
    published: Option<Instant>,
    leases: Vec<Lease>,
    /// The bodies the leases last paired, carried and the player's, for logging each change.
    logged: (Vec<u32>, Vec<u32>),
}

unsafe fn component(world: &hknpWorld, subject: Subject) -> HashSet<u32> {
    let candidates: Vec<_> = (0..=world.m_peakBodyIndex)
        .filter_map(|body| {
            let slot = unsafe { live_body(world, body) }?;
            if unsafe { bodies::body_owner(world, body) } != Some(subject.owner) {
                return None;
            }
            Some((
                body,
                unsafe { bodies::instance_of(world, body) },
                slot.m_motionId,
                slot.m_flags & hknpBody::IS_DYNAMIC != 0,
            ))
        })
        .collect();
    let manager = &world.m_constraintManager;
    let constraints =
        if manager.m_constraintBuffer.is_null() || manager.m_capacity > MAX_CONSTRAINTS {
            &[][..]
        } else {
            unsafe {
                std::slice::from_raw_parts(manager.m_constraintBuffer, manager.m_capacity as usize)
            }
        };
    let mut members = HashSet::from([subject.body]);
    let mut pending = vec![subject.body];
    while let Some(body) = pending.pop() {
        let Some(&(_, instance, motion, dynamic)) =
            candidates.iter().find(|candidate| candidate.0 == body)
        else {
            continue;
        };
        for &(other, other_instance, other_motion, other_dynamic) in &candidates {
            if (instance.is_some() && instance == other_instance
                || dynamic && other_dynamic && motion == other_motion)
                && members.insert(other)
            {
                pending.push(other);
            }
        }
        for constraint in constraints {
            if constraint.m_bodyIdA == hknpConstraint::INVALID_BODY_ID
                || constraint.m_flags & hknpConstraint::IS_DISABLED != 0
            {
                continue;
            }
            let other = if constraint.m_bodyIdA == body {
                constraint.m_bodyIdB
            } else if constraint.m_bodyIdB == body {
                constraint.m_bodyIdA
            } else {
                continue;
            };
            if candidates.iter().any(|candidate| candidate.0 == other) && members.insert(other) {
                pending.push(other);
            }
        }
    }
    members
}

unsafe fn live_body(world: &hknpWorld, body: u32) -> Option<&hknpBody> {
    let slot = unsafe { bodies::slot(world, body) }?;
    (slot.m_flags & (hknpBody::IS_STATIC | hknpBody::IS_DYNAMIC) != 0 && !slot.m_shape.is_null())
        .then_some(slot)
}

unsafe fn separated(world: &hknpWorld, a: u32, b: u32, tuning: &CarryCollisionTuning) -> bool {
    let (Some(a), Some(b)) = (unsafe { bounds(world, a) }, unsafe { bounds(world, b) }) else {
        return false;
    };
    disjoint(&a, &b, tuning.release_margin_m)
}

fn disjoint(a: &hkAabb, b: &hkAabb, margin_m: f32) -> bool {
    (0..3).any(|axis| {
        a.m_min.data[axis] > b.m_max.data[axis] + margin_m
            || b.m_min.data[axis] > a.m_max.data[axis] + margin_m
    })
}

unsafe fn bounds(world: &hknpWorld, body: u32) -> Option<hkAabb> {
    let slot = unsafe { live_body(world, body) }?;
    let shape = unsafe { slot.m_shape.as_ref() }?;
    let mut bounds = hkAabb::default();
    unsafe { shape.calcAabb(&slot.m_transform, &mut bounds) };
    (0..3)
        .all(|axis| {
            bounds.m_min.data[axis].is_finite()
                && bounds.m_max.data[axis].is_finite()
                && bounds.m_min.data[axis] <= bounds.m_max.data[axis]
        })
        .then_some(bounds)
}

const MAX_CONSTRAINTS: u32 = 1 << 16;
static STATE: Mutex<State> = Mutex::new(State {
    wanted: [None; 2],
    published: None,
    leases: Vec::new(),
    logged: (Vec::new(), Vec::new()),
});

/// Whether the physics game object `owner` is a vehicle or one of its parts.
///
/// # Safety
///
/// `owner` must be a live physics game object, on the thread stepping the carry collision.
unsafe fn vehicle_owned(owner: NonZeroUsize) -> bool {
    unsafe { bodies::is_type(owner, Vehicle::TYPE_ID()) || is_vehicle_part(owner) }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_pair_releases_once_clear_bounded_when_overlapping_and_immediately_on_shutdown() {
        let identity = Identity {
            body: 7,
            owner: None,
            instance: None,
            shape: 100,
        };
        let mut lease = Lease {
            context: Context {
                world: 1,
                filter: 2,
            },
            a: identity,
            b: Identity {
                body: 8,
                ..identity
            },
            released: None,
        };
        let tuning = &PHYSICAL_HANDS.get().carry_collision;
        let now = Instant::now();
        let past_grace = now + Duration::from_secs_f32(tuning.release_grace_s * 2.0);
        assert!(lease.retain(true, now, true, false, tuning));
        assert!(lease.retain(false, now, true, false, tuning));
        assert!(!lease.retain(false, past_grace, true, false, tuning));
        assert!(lease.retain(true, past_grace, true, false, tuning));
        assert!(!lease.retain(false, past_grace, false, false, tuning));
        assert!(!lease.retain(true, now, true, true, tuning));
    }

    #[test]
    fn an_assembly_includes_every_owned_body_and_native_group_member() {
        let owner = NonZeroUsize::new(100).unwrap();
        let other = NonZeroUsize::new(200).unwrap();
        let subject = Subject {
            body: 7,
            owner,
            assembly: Assembly::Character,
        };
        assert!(subject.includes(8, Some(owner), 3, 0));
        assert!(subject.includes(9, Some(other), 3, 3));
        assert!(!subject.includes(10, Some(other), 3, 4));
        assert!(!subject.includes(11, None, 0, 0));
    }

    #[test]
    fn a_panel_does_not_suppress_its_vehicle_assembly() {
        let owner = NonZeroUsize::new(100).unwrap();
        let subject = Subject {
            body: 7,
            owner,
            assembly: Assembly::Body,
        };
        assert!(subject.includes(7, Some(owner), 3, 3));
        assert!(!subject.includes(8, Some(owner), 3, 3));
        assert!(!subject.includes(7, NonZeroUsize::new(200), 3, 3));
    }

    #[test]
    fn release_bounds_require_clearance_on_one_axis() {
        let margin = PHYSICAL_HANDS.get().carry_collision.release_margin_m;
        let mut a = hkAabb::default();
        let mut b = hkAabb::default();
        a.m_min.data = [0.0; 3];
        a.m_max.data = [1.0; 3];
        b.m_min.data = [0.5; 3];
        b.m_max.data = [1.5; 3];
        assert!(!disjoint(&a, &b, margin));
        b.m_min.data[0] = 1.0 + margin * 0.5;
        assert!(!disjoint(&a, &b, margin));
        b.m_min.data[0] = 1.0 + margin * 2.0;
        assert!(disjoint(&a, &b, margin));
        assert!(disjoint(&b, &a, margin));
    }
}
