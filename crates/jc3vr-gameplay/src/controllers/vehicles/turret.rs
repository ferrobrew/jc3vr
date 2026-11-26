//! A manned vehicle gun aimed by the hands.
//!
//! A vehicle gun turns on two powered hinges of the vehicle part it is mounted on, which the
//! weapon's own pre-simulation update drives toward a desired yaw and pitch each tick. The desired
//! angles come from the look input when the weapon turns from the stick for a player user
//! ([`WeaponBase::m_MountedInputIsControlledByWeapon`]), and from the aim target otherwise
//! ([`WeaponBase::UpdateMountedInput`] decides). The mod writes the hands' aim point as the aim
//! target, so a gun that turns from the stick ignored the hands: in a jeep's gunner seat on the
//! 14 November take the turret held straight ahead while the hands gripped its handles.
//!
//! While the hands hold a gun's handles and their aim point is fresh, the look-input path is
//! refused, so the update aims the hinges at the hands' point at the gun's own turn speeds and
//! within its limits. Otherwise the game's path runs, so a gamepad turns the turret as before.
//!
//! [`WeaponBase::m_MountedInputIsControlledByWeapon`]: jc3gi::equipment::weapon::WeaponBase::m_MountedInputIsControlledByWeapon
//! [`WeaponBase::UpdateMountedInput`]: jc3gi::equipment::weapon::WeaponBase::UpdateMountedInput

use std::sync::atomic::{AtomicU64, Ordering};

use detours_macro::detour;
use jc3gi::equipment::weapon::WeaponBase;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::vehicles::state::{is_mounted_weapon, mounted_target};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&UPDATE_MOUNTED_INPUT_BINDER)
}

/// How often the hands aimed a manned vehicle gun in place of the look input, for the debug UI.
pub fn hand_aimed_ticks() -> u64 {
    HAND_AIMED.load(Ordering::Relaxed)
}

#[detour(address = jc3gi::equipment::weapon::WeaponBase::UpdateMountedInput_ADDRESS)]
fn update_mounted_input(
    weapon: *mut WeaponBase,
    dt: f32,
    horizontal_angle: *mut f32,
    vertical_angle: *mut f32,
) -> bool {
    let address = weapon as usize;
    // SAFETY: the weapon's own pre-simulation update passes it live.
    let blending_out = unsafe { weapon.as_ref() }.is_none_or(|w| w.m_BlendOutMotors);
    if !blending_out && is_mounted_weapon(address) && mounted_target(address).is_some() {
        let first = HAND_AIMED.fetch_add(1, Ordering::Relaxed) == 0;
        if first {
            // SAFETY: as above.
            let stick =
                unsafe { weapon.as_ref() }.is_some_and(|w| w.m_MountedInputIsControlledByWeapon);
            tracing::info!(
                target: "controllers",
                turns_from_stick = stick,
                "mounted gun: the hands aim a manned vehicle gun in place of the look input"
            );
        }
        return false;
    }
    UPDATE_MOUNTED_INPUT
        .get()
        .unwrap()
        .call(weapon, dt, horizontal_angle, vertical_angle)
}

static HAND_AIMED: AtomicU64 = AtomicU64::new(0);
