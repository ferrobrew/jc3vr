//! The physical hands' engine side: what a ray hit is, the player's melee impulse, and a push on a
//! dynamic body.
//!
//! An object is pushed through `hknpWorld::applyBodyImpulseAt`, which the game's melee hit uses on
//! the objects it strikes and which leaves static and keyframed bodies alone. A fast hit's knock is
//! scaled from the player's own melee impulse, so the hammer upgrades carry over.

use std::{fmt, num::NonZeroUsize};

use glam::Vec3;
use jc3gi::{
    character::{character::Character, player::Player},
    game::GameObject,
    physics::{PfxCharacterInstance, PhysicsGameObject, PhysicsSystem},
    types::vector_math::AlignedVector3,
};

use crate::controllers::{
    aim::raycast::RayHit,
    physical_hands::contact::{Surface, SurfaceHit},
};

/// Why an engine call did not run.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// The physics system or its world does not exist.
    NoPhysicsWorld,
    /// The hit has no valid body.
    InvalidBody,
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::NoPhysicsWorld => {
                write!(f, "physical hands: the physics world does not exist")
            }
            EngineError::InvalidBody => write!(f, "physical hands: the hit body is invalid"),
        }
    }
}

impl std::error::Error for EngineError {}

/// A ray hit as a palm's surface: a character, or anything else.
///
/// # Safety
///
/// The hit must come from a cast this tick, so that its object is live.
pub(super) unsafe fn surface(hit: &RayHit) -> SurfaceHit {
    let surface = match hit.object {
        Some(object) if unsafe { is_character(object) } => Surface::Character,
        object => Surface::Solid {
            object,
            body: hit.body,
        },
    };
    SurfaceHit {
        position: hit.position,
        normal: hit.normal,
        surface,
    }
}

/// The player's melee impulse, from the player object behind `character`.
///
/// # Safety
///
/// `character` must be the live local player character.
pub(super) unsafe fn melee_impulse(character: &Character) -> Option<f32> {
    if !character.m_IsPlayer || character.m_Avatar.is_null() {
        return None;
    }
    let player = character
        .m_Avatar
        .cast::<u8>()
        .wrapping_sub(Player::AVATAR_BASE_OFFSET as usize)
        .cast::<Player>();
    let player = unsafe { player.as_ref() }?;
    Some(player.m_MeleeImpulse)
}

/// Push the body `body` by `impulse` at the game-space point `at`. A static or keyframed body is
/// left alone by the engine.
///
/// # Safety
///
/// Main thread, outside the physics step; `body` must come from a cast this tick.
pub(super) unsafe fn push_body(body: u32, impulse: Vec3, at: Vec3) -> Result<(), EngineError> {
    if body == PfxCharacterInstance::INVALID_BODY_ID || !impulse.is_finite() || !at.is_finite() {
        return Err(EngineError::InvalidBody);
    }
    let physics = unsafe { PhysicsSystem::get() }.ok_or(EngineError::NoPhysicsWorld)?;
    let shift = Vec3::from_array(physics.m_AccumulatedShift.data);
    let world = unsafe { physics.m_World.as_mut() }.ok_or(EngineError::NoPhysicsWorld)?;
    let impulse = aligned(impulse);
    let position = aligned(at - shift);
    unsafe { world.applyBodyImpulseAt(body, &impulse, &position) };
    Ok(())
}

/// Whether the physics game object at `object` is a character.
unsafe fn is_character(object: NonZeroUsize) -> bool {
    let game_object =
        (object.get() + PhysicsGameObject::GAME_OBJECT_BASE_OFFSET as usize) as *const GameObject;
    unsafe { (*game_object).IsType(Character::TYPE_ID()) }
}

fn aligned(v: Vec3) -> AlignedVector3 {
    AlignedVector3 {
        data: v.to_array(),
        _w: 0.0,
    }
}
