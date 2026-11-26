//! The reads the tracking recorder's near set makes of the physics world: a slice of the body
//! slots walked for dynamic bodies whose boxes a test accepts, one dynamic body's pose and
//! velocities, and a body's identity and shape for its introduction. The set itself, and what it
//! records, is `crate::controllers::diagnostics`'s.
//!
//! A walk of every slot up to the world's peak streams the whole body array, so the recorder walks
//! a slice per step and reads only the bodies it keeps on the other steps.

use std::num::NonZeroUsize;

use glam::{Quat, Vec3};
use jc3gi::physics::hknpBody;

use crate::controllers::physical_hands::{
    bodies,
    collider_view::{
        mod_ids,
        model::{Aabb, KindInputs, ModBody, MotionKind, classify},
        scan::{ScanError, Space, geometry, layer, mass, motion_kind, player_proxy},
    },
    physics::havok_affine,
    world::World,
};

/// Where a slice of the walk ended.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct Slice {
    /// The slots the slice went through.
    pub walked: u32,
    /// The world's peak slot.
    pub peak: u32,
    /// The slot the next slice starts at.
    pub next: u32,
}

/// A dynamic body's pose and velocities, game space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct BodyState {
    pub position: Vec3,
    pub orientation: Quat,
    pub linear: Vec3,
    pub angular: Vec3,
    pub aabb: Aabb,
}

/// What a body is, for its introduction.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct BodyDescription {
    pub object: Option<NonZeroUsize>,
    pub kind: &'static str,
    pub mass_kg: Option<f32>,
    pub shape: &'static str,
    pub half_extents: Vec3,
    pub filter: u32,
}

/// Walk up to `count` slots from `start`, wrapping at the world's peak, and hand `keep` each
/// dynamic body in the simulation with its box, game space.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub(crate) unsafe fn walk_dynamic(
    start: u32,
    count: u32,
    mut keep: impl FnMut(u32, &Aabb),
) -> Result<Slice, ScanError> {
    let world = unsafe { World::get() }.ok_or(ScanError::NoPhysicsWorld)?;
    let w = world.world();
    if w.m_bodies.is_null() {
        return Err(ScanError::NoBodies);
    }
    let space = Space::of(w);
    let slots = w.m_peakBodyIndex.saturating_add(1);
    let count = count.min(slots);
    let mut id = if start < slots { start } else { 0 };
    for _ in 0..count {
        // SAFETY: every id up to the peak has a slot.
        let slot = unsafe { &*w.m_bodies.add(id as usize) };
        if motion_kind(slot) == Some(MotionKind::Dynamic) {
            keep(id, &space.restore(slot, world.shift));
        }
        id += 1;
        if id >= slots {
            id = 0;
        }
    }
    Ok(Slice {
        walked: count,
        peak: w.m_peakBodyIndex,
        next: id,
    })
}

/// The dynamic body `id`'s pose, velocities, and box, or `None` when its slot no longer holds a
/// dynamic body in the simulation.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub(crate) unsafe fn dynamic_state(id: u32) -> Option<BodyState> {
    let world = unsafe { World::get() }?;
    let w = world.world();
    let slot = unsafe { dynamic_slot(w, id) }?;
    // SAFETY: the motion array holds the motion of every body in the simulation.
    let motion = unsafe { &*w.m_motions.add(slot.m_motionId as usize) };
    let mut transform = havok_affine(&slot.m_transform);
    transform.translation += glam::Vec3A::from(world.shift);
    let frame = Quat::from_array(motion.m_orientation.data);
    let local_angular = Vec3::from_array(motion.m_angularVelocity.data);
    let state = BodyState {
        position: transform.translation.into(),
        orientation: Quat::from_mat3a(&transform.matrix3),
        linear: Vec3::from_array(motion.m_linearVelocity.data),
        angular: if frame.is_finite() {
            frame.normalize() * local_angular
        } else {
            local_angular
        },
        aabb: Space::of(w).restore(slot, world.shift),
    };
    (state.position.is_finite()
        && state.orientation.is_finite()
        && state.linear.is_finite()
        && state.angular.is_finite())
    .then_some(state)
}

/// What the body `id` is: its owner, kind, mass, shape, and box.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub(crate) unsafe fn describe(id: u32) -> Option<BodyDescription> {
    let world = unsafe { World::get() }?;
    let w = world.world();
    let slot = unsafe { dynamic_slot(w, id) }?;
    let mods = mod_ids();
    let mod_body = if mods.hands.contains(&id) {
        Some(ModBody::Hand)
    } else if mods.guns.contains(&id) {
        Some(ModBody::Gun)
    } else {
        None
    };
    let player = unsafe { player_proxy() }.is_some_and(|p| p.body == id);
    let kind = classify(KindInputs {
        motion: MotionKind::Dynamic,
        layer: layer(slot.m_collisionFilterInfo),
        mod_body,
        player,
    });
    let aabb = Space::of(w).restore(slot, world.shift);
    Some(BodyDescription {
        object: unsafe { bodies::owner_of(id) },
        kind: kind.label(),
        // SAFETY: the slot holds a dynamic body with a motion.
        mass_kg: unsafe { mass(w, slot, MotionKind::Dynamic) },
        // SAFETY: the body holds a reference to its shape.
        shape: unsafe { geometry(slot.m_shape) }.label(),
        half_extents: (aabb.max - aabb.min) * 0.5,
        filter: slot.m_collisionFilterInfo,
    })
}

unsafe fn dynamic_slot(w: &jc3gi::physics::hknpWorld, id: u32) -> Option<&hknpBody> {
    if w.m_bodies.is_null() || w.m_motions.is_null() || id > w.m_peakBodyIndex {
        return None;
    }
    // SAFETY: every id up to the peak has a slot.
    let slot = unsafe { &*w.m_bodies.add(id as usize) };
    (motion_kind(slot) == Some(MotionKind::Dynamic)).then_some(slot)
}
