//! The driver's vehicle weapons aimed by the dominant hand, behind
//! [`VehiclesConfig::driver_weapons_by_hand`](crate::controllers::vehicles::config::VehiclesConfig::driver_weapons_by_hand).
//!
//! The player's post-camera update hands the vehicle the aim control's weapon point through
//! [`Vehicle::SetAimTargetPosition`] only while the player drives, and every built-in weapon of the
//! vehicle aims from the stored point ([`Vehicle::m_AimingTargetPosition`]); turrets turn toward it
//! at their own limited rate, so a hand-aimed turret trails the hand as it trails the gaze. The
//! detour substitutes the dominant hand's aim point, cast by the aim update earlier in the same
//! tick ([`crate::controllers::aim`]), for the local player's vehicle. Off, or with no hand aim, the
//! gaze point goes through unchanged.

use std::sync::atomic::{AtomicU64, Ordering};

use detours_macro::detour;
use glam::Vec3;
use jc3gi::{
    types::math::Vector3,
    vehicle::vehicle::{Seat, Vehicle},
};
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{aim, scheme},
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&SET_AIM_TARGET_POSITION_BINDER)
}

/// How many vehicle aim points the hand replaced, for the debug UI.
pub fn substitutions() -> u64 {
    SUBSTITUTIONS.load(Ordering::Relaxed)
}

#[detour(address = jc3gi::vehicle::vehicle::Vehicle::SetAimTargetPosition_ADDRESS)]
fn set_aim_target_position(vehicle: *mut Vehicle, position: *const Vector3) {
    let original = |position: *const Vector3| {
        SET_AIM_TARGET_POSITION
            .get()
            .unwrap()
            .call(vehicle, position)
    };
    // The local player's seat is recorded on the vehicle itself, so this also rules out every
    // vehicle an AI drives.
    let driven =
        unsafe { vehicle.as_ref() }.is_some_and(|v| v.m_LocalPlayerSeat == Seat::EDriverSeat);
    match hand_point().filter(|_| driven) {
        Some(point) => {
            SUBSTITUTIONS.fetch_add(1, Ordering::Relaxed);
            let point = Vector3 {
                data: point.to_array(),
            };
            original(&point);
        }
        None => original(position),
    }
}

/// The dominant hand's aim point from the last aim update, while the option is on and the hand
/// aims.
fn hand_point() -> Option<Vec3> {
    let (enabled, on, dominant) = Config::lock_query(|c| {
        (
            c.controllers.enabled,
            c.controllers.vehicles.driver_weapons_by_hand,
            c.controllers.dominant_hand,
        )
    });
    if !(enabled && on && scheme::is_controllers()) {
        return None;
    }
    let status = aim::status();
    if status.idle.is_some() {
        return None;
    }
    status.hands[dominant.index()].map(|h| h.target)
}

static SUBSTITUTIONS: AtomicU64 = AtomicU64::new(0);
