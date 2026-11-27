//! Hiding the game's own HUD reticles under the controller scheme.
//!
//! The HUD movie draws a weapon reticle at the centre of the screen and a grapple reticle at the
//! grapple slot's projected aim point, pinned to the centre when the grapple aims at a direct
//! position (`docs/engine/gameplay/aim-pipeline.md`, "The reticles"). Under the controller scheme
//! the guns and the grapple follow the hands, so a reticle on the panel marks a point nothing aims
//! at, or a point the panel cannot show where it is.
//!
//! The grapple reticle is hidden everywhere under the controller scheme, where the grapple aims by
//! hand: on foot, in the air, in the water, and in every seat. The world-space overlay's grapple
//! highlight marks the grapple's target in the world instead, and a flat cursor on the panel adds
//! nothing to it.
//!
//! The weapon reticle is hidden wherever the guns' aim belongs to the hands rather than the gaze:
//!
//! - on foot and in the air, always, armed or not, so it does not show in
//!   the gaps between aim updates (drawing, holstering, a weapon switch, an empty hand);
//! - in the driver's seat, where the vehicle's built-in weapons aim by hand;
//! - at a mounted gun, while the gun's point comes from the hands;
//! - in a passenger seat, while the aim's latest results aimed the wielded weapon by hand.
//!
//! [`decide`] is the pure rule; [`hide_game_reticles`] reads the controllers' master switch, the
//! mode, and the aim's latest results, and holds the reticle clips hidden through [`crate::hud::scaleform`].

use jc3vr_body::controllers::{body_frame, mode, mode::Mode, scheme};
use jc3vr_gameplay::controllers::config::CONTROLLERS_CONFIG;
use jc3vr_vehicles::vehicles;

use crate::hud::tuning::HUD;

/// Which of the game's reticles to hide.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(crate) struct HiddenReticles {
    /// The weapon reticle group (every weapon's crosshair shape).
    pub weapon: bool,
    /// The grapple reticle group (the aim cursor, the attached-wire marker, and its ghost).
    pub grapple: bool,
}

/// Where the player is, as far as who aims is concerned.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(crate) enum AimPlace {
    /// On foot, swimming, falling, in the wingsuit, or under the parachute.
    #[default]
    OnFoot,
    /// In a vehicle's driver's seat.
    Driver,
    /// In a vehicle's other seats.
    Passenger,
    /// A menu is up.
    Menu,
}

impl AimPlace {
    /// The place for a detected mode.
    pub fn of(mode: Mode) -> Self {
        match mode {
            Mode::OnFoot(_) | Mode::Airborne(_) => Self::OnFoot,
            Mode::Vehicle(seat) if seat.driver => Self::Driver,
            Mode::Vehicle(_) => Self::Passenger,
            Mode::Ui => Self::Menu,
        }
    }
}

/// What the rule reads: the controllers' master switch, the scheme, the place, the mounted gun's
/// hand aim, and the aim's latest results.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(crate) struct ReticleInputs {
    /// The controllers' master switch.
    pub enabled: bool,
    /// Whether the controller scheme is in effect.
    pub(crate) controllers_scheme: bool,
    pub place: AimPlace,
    /// Whether a manned mounted gun is aimed by hand now.
    pub(crate) mounted_by_hand: bool,
    /// Whether the aim's results are recent. Stale results belong to an aim update that no longer
    /// runs, and say nothing about what aims now.
    pub(crate) aim_fresh: bool,
    /// Whether the last aim update aimed the wielded weapon by hand.
    pub(crate) weapon_aimed: bool,
}

/// Which reticles to hide for `inputs`.
pub fn decide(inputs: ReticleInputs) -> HiddenReticles {
    if !inputs.enabled || !inputs.controllers_scheme {
        return HiddenReticles::default();
    }
    let weapon_aimed = inputs.aim_fresh && inputs.weapon_aimed;
    // The mounted gun's point has its own freshness, so it does not wait on the aim update.
    let mounted = inputs.mounted_by_hand;
    let weapon = match inputs.place {
        AimPlace::OnFoot | AimPlace::Driver => true,
        AimPlace::Passenger | AimPlace::Menu => weapon_aimed || mounted,
    };
    HiddenReticles {
        weapon,
        grapple: true,
    }
}

