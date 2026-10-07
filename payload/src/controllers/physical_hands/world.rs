//! The physics system's Havok world, as the physical hands reach it.
//!
//! The world is the engine's, so a `'static` reference to it would let two callers hold a shared
//! and a mutable reference at once. [`World`] holds the pointer instead, and lends the world out
//! only for as long as the handle is borrowed, so a reference to it never outlives the call that
//! took it.

use std::ptr::NonNull;

use glam::Vec3;
use jc3gi::physics::{PhysicsSystem, hknpWorld};

/// The physics world, and the shift from its positions to game space.
pub(crate) struct World {
    world: NonNull<hknpWorld>,
    /// The world's moving origin in game space ([`PhysicsSystem::m_AccumulatedShift`]): a Havok
    /// position plus the shift is a game position.
    pub shift: Vec3,
}

impl World {
    /// The physics system's world, or `None` while there is none.
    ///
    /// # Safety
    ///
    /// The thread that steps the physics world, outside the step, or another thread the caller
    /// knows the world is not changed on meanwhile.
    pub(crate) unsafe fn get() -> Option<Self> {
        let physics = unsafe { PhysicsSystem::get() }?;
        let shift = Vec3::from_array(physics.m_AccumulatedShift.data);
        let world = NonNull::new(physics.m_World)?;
        Some(Self { world, shift })
    }

    /// The world, for as long as the handle is borrowed.
    pub(crate) fn world(&self) -> &hknpWorld {
        // SAFETY: the world outlives the call that made the handle (see [`World::get`]), and the
        // borrow is tied to the handle's.
        unsafe { self.world.as_ref() }
    }

    /// The world, mutably, for as long as the handle is borrowed mutably.
    pub(crate) fn world_mut(&mut self) -> &mut hknpWorld {
        // SAFETY: as [`World::world`]; the handle's mutable borrow is the only one it lends.
        unsafe { self.world.as_mut() }
    }
}
