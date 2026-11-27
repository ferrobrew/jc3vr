//! The weapon handling's and the hand bodies' side of the aim: what the aim update hands them and
//! asks of them, and what the per-weapon update ([`crate::controllers::aim::gun`]) asks of the
//! weapon handling before it hands a gun its hand's aim point.
//!
//! Those systems sit above the aim, so they fill one [`AimHooks`] in the body's contributions
//! ([`crate::contributions::BodyContributions::aim_hooks`]), a table of their own functions, and
//! the aim calls through it at the same points, in the same order, as it would call them directly.
//! Calling before the table is registered is a programming error and panics; the install registers
//! it before any hook is enabled.

use glam::Vec3;
use jc3gi::equipment::weapon::{WeaponBase, WeaponInputs};
use jc3vr_input::frame::Pose;

use crate::contributions;

/// The functions the aim calls in the systems above it.
#[derive(Clone, Copy)]
pub struct AimHooks {
    /// The grenade throw's view of each hand's grip in the world, on every aim update (`None` for
    /// both while the aim is idle).
    pub observe_grenade_hands: fn([Option<Pose>; 2]),
    /// The C4 throw's view of each hand's grip in the world, as for the grenade's.
    pub observe_c4_hands: fn([Option<Pose>; 2]),
    /// The C4 stick probe, in the simulation phase outside the physics step.
    ///
    /// # Safety
    ///
    /// The live local player's aim update, outside the physics step.
    pub probe_c4_stick: unsafe fn(),
    /// The gun wall probe, as for the C4 stick probe.
    ///
    /// # Safety
    ///
    /// As [`probe_c4_stick`](Self::probe_c4_stick).
    pub probe_collide: unsafe fn(),
    /// The aim points of the weapons armed in the hands beside the wielded one, by the owner's
    /// address, each the armed weapon's address and its hand's point.
    pub publish_armed_targets: fn(usize, ArmedTargets),
    /// The hand bodies' step after the aim update, with the tick's length in seconds.
    pub after_aim: fn(f32),
    /// The per-weapon update's pin of a single pistol to the gun it shows, for every character's
    /// weapons.
    pub pin_single: fn(&mut WeaponBase),
    /// The weapon handling's refresh of a weapon's attach bones before its per-weapon update.
    ///
    /// # Safety
    ///
    /// The weapon must be live, on the thread updating it.
    pub before_update: unsafe fn(&mut WeaponBase),
    /// The fire input of the hand a weapon is in, in place of the game's, when the weapon handling
    /// gives the weapon its own.
    ///
    /// # Safety
    ///
    /// As [`before_update`](Self::before_update).
    pub substitute_input: unsafe fn(&WeaponBase, &WeaponInputs) -> Option<WeaponInputs>,
}

/// The weapon armed in each hand beside the wielded one and its hand's aim point, by
/// [`Hand::index`](jc3vr_input::frame::Hand::index): the weapon's address and the point.
pub type ArmedTargets = [Option<(usize, Vec3)>; 2];

/// The registered table.
#[inline]
pub(crate) fn hooks() -> &'static AimHooks {
    contributions::get()
        .and_then(|contributions| contributions.aim_hooks.as_ref())
        .expect("aim: no aim hooks are registered; the install registers them")
}
