//! Shooting from a passenger or stunt seat by the hand's aim.
//!
//! Out of the driver's seat the weapon in the hands is the player's wielded weapon, so the per-hand
//! gun aim already aims and fires it along the holding hand's sight line
//! ([`jc3vr_body::controllers::aim::gun`]), and the character-level aim point follows the gun hand. The
//! seated character's upper body is another matter: the vehicle aim blend
//! ([`VehicleAimBlendState::UpdateSamplers`]) turns the torso and the gun arm toward the aim
//! control's own weapon point ([`PlayerAimControl::m_AimPos`]), which the game sets from the gaze.
//! A passenger looking ahead and pointing the gun out of the side window would twist toward the
//! windscreen while the shot leaves for the side. So after the aim control's update, in a passenger
//! or stunt seat and while a gun hand aims, the weapon slot of that point is set to the gun hand's
//! aim point, and the body leans and turns toward where the hand points, as it does toward the
//! gaze for the gamepad. The game copies the slot into the control parameters and the character's
//! weapon point right after, so everything downstream follows the hand.
//!
//! [`VehicleAimBlendState::UpdateSamplers`]: jc3gi::aim::aim::VehicleAimBlendState::UpdateSamplers
//! [`PlayerAimControl::m_AimPos`]: jc3gi::aim::aim::PlayerAimControl::m_AimPos

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use glam::Vec3;
use jc3vr_body::controllers::{mode::VehicleSeat, scheme};
use jc3vr_input::frame::Hand;

/// The passenger aim's readout.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct PassengerStatus {
    /// Whether the last aim update set the body's aim point from the hand.
    pub aiming: bool,
    /// Aim updates that set it, since the payload loaded.
    pub updates: u64,
    /// Times it started, since the payload loaded.
    pub starts: u64,
}

pub fn status() -> PassengerStatus {
    PassengerStatus {
        aiming: AIMING.load(Ordering::Relaxed),
        updates: UPDATES.load(Ordering::Relaxed),
        starts: STARTS.load(Ordering::Relaxed),
    }
}

/// The point the seated body aims at this update: the gun hand's aim point, the acting hand's
/// first, while the player is in a vehicle out of the driver's seat and off a mounted gun. `None`
/// leaves the game's point.
pub fn body_aim_point(
    seat: Option<VehicleSeat>,
    mounted: bool,
    gun_targets: [Option<Vec3>; 2],
) -> Option<Vec3> {
    aim_point(scheme::acting_hand(), seat, mounted, gun_targets)
}

/// [`body_aim_point`] with the hand the roles follow, `acting`, given.
fn aim_point(
    acting: Hand,
    seat: Option<VehicleSeat>,
    mounted: bool,
    gun_targets: [Option<Vec3>; 2],
) -> Option<Vec3> {
    let passenger = seat.is_some_and(|s| !s.driver) && !mounted;
    if !passenger {
        return None;
    }
    gun_targets[acting.index()]
        .or(gun_targets[acting.other().index()])
        .filter(|p| p.is_finite())
}

/// Record whether this update set the body's aim point, and log when that starts and stops.
pub fn note(aiming: bool) {
    let was = AIMING.swap(aiming, Ordering::Relaxed);
    if aiming {
        UPDATES.fetch_add(1, Ordering::Relaxed);
    }
    if aiming != was {
        if aiming {
            STARTS.fetch_add(1, Ordering::Relaxed);
        }
        tracing::info!(
            target: "controllers",
            "the seated body {} the gun hand's aim",
            if aiming { "follows" } else { "stops following" }
        );
    }
}

static AIMING: AtomicBool = AtomicBool::new(false);
static UPDATES: AtomicU64 = AtomicU64::new(0);
static STARTS: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
mod tests {
    use jc3vr_body::controllers::mode::VehicleClass;

    use super::*;

    fn seat(driver: bool) -> Option<VehicleSeat> {
        Some(VehicleSeat {
            class: VehicleClass::Car,
            driver,
        })
    }

    #[test]
    fn a_passenger_s_body_follows_the_gun_hand_and_a_driver_s_does_not() {
        let acting = Hand::Right;
        let mut targets = [None; 2];
        targets[acting.other().index()] = Some(Vec3::X);
        assert_eq!(
            aim_point(acting, seat(false), false, targets),
            Some(Vec3::X)
        );
        targets[acting.index()] = Some(Vec3::Y);
        assert_eq!(
            aim_point(acting, seat(false), false, targets),
            Some(Vec3::Y)
        );
        assert_eq!(aim_point(acting, seat(true), false, targets), None);
        assert_eq!(aim_point(acting, seat(false), true, targets), None);
        assert_eq!(aim_point(acting, None, false, targets), None);
        assert_eq!(aim_point(acting, seat(false), false, [None; 2]), None);
    }
}
