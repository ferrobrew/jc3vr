//! The mounted gun the hands aim, its aim point, the seated body's aim, and each hand's hold on a
//! mounted gun's handle, as the vehicle handling publishes them for the aim, the weapon handling,
//! and the physical hands.
//!
//! The vehicle handling registers one [`MountedSource`] at install
//! ([`crate::contributions::BodyContributions::mounted`]), a table of its own query functions, and every read here calls through it. The reads are live:
//! the per-weapon update reads the mounted gun on the worker threads between the vehicle
//! handling's writes, lock-free for every character's weapons, so a per-tick copy would add a
//! hop the direct read never had.
//!
//! Reading before the source is registered is a programming error and panics; the install
//! registers it before any hook is enabled. The handle holds are a published fact
//! ([`holds`]), written as the vehicle handling's tick finishes and as it stands down.

use glam::Vec3;
use jc3vr_core::published::{Published, Stage};

use crate::{contributions, controllers::mode::VehicleSeat};

/// The vehicle handling's query functions behind every read of this module. Weapons are named by
/// the address of their native object.
#[derive(Clone, Copy)]
pub struct MountedSource {
    /// Whether a weapon is the mounted gun the hands last aimed. Lock-free.
    pub is_mounted_weapon: fn(usize) -> bool,
    /// The mounted gun's aim point for the per-weapon update, when the weapon is the gun the hands
    /// aim.
    pub target: fn(usize) -> Option<Vec3>,
    /// The mounted gun's aim point for the character-level aim point.
    pub aim_point: fn() -> Option<Vec3>,
    /// The point the seated body aims at this update, from the player's seat (`None` out of a
    /// vehicle seat), whether the player is on a mounted gun, and each hand's gun aim point; the
    /// vehicle handling records whether it set one. `None` leaves the game's point.
    pub seated_body_aim: SeatedBodyAimFn,
}

/// The vehicle handling's seated body aim ([`seated_body_aim`]).
pub type SeatedBodyAimFn = fn(Option<VehicleSeat>, bool, [Option<Vec3>; 2]) -> Option<Vec3>;

/// Whether the weapon at `weapon` is the mounted gun the hands last aimed. Lock-free, for the
/// per-weapon update of every character's weapons to rule a weapon out before taking any lock.
#[inline]
pub fn is_mounted_weapon(weapon: usize) -> bool {
    (source().is_mounted_weapon)(weapon)
}

/// The mounted gun's aim point for the per-weapon update, when `weapon` is the gun the hands aim.
#[inline]
pub fn target(weapon: usize) -> Option<Vec3> {
    (source().target)(weapon)
}

/// The mounted gun's aim point for the character-level aim point, which draws the gun.
#[inline]
pub fn aim_point() -> Option<Vec3> {
    (source().aim_point)()
}

/// The point the seated body aims at this update: the gun hand's aim point while the player is in
/// a vehicle out of the driver's seat (`seat`) and off a mounted gun (`mounted`), with the vehicle
/// handling's option on. The vehicle handling records whether it set one. `None` leaves the
/// game's point.
#[inline]
pub fn seated_body_aim(
    seat: Option<VehicleSeat>,
    mounted: bool,
    gun_targets: [Option<Vec3>; 2],
) -> Option<Vec3> {
    (source().seated_body_aim)(seat, mounted, gun_targets)
}

/// Whether each hand holds a mounted gun's handle, indexed by
/// [`Hand::index`](jc3vr_input::frame::Hand::index), for the physical hands to leave a hand on a
/// handle out of their palms.
#[inline]
pub fn holds() -> [bool; 2] {
    HOLDS.latest().map_or([false; 2], |holds| holds.value)
}

/// Publish each hand's hold on a mounted gun's handle for [`holds`].
#[inline]
pub fn publish_holds(holds: [bool; 2]) {
    HOLDS.publish(holds);
}

#[inline]
fn source() -> &'static MountedSource {
    contributions::get()
        .and_then(|contributions| contributions.mounted.as_ref())
        .expect("mounted: no mounted-gun source is registered; the install registers it")
}

static HOLDS: Published<[bool; 2]> = Published::new(Stage::InputGame);
