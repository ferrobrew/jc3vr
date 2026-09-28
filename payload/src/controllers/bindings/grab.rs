//! The grab-volume query: whether a hand is inside a region where a grip press grabs rather than
//! performing the grip's default action (the off-hand's grapple).
//!
//! The volumes are the body holsters, a two-handed weapon's foregrip, the grenade pouch, and, under
//! the opt-in physical controls, the vehicle controls. None of them exists yet, so the query
//! reports no volume and every off-hand grip press fires the grapple; the weapon-handling and
//! vehicle subsystems fill [`grab_volume`] in as they add their volumes.

use crate::controllers::frame::{ControllerFrame, Hand};

/// A region where a grip press grabs.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "the weapon-handling and vehicle subsystems produce these once their volumes exist"
)]
pub enum GrabVolume {
    /// A weapon holster on the body.
    Holster,
    /// The held two-handed weapon's foregrip.
    Foregrip,
    /// The grenade pouch.
    GrenadePouch,
    /// A physical vehicle control (a wheel, handlebars, a flight stick, a turret handle).
    VehicleControl,
}

/// The grab volume `hand` is in at `frame`, if any.
pub fn grab_volume(_frame: &ControllerFrame, _hand: Hand) -> Option<GrabVolume> {
    None
}
