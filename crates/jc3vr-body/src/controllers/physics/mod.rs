//! The physics substrate the hands, the weapons, the grapple, and the vehicles share: the rigid
//! bodies, their poses and velocities, the physics world, the player's proxy, the vehicles behind
//! bodies, the scans of the world's bodies, the rig that carries the hands, the character's wanted
//! velocity, and the references that keep a held body alive. Every helper fails with the one
//! [`error::PhysicsError`].
//!
//! - [`error`]: why the world, a body, or the proxy could not be read or driven;
//! - [`bodies`]: a body's place, motion, mass, and owner, read, driven, and shoved through the Havok
//!   world;
//! - [`rigid`]: a rigid body's response to impulses;
//! - [`world`]: the Havok world, lent out for one call at a time, and the velocity and impulse
//!   writes every driver makes through it;
//! - [`havok`]: conversions to and from Havok's vectors and transforms;
//! - [`proxy`]: the local player's character proxy, its velocity, and its support;
//! - [`vehicles`]: the vehicle behind an object or a body, its body, and its frame;
//! - [`scan`]: bodies' motion kinds and boxes, and the walks that find dynamic bodies by their
//!   boxes;
//! - [`rig`]: the rig that carries the hands, and a reader's watch for its jumps;
//! - [`release`]: the one motion everything a hand lets go of leaves with;
//! - [`wanted`]: the character's wanted velocity and the one order its writers apply in;
//! - [`refs`]: weak and strong references to game objects held across ticks;
//! - [`characters`]: a character's place and object, its proxy body, its protection, and the
//!   ragdolls that take it out of a seat;
//! - [`local_player`]: the local player character for one game tick;
//! - [`grapple_hook`]: the grappling hook and its wire's two ends;
//! - [`steps`]: the physics steps counted so far, which publications for the next step are
//!   stamped and aged by;
//! - [`tuning`]: the substrate's tuning file.
//!
//! The body operations below read and drive one body each for the hands, the weapons, and the
//! grapple, and [`GRAVITY_MPS2`] is the gravity they all work against.

pub mod bodies;
pub mod characters;
pub mod error;
pub mod grapple_hook;
pub mod havok;
pub mod local_player;
pub mod proxy;
pub mod refs;
pub mod release;
pub mod rig;
pub mod rigid;
pub mod scan;
pub mod steps;
pub mod tuning;
pub mod vehicles;
pub mod wanted;
pub mod world;

use std::num::NonZeroUsize;

pub use body_rig::math::constants::GRAVITY_MPS2;
use glam::{Quat, Vec3};

use crate::controllers::physics::{bodies::BodyState, error::PhysicsError};

/// Give the dynamic body `body` the velocities `linear` and `angular`, world space, after checking
/// it still belongs to `owner`: a dropped weapon leaving the hand with the hand's motion.
///
/// # Safety
///
/// The main thread, outside the physics step.
#[track_caller]
#[inline]
pub unsafe fn set_body_velocities(
    body: u32,
    owner: NonZeroUsize,
    linear: Vec3,
    angular: Vec3,
) -> Result<(), PhysicsError> {
    unsafe { bodies::drive(body, owner, rigid::Velocities { linear, angular }) }
}

/// The dynamic body `body`'s centre of mass and orientation, game space, after checking it still
/// belongs to `owner`: a live grenade or a dropped magazine near the hands, which a body still
/// owned by it proves alive.
///
/// # Safety
///
/// The main thread, outside the physics step.
#[inline]
pub unsafe fn body_pose(body: u32, owner: NonZeroUsize) -> Result<(Vec3, Quat), PhysicsError> {
    let state = unsafe { bodies::read(body, owner) }?;
    Ok((state.pose.centre, state.pose.orientation))
}

/// The dynamic body `body`'s state, game space, after checking it still belongs to `owner`: a
/// weapon the grapple's snatch pulls in.
///
/// # Safety
///
/// The main thread, outside the physics step.
#[inline]
pub unsafe fn body_state(body: u32, owner: NonZeroUsize) -> Result<BodyState, PhysicsError> {
    unsafe { bodies::read(body, owner) }
}

/// The dynamic body `body`'s owner and state, whoever owns it: a body the engine named, such as
/// the one behind the grapple hook's point or the player's own proxy body, rather than one taken
/// from an object. `None` for a freed body or one that is not dynamic.
///
/// # Safety
///
/// The main thread, outside the physics step.
pub unsafe fn line_body(body: u32) -> Option<(NonZeroUsize, BodyState)> {
    let owner = unsafe { bodies::owner_of(body) }?;
    let state = unsafe { bodies::read(body, owner) }.ok()?;
    Some((owner, state))
}

/// The mass of the dynamic body `body`, whatever owns it, kilograms: the player's own proxy body,
/// which no object owns as a hand's targets are owned.
///
/// # Safety
///
/// The main thread, outside the physics step.
pub unsafe fn body_mass_kg(body: u32) -> Option<f32> {
    unsafe { bodies::mass_kg(body) }
}
