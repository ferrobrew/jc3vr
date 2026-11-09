use std::{num::NonZeroUsize, time::Instant};

use jc3gi::{
    character::character::Character,
    havok::hkAabb,
    physics::{CustomGroupCollisionFilter as Filter, PhysicsSystem, hknpBody, hknpWorld},
};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        physical_hands::{STALE_AFTER, bodies, characters::engine::proxy_body, world::World},
        scheme,
    },
    hooks,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Subject {
    pub body: u32,
    pub owner: NonZeroUsize,
    pub assembly: bool,
}

impl Subject {
    fn includes(self, body: u32, owner: Option<NonZeroUsize>, seed_group: u32, group: u32) -> bool {
        owner == Some(self.owner) && (self.assembly || self.body == body)
            || self.assembly && seed_group != 0 && seed_group == group
    }
}

pub(super) fn publish(held: [Option<Subject>; 2]) {
    let mut state = STATE.lock();
    state.wanted = held;
    state.published = Some(Instant::now());
}

pub(super) unsafe fn step() {
    let enabled = Config::lock_query(|c| {
        c.controllers.enabled
            && c.controllers.physical_hands.enabled
            && c.controllers.physical_hands.pickup.enabled
    }) && scheme::is_controllers()
        && hooks::in_gameplay()
        && !crate::is_shutting_down();
    let now = Instant::now();
    let mut state = STATE.lock();
    let fresh = state
        .published
        .is_some_and(|at| now.saturating_duration_since(at) <= STALE_AFTER);
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
        let player_body = player_owner.is_some() && owner == player_owner
            || player_group.is_some_and(|group| {
                slot.m_collisionFilterInfo >> Filter::SYSTEM_GROUP_SHIFT == group
            });
        let held = subjects.iter().any(|(subject, group)| {
            subject.includes(
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
        let overlapping =
            current && !held && !unsafe { separated(world.world(), lease.a.body, lease.b.body) };
        if lease.retain(held, now, overlapping, crate::is_shutting_down()) {
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
}

pub(super) unsafe fn clear() {
    let mut state = STATE.lock();
    state.wanted = [None; 2];
    state.published = None;
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
    fn retain(&mut self, held: bool, now: Instant, overlapping: bool, force: bool) -> bool {
        if force {
            return false;
        }
        if held {
            self.released = None;
            return true;
        }
        let released = *self.released.get_or_insert(now);
        overlapping && now.saturating_duration_since(released).as_secs_f32() < RELEASE_GRACE_S
    }
}

struct State {
    wanted: [Option<Subject>; 2],
    published: Option<Instant>,
    leases: Vec<Lease>,
}

unsafe fn live_body(world: &hknpWorld, body: u32) -> Option<&hknpBody> {
    let slot = unsafe { bodies::slot(world, body) }?;
    (slot.m_flags & (hknpBody::IS_STATIC | hknpBody::IS_DYNAMIC) != 0 && !slot.m_shape.is_null())
        .then_some(slot)
}

unsafe fn separated(world: &hknpWorld, a: u32, b: u32) -> bool {
    let (Some(a), Some(b)) = (unsafe { bounds(world, a) }, unsafe { bounds(world, b) }) else {
        return false;
    };
    disjoint(&a, &b)
}

fn disjoint(a: &hkAabb, b: &hkAabb) -> bool {
    (0..3).any(|axis| {
        a.m_min.data[axis] > b.m_max.data[axis] + RELEASE_MARGIN_M
            || b.m_min.data[axis] > a.m_max.data[axis] + RELEASE_MARGIN_M
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

const RELEASE_MARGIN_M: f32 = 0.01;
const RELEASE_GRACE_S: f32 = 1.0;
static STATE: Mutex<State> = Mutex::new(State {
    wanted: [None; 2],
    published: None,
    leases: Vec::new(),
});

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
        let now = Instant::now();
        assert!(lease.retain(true, now, true, false));
        assert!(lease.retain(false, now, true, false));
        assert!(!lease.retain(false, now + Duration::from_secs(2), true, false));
        assert!(lease.retain(true, now + Duration::from_secs(2), true, false));
        assert!(!lease.retain(false, now + Duration::from_secs(2), false, false));
        assert!(!lease.retain(true, now, true, true));
    }

    #[test]
    fn an_assembly_includes_every_owned_body_and_native_group_member() {
        let owner = NonZeroUsize::new(100).unwrap();
        let other = NonZeroUsize::new(200).unwrap();
        let subject = Subject {
            body: 7,
            owner,
            assembly: true,
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
            assembly: false,
        };
        assert!(subject.includes(7, Some(owner), 3, 3));
        assert!(!subject.includes(8, Some(owner), 3, 3));
        assert!(!subject.includes(7, NonZeroUsize::new(200), 3, 3));
    }

    #[test]
    fn release_bounds_require_clearance_on_one_axis() {
        let mut a = hkAabb::default();
        let mut b = hkAabb::default();
        a.m_min.data = [0.0; 3];
        a.m_max.data = [1.0; 3];
        b.m_min.data = [0.5; 3];
        b.m_max.data = [1.5; 3];
        assert!(!disjoint(&a, &b));
        b.m_min.data[0] = 1.005;
        assert!(!disjoint(&a, &b));
        b.m_min.data[0] = 1.02;
        assert!(disjoint(&a, &b));
        assert!(disjoint(&b, &a));
    }
}
