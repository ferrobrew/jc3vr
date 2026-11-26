//! Whether the physical hands hold the body on a climb, and the climb's reads of the bodies it
//! climbs, as the physical hands publish them for the body and the rig.
//!
//! The climb's state is read with its freshness at the moment of reading (a grip the last
//! pre-simulation step left, within a moment of it), and a body's place and identity are read from
//! the physics world when asked, so the climb publishes its readers rather than a copy:
//! [`register`] once at installation, before any reader runs. Until then every reader answers as
//! if there were no climb and no body.

use std::sync::OnceLock;

use body_rig::geometry::point_velocity;
use glam::{Affine3A, Vec3};

/// The climb's readers, registered by the physical hands.
pub struct ClimbReaders {
    /// Whether a hand grips the world, as the last pre-simulation step left it.
    pub climbing: fn() -> bool,
    /// The place and motion of a body, or `None` for a freed slot or a motion that is not finite.
    /// Called on the game thread, outside the physics step.
    pub body_frame: unsafe fn(u32) -> Option<BodyFrame>,
    /// The current identity of an added body. Called outside the physics step, while body creation
    /// and destruction are not running.
    pub body_identity: unsafe fn(u32) -> Option<BodyIdentity>,
}

/// Register the climb's readers. One registration: a second is refused and logged.
pub fn register(readers: ClimbReaders) {
    if READERS.set(readers).is_err() {
        tracing::error!("climb: the climb's readers are already registered; keeping the first");
    }
}

/// Whether the climb's readers are registered.
pub fn is_registered() -> bool {
    READERS.get().is_some()
}

/// Whether a hand grips the world, as the last pre-simulation step left it: the procedural body
/// hangs the legs while one does, and the rig leaves the root to the climb.
pub fn climbing() -> bool {
    READERS.get().is_some_and(|readers| (readers.climbing)())
}

/// The place and motion of the body `body`, or `None` for a freed slot or a motion that is not
/// finite.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub unsafe fn body_frame(body: u32) -> Option<BodyFrame> {
    let readers = READERS.get()?;
    unsafe { (readers.body_frame)(body) }
}

/// The current identity of an added body, checked before reading its engine instance.
///
/// # Safety
///
/// Outside the physics step, while body creation and destruction are not running.
pub unsafe fn body_identity(body: u32) -> Option<BodyIdentity> {
    let readers = READERS.get()?;
    unsafe { (readers.body_identity)(body) }
}

/// The live metadata of a reached body. Static world geometry may have no engine owner.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct BodyIdentity {
    pub world: usize,
    pub bodies: usize,
    pub instance: usize,
    pub owner: usize,
    pub shape: usize,
    pub motion: u32,
    pub static_body: bool,
    pub lifetime: u64,
}

/// A body's place and motion, game space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BodyFrame {
    /// The body's transform.
    pub transform: Affine3A,
    /// Whether the body is static, which never moves.
    pub is_static: bool,
    /// Whether the body is keyframed, moved along a set path.
    pub is_keyframed: bool,
    /// The body's mass, or `None` for an infinite one.
    pub mass_kg: Option<f32>,
    pub centre: Vec3,
    pub linear: Vec3,
    pub angular: Vec3,
}

impl BodyFrame {
    /// The velocity of the body's point at `at`, world space.
    pub fn point_velocity(&self, at: Vec3) -> Vec3 {
        if self.is_static {
            return Vec3::ZERO;
        }
        point_velocity(self.centre, self.linear, self.angular, at)
    }
}

static READERS: OnceLock<ClimbReaders> = OnceLock::new();
