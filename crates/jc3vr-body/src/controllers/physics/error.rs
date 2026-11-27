//! The physics substrate's one error: why the world, a body, or the player's proxy could not be read
//! or driven. Every helper in [`crate::controllers::physics`] returns it, so a caller that reaches
//! the engine through several of them handles one set of causes.

use std::fmt;

/// Why the physics world, a body, or the player's proxy could not be read or driven.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PhysicsError {
    /// The physics system or its world does not exist.
    NoPhysicsWorld,
    /// The body id is invalid or has no slot, its slot was freed, or the world holds no bodies.
    NoBody,
    /// The body is not dynamic: static, fixed, or keyframed.
    NotDynamic,
    /// The body no longer belongs to the object it was taken from.
    Reowned,
    /// The body's motion, or a value to drive it with, is not finite.
    BadMotion,
    /// A fixed gameplay fixture is not activated or carried.
    Fixture,
    /// There is no local player character, or it has no proxy or proxy body.
    NoPlayer,
}

impl fmt::Display for PhysicsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self {
            PhysicsError::NoPhysicsWorld => "the physics world does not exist",
            PhysicsError::NoBody => "the body has been freed",
            PhysicsError::NotDynamic => "the body is not dynamic",
            PhysicsError::Reowned => "the body belongs to another object",
            PhysicsError::BadMotion => "the body's motion is not finite",
            PhysicsError::Fixture => "the object is a fixed gameplay fixture",
            PhysicsError::NoPlayer => "the player has no character proxy",
        };
        write!(f, "physics: {what}")
    }
}

impl std::error::Error for PhysicsError {}
