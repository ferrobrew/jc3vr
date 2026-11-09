use std::{
    num::NonZeroUsize,
    time::{Duration, Instant},
};

use jc3gi::{
    character::character::Character,
    havok::hkAabb,
    physics::{CustomGroupCollisionFilter as Filter, hknpBody, hknpWorld},
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

pub(super) fn publish(held: [Option<(u32, NonZeroUsize)>; 2]) {
    let mut state = STATE.lock();
    state.wanted = held;
    state.published = Some(Instant::now());
}

pub(super) unsafe fn step() {
    let enabled = Config::lock_query(|c| {
        c.controllers.physical_hands.enabled && c.controllers.physical_hands.pickup.enabled
    }) && scheme::is_controllers()
        && hooks::in_gameplay()
        && !crate::is_shutting_down();
    let now = Instant::now();
    let (wanted, mut leases, mut inventory) = {
        let mut state = STATE.lock();
        let fresh = state
            .published
            .is_some_and(|at| now.saturating_duration_since(at) <= STALE_AFTER);
        (
            if enabled && fresh {
                state.wanted
            } else {
                [None; 2]
            },
            std::mem::take(&mut state.leases),
            state.inventory.take(),
        )
    };
    if leases.is_empty() && wanted.iter().all(Option::is_none) {
        return;
    }
    let Some(mut world) = (unsafe { World::get() }) else {
        let mut state = STATE.lock();
        state.leases = leases;
        state.inventory = inventory;
        return;
    };
    let player_group = unsafe { player_group(world.world()) };
    let world_id = world.world() as *const hknpWorld as usize;
    if let Some(group) = player_group
        && inventory.as_ref().is_none_or(|cached| {
            cached.world != world_id
                || cached.group != group
                || cached.peak != world.world().m_peakBodyIndex
                || now.saturating_duration_since(cached.at) >= INVENTORY_INTERVAL
        })
    {
        inventory = Some(Inventory {
            world: world_id,
            group,
            peak: world.world().m_peakBodyIndex,
            at: now,
            bodies: (0..=world.world().m_peakBodyIndex)
                .filter(|body| {
                    unsafe { live_body(world.world(), *body) }.is_some_and(|slot| {
                        slot.m_collisionFilterInfo >> Filter::SYSTEM_GROUP_SHIFT == group
                    })
                })
                .collect(),
        });
    }
    let player_bodies: Vec<(u32, u32)> = inventory
        .as_ref()
        .filter(|cached| Some(cached.group) == player_group && cached.world == world_id)
        .into_iter()
        .flat_map(|cached| cached.bodies.iter().copied())
        .filter_map(|body| {
            unsafe { live_body(world.world(), body) }.map(|slot| (body, slot.m_collisionFilterInfo))
        })
        .filter(|(_, info)| Some(*info >> Filter::SYSTEM_GROUP_SHIFT) == player_group)
        .collect();
    let mut retained = Vec::with_capacity(leases.len() + 2);
    for mut lease in leases.drain(..) {
        let Some(current) = (unsafe { lease.current(world.world()) }) else {
            continue;
        };
        if current & OWNED_MASK != lease.applied & OWNED_MASK {
            continue;
        }
        let held = wanted.contains(&Some((lease.body, lease.owner)));
        if held
            && player_group == Some(lease.group)
            && let Some(applied) = exclusion(&player_bodies, current, lease.group)
        {
            if applied != current {
                unsafe { set_filter(world.world_mut(), lease.body, applied) };
                lease.applied = applied;
            }
            retained.push(lease);
            continue;
        }
        if unsafe { clear_of_player(world.world(), lease.body, lease.original, &player_bodies) } {
            let restored = (current & !OWNED_MASK) | (lease.original & OWNED_MASK);
            unsafe { set_filter(world.world_mut(), lease.body, restored) };
        } else {
            retained.push(lease);
        }
    }
    if let Some(group) = player_group {
        for (body, owner) in wanted.into_iter().flatten() {
            if retained
                .iter()
                .any(|lease| lease.body == body && lease.owner == owner)
                || retained.len() >= MAX_LEASES
            {
                continue;
            }
            let Some(mut lease) = (unsafe { Lease::observe(world.world(), body, owner, group) })
            else {
                continue;
            };
            let Some(applied) = exclusion(&player_bodies, lease.original, group) else {
                continue;
            };
            lease.applied = applied;
            unsafe { set_filter(world.world_mut(), body, applied) };
            retained.push(lease);
        }
    }
    let mut state = STATE.lock();
    state.leases = retained;
    state.inventory = inventory;
}

pub(super) unsafe fn clear() {
    publish([None; 2]);
    if crate::is_shutting_down() {
        let leases = std::mem::take(&mut STATE.lock().leases);
        if let Some(mut world) = unsafe { World::get() } {
            for lease in leases {
                if let Some(current) = unsafe { lease.current(world.world()) }
                    && current & OWNED_MASK == lease.applied & OWNED_MASK
                {
                    let restored = (current & !OWNED_MASK) | (lease.original & OWNED_MASK);
                    unsafe { set_filter(world.world_mut(), lease.body, restored) };
                }
            }
        }
        STATE.lock().inventory = None;
    } else {
        unsafe { step() };
    }
}

struct State {
    wanted: [Option<(u32, NonZeroUsize)>; 2],
    published: Option<Instant>,
    leases: Vec<Lease>,
    inventory: Option<Inventory>,
}

struct Inventory {
    world: usize,
    group: u32,
    peak: u32,
    at: Instant,
    bodies: Vec<u32>,
}

struct Lease {
    world: usize,
    body: u32,
    owner: NonZeroUsize,
    instance: usize,
    original: u32,
    applied: u32,
    group: u32,
}

impl Lease {
    unsafe fn observe(
        world: &hknpWorld,
        body: u32,
        owner: NonZeroUsize,
        group: u32,
    ) -> Option<Self> {
        let slot = unsafe { live_body(world, body) }?;
        if unsafe { bodies::body_owner(world, body) } != Some(owner)
            || slot.m_flags & hknpBody::IS_DYNAMIC == 0
            || slot.m_shape.is_null()
        {
            return None;
        }
        Some(Self {
            world: world as *const hknpWorld as usize,
            body,
            owner,
            instance: unsafe { bodies::instance_of(world, body) }?,
            original: {
                if slot.m_collisionFilterInfo >> Filter::SYSTEM_GROUP_SHIFT != 0 {
                    return None;
                }
                slot.m_collisionFilterInfo
            },
            applied: slot.m_collisionFilterInfo,
            group,
        })
    }

    unsafe fn current(&self, world: &hknpWorld) -> Option<u32> {
        if self.world != world as *const hknpWorld as usize
            || unsafe { bodies::body_owner(world, self.body) } != Some(self.owner)
            || unsafe { bodies::instance_of(world, self.body) } != Some(self.instance)
        {
            return None;
        }
        let slot = unsafe { live_body(world, self.body) }?;
        Some(slot.m_collisionFilterInfo)
    }
}

unsafe fn live_body(world: &hknpWorld, body: u32) -> Option<&hknpBody> {
    let slot = unsafe { bodies::slot(world, body) }?;
    (slot.m_flags & (hknpBody::IS_STATIC | hknpBody::IS_DYNAMIC) != 0).then_some(slot)
}

unsafe fn player_group(world: &hknpWorld) -> Option<u32> {
    let player = unsafe { Character::GetLocalPlayerCharacter().as_ref() }?;
    let body = unsafe { proxy_body(player) }?;
    let slot = unsafe { live_body(world, body) }?;
    let group = slot.m_collisionFilterInfo >> Filter::SYSTEM_GROUP_SHIFT;
    (group != 0).then_some(group)
}

fn exclusion(others: &[(u32, u32)], original: u32, group: u32) -> Option<u32> {
    if others.is_empty() {
        return None;
    }
    for subsystem in 0..=Filter::FIELD_MASK {
        for no_collide in 0..=Filter::FIELD_MASK {
            let candidate = (original & !OWNED_MASK)
                | (group << Filter::SYSTEM_GROUP_SHIFT)
                | (subsystem << Filter::SUBSYSTEM_SHIFT)
                | (no_collide << Filter::NO_COLLIDE_SHIFT);
            if others
                .iter()
                .all(|(_, other)| !same_group_collision(candidate, *other))
            {
                return Some(candidate);
            }
        }
    }
    None
}

fn same_group_collision(a: u32, b: u32) -> bool {
    let subsystem = |info: u32| (info >> Filter::SUBSYSTEM_SHIFT) & Filter::FIELD_MASK;
    let no_collide = |info: u32| (info >> Filter::NO_COLLIDE_SHIFT) & Filter::FIELD_MASK;
    let a_sub = subsystem(a);
    let b_sub = subsystem(b);
    (a_sub != 0 || b_sub != 0)
        && a_sub != no_collide(b)
        && b_sub != no_collide(a)
        && (a & Filter::LAYER_MASK) == (b & Filter::LAYER_MASK)
}

unsafe fn clear_of_player(
    world: &hknpWorld,
    body: u32,
    original: u32,
    player_bodies: &[(u32, u32)],
) -> bool {
    let Some(group) = (unsafe { player_group(world) }) else {
        return false;
    };
    let Some(body_bounds) = (unsafe { bounds(world, body) }) else {
        return false;
    };
    for &(other, filter) in player_bodies {
        if other == body {
            continue;
        }

        if filter >> Filter::SYSTEM_GROUP_SHIFT != group {
            continue;
        }
        if original >> Filter::SYSTEM_GROUP_SHIFT == group
            && !same_group_collision(original, filter)
        {
            continue;
        }
        let Some(other_bounds) = (unsafe { bounds(world, other) }) else {
            return false;
        };
        if (0..3).all(|axis| {
            body_bounds.m_min.data[axis] <= other_bounds.m_max.data[axis] + RELEASE_MARGIN_M
                && other_bounds.m_min.data[axis] <= body_bounds.m_max.data[axis] + RELEASE_MARGIN_M
        }) {
            return false;
        }
    }
    true
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

unsafe fn set_filter(world: &mut hknpWorld, body: u32, filter: u32) {
    unsafe {
        world.setBodyCollisionFilterInfo(body, filter, hknpWorld::REBUILD_COLLISION_CACHES);
    }
}

const OWNED_MASK: u32 = (!0u32 << Filter::SYSTEM_GROUP_SHIFT)
    | (Filter::FIELD_MASK << Filter::SUBSYSTEM_SHIFT)
    | (Filter::FIELD_MASK << Filter::NO_COLLIDE_SHIFT);
const RELEASE_MARGIN_M: f32 = 0.01;
const INVENTORY_INTERVAL: Duration = Duration::from_millis(250);
const MAX_LEASES: usize = 128;
static STATE: Mutex<State> = Mutex::new(State {
    wanted: [None; 2],
    published: None,
    leases: Vec::new(),
    inventory: None,
});
