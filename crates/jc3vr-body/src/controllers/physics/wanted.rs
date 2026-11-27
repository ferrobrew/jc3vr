//! The local player character's wanted velocity, and the one order its writers apply in.
//!
//! The engine moves the character's proxy by the wanted velocity its movement task writes each
//! simulation tick ([`Character::SetWantedWorldVelocity`]); the mod's writers adjust it around the
//! task's write. Each writer is a [`Contributor`], and a tick's writes go through one
//! [`Composition`], which applies them in the contributors' declared order: each contribution reads
//! the wanted velocity as the previous one left it and returns the velocity to write, or `None` to
//! leave it as it is. The read and the write go through the composition's [`WantedStore`], so the
//! contributions themselves are pure, and the order and the sums are pinned by the module's tests.
//!
//! The order, which is [`Contributor`]'s, within one frame:
//!
//! 1. Before the world update, on the game thread: the vehicle rail's entry or step-off
//!    ([`Contributor::Rail`]) and the grapple lock's release and its handback
//!    ([`Contributor::GrappleLock`]). The character's movement task runs after them and writes the
//!    velocity afresh, so these are the inputs the task carries into its tick.
//! 2. In the character's update, exactly one movement task runs:
//!    - on foot, the locomotion task's post-call composes, in order, the stick's velocity in place
//!      of the clip's horizontal one ([`Contributor::Locomotion`]), the palms' push added to it
//!      ([`Contributor::PalmPush`]), and the step drive added to the sum and capped
//!      ([`Contributor::StepDrive`]);
//!    - swimming, the swim core's post-call joins the arm strokes' speed to the core's velocity
//!      ([`Contributor::Strokes`]);
//!    - reeling, the reel's arrival hands the grapple lock's velocity on
//!      ([`Contributor::GrappleLock`]).
//!
//! The rail and the grapple lock never run in one frame (one stands Rico on a helicopter's skid,
//! the other swings him on the line), and the three movement tasks are exclusive, so the only
//! composition with more than one contributor is the on-foot one, whose order the
//! [`Composition`] checks.

use glam::{Vec2, Vec3};
use jc3gi::{character::character::Character, types::math::Vector3};

/// A writer of the character's wanted velocity, in the order the writers apply within a frame.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Contributor {
    /// The vehicle rail taking Rico out of the helicopter's interaction graph: moving with its skid
    /// at the rail's entry, or at his own velocity when the jump steps him off during the door-side
    /// arrival (the vehicle handling).
    Rail,
    /// The grapple lock's release, its handback after the reel's cancel, and the velocity it hands
    /// on at the reel's arrival (the grapple).
    GrappleLock,
    /// The stick's velocity in place of the on-foot clip's horizontal velocity (the body's
    /// locomotion).
    Locomotion,
    /// The palms' push off what they press into, added to the stick's velocity (the physical
    /// hands, through [`crate::contributions::BodyContributions::palm_push`]).
    PalmPush,
    /// The step drive's velocity, added to the task's and capped with it (the body rig's root).
    StepDrive,
    /// The arm strokes' swim speed, joined to the swim core's velocity along the way the swimmer
    /// faces (the body's swim hooks).
    Strokes,
}

impl Contributor {
    /// Every contributor, in order.
    pub const ORDER: [Contributor; 6] = [
        Contributor::Rail,
        Contributor::GrappleLock,
        Contributor::Locomotion,
        Contributor::PalmPush,
        Contributor::StepDrive,
        Contributor::Strokes,
    ];

    /// A short name for a log.
    pub const fn label(self) -> &'static str {
        match self {
            Contributor::Rail => "rail",
            Contributor::GrappleLock => "grapple lock",
            Contributor::Locomotion => "locomotion",
            Contributor::PalmPush => "palm push",
            Contributor::StepDrive => "step drive",
            Contributor::Strokes => "strokes",
        }
    }
}

/// Where a composition reads and writes the wanted velocity, world metres per second.
pub(crate) trait WantedStore {
    fn read(&self) -> Vec3;
    fn write(&mut self, wanted: Vec3);
}

