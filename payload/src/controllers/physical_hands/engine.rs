//! The physical hands' engine side: what a ray hit is, the player's melee tuning, a strike's damage
//! message, and a push on a dynamic body.
//!
//! A strike is the engine's own melee hit made by hand: a melee damage message
//! ([`DamageFlags::DMG_TYPE_MELEE`]) built the way the player's melee hit message builds it
//! (`Character::HandleTrackMessage`), handed to the struck character's
//! [`PhysicsGameObject::OnDamage`], which takes the health and runs the character's own hit reaction
//! from the message's impulse, position, and direction. A strong strike also queues one of the
//! game's melee stumbles, as the game's melee input task does for its target. An object is pushed
//! through `hknpWorld::applyBodyImpulseAt`, which the melee hit uses on the objects it strikes and
//! which leaves static and keyframed bodies alone.

use std::{fmt, mem::MaybeUninit, num::NonZeroUsize};

use glam::Vec3;
use jc3gi::{
    character::{character::Character, player::Player},
    damage::{DamageFlags, DamageMsg},
    game::GameObject,
    physics::{PfxCharacterInstance, PhysicsGameObject, PhysicsSystem},
    types::{math::Vector3, vector_math::AlignedVector3},
};

use crate::{
    controllers::{
        aim::raycast::RayHit,
        physical_hands::{
            contact::{Surface, SurfaceHit},
            strike::{StrikePlan, Stumble},
        },
    },
    hooks::character::resolve_act_id,
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
        Some(object) if unsafe { is_character(object) } => Surface::Character(object),
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

/// The player's melee damage and impulse, from the player object behind `character`.
///
/// # Safety
///
/// `character` must be the live local player character.
pub(super) unsafe fn melee_tuning(character: &Character) -> Option<(i32, f32)> {
    if !character.m_IsPlayer || character.m_Avatar.is_null() {
        return None;
    }
    let player = character
        .m_Avatar
        .cast::<u8>()
        .wrapping_sub(Player::AVATAR_BASE_OFFSET as usize)
        .cast::<Player>();
    let player = unsafe { player.as_ref() }?;
    Some((player.m_MeleeDamage, player.m_MeleeImpulse))
}

/// Deal `plan` to the character `victim`, struck at `at` along `direction` by `attacker`, and
/// return the damage it took.
///
/// # Safety
///
/// Main thread, outside the physics step; `attacker` must be the live local player character and
/// `victim` a live character found by a cast this tick.
pub(super) unsafe fn strike_character(
    attacker: &Character,
    victim: NonZeroUsize,
    at: Vec3,
    direction: Vec3,
    plan: &StrikePlan,
) -> i16 {
    let victim = victim.get() as *mut Character;
    if plan.stumble {
        // SAFETY: forwarded from the caller.
        let forward = -glam::Mat4::from(unsafe { (*victim).m_WorldMatrixT1 })
            .z_axis
            .truncate();
        let stumble = Stumble::of(forward, direction);
        if let Some(id) = resolve_act_id(stumble.act()) {
            let id = id as u32;
            unsafe {
                if (*victim).m_AnimatedModel.TryAct(&id) {
                    (*victim).QueueAct(&id);
                }
            }
        }
    }

    // Every field is a pointer, an integer, or a float, so all-zero is a valid value to construct
    // over: the smart pointers start null, as the engine clears them before `Clear`.
    let mut msg = unsafe { MaybeUninit::<DamageMsg>::zeroed().assume_init() };
    msg.__vftable = DamageMsg::VFTABLE;
    unsafe { msg.Clear() };
    msg.m_Damage = plan.damage;
    msg.m_OriginalDamage = plan.damage;
    msg.m_DamageFlags = DamageFlags::DMG_TYPE_MELEE;
    unsafe { msg.m_Damager.Assign(&attacker.m_WeakThis) };
    msg.m_Position = vector(at);
    msg.m_Direction = vector(direction.normalize_or_zero());
    msg.m_DamagerPosition = vector(glam::Mat4::from(attacker.m_WorldMatrixT1).w_axis.truncate());
    msg.m_Impulse = plan.impulse;
    msg.m_Faction = unsafe { attacker.GetFaction() };
    msg.m_DamagerSourceStateInfo = unsafe { attacker.GetCurrentDamageSourceStateInfo() };
    // SAFETY: a character is a physics game object, with the damage virtual in its primary table.
    let dealt = unsafe { (*victim.cast::<PhysicsGameObject>()).OnDamage(&mut msg) };
    unsafe { msg.Destruct() };
    dealt
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

fn vector(v: Vec3) -> Vector3 {
    Vector3 { data: v.to_array() }
}

fn aligned(v: Vec3) -> AlignedVector3 {
    AlignedVector3 {
        data: v.to_array(),
        _w: 0.0,
    }
}
