//! The physics substrate the hands, the weapons, and the grapple share: the rigid bodies, their
//! poses and velocities, the physics world, and the references that keep a held body alive.
//!
//! - [`bodies`]: a body's place, motion, mass, and owner, read and driven through the Havok world;
//! - [`rigid`]: a rigid body's response to impulses;
//! - [`world`]: the Havok world, lent out for one call at a time;
//! - [`havok`]: conversions to and from Havok's vectors and transforms;
//! - [`refs`]: weak and strong references to game objects held across ticks;
//! - [`characters`]: a character's proxy body, its protection, and the ragdolls that take it out
//!   of a seat;
//! - [`grapple_hook`]: the grappling hook and its wire's two ends;
//! - [`steps`]: the physics steps counted so far, which publications for the next step are
//!   stamped and aged by.
//!
//! The body operations below read and drive one body each for the hands, the weapons, and the
//! grapple.

pub mod bodies;
pub mod characters;
pub mod grapple_hook;
pub mod havok;
pub(crate) mod refs;
pub mod rigid;
pub mod steps;
pub(crate) mod world;

use std::num::NonZeroUsize;

use glam::{Quat, Vec3};

use crate::controllers::physics::bodies::{BodyError, BodyState};

/// Give the dynamic body `body` the velocities `linear` and `angular`, world space, after checking
/// it still belongs to `owner`: a dropped weapon leaving the hand with the hand's motion.
///
/// # Safety
///
/// The main thread, outside the physics step.
#[track_caller]
pub(crate) unsafe fn set_body_velocities(
    body: u32,
    owner: NonZeroUsize,
    linear: Vec3,
    angular: Vec3,
) -> Result<(), BodyError> {
    unsafe { bodies::drive(body, owner, rigid::Velocities { linear, angular }) }
}

/// The dynamic body `body`'s centre of mass and orientation, game space, after checking it still
/// belongs to `owner`: a live grenade or a dropped magazine near the hands, which a body still
/// owned by it proves alive.
///
/// # Safety
///
/// The main thread, outside the physics step.
pub(crate) unsafe fn body_pose(body: u32, owner: NonZeroUsize) -> Result<(Vec3, Quat), BodyError> {
    let state = unsafe { bodies::read(body, owner) }?;
    Ok((state.pose.centre, state.pose.orientation))
}

/// The dynamic body `body`'s state, game space, after checking it still belongs to `owner`: a
/// weapon the grapple's snatch pulls in.
///
/// # Safety
///
/// The main thread, outside the physics step.
pub(crate) unsafe fn body_state(body: u32, owner: NonZeroUsize) -> Result<BodyState, BodyError> {
    unsafe { bodies::read(body, owner) }
}

/// The dynamic body `body`'s owner and state, whoever owns it: a body the engine named, such as
/// the one behind the grapple hook's point or the player's own proxy body, rather than one taken
/// from an object. `None` for a freed body or one that is not dynamic.
///
/// # Safety
///
/// The main thread, outside the physics step.
pub(crate) unsafe fn line_body(body: u32) -> Option<(NonZeroUsize, BodyState)> {
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
pub(crate) unsafe fn body_mass_kg(body: u32) -> Option<f32> {
    unsafe { bodies::mass_kg(body) }
}
