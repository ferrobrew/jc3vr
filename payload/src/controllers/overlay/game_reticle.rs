//! Hiding the game's own HUD reticles while the hands aim
//! ([`OverlayConfig::hide_game_reticle`](crate::controllers::config::OverlayConfig::hide_game_reticle)).
//!
//! The HUD movie draws a weapon reticle at the centre of the screen and a grapple reticle at the
//! grapple slot's projected aim point, pinned to the centre when the grapple aims at a direct
//! position (`docs/engine/gameplay/aim-pipeline.md`, "The reticles"). Under the controller scheme
//! the guns and the grapple follow the hands, so a reticle at the gaze marks a point nothing aims
//! at. Each reticle is hidden while its own aim follows a hand: the weapon reticle while the
//! wielded weapon or a manned mounted gun is aimed by hand, the grapple reticle while the grapple
//! is. A gun the gaze still aims (a vehicle's built-in weapons) keeps the reticle. The world-space
//! overlay's grapple highlight and gun readout stand in for them.
//!
//! [`decide`] is the pure rule; [`hide_game_reticles`] reads the aim's latest results and holds the reticle
//! clips hidden through [`crate::hud::scaleform`].

use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{aim, scheme, vehicles},
};

/// Which of the game's reticles to hide.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct HiddenReticles {
    /// The weapon reticle group (every weapon's crosshair shape).
    pub weapon: bool,
    /// The grapple reticle group (the aim cursor, the attached-wire marker, and its ghost).
    pub grapple: bool,
}

/// What the rule reads: the toggle, the scheme, and the aim's latest results.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct ReticleInputs {
    /// The toggle, with the controllers' master switch.
    pub enabled: bool,
    /// Whether the controller scheme is in effect.
    pub controllers_scheme: bool,
    /// Whether the aim's results are recent. Stale results belong to an aim update that no longer
    /// runs, and say nothing about what aims now.
    pub aim_fresh: bool,
    /// Whether the last aim update aimed the wielded weapon by hand.
    pub weapon_by_hand: bool,
    /// Whether a manned mounted gun is aimed by hand.
    pub mounted_by_hand: bool,
    /// Whether the last aim update aimed the grapple by hand.
    pub grapple_by_hand: bool,
}

/// Which reticles to hide for `inputs`.
pub fn decide(inputs: ReticleInputs) -> HiddenReticles {
    if !inputs.enabled || !inputs.controllers_scheme {
        return HiddenReticles::default();
    }
    HiddenReticles {
        // The mounted gun's point has its own freshness, so it does not wait on the aim update.
        weapon: (inputs.aim_fresh && inputs.weapon_by_hand) || inputs.mounted_by_hand,
        grapple: inputs.aim_fresh && inputs.grapple_by_hand,
    }
}

/// Hold the game's reticles hidden while the hands aim, and restore them otherwise.
///
/// # Safety
///
/// Must be called from the [`MovieImpl::Capture`](jc3gi::ui::scaleform::MovieImpl) detour, as
/// [`crate::hud::scaleform::apply_reticle_suppression`] requires.
pub(crate) unsafe fn hide_game_reticles() {
    let enabled =
        Config::lock_query(|c| c.controllers.enabled && c.controllers.overlay.hide_game_reticle);
    let hidden = if enabled {
        let status = aim::status();
        decide(ReticleInputs {
            enabled,
            controllers_scheme: scheme::is_controllers(),
            aim_fresh: aim_fresh(status.updates),
            weapon_by_hand: status.weapon.is_some(),
            mounted_by_hand: vehicles::mounted_aim_point().is_some(),
            grapple_by_hand: status.grapple.is_some(),
        })
    } else {
        HiddenReticles::default()
    };
    // SAFETY: forwarded from the caller.
    unsafe { crate::hud::scaleform::apply_reticle_suppression(hidden.weapon, hidden.grapple) };
}

/// How long the aim's results count as recent after its update count last moved.
const AIM_MAX_AGE: Duration = Duration::from_millis(250);

/// The aim's update count when it was last seen to change, and when.
static LAST_AIM_UPDATE: Mutex<Option<(u64, Instant)>> = Mutex::new(None);

/// Whether the aim's update count `updates` has moved within [`AIM_MAX_AGE`].
fn aim_fresh(updates: u64) -> bool {
    let now = Instant::now();
    let mut last = LAST_AIM_UPDATE.lock();
    match *last {
        Some((seen, at)) if seen == updates => now.duration_since(at) < AIM_MAX_AGE,
        _ => {
            *last = Some((updates, now));
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AIMING: ReticleInputs = ReticleInputs {
        enabled: true,
        controllers_scheme: true,
        aim_fresh: true,
        weapon_by_hand: true,
        mounted_by_hand: false,
        grapple_by_hand: true,
    };

    #[test]
    fn hand_aim_hides_each_reticle_it_drives() {
        assert_eq!(
            decide(AIMING),
            HiddenReticles {
                weapon: true,
                grapple: true
            }
        );
        let grapple_only = ReticleInputs {
            weapon_by_hand: false,
            ..AIMING
        };
        assert_eq!(
            decide(grapple_only),
            HiddenReticles {
                weapon: false,
                grapple: true
            }
        );
    }

    #[test]
    fn the_toggle_and_the_gamepad_scheme_keep_both_reticles() {
        let off = ReticleInputs {
            enabled: false,
            ..AIMING
        };
        let gamepad = ReticleInputs {
            controllers_scheme: false,
            mounted_by_hand: true,
            ..AIMING
        };
        assert_eq!(decide(off), HiddenReticles::default());
        assert_eq!(decide(gamepad), HiddenReticles::default());
    }

    #[test]
    fn stale_aim_results_keep_the_reticles_but_a_mounted_gun_still_hides_its_own() {
        let stale = ReticleInputs {
            aim_fresh: false,
            ..AIMING
        };
        assert_eq!(decide(stale), HiddenReticles::default());
        let mounted = ReticleInputs {
            mounted_by_hand: true,
            ..stale
        };
        assert_eq!(
            decide(mounted),
            HiddenReticles {
                weapon: true,
                grapple: false
            }
        );
    }
}
