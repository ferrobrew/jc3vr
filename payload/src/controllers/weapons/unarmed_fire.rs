//! The fire input with nothing in the hands: under the controller scheme it does not draw a weapon.
//!
//! The game's weapon handling ([`Inventory::UpdateWeaponHandling`]) draws the selected weapon when
//! the fire input arrives with nothing wielded or while a holster plays, and draws it for the
//! precision-aim input with nothing wielded. With the holsters, a weapon comes out of a holster or
//! not at all, so for the length of that call the local player's fire and precision-aim inputs read
//! zero whenever they would draw, and are put back afterwards for everything that reads them later
//! in the tick. `weapons.fire_draws_weapon` restores the game's draw.
//!
//! [`Inventory::UpdateWeaponHandling`]: jc3gi::character::inventory::Inventory::UpdateWeaponHandling

use std::sync::atomic::{AtomicU64, Ordering};

use jc3gi::character::character::{Character, WeaponInput};

use crate::{config::Config, controllers::scheme};

/// The draw-on-fire suppression's readout.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct UnarmedFireStatus {
    /// Weapon-handling calls whose fire or precision-aim input was held back, since the payload
    /// loaded.
    pub held_back: u64,
}

pub fn status() -> UnarmedFireStatus {
    UnarmedFireStatus {
        held_back: HELD_BACK.load(Ordering::Relaxed),
    }
}

/// The inputs held back for one weapon-handling call, to be put back after it.
#[must_use]
pub(crate) struct HeldDrawInputs {
    fire: f32,
    precision_aim: f32,
}

impl HeldDrawInputs {
    /// Put the held inputs back.
    ///
    /// # Safety
    ///
    /// `character` must be the character the inputs were held back from, on the thread updating
    /// it.
    pub(crate) unsafe fn restore(self, character: &mut Character) {
        let inputs = &mut character.m_WeaponInputs;
        inputs[WeaponInput::E_INPUT_MAIN_WEAPON as usize] = self.fire;
        inputs[WeaponInput::E_INPUT_PRECISION_AIM as usize] = self.precision_aim;
    }
}

/// Hold back the local player's fire and precision-aim inputs for the weapon-handling call about to
/// run, when they would draw a weapon. `holstering` is whether a holster is playing.
///
/// # Safety
///
/// `character` must be the live local player character, on the thread running its weapon
/// handling, just before the call.
pub(crate) unsafe fn hold_back(
    character: &mut Character,
    holstering: bool,
) -> Option<HeldDrawInputs> {
    let (enabled, draws) = Config::lock_query(|c| {
        (
            c.controllers.enabled,
            c.controllers.weapons.fire_draws_weapon,
        )
    });
    if !enabled || draws || !scheme::is_controllers() {
        return None;
    }
    let wielding =
        unsafe { character.GetWieldingWeapon(false).as_ref() }.is_some_and(|w| !w.px.is_null());
    let inputs = &mut character.m_WeaponInputs;
    let held = HeldDrawInputs {
        fire: inputs[WeaponInput::E_INPUT_MAIN_WEAPON as usize],
        precision_aim: inputs[WeaponInput::E_INPUT_PRECISION_AIM as usize],
    };
    if !would_draw(wielding, holstering, held.fire, held.precision_aim) {
        return None;
    }
    inputs[WeaponInput::E_INPUT_MAIN_WEAPON as usize] = 0.0;
    inputs[WeaponInput::E_INPUT_PRECISION_AIM as usize] = 0.0;
    HELD_BACK.fetch_add(1, Ordering::Relaxed);
    Some(held)
}

/// Whether the weapon handling would draw a weapon from these inputs: the fire input with nothing
/// wielded or during a holster, or the precision-aim input with nothing wielded.
pub fn would_draw(wielding: bool, holstering: bool, fire: f32, precision_aim: f32) -> bool {
    (fire > 0.0 && (!wielding || holstering)) || (precision_aim > 0.0 && !wielding)
}

static HELD_BACK: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fire_draws_only_with_empty_hands_or_during_a_holster() {
        assert!(would_draw(false, false, 1.0, 0.0));
        assert!(would_draw(true, true, 0.5, 0.0));
        assert!(!would_draw(true, false, 1.0, 0.0));
        assert!(!would_draw(false, false, 0.0, 0.0));
    }

    #[test]
    fn precision_aim_draws_only_with_empty_hands() {
        assert!(would_draw(false, false, 0.0, 1.0));
        assert!(!would_draw(true, true, 0.0, 1.0));
    }
}