/// The local player character's wanted velocity through the engine's world-velocity accessors
/// ([`Character::GetWantedWorldVelocity`], [`Character::SetWantedWorldVelocity`]), which convert
/// to and from the character's frame.
pub(crate) struct CharacterWanted<'c> {
    character: &'c mut Character,
}

impl<'c> CharacterWanted<'c> {
    /// The store of `character`'s wanted velocity.
    ///
    /// # Safety
    ///
    /// `character` must be live, on the thread running its update or before the world update on
    /// the game thread, for as long as the store is.
    pub unsafe fn new(character: &'c mut Character) -> Self {
        Self { character }
    }

    /// The character, mutably, for a contribution whose engine reads take it so.
    pub fn character_mut(&mut self) -> &mut Character {
        self.character
    }
}

impl WantedStore for CharacterWanted<'_> {
    fn read(&self) -> Vec3 {
        let mut wanted = Vector3 { data: [0.0; 3] };
        // SAFETY: the store's contract (see [`CharacterWanted::new`]).
        unsafe { self.character.GetWantedWorldVelocity(&mut wanted) };
        Vec3::from_array(wanted.data)
    }

    fn write(&mut self, wanted: Vec3) {
        let wanted = Vector3 {
            data: wanted.to_array(),
        };
        // SAFETY: as [`Self::read`].
        unsafe { self.character.SetWantedWorldVelocity(&wanted) };
    }
}

/// One composition of the wanted velocity: the contributions of one context of a frame, applied
/// through `S` in [`Contributor`] order.
pub(crate) struct Composition<S> {
    store: S,
    applied: Option<Contributor>,
}

impl<S: WantedStore> Composition<S> {
    /// A composition that has applied nothing.
    pub fn new(store: S) -> Self {
        Self {
            store,
            applied: None,
        }
    }

    /// The wanted velocity as the contributions so far left it.
    pub fn read(&self) -> Vec3 {
        self.store.read()
    }

    /// Apply `contributor`'s contribution: read the wanted velocity, hand it to `contribution`
    /// with the store, and write what it returns. A contributor applied after one later in the
    /// order is a programming error.
    pub fn apply(
        &mut self,
        contributor: Contributor,
        contribution: impl FnOnce(&mut S, Vec3) -> Option<Vec3>,
    ) {
        debug_assert!(
            self.applied.is_none_or(|applied| applied < contributor),
            "wanted velocity: {} applied after {}",
            contributor.label(),
            self.applied.map_or("nothing", Contributor::label),
        );
        self.applied = Some(contributor);
        let current = self.store.read();
        if let Some(wanted) = contribution(&mut self.store, current) {
            self.store.write(wanted);
        }
    }

    /// The store, for reads between the contributions.
    #[cfg(test)]
    pub(crate) fn store(&self) -> &S {
        &self.store
    }
}

/// Write `wanted` as `contributor`'s whole contribution: a writer that sets the velocity outright.
///
/// # Safety
///
/// As [`CharacterWanted::new`].
pub unsafe fn set(character: &mut Character, contributor: Contributor, wanted: Vec3) {
    let mut composition = Composition::new(unsafe { CharacterWanted::new(character) });
    composition.apply(contributor, |_, _| Some(wanted));
}

/// `character`'s wanted velocity, world metres per second.
///
/// # Safety
///
/// `character` must be live, on the thread running its update or before the world update on the
/// game thread.
pub unsafe fn read(character: &Character) -> Vec3 {
    let mut wanted = Vector3 { data: [0.0; 3] };
    unsafe { character.GetWantedWorldVelocity(&mut wanted) };
    Vec3::from_array(wanted.data)
}

/// [`Contributor::Locomotion`]'s write: the horizontal velocity `velocity`, world `(x, z)`, with
/// the task's vertical velocity kept.
pub(crate) fn with_horizontal(wanted: Vec3, velocity: Vec2) -> Vec3 {
    Vec3::new(velocity.x, wanted.y, velocity.y)
}

/// [`Contributor::PalmPush`]'s write: the push, world `(x, z)`, added to the horizontal velocity,
/// or nothing without one.
pub fn pushed(wanted: Vec3, push: Option<Vec2>) -> Option<Vec3> {
    push.map(|push| Vec3::new(wanted.x + push.x, wanted.y, wanted.z + push.y))
}

#[cfg(test)]
mod tests;
