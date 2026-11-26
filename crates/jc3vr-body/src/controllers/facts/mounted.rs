//! The mounted gun the hands aim, its aim point, the seated body's aim, and each hand's hold on a
//! mounted gun's handle, as the vehicle handling publishes them for the aim, the weapon handling,
//! and the physical hands.
//!
//! The vehicle handling registers one [`MountedSource`] at install ([`register_mounted_source`]), a
//! table of its own query functions, and every read here calls through it. The reads are live:
//! the per-weapon update reads the mounted gun on the worker threads between the vehicle
//! handling's writes, lock-free for every character's weapons, so a per-tick copy would add a
//! hop the direct read never had.
//!
//! Reading before the source is registered is a programming error and panics; the controllers'
//! install registers it before any hook is enabled. The handle holds are a plain published fact
//! ([`holds`]), written as the vehicle handling's tick finishes and as it stands down.

use std::sync::{
    OnceLock,
    atomic::{AtomicBool, Ordering},
};

use glam::Vec3;

use crate::controllers::mode::VehicleSeat;

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

/// Register the vehicle handling's source; the first registration stands.
pub fn register_mounted_source(source: MountedSource) {
    if SOURCE.set(source).is_err() {
        tracing::warn!(
            "mounted: a mounted-gun source is already registered; the new one is ignored"
        );
    }
}

/// Whether the vehicle handling has registered its source.
pub fn is_registered() -> bool {
    SOURCE.get().is_some()
}

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
/// [`Hand::index`](jc3vr_input::frame::Hand::index). Lock-free, for the physical hands to leave a
/// hand on a handle out of their palms.
#[inline]
pub fn holds() -> [bool; 2] {
    HOLDS.each_ref().map(|hold| hold.load(Ordering::Relaxed))
}

/// Publish each hand's hold on a mounted gun's handle for [`holds`].
#[inline]
pub fn publish_holds(holds: [bool; 2]) {
    for (slot, hold) in HOLDS.iter().zip(holds) {
        slot.store(hold, Ordering::Relaxed);
    }
}

#[inline]
fn source() -> &'static MountedSource {
    SOURCE.get().expect(
        "mounted: no mounted-gun source is registered; the controllers' install registers it",
    )
}

static SOURCE: OnceLock<MountedSource> = OnceLock::new();

static HOLDS: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
