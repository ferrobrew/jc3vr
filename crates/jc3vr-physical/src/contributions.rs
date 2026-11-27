//! What the physical crate reads from the crates above it: one [`PhysicalContributions`] record,
//! registered once at install by the crate that provides it (`docs/architecture.md`, "Crate
//! layering").
//!
//! The record has one field per contribution, so a registrant that leaves one out fails to compile;
//! whether the record was registered at all is reported to the install's registration check through
//! [`registered`].

use jc3vr_body::controllers::hands::CockpitHand;

use crate::controllers::physical_hands::physics as hand_bodies;

/// The contributions the hand bodies run every frame.
pub struct PhysicalContributions {
    /// The vehicle rail's part of the hand bodies' pre-simulation step, which carries the player
    /// along the skid. It runs after the drawn body's step and before the hand step, so a gripping
    /// hand's carry is the one the proxy keeps. Called on the thread that steps the physics world,
    /// outside the step, after the characters' proxy updates.
    pub rail_step: unsafe fn(),
    /// The vehicle controls' targets: called with the cockpit-frame hands, it draws a hand holding
    /// a control or a handle there, for the hand bodies' drive.
    pub vehicle_targets: fn(&mut [Option<CockpitHand>; 2]),
}

/// Register the contributions. One registration: a second is refused and logged.
pub fn register(contributions: PhysicalContributions) {
    if registered() {
        tracing::error!("physical: the contributions are already registered; keeping the first");
        return;
    }
    let PhysicalContributions {
        rail_step,
        vehicle_targets,
    } = contributions;
    hand_bodies::register_rail_step(rail_step);
    hand_bodies::guns::targets::register_vehicle_targets(vehicle_targets);
}

/// Whether the contributions are registered.
pub fn registered() -> bool {
    hand_bodies::rail_step_registered() && hand_bodies::guns::targets::vehicle_targets_registered()
}
