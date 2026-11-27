//! The physics system's Havok world, lent out for one call at a time, and the writes every driver
//! of a body makes through it: a body's velocities and an impulse at a game-space point.
//!
//! The world is the engine's, so a `'static` reference to it would let two callers hold a shared
//! and a mutable reference at once. [`World`] holds the pointer instead, and lends the world out
//! only for as long as the handle is borrowed, so a reference to it never outlives the call that
//! took it.

use std::ptr::NonNull;

use glam::Vec3;
use jc3gi::physics::{PhysicsSystem, hknpBody, hknpMotion, hknpWorld};

use crate::controllers::physics::{bodies, error::PhysicsError, havok::vector as havok_vector};

/// The physics world, and the shift from its positions to game space.
pub struct World {
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
    pub unsafe fn get() -> Option<Self> {
        let physics = unsafe { PhysicsSystem::get() }?;
        let shift = Vec3::from_array(physics.m_AccumulatedShift.data);
        let world = NonNull::new(physics.m_World)?;
        Some(Self { world, shift })
    }

    /// The physics system's world, or [`PhysicsError::NoPhysicsWorld`] while there is none.
    ///
    /// # Safety
    ///
    /// As [`World::get`].
    pub unsafe fn require() -> Result<Self, PhysicsError> {
        unsafe { Self::get() }.ok_or(PhysicsError::NoPhysicsWorld)
    }

    /// The world, for as long as the handle is borrowed.
    pub fn world(&self) -> &hknpWorld {
        // SAFETY: the world outlives the call that made the handle (see [`World::get`]), and the
        // borrow is tied to the handle's.
        unsafe { self.world.as_ref() }
    }

    /// The world, mutably, for as long as the handle is borrowed mutably.
    pub fn world_mut(&mut self) -> &mut hknpWorld {
        // SAFETY: as [`World::world`]; the handle's mutable borrow is the only one it lends.
        unsafe { self.world.as_mut() }
    }

    /// The world's slot for the body `body`, while it holds a body ([`bodies::slot`]).
    pub fn slot(&self, body: u32) -> Option<&hknpBody> {
        // SAFETY: the world is live for the handle's borrow, read on the thread the handle was
        // taken on (see [`World::get`]).
        unsafe { bodies::slot(self.world(), body) }
    }

    /// The motion of the body in `slot`, a slot of this world, or `None` while the world has no
    /// motions.
    pub fn motion(&self, slot: &hknpBody) -> Option<&hknpMotion> {
        let motions = self.world().m_motions;
        // SAFETY: a slot that holds a body names one of the world's motions.
        (!motions.is_null()).then(|| unsafe { &*motions.add(slot.m_motionId as usize) })
    }

    /// Set the body `body`'s velocities, world space ([`hknpWorld::setBodyVelocity`]). The caller
    /// has checked that the id names the body it means and that the velocities are finite.
    ///
    /// # Safety
    ///
    /// As [`World::get`], outside the physics step; `body` must hold a body.
    pub unsafe fn set_velocities(&mut self, body: u32, linear: Vec3, angular: Vec3) {
        let linear = havok_vector(linear);
        let angular = havok_vector(angular);
        unsafe { self.world_mut().setBodyVelocity(body, &linear, &angular) };
    }

    /// Apply the linear impulse `impulse` to the body `body` at the game-space point `at`
    /// ([`hknpWorld::applyBodyImpulseAt`], which leaves static and keyframed bodies alone). The
    /// caller has checked that the id names the body it means and that both are finite.
    ///
    /// # Safety
    ///
    /// As [`World::get`], outside the physics step; `body` must be a valid id.
    pub unsafe fn apply_impulse(&mut self, body: u32, impulse: Vec3, at: Vec3) {
        let impulse = havok_vector(impulse);
        let position = havok_vector(at - self.shift);
        unsafe {
            self.world_mut()
                .applyBodyImpulseAt(body, &impulse, &position)
        };
    }
}