/// Hold the game's reticles hidden while the rule hides them, and restore them otherwise.
///
/// # Safety
///
/// Must be called from the [`MovieImpl::Capture`](jc3gi::ui::scaleform::MovieImpl) detour, as
/// [`crate::hud::scaleform::apply_reticle_suppression`] requires.
pub(crate) unsafe fn hide_game_reticles() {
    let enabled = CONTROLLERS_CONFIG.query(|controllers| controllers.enabled);
    let hidden = if enabled {
        let aim = body_frame::fresh_aim(HUD.get().game_reticle.aim_window());
        decide(ReticleInputs {
            enabled,
            controllers_scheme: scheme::is_controllers(),
            place: AimPlace::of(mode::current().mode),
            mounted_by_hand: vehicles::mounted_aim_point().is_some(),
            aim_fresh: aim.is_some(),
            weapon_aimed: aim.is_some_and(|aim| aim.weapon.is_some()),
        })
    } else {
        HiddenReticles::default()
    };
    // SAFETY: forwarded from the caller.
    unsafe { crate::hud::scaleform::apply_reticle_suppression(hidden.weapon, hidden.grapple) };
}

#[cfg(test)]
mod tests {
    use jc3vr_body::controllers::mode::{
        AirborneVariant, OnFootVariant, VehicleClass, VehicleSeat,
    };

    use super::*;

    /// On foot under the controller scheme, with nothing aimed.
    const ON_FOOT: ReticleInputs = ReticleInputs {
        enabled: true,
        controllers_scheme: true,
        place: AimPlace::OnFoot,
        mounted_by_hand: false,
        aim_fresh: false,
        weapon_aimed: false,
    };

    const BOTH: HiddenReticles = HiddenReticles {
        weapon: true,
        grapple: true,
    };

    /// Only the grapple reticle hidden.
    const GRAPPLE: HiddenReticles = HiddenReticles {
        weapon: false,
        grapple: true,
    };

    #[test]
    fn on_foot_the_hand_aim_hides_both_reticles_armed_or_not() {
        assert_eq!(decide(ON_FOOT), BOTH);
        let aiming = ReticleInputs {
            aim_fresh: true,
            weapon_aimed: true,
            ..ON_FOOT
        };
        assert_eq!(decide(aiming), BOTH);
    }

    #[test]
    fn the_grapple_reticle_is_hidden_everywhere() {
        for place in [
            AimPlace::OnFoot,
            AimPlace::Driver,
            AimPlace::Passenger,
            AimPlace::Menu,
        ] {
            let inputs = ReticleInputs { place, ..ON_FOOT };
            assert!(decide(inputs).grapple, "{place:?}");
        }
    }

    #[test]
    fn the_toggle_and_the_gamepad_scheme_keep_both_reticles() {
        let off = ReticleInputs {
            enabled: false,
            ..ON_FOOT
        };
        let gamepad = ReticleInputs {
            controllers_scheme: false,
            mounted_by_hand: true,
            ..ON_FOOT
        };
        assert_eq!(decide(off), HiddenReticles::default());
        assert_eq!(decide(gamepad), HiddenReticles::default());
    }

    #[test]
    fn the_driver_hides_both_reticles() {
        let driver = ReticleInputs {
            place: AimPlace::Driver,
            ..ON_FOOT
        };
        assert_eq!(decide(driver), BOTH);
    }

    #[test]
    fn a_passenger_follows_the_aim_results_and_the_mounted_gun() {
        let passenger = ReticleInputs {
            place: AimPlace::Passenger,
            ..ON_FOOT
        };
        assert_eq!(decide(passenger), GRAPPLE);
        let mounted = ReticleInputs {
            mounted_by_hand: true,
            ..passenger
        };
        assert_eq!(decide(mounted), BOTH);
        let stale = ReticleInputs {
            weapon_aimed: true,
            ..passenger
        };
        assert_eq!(decide(stale), GRAPPLE);
        let fresh = ReticleInputs {
            aim_fresh: true,
            ..stale
        };
        assert_eq!(decide(fresh), BOTH);
    }

    #[test]
    fn the_modes_map_to_places() {
        assert_eq!(
            AimPlace::of(Mode::OnFoot(OnFootVariant::Swimming)),
            AimPlace::OnFoot
        );
        assert_eq!(
            AimPlace::of(Mode::Airborne(AirborneVariant::Wingsuit)),
            AimPlace::OnFoot
        );
        let seat = |driver| {
            Mode::Vehicle(VehicleSeat {
                class: VehicleClass::Car,
                driver,
            })
        };
        assert_eq!(AimPlace::of(seat(true)), AimPlace::Driver);
        assert_eq!(AimPlace::of(seat(false)), AimPlace::Passenger);
        assert_eq!(AimPlace::of(Mode::Ui), AimPlace::Menu);
    }
}
